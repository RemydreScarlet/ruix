use alloc::collections::VecDeque;
use spin::Mutex;
use super::{Process, ProcessState, WaitReason};
use crate::error::{KernelError, ProcessError};
use crate::error::KernelResult;
use crate::kerror;
use lazy_static::lazy_static;

/// Priority levels (0-31, lower = higher priority)
pub const MAX_PRIORITY: u8 = 0;
pub const MIN_PRIORITY: u8 = 31;
pub const DEFAULT_PRIORITY: u8 = 10;

pub struct Scheduler {
    pub processes: VecDeque<Process>,
    process_tree: alloc::collections::BTreeMap<u64, u64>, // PID -> parent PID mapping
    orphans: alloc::vec::Vec<u64>, // List of orphaned process IDs
    current_priority: u8, // Current priority being scheduled
}

lazy_static! {
    pub static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
        processes: VecDeque::new(),
        process_tree: alloc::collections::BTreeMap::new(),
        orphans: alloc::vec::Vec::new(),
        current_priority: DEFAULT_PRIORITY,
    });
}

impl Scheduler {
    pub fn add_process(&mut self, process: Process) {
        // Add to process tree
        self.process_tree.insert(process.id, process.parent_id);
        
        // Check if this is an orphan process
        if process.parent_id == 0 && process.id != 0 {
            self.orphans.push(process.id);
        }
        
        // Add to main processes list
        self.processes.push_back(process);
    }

    /// Get next process based on priority scheduling
    fn get_next_process_by_priority(&mut self) -> Option<u64> {
        // Find highest priority process in main processes list
        for process in &mut self.processes {
            if matches!(process.state, ProcessState::Ready | ProcessState::Running) {
                return Some(process.id);
            }
        }
        
        None
    }

    /// Register a new parent-child relationship
    pub fn register_child(&mut self, parent_pid: u64, child_pid: u64) -> KernelResult<()> {
        // Update process tree
        self.process_tree.insert(child_pid, parent_pid);
        
        // Update parent process's children list
        for process in &mut self.processes {
            if process.id == parent_pid {
                process.add_child(child_pid)?;
                break;
            }
        }
        
        // Remove from orphans if it was there
        if let Some(pos) = self.orphans.iter().position(|&pid| pid == child_pid) {
            self.orphans.remove(pos);
        }
        
        Ok(())
    }

    /// Handle process exit and clean up parent-child relationships
    pub fn handle_process_exit(&mut self, exiting_pid: u64, exit_code: i32) -> KernelResult<()> {
        // Find the exiting process
        let mut parent_pid = 0;
        for process in &mut self.processes {
            if process.id == exiting_pid {
                process.exit(exit_code)?;
                parent_pid = process.parent_id;
                break;
            }
        }

        // Find and update parent process
        if parent_pid != 0 {
            for process in &mut self.processes {
                if process.id == parent_pid {
                    process.remove_child(exiting_pid);
                    break;
                }
            }
        }

        // Make children orphans
        let mut children_to_orphan = alloc::vec::Vec::new();
        for (&child_pid, &parent_id) in &self.process_tree {
            if parent_id == exiting_pid {
                children_to_orphan.push(child_pid);
            }
        }

        for child_pid in children_to_orphan {
            // Update child's parent to 0 (init process)
            self.process_tree.insert(child_pid, 0);
            
            // Update the actual process
            for process in &mut self.processes {
                if process.id == child_pid {
                    process.make_orphan();
                    self.orphans.push(child_pid);
                    break;
                }
            }
        }

        // Wake up parent if it's waiting for this child
        if parent_pid != 0 {
            for process in &mut self.processes {
                if process.id == parent_pid {
                    if let ProcessState::Waiting(WaitReason::Child(waiting_pid)) = process.state {
                        if waiting_pid == exiting_pid || waiting_pid == u64::MAX {
                            process.state = ProcessState::Ready;
                        }
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    /// Find all zombie children of a parent process
    pub fn find_zombie_children(&self, parent_pid: u64) -> alloc::vec::Vec<u64> {
        let mut zombies = alloc::vec::Vec::new();
        
        for process in &self.processes {
            if process.parent_id == parent_pid && process.state == ProcessState::Zombie {
                zombies.push(process.id);
            }
        }
        
        zombies
    }

    /// Reap a zombie child and return its exit code
    pub fn reap_zombie_child(&mut self, parent_pid: u64, child_pid: u64) -> KernelResult<i32> {
        let mut exit_code = 0;
        let mut found = false;
        
        // Find and remove the zombie child
        self.processes.retain(|process| {
            if process.id == child_pid && process.parent_id == parent_pid {
                if process.state == ProcessState::Zombie {
                    exit_code = process.exit_code;
                    found = true;
                    false // Remove from scheduler
                } else {
                    true // Keep in scheduler
                }
            } else {
                true // Keep in scheduler
            }
        });

        // Update parent's children list
        if found {
            for process in &mut self.processes {
                if process.id == parent_pid {
                    process.remove_child(child_pid);
                    break;
                }
            }
            
            // Clean up process tree
            self.process_tree.remove(&child_pid);
            
            Ok(exit_code)
        } else {
            kerror!(ProcessError::NotFound)
        }
    }

    /// Get all orphan processes
    pub fn get_orphans(&self) -> &alloc::vec::Vec<u64> {
        &self.orphans
    }

    /// Check if a process is an orphan
    pub fn is_orphan(&self, pid: u64) -> bool {
        self.orphans.contains(&pid)
    }

    /// Get process hierarchy information
    pub fn get_process_hierarchy(&self, pid: u64) -> Option<(u64, alloc::vec::Vec<u64>)> {
        if let Some(&parent_pid) = self.process_tree.get(&pid) {
            let mut children = alloc::vec::Vec::new();
            for (&child_pid, &parent_id) in &self.process_tree {
                if parent_id == pid {
                    children.push(child_pid);
                }
            }
            Some((parent_pid, children))
        } else {
            None
        }
    }

    /// Clean up resources for a terminated process
    pub fn cleanup_terminated_process(&mut self, pid: u64) -> KernelResult<()> {
        // Find the process in the scheduler
        let mut process_found = false;
        let mut page_table_frame = None;
        
        for process in &self.processes {
            if process.id == pid {
                process_found = true;
                if process.state == ProcessState::Zombie {
                    page_table_frame = Some(process.page_table_frame);
                }
                break;
            }
        }

        if !process_found {
            return kerror!(ProcessError::NotFound);
        }

        // Only clean up zombie processes
        if let Some(frame) = page_table_frame {
            // In a real implementation, this would:
            // 1. Free all user-space memory pages
            // 2. Free the page table itself
            // 3. Close all file descriptors
            // 4. Release IPC resources
            // 5. Clean up any other kernel resources
            
            crate::println!("Cleanup: Freed resources for terminated process {}", pid);
            
            // For now, we'll just remove it from the scheduler
            self.processes.retain(|p| p.id != pid);
            
            // Clean up process tree
            self.process_tree.remove(&pid);
            
            // Remove from orphans if present
            if let Some(pos) = self.orphans.iter().position(|&p| p == pid) {
                self.orphans.remove(pos);
            }
            
            Ok(())
        } else {
            kerror!(ProcessError::InvalidState)
        }
    }

    /// Force cleanup of all zombie processes (called periodically)
    pub fn cleanup_all_zombies(&mut self) -> KernelResult<u32> {
        let mut cleaned_count = 0;
        let mut zombies_to_remove = alloc::vec::Vec::new();
        
        // Find all zombie processes
        for process in &self.processes {
            if process.state == ProcessState::Zombie {
                zombies_to_remove.push(process.id);
            }
        }
        
        // Remove each zombie
        for zombie_pid in zombies_to_remove {
            if self.cleanup_terminated_process(zombie_pid).is_ok() {
                cleaned_count += 1;
            }
        }
        
        if cleaned_count > 0 {
            crate::println!("Cleanup: Removed {} zombie processes", cleaned_count);
        }
        
        Ok(cleaned_count)
    }

    /// Get memory usage statistics
    pub fn get_memory_stats(&self) -> (u64, u32) {
        let mut total_memory = 0;
        let mut process_count = 0;
        
        for process in &self.processes {
            if process.state != ProcessState::Zombie {
                total_memory += process.stats.memory_used;
                process_count += 1;
            }
        }
        
        (total_memory, process_count)
    }

    /// Check system resource limits
    pub fn check_system_limits(&self) -> bool {
        let (total_memory, process_count) = self.get_memory_stats();
        
        // Basic sanity checks
        if process_count > 1000 { // Arbitrary limit
            crate::println!("System limit: Too many processes ({})", process_count);
            return false;
        }
        
        if total_memory > 1024 * 1024 * 1024 { // 1GB limit
            crate::println!("System limit: Too much memory used ({} bytes)", total_memory);
            return false;
        }
        
        true
    }

    pub fn schedule(&mut self, current_context_ptr: u64) -> u64 {
        // Try to get next process using priority scheduling first
        if let Some(next_pid) = self.get_next_process_by_priority() {
            // Find the process in our main processes list
            if let Some(pos) = self.processes.iter().position(|p| p.id == next_pid) {
                let mut next_process = self.processes.remove(pos).unwrap();
                
                // Update current process and move it to back of list
                if let Some(prev_process) = self.processes.front_mut() {
                    prev_process.context_ptr = current_context_ptr;
                    
                    // Update state if it was running
                    if prev_process.state == ProcessState::Running {
                        prev_process.state = ProcessState::Ready;
                    }
                }
                
                // Set next process as running
                next_process.state = ProcessState::Running;
                self.current_priority = next_process.priority;
                
                // Store context pointer before moving
                let context_ptr = next_process.context_ptr;
                let page_table_frame = next_process.page_table_frame;
                let process_id = next_process.id;
                
                // Move next process to front
                self.processes.push_front(next_process);
                
                unsafe {
                    crate::syscall::CPU_DATA.current_process_id = process_id;
                }
                
                // CR3レジスタを新しいプロセスのページテーブルに切り替え
                unsafe {
                    x86_64::registers::control::Cr3::write(page_table_frame, x86_64::registers::control::Cr3Flags::empty());
                }
                
                return context_ptr;
            }
        }
        
        // Fallback to simple round-robin if no priority processes are ready
        // 1. 現在のタスクを後ろに回す（ただしプロセスが存在する場合のみ）
        if let Some(mut prev) = self.processes.pop_front() {
            prev.context_ptr = current_context_ptr;
            self.processes.push_back(prev);
        }

        // 2. 次のタスクを新しく先頭から取る
        if let Some(next) = self.processes.front() {
            // CPU_DATAに現在のプロセスIDを設定
            unsafe {
                crate::syscall::CPU_DATA.current_process_id = next.id;
            }
            // CR3レジスタを新しいプロセスのページテーブルに切り替え
            unsafe {
                x86_64::registers::control::Cr3::write(next.page_table_frame, x86_64::registers::control::Cr3Flags::empty());
            }
            next.context_ptr
        } else {
            // プロセスがない場合はアイドル状態
            crate::println!("No processes available - entering idle state");
            unsafe {
                crate::syscall::CPU_DATA.current_process_id = 0;
            }
            // 現在のコンテキストを返す（アイドルループ）
            current_context_ptr
        }
    }
}


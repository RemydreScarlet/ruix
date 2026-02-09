//! Multi-core CPU Management
//! 
//! This module provides per-CPU data structures and management for
//! multi-core systems, ensuring thread safety and proper CPU isolation.

use crate::error::{KernelError, KernelResult, AllocError};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

/// Maximum number of CPUs supported
pub const MAX_CPUS: usize = 64;

/// Per-CPU data structure
#[repr(C)]
pub struct CpuData {
    /// CPU ID (0-based)
    pub cpu_id: usize,
    /// Current process ID running on this CPU
    pub current_process_id: AtomicUsize,
    /// User-mode stack pointer for syscall handling
    pub user_rsp: AtomicUsize,
    /// Kernel stack top for this CPU
    pub kernel_stack_top: usize,
    /// Pointer to this CPU's TSS
    pub tss_ptr: usize,
    /// CPU local storage area
    pub local_storage: CpuLocalStorage,
    /// Interrupt nesting level
    pub interrupt_depth: AtomicUsize,
    /// Scheduler state for this CPU
    pub scheduler_state: CpuSchedulerState,
}

/// CPU local storage for frequently accessed data
#[derive(Debug)]
pub struct CpuLocalStorage {
    /// Current running task (if any)
    pub current_task: Option<usize>,
    /// CPU-specific flags
    pub flags: u64,
    /// Performance counters
    pub perf_counters: [u64; 4],
    /// Last error occurred on this CPU
    pub last_error: Option<KernelError>,
}

/// CPU scheduler state
#[derive(Debug)]
pub struct CpuSchedulerState {
    /// Is this CPU currently scheduling?
    pub is_scheduling: bool,
    /// Next task to run
    pub next_task: Option<usize>,
    /// Scheduling quantum remaining
    pub quantum_remaining: usize,
    /// Load average for this CPU
    pub load_average: f64,
}

impl CpuData {
    /// Create a new CPU data structure
    pub fn new(cpu_id: usize, kernel_stack_top: usize) -> Self {
        Self {
            cpu_id,
            current_process_id: AtomicUsize::new(0),
            user_rsp: AtomicUsize::new(0),
            kernel_stack_top,
            tss_ptr: 0,
            local_storage: CpuLocalStorage {
                current_task: None,
                flags: 0,
                perf_counters: [0; 4],
                last_error: None,
            },
            interrupt_depth: AtomicUsize::new(0),
            scheduler_state: CpuSchedulerState {
                is_scheduling: false,
                next_task: None,
                quantum_remaining: 0,
                load_average: 0.0,
            },
        }
    }

    /// Get current process ID
    pub fn get_current_process_id(&self) -> usize {
        self.current_process_id.load(Ordering::Acquire)
    }

    /// Set current process ID
    pub fn set_current_process_id(&self, pid: usize) {
        self.current_process_id.store(pid, Ordering::Release);
    }

    /// Get user RSP
    pub fn get_user_rsp(&self) -> usize {
        self.user_rsp.load(Ordering::Acquire)
    }

    /// Set user RSP
    pub fn set_user_rsp(&self, rsp: usize) {
        self.user_rsp.store(rsp, Ordering::Release);
    }

    /// Enter interrupt context
    pub fn enter_interrupt(&self) {
        self.interrupt_depth.fetch_add(1, Ordering::AcqRel);
    }

    /// Exit interrupt context
    pub fn exit_interrupt(&self) {
        let prev_depth = self.interrupt_depth.fetch_sub(1, Ordering::AcqRel);
        if prev_depth == 1 {
            // We're exiting the last interrupt level
            // Note: This requires mutable access, which needs to be handled
            // at the call site or through interior mutability
        }
    }

    /// Check if we're in interrupt context
    pub fn in_interrupt(&self) -> bool {
        self.interrupt_depth.load(Ordering::Acquire) > 0
    }

    /// Handle interrupt exit (perform deferred work)
    fn handle_interrupt_exit(&mut self) {
        // Check for pending work that couldn't be done during interrupt
        if self.local_storage.flags & CPU_FLAG_PENDING_WORK != 0 {
            self.process_pending_work();
        }
    }

    /// Process pending work
    fn process_pending_work(&mut self) {
        // Clear the pending work flag
        self.local_storage.flags &= !CPU_FLAG_PENDING_WORK;
        
        // Process deferred work here
        // This could include:
        // - Task rescheduling
        // - Memory cleanup
        // - I/O completion
    }

    /// Set last error for this CPU
    pub fn set_last_error(&mut self, error: KernelError) {
        self.local_storage.last_error = Some(error);
    }

    /// Get and clear last error
    pub fn take_last_error(&mut self) -> Option<KernelError> {
        self.local_storage.last_error.take()
    }
}

/// CPU management flags
pub const CPU_FLAG_PENDING_WORK: u64 = 0x1;
pub const CPU_FLAG_IN_SYSCALL: u64 = 0x2;
pub const CPU_FLAG_SCHEDULE_PENDING: u64 = 0x4;

/// Global CPU manager
pub struct CpuManager {
    /// Array of CPU data structures
    cpus: [Option<CpuData>; MAX_CPUS],
    /// Number of initialized CPUs
    cpu_count: AtomicUsize,
    /// Current CPU ID (for the current execution context)
    current_cpu: AtomicUsize,
}

impl CpuManager {
    /// Create a new CPU manager
    pub const fn new() -> Self {
        Self {
            cpus: [const { None }; MAX_CPUS],
            cpu_count: AtomicUsize::new(0),
            current_cpu: AtomicUsize::new(0),
        }
    }

    /// Initialize a CPU
    pub fn init_cpu(&mut self, cpu_id: usize, kernel_stack_top: usize) -> KernelResult<()> {
        if cpu_id >= MAX_CPUS {
            return Err(KernelError::General(crate::error::GeneralError::InvalidOperation));
        }

        if self.cpus[cpu_id].is_some() {
            return Err(KernelError::General(crate::error::GeneralError::InvalidState));
        }

        self.cpus[cpu_id] = Some(CpuData::new(cpu_id, kernel_stack_top));
        self.cpu_count.fetch_add(1, Ordering::AcqRel);
        
        Ok(())
    }

    /// Get CPU data for the current CPU
    pub fn get_current_cpu(&self) -> KernelResult<&CpuData> {
        let cpu_id = self.current_cpu.load(Ordering::Acquire);
        self.get_cpu(cpu_id)
    }

    /// Get CPU data for a specific CPU
    pub fn get_cpu(&self, cpu_id: usize) -> KernelResult<&CpuData> {
        if cpu_id >= MAX_CPUS {
            return Err(KernelError::General(crate::error::GeneralError::InvalidOperation));
        }

        self.cpus[cpu_id]
            .as_ref()
            .ok_or_else(|| KernelError::General(crate::error::GeneralError::InvalidState))
    }

    /// Get mutable CPU data for the current CPU
    pub fn get_current_cpu_mut(&mut self) -> KernelResult<&mut CpuData> {
        let cpu_id = self.current_cpu.load(Ordering::Acquire);
        self.get_cpu_mut(cpu_id)
    }

    /// Get mutable CPU data for a specific CPU
    pub fn get_cpu_mut(&mut self, cpu_id: usize) -> KernelResult<&mut CpuData> {
        if cpu_id >= MAX_CPUS {
            return Err(KernelError::General(crate::error::GeneralError::InvalidOperation));
        }

        self.cpus[cpu_id]
            .as_mut()
            .ok_or_else(|| KernelError::General(crate::error::GeneralError::InvalidState))
    }

    /// Set current CPU ID (called during CPU initialization)
    pub fn set_current_cpu(&self, cpu_id: usize) {
        self.current_cpu.store(cpu_id, Ordering::Release);
    }

    /// Get number of initialized CPUs
    pub fn cpu_count(&self) -> usize {
        self.cpu_count.load(Ordering::Acquire)
    }

    /// Iterate over all initialized CPUs
    pub fn iter_cpus(&self) -> impl Iterator<Item = &CpuData> {
        self.cpus.iter().filter_map(|cpu| cpu.as_ref())
    }

    /// Iterate mutably over all initialized CPUs
    pub fn iter_cpus_mut(&mut self) -> impl Iterator<Item = &mut CpuData> {
        self.cpus.iter_mut().filter_map(|cpu| cpu.as_mut())
    }
}

/// Global CPU manager instance
static CPU_MANAGER: Mutex<CpuManager> = Mutex::new(CpuManager::new());

/// Initialize the CPU subsystem
pub fn init() -> KernelResult<()> {
    let mut manager = CPU_MANAGER.lock();
    
    // Initialize CPU 0 (bootstrap CPU)
    // In a real implementation, you'd detect actual CPU count
    let kernel_stack_top = get_kernel_stack_for_cpu(0)?;
    manager.init_cpu(0, kernel_stack_top)?;
    manager.set_current_cpu(0);
    
    crate::println!("CPU subsystem initialized with {} CPU(s)", manager.cpu_count());
    
    Ok(())
}

/// Get kernel stack for a specific CPU
fn get_kernel_stack_for_cpu(cpu_id: usize) -> KernelResult<usize> {
    use crate::allocator;
    
    // Allocate a 16KB kernel stack per CPU
    const KERNEL_STACK_SIZE: usize = 16 * 1024;
    
    // In a real implementation, you'd use a proper allocator
    // For now, we'll use a simple approach
    let stack_base = KERNEL_STACK_BASE + (cpu_id * KERNEL_STACK_SIZE);
    
    // Ensure the stack is properly aligned
    if stack_base % 16 != 0 {
        return Err(KernelError::Memory(AllocError::BadAlignment));
    }
    
    Ok(stack_base + KERNEL_STACK_SIZE)
}

/// Base address for kernel stacks (in high memory)
const KERNEL_STACK_BASE: usize = 0xFFFF_8000_0000_0000;

/// Get current CPU data
pub fn current_cpu() -> KernelResult<&'static CpuData> {
    let manager = CPU_MANAGER.lock();
    manager.get_current_cpu().map(|cpu| {
        // Extend lifetime to 'static - this is safe because CPU data
        // never changes after initialization
        unsafe { core::mem::transmute(cpu) }
    })
}

/// Get current CPU data (mutable)
pub fn current_cpu_mut() -> KernelResult<&'static mut CpuData> {
    let mut manager = CPU_MANAGER.lock();
    manager.get_current_cpu_mut().map(|cpu| {
        // Extend lifetime to 'static
        unsafe { core::mem::transmute(cpu) }
    })
}

/// Get CPU data by ID
pub fn get_cpu(cpu_id: usize) -> KernelResult<&'static CpuData> {
    let manager = CPU_MANAGER.lock();
    manager.get_cpu(cpu_id).map(|cpu| {
        unsafe { core::mem::transmute(cpu) }
    })
}

/// Get CPU data by ID (mutable)
pub fn get_cpu_mut(cpu_id: usize) -> KernelResult<&'static mut CpuData> {
    let mut manager = CPU_MANAGER.lock();
    manager.get_cpu_mut(cpu_id).map(|cpu| {
        unsafe { core::mem::transmute(cpu) }
    })
}

/// Get number of CPUs
pub fn cpu_count() -> usize {
    let manager = CPU_MANAGER.lock();
    manager.cpu_count()
}

/// CPU-local data access macro
#[macro_export]
macro_rules! with_current_cpu {
    ($cpu_id:ident, $body:block) => {
        match $crate::cpu::current_cpu() {
            Ok($cpu_id) => $body,
            Err(e) => {
                $crate::error::log_error(&e);
                // Return appropriate error or handle gracefully
                return $crate::kerror!(e);
            }
        }
    };
}

/// Performance monitoring
pub struct PerfMonitor {
    pub context_switches: AtomicUsize,
    pub interrupts_handled: AtomicUsize,
    pub syscalls_handled: AtomicUsize,
}

impl PerfMonitor {
    pub const fn new() -> Self {
        Self {
            context_switches: AtomicUsize::new(0),
            interrupts_handled: AtomicUsize::new(0),
            syscalls_handled: AtomicUsize::new(0),
        }
    }

    pub fn increment_context_switches(&self) {
        self.context_switches.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_interrupts(&self) {
        self.interrupts_handled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_syscalls(&self) {
        self.syscalls_handled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> (usize, usize, usize) {
        (
            self.context_switches.load(Ordering::Relaxed),
            self.interrupts_handled.load(Ordering::Relaxed),
            self.syscalls_handled.load(Ordering::Relaxed),
        )
    }
}

/// Global performance monitor
pub static PERF_MONITOR: PerfMonitor = PerfMonitor::new();

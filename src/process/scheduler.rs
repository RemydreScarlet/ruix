use alloc::collections::VecDeque;
use spin::Mutex;
use super::Process;
use lazy_static::lazy_static;

pub struct Scheduler {
    pub processes: VecDeque<Process>,
}

lazy_static! {
    pub static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
        processes: VecDeque::new(),
    });
}

impl Scheduler {
    pub fn add_process(&mut self, process: Process) {
        self.processes.push_back(process);
    }

    pub fn schedule(&mut self, current_context_ptr: u64) -> u64 {
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


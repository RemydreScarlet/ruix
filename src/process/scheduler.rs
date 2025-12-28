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
        // 1. 現在のタスクを後ろに回す
        if let Some(mut prev) = self.processes.pop_front() {
            prev.context_ptr = current_context_ptr;
            self.processes.push_back(prev);
        }

        // 2. 次のタスクを新しく先頭から取る
        if let Some(next) = self.processes.front() {
            next.context_ptr
        } else {
            current_context_ptr
        }
    }
}


use x86_64::instructions::port::Port;
use spin::Mutex;
use alloc::vec::Vec;
use lazy_static::lazy_static;

const PIT_FREQUENCY: u32 = 1193182; // PITの基本周波数
const TIMER_INTERVAL: u32 = 10; // 10Hz for faster timeout testing
const DEFAULT_TIMEOUT_LIMIT: u64 = 30; // 3 seconds at 10Hz

// プロセスごとのタイムアウト状態
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeoutState {
    Normal,
    Warning,
    TimedOut,
}

#[derive(Debug)]
struct ProcessTimeout {
    pid: u64,
    start_time: u64,
    limit: u64,
    state: TimeoutState,
    warning_sent: bool,
}

impl ProcessTimeout {
    fn new(pid: u64, limit: u64) -> Self {
        Self {
            pid,
            start_time: 0,
            limit,
            state: TimeoutState::Normal,
            warning_sent: false,
        }
    }
    
    fn reset(&mut self) {
        self.start_time = 0;
        self.state = TimeoutState::Normal;
        self.warning_sent = false;
    }
    
    fn start(&mut self, current_tick: u64) {
        self.start_time = current_tick;
        self.state = TimeoutState::Normal;
        self.warning_sent = false;
    }
    
    fn check_timeout(&mut self, current_tick: u64) -> TimeoutState {
        if self.state == TimeoutState::TimedOut {
            return TimeoutState::TimedOut;
        }
        
        let elapsed = current_tick.saturating_sub(self.start_time);
        let warning_threshold = self.limit / 2; // 50%で警告
        
        if elapsed >= self.limit {
            self.state = TimeoutState::TimedOut;
            TimeoutState::TimedOut
        } else if elapsed >= warning_threshold && !self.warning_sent {
            self.warning_sent = true;
            TimeoutState::Warning
        } else {
            TimeoutState::Normal
        }
    }
}

// タイムアウト管理用のグローバル変数
lazy_static! {
    static ref TIMEOUT_MANAGER: Mutex<TimeoutManager> = Mutex::new(TimeoutManager::new());
}
static GLOBAL_TICK_COUNTER: Mutex<u64> = Mutex::new(0);

// タイムアウト管理構造体
struct TimeoutManager {
    processes: Vec<ProcessTimeout>,
    current_tick: u64,
    user_mode_active: bool,
    current_user_pid: u64,
}

impl TimeoutManager {
    fn new() -> Self {
        Self {
            processes: Vec::new(),
            current_tick: 0,
            user_mode_active: false,
            current_user_pid: 0,
        }
    }
    
    fn register_process(&mut self, pid: u64, limit: Option<u64>) {
        let timeout_limit = limit.unwrap_or(DEFAULT_TIMEOUT_LIMIT);
        self.processes.push(ProcessTimeout::new(pid, timeout_limit));
        println!("TIMEOUT: Process {} registered with limit {} ticks", pid, timeout_limit);
    }
    
    fn unregister_process(&mut self, pid: u64) {
        self.processes.retain(|p| p.pid != pid);
        println!("TIMEOUT: Process {} unregistered", pid);
    }
    
    fn start_user_mode(&mut self, pid: u64) {
        self.user_mode_active = true;
        self.current_user_pid = pid;
        
        // 対応するプロセスのタイムアウトを開始
        if let Some(process) = self.processes.iter_mut().find(|p| p.pid == pid) {
            process.start(self.current_tick);
            println!("TIMEOUT: User mode started for PID {} at tick {}", pid, self.current_tick);
        } else {
            // プロセスが見つからない場合は登録して開始
            self.register_process(pid, None);
            if let Some(process) = self.processes.iter_mut().find(|p| p.pid == pid) {
                process.start(self.current_tick);
            }
        }
    }
    
    fn end_user_mode(&mut self) {
        self.user_mode_active = false;
        if let Some(process) = self.processes.iter_mut().find(|p| p.pid == self.current_user_pid) {
            process.reset();
            println!("TIMEOUT: User mode ended for PID {}", self.current_user_pid);
        }
        self.current_user_pid = 0;
    }
    
    fn increment_tick(&mut self) {
        self.current_tick = self.current_tick.wrapping_add(1);
        
        if self.user_mode_active {
            self.check_timeouts();
        }
    }
    
    fn check_timeouts(&mut self) {
        if let Some(process) = self.processes.iter_mut().find(|p| p.pid == self.current_user_pid) {
            let timeout_state = process.check_timeout(self.current_tick);
            let pid = process.pid; // 借用を避けるためにpidをコピー
            
            match timeout_state {
                TimeoutState::Warning => {
                    let remaining = process.limit.saturating_sub(
                        self.current_tick.saturating_sub(process.start_time)
                    );
                    println!("TIMEOUT WARNING: PID {} has {} ticks remaining", 
                            pid, remaining);
                }
                TimeoutState::TimedOut => {
                    self.handle_timeout(pid);
                }
                TimeoutState::Normal => {
                    // 正常状態
                }
            }
        }
    }
    
    fn handle_timeout(&mut self, pid: u64) {
        println!("TIMEOUT: Process {} exceeded time limit!", pid);
        
        // プロセス情報を取得してから借用を解放
        let (start_tick, limit) = if let Some(process) = self.processes.iter().find(|p| p.pid == pid) {
            (process.start_time, process.limit)
        } else {
            (0, 0)
        };
        
        println!("TIMEOUT: Current tick: {}, Start tick: {}, Limit: {}", 
                self.current_tick, start_tick, limit);
        
        // プロセスを終了させる
        self.kill_process(pid);
        
        // ユーザーモードを終了
        self.end_user_mode();
    }
    
    fn kill_process(&mut self, pid: u64) {
        println!("TIMEOUT: Killing process {} due to timeout", pid);
        
        // プロセススケジューラに終了を通知
        use crate::process::scheduler::SCHEDULER;
        let mut sched = SCHEDULER.lock();
        
        // プロセスをZombie状態に設定
        for process in &mut sched.processes {
            if process.id == pid {
                process.state = crate::process::ProcessState::Zombie;
                process.exit_code = -1; // タイムアウト終了コード
                println!("TIMEOUT: Process {} marked as zombie", pid);
                break;
            }
        }
        
        // 親プロセスを起床させる
        for process in &mut sched.processes {
            if let crate::process::ProcessState::Waiting(
                crate::process::WaitReason::Child(waiting_pid)
            ) = process.state {
                if waiting_pid == pid || waiting_pid == (-1i64 as u64) {
                    process.state = crate::process::ProcessState::Ready;
                    println!("TIMEOUT: Woke up parent {} from waiting for child {}", 
                            process.id, pid);
                }
            }
        }
    }
    
    fn set_timeout_limit(&mut self, pid: u64, limit: u64) {
        if let Some(process) = self.processes.iter_mut().find(|p| p.pid == pid) {
            process.limit = limit;
            println!("TIMEOUT: Set limit {} ticks for PID {}", limit, pid);
        } else {
            // プロセスが存在しない場合は新規登録
            self.register_process(pid, Some(limit));
        }
    }
    
    fn get_status(&self, pid: u64) -> Option<(TimeoutState, u64, u64)> {
        self.processes.iter()
            .find(|p| p.pid == pid)
            .map(|p| {
                let elapsed = self.current_tick.saturating_sub(p.start_time);
                (p.state, elapsed, p.limit)
            })
    }
}

// PITを初期化してタイマー割り込みを開始
pub fn init() {
    let divisor = PIT_FREQUENCY / TIMER_INTERVAL;
    
    unsafe {
        // PITコマンドポート（0x43）
        let mut command_port = Port::new(0x43);
        // チャネル0、square waveモード、アクセスモードはlow/highバイト両方
        command_port.write(0x36u8);
        
        // ディバイダ設定（チャネル0、ポート0x40）
        let mut data_port = Port::new(0x40);
        // 下位バイト
        data_port.write((divisor & 0xFF) as u8);
        // 上位バイト  
        data_port.write(((divisor >> 8) & 0xFF) as u8);
    }
    
    println!("Timer initialized: {}Hz (default timeout: {} ticks)", 
             TIMER_INTERVAL, DEFAULT_TIMEOUT_LIMIT);
}

// タイマーティックをインクリメント（タイマー割り込みから呼ばれる）
pub fn increment_tick() {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.increment_tick();
    
    // グローバルカウンタも更新（後方互換性のため）
    let mut global_counter = GLOBAL_TICK_COUNTER.lock();
    *global_counter = manager.current_tick;
}

// 後方互換性のための関数（廃止予定）
#[deprecated(note = "Use increment_tick() instead")]
pub fn increment_timeout() {
    increment_tick();
}

// ユーザーモードを開始
pub fn start_user_mode(pid: u64) {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.start_user_mode(pid);
}

// ユーザーモードを終了
pub fn end_user_mode() {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.end_user_mode();
}

// プロセスをタイムアウト管理に登録
pub fn register_process(pid: u64, timeout_limit: Option<u64>) {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.register_process(pid, timeout_limit);
}

// プロセスをタイムアウト管理から削除
pub fn unregister_process(pid: u64) {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.unregister_process(pid);
}

// プロセスのタイムアウト制限を設定
pub fn set_timeout_limit(pid: u64, limit: u64) {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.set_timeout_limit(pid, limit);
}

// プロセスのタイムアウト状態を取得
pub fn get_timeout_status(pid: u64) -> Option<(TimeoutState, u64, u64)> {
    let manager = TIMEOUT_MANAGER.lock();
    manager.get_status(pid)
}

// グローバルティックカウンタを取得
pub fn get_global_tick() -> u64 {
    *GLOBAL_TICK_COUNTER.lock()
}

// 後方互換性のための関数（廃止予定）
#[deprecated(note = "Use get_global_tick() instead")]
pub fn get_timeout_counter() -> u64 {
    get_global_tick()
}

// 後方互換性のための関数（廃止予定）
#[deprecated(note = "Use register_process/end_user_mode instead")]
pub fn reset_timeout() {
    let mut manager = TIMEOUT_MANAGER.lock();
    manager.end_user_mode();
}

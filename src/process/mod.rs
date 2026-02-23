use x86_64::{structures::paging::{PhysFrame, Size4KiB, FrameAllocator, OffsetPageTable}};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::error::{KernelError, ProcessError};
use crate::error::KernelResult;
use crate::kerror;
use core::{future::Future, pin::Pin, task::{Context, Poll}};
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use crossbeam_queue::ArrayQueue;
use alloc::task::Wake;
use conquer_once::spin::OnceCell;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use futures_util::task::AtomicWaker;
use futures_util::stream::{Stream, StreamExt};

pub mod scheduler;

pub const DEFAULT_PRIORITY: u8 = 10;

lazy_static! {
    static ref NEXT_PID: Mutex<u64> = Mutex::new(1);
    static ref NEXT_PGID: Mutex<u64> = Mutex::new(1);
    static ref NEXT_SID: Mutex<u64> = Mutex::new(1);
}

// Simple timestamp counter for process creation times
static mut TIMESTAMP_COUNTER: u64 = 0;

pub fn get_current_time() -> u64 {
    unsafe {
        TIMESTAMP_COUNTER += 1;
        TIMESTAMP_COUNTER
    }
}

pub fn allocate_pid() -> u64 {
    let mut pid = NEXT_PID.lock();
    let current = *pid;
    *pid += 1;
    current
}

pub fn allocate_pgid() -> u64 {
    let mut pgid = NEXT_PGID.lock();
    let current = *pgid;
    *pgid += 1;
    current
}

pub fn allocate_sid() -> u64 {
    let mut sid = NEXT_SID.lock();
    let current = *sid;
    *sid += 1;
    current
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Waiting(WaitReason),
    Zombie,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitReason {
    Child(u64),
    IpcReceive(u64),
    IpcSend(u64),
    Sleep(u64),
    AsyncPoll,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskType {
    Process,
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        TaskId(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
    
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

pub trait TaskBehavior {
    fn get_id(&self) -> u64;
    fn get_task_type(&self) -> TaskType;
    fn get_state(&self) -> ProcessState;
    fn set_state(&mut self, state: ProcessState);
    fn get_priority(&self) -> u8;
    fn set_priority(&mut self, priority: u8) -> KernelResult<()>;
    fn can_schedule(&self) -> bool;
    fn poll_task(&mut self) -> Poll<()>;
    fn wake_task(&mut self);
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ProcessContext {
    // 汎用レジスタ (アセンブリの pop r15...rax の順順)
    r15: u64, r14: u64, r13: u64, r12: u64,
    rbp: u64, rbx: u64, r11: u64, r10: u64,
    r9: u64, r8: u64, rdi: u64, rsi: u64,
    rdx: u64, rcx: u64, rax: u64,

    // CPUが自動で積むIRETQ用フレーム
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

pub struct AsyncTask {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    state: ProcessState,
    priority: u8,
    waker_cache: BTreeMap<TaskId, core::task::Waker>,
}

impl AsyncTask {
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            id: TaskId::new(),
            future: Box::pin(future),
            state: ProcessState::Ready,
            priority: DEFAULT_PRIORITY,
            waker_cache: BTreeMap::new(),
        }
    }
    
    pub fn poll(&mut self, context: &mut core::task::Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_memory: u64,      // Maximum memory in bytes
    pub max_cpu_time: u64,    // Maximum CPU time in milliseconds
    pub max_processes: u32,   // Maximum number of child processes
    pub max_files: u32,       // Maximum number of open files
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024,  // 64MB default
            max_cpu_time: 30 * 1000,       // 30 seconds default
            max_processes: 32,             // 32 processes default
            max_files: 16,                 // 16 files default
        }
    }
}

#[derive(Debug)]
pub struct ProcessStats {
    pub cpu_time_used: u64,    // CPU time used in milliseconds
    pub memory_used: u64,       // Memory currently used in bytes
    pub children_count: u32,    // Number of living children
    pub files_opened: u32,     // Number of open files
}

impl Default for ProcessStats {
    fn default() -> Self {
        Self {
            cpu_time_used: 0,
            memory_used: 0,
            children_count: 0,
            files_opened: 0,
        }
    }
}

pub struct Process {
    pub id: u64,
    pub context_ptr: u64,
    pub page_table_frame: PhysFrame,
    pub state: ProcessState,
    pub parent_id: u64,
    pub children: alloc::vec::Vec<u64>,
    pub exit_code: i32,
    pub priority: u8,           // Priority level (0-31, lower = higher priority)
    pub resource_limits: ResourceLimits,
    pub stats: ProcessStats,
    pub process_group_id: u64,   // Process group ID
    pub session_id: u64,         // Session ID
    pub creation_time: u64,      // Process creation timestamp
}

impl Process {
    pub fn new(id: u64, entry_point: u64, stack_top: u64, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Self {
        // 1. ProcessContext構造体のサイズ分だけスタックの「下」を指す
        let context_ptr = (stack_top - core::mem::size_of::<ProcessContext>() as u64) as *mut ProcessContext;

        // 2. プロセス固有のページテーブルを作成
        let page_table_frame = create_process_page_table_with_user_mappings(mapper, frame_allocator);

        unsafe {
            // 3. その場所に初期値を書き込む
            (*context_ptr) = ProcessContext {
                r15: 0, r14: 0, r13: 0, r12: 0,
                rbp: 0, rbx: 0,
                r11: 0, r10: 0, r9: 0, r8: 0,
                rdi: 0, rsi: 0, rdx: 0, rcx: 0, rax: 0,

                rip: entry_point,
                cs: 0x23,         // ユーザーコードセグメント (GDTのインデックスに合わせて！)
                rflags: 0x202,    // 割り込み許可フラグ
                rsp: stack_top,   // ユーザーモードでのスタックポインタ
                ss: 0x1b,         // ユーザーデータセグメント
            };
        }

        Process {
            id,
            context_ptr: context_ptr as u64,
            page_table_frame,
            state: ProcessState::Ready,
            parent_id: 0,
            children: alloc::vec::Vec::new(),
            exit_code: 0,
            priority: DEFAULT_PRIORITY,  // Default priority (medium)
            resource_limits: ResourceLimits::default(),
            stats: ProcessStats::default(),
            process_group_id: id,  // Initially, process is its own group leader
            session_id: id,        // Initially, process is its own session leader
            creation_time: get_current_time(),
        }
    }

    pub fn fork(&self, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Result<Self, &'static str> {
        // Allocate new PID for child
        let child_pid = allocate_pid();
        
        // Copy the current context (registers will be set after fork)
        let current_context = unsafe { &*(self.context_ptr as *const ProcessContext) };
        
        // Create child process with same entry point and stack
        let mut child = Self::new(child_pid, current_context.rip, current_context.rsp, mapper, frame_allocator);
        
        // Set parent-child relationship
        child.parent_id = self.id;
        child.process_group_id = self.process_group_id;  // Inherit process group
        child.session_id = self.session_id;              // Inherit session
        child.priority = self.priority;                  // Inherit priority
        child.resource_limits = self.resource_limits.clone(); // Inherit limits
        
        // Copy register state from parent
        let child_context = unsafe { &mut *(child.context_ptr as *mut ProcessContext) };
        *child_context = current_context.clone();
        
        // Set return values:
        // Parent gets child PID, child gets 0
        child_context.rax = 0; // Child returns 0
        
        Ok(child)
    }

    /// Exit the current process with the given exit code
    pub fn exit(&mut self, exit_code: i32) -> KernelResult<()> {
        // Validate exit code
        if exit_code < -255 || exit_code > 255 {
            return kerror!(ProcessError::InvalidState);
        }

        // Set process state to Zombie
        self.state = ProcessState::Zombie;
        self.exit_code = exit_code;
        
        crate::println!("Process {} exiting with code {}", self.id, exit_code);
        Ok(())
    }

    /// Wait for a child process to exit
    pub fn wait(&mut self, child_pid: u64) -> KernelResult<i32> {
        // Check if we have any children
        if self.children.is_empty() {
            return kerror!(ProcessError::NotFound);
        }

        // Look for zombie children
        for &child_id in &self.children {
            if child_pid == u64::MAX || child_id == child_pid {
                // This would need to be implemented with scheduler integration
                // For now, return a placeholder
                return Ok(0);
            }
        }

        // No zombie children found
        kerror!(ProcessError::NotFound)
    }

    /// Join a process (wait for it to complete and get its exit code)
    pub fn join(&mut self, target_pid: u64) -> KernelResult<i32> {
        // Similar to wait but for any process, not just children
        if self.state == ProcessState::Zombie && self.id == target_pid {
            return Ok(self.exit_code);
        }

        // Block until target process exits
        self.state = ProcessState::Waiting(WaitReason::Child(target_pid));
        Ok(self.exit_code)
    }

    /// Check if the process can be safely terminated
    pub fn can_terminate(&self) -> bool {
        match self.state {
            ProcessState::Running | ProcessState::Ready | ProcessState::Waiting(_) => true,
            ProcessState::Zombie | ProcessState::Stopped => false,
        }
    }

    /// Set process priority (0-31, lower = higher priority)
    pub fn set_priority(&mut self, priority: u8) -> KernelResult<()> {
        if priority > 31 {
            return kerror!(ProcessError::InvalidState);
        }
        self.priority = priority;
        Ok(())
    }

    /// Update resource limits
    pub fn set_resource_limits(&mut self, limits: ResourceLimits) -> KernelResult<()> {
        // Validate limits
        if limits.max_memory == 0 || limits.max_cpu_time == 0 {
            return kerror!(ProcessError::InvalidState);
        }
        self.resource_limits = limits;
        Ok(())
    }

    /// Check if process has exceeded resource limits
    pub fn check_resource_limits(&self) -> bool {
        self.stats.memory_used <= self.resource_limits.max_memory &&
        self.stats.cpu_time_used <= self.resource_limits.max_cpu_time &&
        self.stats.children_count <= self.resource_limits.max_processes &&
        self.stats.files_opened <= self.resource_limits.max_files
    }

    /// Add a child process to this process's children list
    pub fn add_child(&mut self, child_pid: u64) -> KernelResult<()> {
        if self.stats.children_count >= self.resource_limits.max_processes {
            return kerror!(ProcessError::InvalidState);
        }
        
        self.children.push(child_pid);
        self.stats.children_count += 1;
        Ok(())
    }

    /// Remove a child process from this process's children list
    pub fn remove_child(&mut self, child_pid: u64) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child_pid) {
            self.children.remove(pos);
            self.stats.children_count = self.stats.children_count.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Make this process an orphan (parent has exited)
    pub fn make_orphan(&mut self) {
        self.parent_id = 0; // Init process (PID 0) becomes the new parent
    }

    /// Check if this process is an orphan
    pub fn is_orphan(&self) -> bool {
        self.parent_id == 0 && self.id != 0
    }

    /// Get process age in time units
    pub fn get_age(&self) -> u64 {
        get_current_time().saturating_sub(self.creation_time)
    }

    /// Update CPU usage statistics
    pub fn update_cpu_usage(&mut self, cpu_time: u64) {
        self.stats.cpu_time_used += cpu_time;
    }

    /// Update memory usage statistics
    pub fn update_memory_usage(&mut self, memory_used: u64) {
        self.stats.memory_used = memory_used;
    }

    /// Increment file open count
    pub fn increment_file_count(&mut self) -> KernelResult<()> {
        if self.stats.files_opened >= self.resource_limits.max_files {
            return kerror!(ProcessError::InvalidState);
        }
        self.stats.files_opened += 1;
        Ok(())
    }

    /// Decrement file open count
    pub fn decrement_file_count(&mut self) {
        self.stats.files_opened = self.stats.files_opened.saturating_sub(1);
    }

    /// Create a new process group with this process as leader
    pub fn create_process_group(&mut self) -> KernelResult<u64> {
        let new_pgid = allocate_pgid();
        self.process_group_id = new_pgid;
        Ok(new_pgid)
    }

    /// Join an existing process group
    pub fn join_process_group(&mut self, pgid: u64) -> KernelResult<()> {
        if pgid == 0 {
            return kerror!(ProcessError::InvalidPid);
        }
        self.process_group_id = pgid;
        Ok(())
    }

    /// Create a new session with this process as leader
    pub fn create_session(&mut self) -> KernelResult<u64> {
        // Only process group leaders can create sessions
        if self.process_group_id != self.id {
            return kerror!(ProcessError::InvalidState);
        }
        
        let new_sid = allocate_sid();
        self.session_id = new_sid;
        Ok(new_sid)
    }
}

impl TaskBehavior for Process {
    fn get_id(&self) -> u64 {
        self.id
    }
    
    fn get_task_type(&self) -> TaskType {
        TaskType::Process
    }
    
    fn get_state(&self) -> ProcessState {
        self.state
    }
    
    fn set_state(&mut self, state: ProcessState) {
        self.state = state;
    }
    
    fn get_priority(&self) -> u8 {
        self.priority
    }
    
    fn set_priority(&mut self, priority: u8) -> KernelResult<()> {
        self.set_priority(priority)
    }
    
    fn can_schedule(&self) -> bool {
        matches!(self.state, ProcessState::Ready | ProcessState::Running)
    }
    
    fn poll_task(&mut self) -> Poll<()> {
        // Processes are always "ready" for scheduling purposes
        Poll::Ready(())
    }
    
    fn wake_task(&mut self) {
        if matches!(self.state, ProcessState::Waiting(_)) {
            self.state = ProcessState::Ready;
        }
    }
}

impl TaskBehavior for AsyncTask {
    fn get_id(&self) -> u64 {
        self.id.as_u64()
    }
    
    fn get_task_type(&self) -> TaskType {
        TaskType::Async
    }
    
    fn get_state(&self) -> ProcessState {
        self.state
    }
    
    fn set_state(&mut self, state: ProcessState) {
        self.state = state;
    }
    
    fn get_priority(&self) -> u8 {
        self.priority
    }
    
    fn set_priority(&mut self, priority: u8) -> KernelResult<()> {
        if priority > 31 {
            return kerror!(ProcessError::InvalidState);
        }
        self.priority = priority;
        Ok(())
    }
    
    fn can_schedule(&self) -> bool {
        matches!(self.state, ProcessState::Ready)
    }
    
    fn poll_task(&mut self) -> Poll<()> {
        // For now, return Ready to indicate the task can be scheduled
        // In a full implementation, this would do actual async polling
        if matches!(self.state, ProcessState::Ready) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    
    fn wake_task(&mut self) {
        self.state = ProcessState::Ready;
    }
}

struct TaskWaker {
    task_id: TaskId,
    task_queue: Arc<ArrayQueue<TaskId>>,
}

impl TaskWaker {
    fn wake_task(&self) {
        if let Err(_) = self.task_queue.push(self.task_id) {
            crate::println!("WARNING: async task queue full");
        }
    }
    
    fn new(task_id: TaskId, task_queue: Arc<ArrayQueue<TaskId>>) -> core::task::Waker {
        core::task::Waker::from(Arc::new(TaskWaker {
            task_id,
            task_queue,
        }))
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_task();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wake_task();
    }
}

// Keyboard task functionality integrated from task module
static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static KEYBOARD_WAKER: AtomicWaker = AtomicWaker::new();

// Keyboard interrupt handler integration
pub fn add_keyboard_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if let Err(_) = queue.push(scancode) {
            crate::println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            KEYBOARD_WAKER.wake();
        }
    } else {
        crate::println!("WARNING: scancode queue uninitialized");
    }
}

pub struct KeyboardScancodeStream {
    _private: (),
}

impl KeyboardScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(100))
            .expect("KeyboardScancodeStream::new should only be called once");
        KeyboardScancodeStream { _private: () }
    }
}

impl Stream for KeyboardScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut core::task::Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE
            .try_get()
            .expect("scancode queue not initialized");

        if let Ok(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        KEYBOARD_WAKER.register(cx.waker());
        match queue.pop() {
            Ok(scancode) => {
                KEYBOARD_WAKER.take();
                Poll::Ready(Some(scancode))
            }
            Err(_) => Poll::Pending,
        }
    }
}

pub async fn process_keyboard_input() {
    let mut scancodes = KeyboardScancodeStream::new();
    let mut keyboard = Keyboard::new(ScancodeSet1::new(),
        layouts::Us104Key, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => crate::print!("{}", character),
                    DecodedKey::RawKey(key) => crate::println!("{:?}", key),
                }
            }
        }
    }
}

// プロセス固有のページテーブルを作成し、ユーザー空間のマッピングをコピーする関数
fn create_process_page_table_with_user_mappings(mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> PhysFrame {
    use x86_64::structures::paging::PageTable;

    // 新しいL4ページテーブルフレームを割り当てる
    let page_table_frame = frame_allocator.allocate_frame().expect("no frames available for page table");

    // 物理メモリオフセットを取得
    let phys_offset = mapper.phys_offset();

    // 新しいページテーブルの仮想アドレスを取得
    let new_table_virt = phys_offset + page_table_frame.start_address().as_u64();
    let new_table = unsafe { &mut *(new_table_virt.as_mut_ptr() as *mut PageTable) };

    // 現在のページテーブル（カーネルページテーブル）を取得
    let current_table = mapper.level_4_table();

    // 全てのエントリをコピー（カーネルマッピング + ユーザーマッピング）
    for i in 0..512 {
        new_table[i] = current_table[i].clone();
    }

    page_table_frame
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_switch(current_context_ptr: u64) -> u64 {
    use crate::process::scheduler::SCHEDULER;

    // 1. まず何よりも先に EOI を送る（PICを黙らせる）
    unsafe {
        use x86_64::instructions::port::Port;
        let mut master_pic_port = Port::new(0x20);
        master_pic_port.write(0x20u8); // 0x20 は EOI (End of Interrupt) コマンド
    }

    let ctx = unsafe { &*(current_context_ptr as *const ProcessContext) };

    // この ctx.rsp こそが、ユーザーモードで動いていた時のRSPです！
    // Task 1 なら 0x601000 付近、Task 2 なら staticなSTACKのアドレスが出るはず
    println!("Switching! Task User RSP: {:#x}", ctx.rsp);

    let mut sched = SCHEDULER.lock();
    // 2. 切り替えロジック
    sched.schedule(current_context_ptr)

}

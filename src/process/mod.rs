use x86_64::{structures::paging::{PhysFrame, Size4KiB, FrameAllocator, OffsetPageTable}};
use spin::Mutex;
use lazy_static::lazy_static;

pub mod scheduler;

lazy_static! {
    static ref NEXT_PID: Mutex<u64> = Mutex::new(1);
}

pub fn allocate_pid() -> u64 {
    let mut pid = NEXT_PID.lock();
    let current = *pid;
    *pid += 1;
    current
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Waiting(WaitReason),
    Zombie,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitReason {
    Child(u64),
    IpcReceive(u64),
    IpcSend(u64),
    Sleep(u64),
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
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

pub struct Process {
    pub id: u64,
    pub context_ptr: u64,
    pub page_table_frame: PhysFrame,
    pub state: ProcessState,
    pub parent_id: u64,
    pub children: alloc::vec::Vec<u64>,
    pub exit_code: i32,
}

impl Process {
    pub fn new(id: u64, entry_point: u64, stack_top: u64, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Self {
        // 1. Context構造体のサイズ分だけスタックの「下」を指す
        let context_ptr = (stack_top - core::mem::size_of::<Context>() as u64) as *mut Context;

        // 2. プロセス固有のページテーブルを作成
        let page_table_frame = create_process_page_table_with_user_mappings(mapper, frame_allocator);

        unsafe {
            // 3. その場所に初期値を書き込む
            (*context_ptr) = Context {
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
        }
    }

    pub fn fork(&self, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Result<Self, &'static str> {
        // Allocate new PID for child
        let child_pid = allocate_pid();
        
        // Copy the current context (registers will be set after fork)
        let current_context = unsafe { &*(self.context_ptr as *const Context) };
        
        // Create child process with same entry point and stack
        let mut child = Self::new(child_pid, current_context.rip, current_context.rsp, mapper, frame_allocator);
        
        // Set parent-child relationship
        child.parent_id = self.id;
        
        // Copy register state from parent
        let child_context = unsafe { &mut *(child.context_ptr as *mut Context) };
        *child_context = current_context.clone();
        
        // Set return values:
        // Parent gets child PID, child gets 0
        child_context.rax = 0; // Child returns 0
        
        Ok(child)
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

    let ctx = unsafe { &*(current_context_ptr as *const Context) };

    // この ctx.rsp こそが、ユーザーモードで動いていた時のRSPです！
    // Task 1 なら 0x601000 付近、Task 2 なら staticなSTACKのアドレスが出るはず
    println!("Switching! Task User RSP: {:#x}", ctx.rsp);

    let mut sched = SCHEDULER.lock();
    // 2. 切り替えロジック
    sched.schedule(current_context_ptr)

}

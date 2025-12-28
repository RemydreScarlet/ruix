pub mod scheduler;

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
    pub context_ptr: u64, // 保存されたContext構造体へのポインタ
    // TODO: ページテーブル(CR3)なども追加する
}

impl Process {
    pub fn new(id: u64, entry_point: u64, stack_top: u64) -> Self {
        // 1. Context構造体のサイズ分だけスタックの「下」を指す
        let context_ptr = (stack_top - core::mem::size_of::<Context>() as u64) as *mut Context;

        unsafe {
            // 2. その場所に初期値を書き込む
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
        }
    }
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

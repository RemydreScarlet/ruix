use x86_64::registers::model_specific::{LStar, Star, SFMask, GsBase, KernelGsBase};
use x86_64::structures::gdt::SegmentSelector;
use x86_64::registers::rflags::RFlags;
use crate::gdt;
use core::arch::naked_asm;

#[repr(C)]
struct CpuData {
    // SYSCALL時にユーザーのRSPを一時退避する場所 (offset 0)
    user_rsp: u64,
    // このCPU用のカーネルスタックのトップ (offset 8)
    kernel_stack_top: u64,
    // Todo: 現在実行中のプロセスのIDやTSSへのポインタなど
}

// 起動時はゼロで初期化。
// Lazy Staticの使い方が飛んだので許してください
static mut CPU_DATA: CpuData = CpuData {
    user_rsp: 0,
    kernel_stack_top: 0,
};

pub fn init() {
    use x86_64::registers::model_specific::Efer;
    
    // SYSCALLを有効化
    unsafe {
        Efer::update(|f| f.insert(x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS));
        SFMask::write(RFlags::INTERRUPT_FLAG);
    }

    let selectors = gdt::get_selectors();
    let stack_top = gdt::kernel_stack_top();

    unsafe {
        CPU_DATA.kernel_stack_top = stack_top.as_u64();
        let ptr = core::ptr::addr_of!(CPU_DATA);
        crate::println!("CPU_DATA address: {:p}", ptr);

        // GsBaseに構造体のアドレスを書き込む
        x86_64::registers::model_specific::GsBase::write(
            x86_64::VirtAddr::from_ptr(core::ptr::addr_of!(CPU_DATA))
        );
        KernelGsBase::write(x86_64::VirtAddr::from_ptr(core::ptr::addr_of!(CPU_DATA)));

        // STARレジスタの設定
        // SYSCALL時に使われるカーネルセグメントと、
        // SYSRET時に使われるユーザーセグメントのベースを指定
        Star::write(
            SegmentSelector(selectors.user_code_selector.0 | 3),
            SegmentSelector(selectors.user_data_selector.0 | 3),
            selectors.code_selector,
            selectors.data_selector,
        ).unwrap();

        // LSTARレジスタの設定: ハンドラのアドレスを登録
        LStar::write(x86_64::VirtAddr::new(asm_syscall_handler as u64));

        // SFMASKレジスタの設定: SYSCALL時にRFLAGSからクリアするビット
        // 割り込みフラグ(IF)をクリアして、ハンドラ実行中の割り込みを禁止する
        SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG);
    }
}

// システムコールのエントリポイント（アセンブリ）
// 保存すべきレジスタをスタックに積み、Rustのハンドラを呼び出す
#[unsafe(naked)]
unsafe extern "C" fn asm_syscall_handler() {
    naked_asm!(
        // GSベースの切り替え
        "swapgs",
        "mov gs:[0], rsp",      // [gs:0] へのユーザーRSP退避
        "mov rsp, gs:[8]",      // [gs:8] からカーネルスタックをロード

        // コンテキスト保存
        "push r11",             // RFLAGS
        "push rcx",             // 復帰用RIP
        
        // スタックアライメント調整 (16byte境界)
        "sub rsp, 8",
        
        "mov rdi, rsp",         // 第1引数に現在のスタックポインタ
        "call {rust_handler}",
        
        "add rsp, 8",           // 調整戻し

        "pop rcx",
        "pop r11",
        
        "mov rsp, gs:[0]",      // ユーザーRSP復元
        "swapgs",
        "sysretq",
        rust_handler = sym rust_syscall_handler,
    );
}

// Rust側のシステムコール処理ロジック
extern "C" fn rust_syscall_handler(stack_ptr: u64) {
    // 本来ならRAXレジスタの値などで処理を分岐
    // 現在はデバッグ用にprintln!を出すだけにする
    //crate::println!("Syscall triggered! Stack at: {:#x}", stack_ptr);
}
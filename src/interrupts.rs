use x86_64::VirtAddr;
use core::arch::naked_asm;

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use pic8259::ChainedPics;

use lazy_static::lazy_static;
use spin;

use crate::hlt_loop;
use crate::gdt;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

// 割り込み記述子テーブル(IDT)の初期化
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        let timer_addr = VirtAddr::new(timer_interrupt_handler as *const () as u64);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
            idt[InterruptIndex::Timer.as_usize()].set_handler_addr(timer_addr);
        }
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        
        // INT 0x80 (ソフトウェア割り込み)用のハンドラを設定
        let syscall_addr = VirtAddr::new(syscall_interrupt_handler as *const () as u64);
        unsafe {
            idt[0x80].set_handler_addr(syscall_addr);
        }
        
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

// ブレークポイント例外ハンドラ
extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64)
    -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    hlt_loop();
}

// キーボード割り込み、タイマーハンドラ
#[unsafe(naked)]
pub unsafe extern "C" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    naked_asm!(
        // 1. 汎用レジスタをすべてスタックに積む (Context構造体の並びに合わせる)
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "sub rsp, 8",         // アライメント調整（16バイト境界にする）
        "mov rdi, rsp",
        "add rdi, 8",         // 引数には「元のContextの先頭」を渡す
        "call {switch_handler}",
        "add rsp, 8",         // 調整を戻す
        
        // タイムアウトチェックを呼び出す
        "call {timeout_handler}",

        "mov rsp, rax",

        // 新しいタスクのレジスタを復元
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",

        "iretq",
        switch_handler = sym crate::process::handle_switch,
        timeout_handler = sym crate::timer::increment_tick,
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    naked_asm!(
        // 汎用レジスタを保存
        "push rax",
        "push rcx", 
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "sub rsp, 8",         // アライメント調整
        "mov rdi, rsp",         // 第1引数に現在のスタックポインタ
        "add rdi, 8",         // 引数には「元のContextの先頭」を渡す
        "call {syscall_handler}",
        "add rsp, 8",         // 調整を戻す

        // システムコールの結果はRAXにあるのでそのまま

        // レジスタを復元
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        // RAXは結果が入っているので最後に復元しない

        "iretq",
        syscall_handler = sym crate::syscall::rust_syscall_handler,
    );
}

// タイマー割り込みのEOIを送る関数
pub fn send_timer_eoi() {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame
) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}


#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}
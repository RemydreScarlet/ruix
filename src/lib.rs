#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)] // アロケータのエラーハンドラを使うために必要

#[macro_use]
// グラフィックドライバ
// TODO: 外部タスク化
pub mod vga_buffer;
// 割り込み
// TODO: 部分的な外部タスク化
pub mod interrupts;
pub mod gdt;
// メモリ管理
pub mod memory;
pub mod allocator;
// マルチタスク
pub mod task;
// システムコール
pub mod syscall;

pub mod process;


extern crate alloc;

pub fn init() {
    interrupts::init_idt();
    gdt::init();
    syscall::init();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

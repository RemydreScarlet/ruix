#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

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
// 統合タスク管理 (process + async task)
pub mod process;
// システムコール
pub mod syscall;
// タイマー
pub mod timer;
// シリアル通信
pub mod serial;
// IPC (プロセス間通信)
pub mod ipc;

// 新しいスケーラブルなサブシステム
pub mod error;
pub mod cpu;
pub mod testing;

// テストスイート
#[cfg(test)]
pub mod tests;

pub fn init() {
    serial::init(); // 最初にシリアルポートを初期化（デバッグ用）
    interrupts::init_idt();
    gdt::init();
    syscall::init();
    timer::init(); // タイマーを再有効化
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

#![no_std]
#![no_main]     // なんと、Rustカーネルを作るときはmain関数はだめらしい。
use core::panic::PanicInfo;
use ruix::println;
use bootloader::{BootInfo, entry_point};
use x86_64::structures::paging::Page;

extern crate alloc;
use alloc::{boxed::Box, vec, vec::Vec, rc::Rc};

// パニック時のハンドラらしい。カーネルを作るときはこれがないといけない。
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // やった！パニックを表示できた！
    println!("{}", _info);

    ruix::hlt_loop(); // ハルトループに入る
}

entry_point!(kernel_main);

use ruix::task::{Task, executor::Executor};
use ruix::task::keyboard;

#[unsafe(no_mangle)]
fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use ruix::memory::{self, BootInfoFrameAllocator};
    use ruix::allocator;
    use x86_64::{structures::paging::Translate, VirtAddr};
    println!("Starting Ruix {}", "0.1");
    ruix::init(); // 割り込みの初期化

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    
    
    let heap_value = Box::new(41);
    println!("heap_value at {:p}", heap_value);

    // マルチタスクのテスト
    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();

    println!("It did not crash!");

    ruix::hlt_loop(); // ハルトループに入る
}

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    println!("async number: {}", number);
}


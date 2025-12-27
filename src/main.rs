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
    use x86_64::{structures::paging::{Translate, FrameAllocator}, VirtAddr};
    println!("Starting Ruix {}", "0.1");
    ruix::init(); // 割り込みの初期化

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    
    
    // ユーザー空間の構築
    let user_code_addr = VirtAddr::new(0x400_000); // 4MB地点
    let code_page = Page::containing_address(user_code_addr);
    let code_frame = frame_allocator.allocate_frame().expect("no frames");

    memory::map_user_page(code_page, code_frame, &mut mapper, &mut frame_allocator);

    // ユーザーコードの書き込み
    // 物理メモリのオフセットを使って確保したフレームに直接書き込む
    // ゴリ押しってやつ
    unsafe {
        let virt = phys_mem_offset + code_frame.start_address().as_u64();
        let dest = virt.as_mut_ptr::<u8>();
        core::ptr::write_volatile(dest.add(0), 0x0F);
        core::ptr::write_volatile(dest.add(1), 0x05);
        core::ptr::write_volatile(dest.add(2), 0xEB);
        core::ptr::write_volatile(dest.add(3), 0xFE);
        // CPUの命令キャッシュやTLBをリフレッシュ
        x86_64::instructions::tlb::flush_all();
    }

    // スタック領域のマップ (0x600_000 = 6MiB地点)
    let user_stack_base = VirtAddr::new(0x600_000);
    let stack_page = Page::containing_address(user_stack_base);
    let stack_frame = frame_allocator.allocate_frame().expect("no frames for stack");
    
    memory::map_user_page(stack_page, stack_frame, &mut mapper, &mut frame_allocator);
    
    // スタックは高いアドレスから低いアドレスへ伸びるので、ページ末尾を指定
    let user_stack_top = user_stack_base + 4096u64;

    println!("Memory prepared. Jumping to Ring 3...");

    // ユーザーモードへジャンプ！
    unsafe {
        ruix::gdt::jump_to_user_mode(user_code_addr, user_stack_top);
    }

    // こっちは実行されないはず

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


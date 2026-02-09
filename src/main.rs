#![no_std]
#![no_main]     // なんと、Rustカーネルを作るときはmain関数はだめらしい。
use core::panic::PanicInfo;
use ruix::println;
use ruix::serial_println;
use bootloader::{BootInfo, entry_point};
use ruix::process::{Process, scheduler::SCHEDULER};

// 適当なユーザー用スタック領域（本来はメモリ管理が必要。まずはテスト用に）
static mut STACK1: [u8; 4096] = [0; 4096];
static mut STACK2: [u8; 4096] = [0; 4096];

use ruix::memory::BootInfoFrameAllocator;
use x86_64::{structures::paging::Page};
use x86_64::structures::paging::OffsetPageTable;

fn init_tasks(mapper: &mut OffsetPageTable, frame_allocator: &mut BootInfoFrameAllocator) {
    let mut sched = SCHEDULER.lock();
    
    // プロセス1: 無限ループの中で何か表示（システムコール経由など）
    let proc1 = Process::new(1, 0x400000, (&raw mut STACK1 as u64) + 4096, mapper, frame_allocator);
    
    // プロセス2: 別のエントリポイント
    let proc2 = Process::new(2, 0x500000, (&raw mut STACK2 as u64) + 4096, mapper, frame_allocator);

    sched.add_process(proc1);
    sched.add_process(proc2);
}

// パニック時のハンドラらしい。カーネルを作るときはこれがないといけない。
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // やった！パニックを表示できた！
    println!("{}", _info);

    ruix::hlt_loop(); // ハルトループに入る
}

entry_point!(kernel_main);


#[unsafe(no_mangle)]
fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use ruix::memory::{self, BootInfoFrameAllocator};
    use ruix::allocator;
    use x86_64::{structures::paging::FrameAllocator, VirtAddr};
    println!("Starting Ruix 0.1");
    
    // 割り込みの初期化（タイマーはまだ開始しない）
    ruix::interrupts::init_idt();
    ruix::gdt::init();
    ruix::syscall::init();

    // シリアルポートのテスト
    println!("Testing serial port...");
    ruix::serial::init();
    println!("Serial port test completed");

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    

    // ユーザー空間の構築 - プロセス作成より先に！
    let user_code_addr = VirtAddr::new(0x400_000); // 4MB地点
    let code_page = Page::containing_address(user_code_addr);
    let code_frame = frame_allocator.allocate_frame().expect("no frames");

    memory::map_user_page(code_page, code_frame, &mut mapper, &mut frame_allocator);
    
    // スタック領域のマップ (0x600_000 = 6MiB地点)
    let user_stack_base = VirtAddr::new(0x600_000);
    let stack_page = Page::containing_address(user_stack_base);
    let stack_frame = frame_allocator.allocate_frame().expect("no frames for stack");
    
    memory::map_user_page(stack_page, stack_frame, &mut mapper, &mut frame_allocator);
    
    // スタックは高いアドレスから低いアドレスへ伸びるので、ページ末尾を指定
    let user_stack_top = user_stack_base + 4096u64;

    // ユーザーコードの書き込み - 簡単な無限ループ
    unsafe {
        let virt = phys_mem_offset + code_frame.start_address().as_u64();
        let dest = virt.as_mut_ptr::<u8>();
        
        // 単純な無限ループ: jmp $ (-2 bytes)
        core::ptr::write_volatile(dest.add(0), 0xEB); // JMP rel8
        core::ptr::write_volatile(dest.add(1), 0xFE); // -2 (無限ループ)
    }

    // ユーザーコードの書き込み2
    let user_code_addr2 = VirtAddr::new(0x500_000);
    let code_page2 = Page::containing_address(user_code_addr2);
    let code_frame2 = frame_allocator.allocate_frame().expect("no frames");

    serial_println!("Mapping user code 2 at {:#x} to frame {:#x}", user_code_addr2.as_u64(), code_frame2.start_address().as_u64());
    memory::map_user_page(code_page2, code_frame2, &mut mapper, &mut frame_allocator);
    serial_println!("User code 2 mapped successfully");

    unsafe {
        let virt = phys_mem_offset + code_frame2.start_address().as_u64();
        let dest = virt.as_mut_ptr::<u8>();
        
        // プロセス2もシステムコールを呼ぶようにする
        // プロセス1とは少し違うメッセージにして区別
        
        // スタックに "Bye!" を書き込んでおく
        let stack_dest2 = (user_stack_top.as_u64() - 4) as *mut u8;
        core::ptr::write_volatile(stack_dest2, b'B');
        core::ptr::write_volatile(stack_dest2.add(1), b'y');
        core::ptr::write_volatile(stack_dest2.add(2), b'e');
        core::ptr::write_volatile(stack_dest2.add(3), b'!');
        
        // Process 2: IPCデモ - メッセージを受信して表示
        // mov rax, 4        ; sys_receive_message
        // mov rdi, 1        ; channel_id = 1 (プロセス1が作成したチャネル)
        // mov rsi, rsp      ; buffer_ptr = stack
        // sub rsi, 256      ; バッファ領域を確保 (RSP - 256)
        // mov rdx, 256      ; buffer_size = 256
        // syscall
        core::ptr::write_volatile(dest.add(59), 0x00); // imm32 = 256
        core::ptr::write_volatile(dest.add(60), 0x01);
        core::ptr::write_volatile(dest.add(61), 0x00);
        core::ptr::write_volatile(dest.add(62), 0x00);
        
        core::ptr::write_volatile(dest.add(63), 0x58); // POP rdx (message length)
        
        core::ptr::write_volatile(dest.add(64), 0x0F); // SYSCALL
        core::ptr::write_volatile(dest.add(65), 0x05);
        
        // no_msg label - print original "Bye!"
        core::ptr::write_volatile(dest.add(66), 0x48); // REX.W
        core::ptr::write_volatile(dest.add(67), 0xC7); // MOV rax, imm32
        core::ptr::write_volatile(dest.add(68), 0xC0); // /0, rax
        core::ptr::write_volatile(dest.add(69), 0x01); // imm32 = 1 (sys_write)
        core::ptr::write_volatile(dest.add(70), 0x00);
        core::ptr::write_volatile(dest.add(71), 0x00);
        core::ptr::write_volatile(dest.add(72), 0x00);
        
        core::ptr::write_volatile(dest.add(73), 0x48); // REX.W  
        core::ptr::write_volatile(dest.add(74), 0xC7); // MOV rdi, imm32
        core::ptr::write_volatile(dest.add(75), 0xC7); // /7, rdi
        core::ptr::write_volatile(dest.add(76), 0x01); // imm32 = 1 (stdout)
        core::ptr::write_volatile(dest.add(77), 0x00);
        core::ptr::write_volatile(dest.add(78), 0x00);
        core::ptr::write_volatile(dest.add(79), 0x00);
        
        core::ptr::write_volatile(dest.add(80), 0x48); // REX.W
        core::ptr::write_volatile(dest.add(81), 0x89); // MOV rsi, rsp
        core::ptr::write_volatile(dest.add(82), 0xE6); // /6, rsi
        
        core::ptr::write_volatile(dest.add(83), 0x48); // REX.W
        core::ptr::write_volatile(dest.add(84), 0xC7); // MOV rdx, imm32
        core::ptr::write_volatile(dest.add(85), 0xC2); // /2, rdx
        core::ptr::write_volatile(dest.add(86), 0x04); // imm32 = 4
        core::ptr::write_volatile(dest.add(87), 0x00);
        core::ptr::write_volatile(dest.add(88), 0x00);
        core::ptr::write_volatile(dest.add(89), 0x00);
        
        core::ptr::write_volatile(dest.add(90), 0x0F); // SYSCALL
        core::ptr::write_volatile(dest.add(91), 0x05);
        
        core::ptr::write_volatile(dest.add(92), 0xEB); // JMP -1 (infinite loop)
        core::ptr::write_volatile(dest.add(93), 0xFE);
        
        // CPUの命令キャッシュやTLBをリフレッシュ
        x86_64::instructions::tlb::flush_all();
    }

    // プロセス作成 - マッピング完了後に！
    init_tasks(&mut mapper, &mut frame_allocator);
    
    // タイマー開始（プロセスが準備できてから）- 一時的に無効化
    // ruix::timer::init();
    
    // ユーザーモードへのジャンプを一時的に無効化してテスト
    println!("Skipping user mode jump for testing...");
    
    // ユーザーモードへジャンプ！
    // unsafe {
    //     ruix::gdt::jump_to_user_mode(user_code_addr, user_stack_top);
    // }

    // こっちは実行されないはず

    // マルチタスクのテスト
    /*
    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();
    */

    println!("It did not crash!");

    ruix::hlt_loop(); // ハルトループに入る
}

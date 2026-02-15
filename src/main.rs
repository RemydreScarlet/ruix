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

    // ユーザーコードの書き込み - 簡単な無限ループテスト
    let adjusted_stack_top;
    unsafe {
        let virt = phys_mem_offset + code_frame.start_address().as_u64();
        let dest = virt.as_mut_ptr::<u8>();
        
        // 単純な無限ループ: jmp $ (-2 bytes)
        core::ptr::write_volatile(dest.add(0), 0xEB); // JMP rel8
        core::ptr::write_volatile(dest.add(1), 0xFE); // -2 (無限ループ)
        
        // スタックポインタを調整しない（単純テストのため）
        adjusted_stack_top = user_stack_top;
    }

    // Load and run syscall test program
    let user_code_addr2 = VirtAddr::new(0x500_000);
    let code_page2 = Page::containing_address(user_code_addr2);
    let code_frame2 = frame_allocator.allocate_frame().expect("no frames");
    
    serial_println!("Loading syscall test at {:#x} to frame {:#x}", user_code_addr2.as_u64(), code_frame2.start_address().as_u64());
    memory::map_user_page(code_page2, code_frame2, &mut mapper, &mut frame_allocator);
    serial_println!("Syscall test loaded successfully");
    
    unsafe {
        let virt = phys_mem_offset + code_frame2.start_address().as_u64();
        let dest = virt.as_mut_ptr::<u8>();
        
        // Simple syscall test: call getpid and print result
        // mov rax, 39        ; getpid
        // syscall
        core::ptr::write_volatile(dest.add(0), 0x48); // REX.W
        core::ptr::write_volatile(dest.add(1), 0xC7); // MOV rax, imm32
        core::ptr::write_volatile(dest.add(2), 0x27); // /39, rax
        core::ptr::write_volatile(dest.add(3), 0x00);
        core::ptr::write_volatile(dest.add(4), 0x00);
        core::ptr::write_volatile(dest.add(5), 0x00);
        core::ptr::write_volatile(dest.add(6), 0x00);
        core::ptr::write_volatile(dest.add(7), 0x00);
        
        // syscall
        core::ptr::write_volatile(dest.add(8), 0x0F); // SYSCALL
        core::ptr::write_volatile(dest.add(9), 0x05);
        
        // Print result - simple approach
        // mov rdi, 1        ; stdout
        // mov rsi, rsp      ; buffer
        // mov rdx, 20      ; length
        // syscall
        core::ptr::write_volatile(dest.add(10), 0x48); // REX.W
        core::ptr::write_volatile(dest.add(11), 0xC7); // MOV rax, imm32
        core::ptr::write_volatile(dest.add(12), 0x27); // /1, rax
        core::ptr::write_volatile(dest.add(13), 0x00);
        core::ptr::write_volatile(dest.add(14), 0x00);
        core::ptr::write_volatile(dest.add(15), 0x00);
        core::ptr::write_volatile(dest.add(16), 0x00);
        core::ptr::write_volatile(dest.add(17), 0x00);
        core::ptr::write_volatile(dest.add(18), 0x00);
        core::ptr::write_volatile(dest.add(19), 0x00);
        
        // jmp $ (infinite loop)
        core::ptr::write_volatile(dest.add(20), 0xEB); // JMP rel8
        core::ptr::write_volatile(dest.add(21), 0xFE); // -2 (無限ループ）
    }

    // プロセス作成 - マッピング完了後に！
    init_tasks(&mut mapper, &mut frame_allocator);
    
    // タイマー開始（プロセスが準備できてから）
    ruix::timer::init();
    
    println!("Preparing to jump to user mode...");
    println!("User code address: {:#x}, Stack top: {:#x}", user_code_addr.as_u64(), adjusted_stack_top.as_u64());
    
    // タイムアウト付きでユーザーモードジャンプをテスト
    println!("Attempting user mode jump with timeout protection...");
    
    // タイムアウトカウンタをリセット
    // 新しいAPI: プロセスを登録してユーザーモードを開始
    // 現在のプロセスIDを取得（テスト用に仮のID）
    let test_pid = 1u64;
    ruix::timer::register_process(test_pid, Some(30)); // 30 ticks = 3 seconds
    ruix::timer::start_user_mode(test_pid);
    
    // まずはカーネルモードでテスト
    println!("Testing in kernel mode first...");
    unsafe {
        // ユーザーコードをカーネルモードで実行テスト
        let virt = phys_mem_offset + code_frame.start_address().as_u64();
        let func_ptr: fn() = core::mem::transmute(virt.as_u64());
        println!("Executing user code in kernel mode at {:#x}...", virt.as_u64());
        func_ptr();
        println!("User code executed successfully in kernel mode!");
    }
    
    unsafe {
        // ユーザーモードへジャンプ！
        println!("Now jumping to user mode...");
        ruix::gdt::jump_to_user_mode(user_code_addr, adjusted_stack_top);
    }

    // こっちは実行されないはず
    println!("ERROR: Returned from user mode jump!");

    ruix::hlt_loop(); // ハルトループに入る
}

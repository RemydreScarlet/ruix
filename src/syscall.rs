use x86_64::registers::model_specific::{LStar, Star, SFMask, KernelGsBase};
use x86_64::structures::gdt::SegmentSelector;
use x86_64::registers::rflags::RFlags;
use crate::gdt;
use core::arch::naked_asm;

// セキュリティ：安全なシステムコール引数解析
#[derive(Debug)]
pub struct SyscallArgs {
    pub syscall_number: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
    pub arg6: u64,
}

impl SyscallArgs {
    // 安全な引数解析関数
    pub fn safe_parse(stack_ptr: u64) -> Result<Self, SyscallError> {
        // スタックポインタの検証
        if !validate_stack_pointer(stack_ptr) {
            return Err(SyscallError::InvalidStackPointer);
        }

        // システムコール番号と引数を安全に取得
        let syscall_number = safe_read_register(stack_ptr, -1)?; // RAX
        let arg1 = safe_read_register(stack_ptr, 2)?;  // RDI
        let arg2 = safe_read_register(stack_ptr, 4)?;  // RSI
        let arg3 = safe_read_register(stack_ptr, 5)?;  // RDX
        let arg4 = safe_read_register(stack_ptr, 6)?;  // R10
        let arg5 = safe_read_register(stack_ptr, 7)?;  // R8
        let arg6 = safe_read_register(stack_ptr, 8)?;  // R9

        // システムコール番号の検証
        if !validate_syscall_number(syscall_number) {
            return Err(SyscallError::InvalidSyscallNumber(syscall_number));
        }

        crate::println!("SECURITY: Syscall args parsed safely for syscall {}", syscall_number);

        Ok(SyscallArgs {
            syscall_number,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
        })
    }
}

// セキュリティエラー型
#[derive(Debug)]
pub enum SyscallError {
    InvalidStackPointer,
    InvalidSyscallNumber(u64),
    InvalidPointer(u64),
    AccessViolation(u64),
    BoundsExceeded,
}

// スタックポインタを検証
fn validate_stack_pointer(stack_ptr: u64) -> bool {
    // スタックポインタが合理的な範囲にあることを確認
    // カーネルスタックは通常0xFFFF_8000_0000_0000以降
    if stack_ptr < 0xFFFF_8000_0000_0000 || stack_ptr > 0xFFFF_FFFF_FFFF_FFFF {
        crate::println!("SECURITY: Invalid stack pointer: {:#x}", stack_ptr);
        return false;
    }
    
    // 16バイト境界にアライメントされていることを確認
    if stack_ptr % 16 != 0 {
        crate::println!("SECURITY: Stack pointer not aligned: {:#x}", stack_ptr);
        return false;
    }
    
    true
}

// システムコール番号を検証
fn validate_syscall_number(syscall_number: u64) -> bool {
    // サポートされているシステムコール番号の範囲チェック
    match syscall_number {
        0 | 1 | 2 | 3 | 4 | 24 | 39 | 57 | 61 => {
            crate::println!("SECURITY: Valid syscall number: {}", syscall_number);
            true
        }
        _ => {
            crate::println!("SECURITY: Invalid syscall number: {}", syscall_number);
            false
        }
    }
}

// 安全なレジスタ読み取り
fn safe_read_register(stack_ptr: u64, offset: isize) -> Result<u64, SyscallError> {
    // メモリアクセスの境界チェック
    let addr = stack_ptr as isize + (offset * 8) as isize;
    if addr < 0 || addr > 0x7FFF_FFFF_FFFF_FFFF {
        return Err(SyscallError::InvalidPointer(addr as u64));
    }

    // 安全なメモリ読み取り
    let ptr = unsafe { (stack_ptr as *const u64).offset(offset) };
    let value = unsafe { *ptr };
    
    // 値の合理性チェック
    if value == 0xDEAD_BEEF_DEAD_BEEF {
        return Err(SyscallError::AccessViolation(value));
    }
    
    Ok(value)
}

// ポインタ引数を安全に検証
pub fn validate_user_pointer(ptr: u64, size: usize) -> Result<(), SyscallError> {
    // ヌルポインタチェック
    if ptr == 0 {
        return Err(SyscallError::InvalidPointer(0));
    }

    // ユーザー空間の範囲チェック
    if ptr < 0x400_000 || ptr > 0x7FFF_FFFF {
        return Err(SyscallError::InvalidPointer(ptr));
    }

    // バッファサイズのチェック
    if size > 0x1000 { // 4KB制限
        return Err(SyscallError::BoundsExceeded);
    }

    // バッファがユーザー空間内に収まることを確認
    let end_addr = ptr.checked_add(size as u64)
        .ok_or(SyscallError::BoundsExceeded)?;
    
    if end_addr > 0x7FFF_FFFF {
        return Err(SyscallError::BoundsExceeded);
    }

    crate::println!("SECURITY: User pointer validated: {:#x} (size: {})", ptr, size);
    Ok(())
}

#[repr(C)]
pub struct CpuData {
    // SYSCALL時にユーザーのRSPを一時退避する場所 (offset 0)
    pub user_rsp: u64,
    // このCPU用のカーネルスタックのトップ (offset 8)
    pub kernel_stack_top: u64,
    // 現在実行中のプロセスID (offset 16)
    pub current_process_id: u64,
    // TSSへのポインタ（将来的な割り込み処理用） (offset 24)
    pub tss_ptr: u64,
}

// 起動時はゼロで初期化。
// Lazy Staticの使い方が飛んだので許してください
pub static mut CPU_DATA: CpuData = CpuData {
    user_rsp: 0,
    kernel_stack_top: 0,
    current_process_id: 0,
    tss_ptr: 0,
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
        LStar::write(x86_64::VirtAddr::new(asm_syscall_handler as *const () as u64));

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
        "call {rust_handler}",  // Rustハンドラを呼び出し、RAXに結果が返る
        // 結果はすでにRAXにある

        "add rsp, 8",           // 調整を戻す

        "pop rcx",
        "pop r11",
        
        "mov rsp, gs:[0]",      // ユーザーRSP復元
        "swapgs",
        "sysretq",
        rust_handler = sym rust_syscall_handler,
    );
}

// Rust側のシステムコール処理ロジック
pub extern "C" fn rust_syscall_handler(stack_ptr: u64) -> u64 {
    // 現在のプロセスIDを取得（デバッグ用）
    let current_pid = unsafe { CPU_DATA.current_process_id };
    
    // セキュリティ：安全な引数解析を使用
    let args = match SyscallArgs::safe_parse(stack_ptr) {
        Ok(args) => args,
        Err(err) => {
            crate::println!("SECURITY ERROR: Syscall argument parsing failed: {:?}", err);
            return -1i64 as u64; // エラーコードを返す
        }
    };
    
    crate::println!("SECURITY: Safe syscall processing for PID {}, syscall {}", current_pid, args.syscall_number);
    
    let result = match args.syscall_number {
        39 => {
            // getpid: Return current process ID
            crate::println!("Syscall: getpid from PID {}", current_pid);
            current_pid as i64
        }
        57 => {
            // fork: Create child process
            crate::println!("Syscall: fork from PID {}", current_pid);
            
            use crate::process::scheduler::SCHEDULER;
            
            let mut sched = SCHEDULER.lock();
            if let Some(parent_process) = sched.processes.front() {
                if parent_process.id == current_pid {
                    // 安全なPID割り当て
                    let child_pid = crate::process::allocate_pid();
                    
                    // 子プロセスを親プロセスのchildrenリストに追加
                    // スケジューラーロック内で安全に操作
                    for process in &mut sched.processes {
                        if process.id == current_pid {
                            process.children.push(child_pid);
                            break;
                        }
                    }
                    
                    crate::println!("Fork: Parent {} created child {}", current_pid, child_pid);
                    child_pid as i64 // Parent returns child PID
                } else {
                    -1i64 // Error: process not found
                }
            } else {
                -1i64 // Error: no current process
            }
        }
        61 => {
            // wait4: Wait for child process to exit
            // Arguments: RDI=pid, RSI=status_ptr, RDX=options, R10=ru_ptr
            let target_pid = args.arg1;
            let status_ptr = args.arg2;
            let options = args.arg3;
            
            // セキュリティ：引数の検証
            if target_pid != -1i64 as u64 && (target_pid < 1000 || target_pid > 10000) {
                crate::println!("SECURITY: Invalid target PID for wait4: {}", target_pid);
                return -1i64 as u64;
            }
            
            // セキュリティ：status_ptrの検証
            if status_ptr != 0 {
                if let Err(err) = validate_user_pointer(status_ptr, 4) { // i32 = 4 bytes
                    crate::println!("SECURITY: Invalid status pointer in wait4: {:?}", err);
                    return -1i64 as u64;
                }
            }
            
            // セキュリティ：optionsの検証（現在は0のみサポート）
            if options != 0 {
                crate::println!("SECURITY: Unsupported wait4 options: {}", options);
                return -1i64 as u64;
            }
            
            crate::println!("Syscall: wait4 from PID {}, waiting for {}", current_pid, target_pid);
            
            use crate::process::scheduler::SCHEDULER;
            use crate::process::{ProcessState, WaitReason};
            
            let mut sched = SCHEDULER.lock();
            
            // Find zombie children
            let mut zombie_child = None;
            for process in &sched.processes {
                if process.parent_id == current_pid && process.state == ProcessState::Zombie {
                    if target_pid == -1i64 as u64 || process.id == target_pid {
                        zombie_child = Some(process.id);
                        break;
                    }
                }
            }
            
            if let Some(child_pid) = zombie_child {
                // Remove zombie child and get exit code
                let mut exit_code = 0;
                sched.processes.retain(|p| {
                    if p.id == child_pid {
                        exit_code = p.exit_code;
                        false // Remove from scheduler
                    } else {
                        true // Keep in scheduler
                    }
                });
                
                // Write exit code to status pointer if provided
                if status_ptr != 0 {
                    unsafe {
                        *(status_ptr as *mut i32) = exit_code;
                    }
                }
                
                crate::println!("Wait4: PID {} reaped child {} with exit code {}", current_pid, child_pid, exit_code);
                child_pid as i64
            } else {
                // No zombie children available - properly block current process
                if let Some(current_process) = sched.processes.front_mut() {
                    if current_process.id == current_pid {
                        current_process.state = ProcessState::Waiting(WaitReason::Child(target_pid));
                        crate::println!("Wait4: PID {} blocking for child {}", current_pid, target_pid);
                        
                        // スケジューラーでブロックされたプロセスを最後に移動
                        sched.processes.rotate_left(1);
                        
                        // 次のラウンドで再スケジュール
                        if let Some(next_process) = sched.processes.front() {
                            unsafe {
                                crate::syscall::CPU_DATA.current_process_id = next_process.id;
                            }
                            // CR3レジスタを新しいプロセスのページテーブルに切り替え
                            unsafe {
                                x86_64::registers::control::Cr3::write(next_process.page_table_frame, x86_64::registers::control::Cr3Flags::empty());
                            }
                            return next_process.context_ptr;
                        }
                    }
                }
                
                // 緊急時のフォールバック
                crate::println!("Wait4: PID {} - blocking implementation failed", current_pid);
                -1i64 // Block (would normally resume when child exits)
            }
        }
        24 => {
            // sched_yield: Yield CPU to another process
            crate::println!("Syscall: sched_yield from PID {}", current_pid);
            
            // Set current process state to Ready and trigger scheduling
            use crate::process::scheduler::SCHEDULER;
            use crate::process::ProcessState;
            
            let mut sched = SCHEDULER.lock();
            if let Some(process) = sched.processes.front_mut() {
                if process.id == current_pid {
                    process.state = ProcessState::Ready;
                }
            }
            
            0i64 // Success
        }
        0 => {
            // sys_exit: プロセス終了
            // Arguments: RDI=exit_code
            let exit_code = args.arg1 as i32;
            
            // セキュリティ：終了コードの検証
            if exit_code < -255 || exit_code > 255 {
                crate::println!("SECURITY: Invalid exit code: {}", exit_code);
                return -1i64 as u64;
            }
            
            crate::println!("Syscall: sys_exit from PID {} with code {}", current_pid, exit_code);
            
            use crate::process::scheduler::SCHEDULER;
            use crate::process::{ProcessState, WaitReason};
            
            let mut sched = SCHEDULER.lock();
            
            // Find the exiting process and set it to Zombie state
            for process in &mut sched.processes {
                if process.id == current_pid {
                    process.state = ProcessState::Zombie;
                    process.exit_code = exit_code;
                    crate::println!("Process {} entered Zombie state with exit code {}", current_pid, exit_code);
                    break;
                }
            }
            
            // Wake up parent if it's waiting for this child
            for process in &mut sched.processes {
                if let ProcessState::Waiting(WaitReason::Child(waiting_pid)) = process.state {
                    if waiting_pid == current_pid || waiting_pid == -1i64 as u64 {
                        process.state = ProcessState::Ready;
                        crate::println!("Woke up parent {} from waiting for child {}", process.id, current_pid);
                    }
                }
            }
            
            // Don't remove from scheduler immediately - let parent reap it
            // Current process ID will be reset by scheduler on next context switch
            unsafe { crate::syscall::CPU_DATA.current_process_id = 0; }
            
            0 // 成功を示す戻り値
        }
        1 => {
            // sys_write: デバッグ出力
            // 引数: RDI=fd, RSI=buf*, RDX=count
            let fd = args.arg1;
            let buf = args.arg2;
            let count = args.arg3;
            
            // セキュリティ：引数の検証
            if fd > 2 { // stdin(0), stdout(1), stderr(2) のみ許可
                crate::println!("SECURITY: Invalid file descriptor: {}", fd);
                return -1i64 as u64;
            }
            
            if count > 4096 { // 4KB制限
                crate::println!("SECURITY: Write count too large: {}", count);
                return -1i64 as u64;
            }
            
            // セキュリティ：ユーザーポインタの検証
            if let Err(err) = validate_user_pointer(buf, count as usize) {
                crate::println!("SECURITY: Invalid user pointer in write: {:?}", err);
                return -1i64 as u64;
            }
            
            if fd == 1 { // stdout
                // 簡単な文字列出力（バッファから文字列を取得）
                let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, count as usize) };
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::print!("{}", s);
                    count as i64 // 書き込んだバイト数を返す
                } else {
                    -1i64 // エラー
                }
            } else {
                -1i64 // 無効なファイルディスクリプタ
            }
        }
        2 => {
            // sys_create_channel: IPCチャネルの作成
            // 引数: RDI=target_pid
            let target_pid = args.arg1;
            
            // セキュリティ：target_pidの検証
            if target_pid == 0 || target_pid > 10000 {
                crate::println!("SECURITY: Invalid target PID for create_channel: {}", target_pid);
                return -1i64 as u64;
            }
            
            crate::println!("IPC: Process {} creating channel with {}", current_pid, target_pid);
            match crate::ipc::syscalls::create_channel(target_pid) {
                Ok(channel_id) => {
                    crate::println!("IPC: Channel {} created successfully", channel_id);
                    channel_id as i64
                }
                Err(_) => {
                    crate::println!("IPC: Channel creation failed");
                    -1i64 // エラー
                }
            }
        }
        3 => {
            // sys_send_message: メッセージ送信
            // 引数: RDI=channel_id, RSI=msg_type, RDX=data_ptr, R10=data_len
            let channel_id = args.arg1;
            let msg_type = args.arg2 as u32;
            let data_ptr = args.arg3;
            let data_len = args.arg4;
            
            // セキュリティ：引数の検証
            if channel_id == 0 || channel_id > 1000 {
                crate::println!("SECURITY: Invalid channel ID for send_message: {}", channel_id);
                return -1i64 as u64;
            }
            
            if data_len > 256 { // IPCメッセージの最大サイズ
                crate::println!("SECURITY: Message too large: {}", data_len);
                return -1i64 as u64;
            }
            
            // セキュリティ：データポインタの検証
            if data_ptr != 0 {
                if let Err(err) = validate_user_pointer(data_ptr, data_len as usize) {
                    crate::println!("SECURITY: Invalid data pointer in send_message: {:?}", err);
                    return -1i64 as u64;
                }
            }
            
            let data_slice = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len as usize) };
            
            crate::println!("IPC: Process {} sending message to channel {}, type {}, len {}", current_pid, channel_id, msg_type, data_len);
            match crate::ipc::syscalls::send_message(channel_id, msg_type, data_slice) {
                Ok(_) => {
                    crate::println!("IPC: Message sent successfully");
                    0i64 // 成功
                }
                Err(_) => {
                    crate::println!("IPC: Message send failed");
                    -1i64 // エラー
                }
            }
        }
        4 => {
            // sys_receive_message: メッセージ受信
            // 引数: RDI=channel_id, RSI=buffer_ptr, RDX=buffer_size
            // 戻り値: 受信したメッセージのサイズ、または-1（エラー）、または-2（メッセージなし）
            let channel_id = args.arg1;
            let buffer_ptr = args.arg2;
            let buffer_size = args.arg3;
            
            // セキュリティ：引数の検証
            if channel_id == 0 || channel_id > 1000 {
                crate::println!("SECURITY: Invalid channel ID for receive_message: {}", channel_id);
                return -1i64 as u64;
            }
            
            if buffer_size > 256 { // IPCメッセージの最大サイズ
                crate::println!("SECURITY: Buffer too large: {}", buffer_size);
                return -1i64 as u64;
            }
            
            // セキュリティ：バッファポインタの検証
            if buffer_ptr != 0 {
                if let Err(err) = validate_user_pointer(buffer_ptr, buffer_size as usize) {
                    crate::println!("SECURITY: Invalid buffer pointer in receive_message: {:?}", err);
                    return -1i64 as u64;
                }
            }
            
            crate::println!("IPC: Process {} receiving message from channel {}", current_pid, channel_id);
            match crate::ipc::syscalls::receive_message(channel_id) {
                Ok(Some(message)) => {
                    // メッセージを受信したらバッファにコピー
                    let copy_len = core::cmp::min(message.data_len, buffer_size as usize);
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            message.data.as_ptr(),
                            buffer_ptr as *mut u8,
                            copy_len
                        );
                    }
                    crate::println!("IPC: Message received, len {}", copy_len);
                    copy_len as i64 // コピーしたバイト数を返す
                }
                Ok(None) => {
                    crate::println!("IPC: No message available");
                    -2i64 // メッセージなし
                }
                Err(_) => {
                    crate::println!("IPC: Message receive failed");
                    -1i64 // エラー
                }
            }
        }
        _ => {
            crate::println!("Unknown syscall {} from PID {}", args.syscall_number, current_pid);
            -1i64 // エラー
        }
    };

    // 結果をu64として返す（負の値は符号拡張される）
    result as u64
}
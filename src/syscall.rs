use x86_64::registers::model_specific::{LStar, Star, SFMask, KernelGsBase};
use x86_64::structures::gdt::SegmentSelector;
use x86_64::registers::rflags::RFlags;
use crate::gdt;
use core::arch::naked_asm;

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
    // SYSCALL命令の後、スタックレイアウトは以下の通り：
    // [RSP+8]  = RCX (戻りアドレス)
    // [RSP+16] = R11 (保存されたRFLAGS)  
    // [RSP+24] = RDI (第1引数)
    // [RSP+32] = RSI (第2引数)
    // [RSP+40] = RDX (第3引数)
    // [RSP+48] = R10 (第4引数)
    // [RSP+56] = R8  (第5引数)
    // [RSP+64] = R9  (第6引数)
    // RAXレジスタにはシステムコール番号が入っている
    
    // RAXからシステムコール番号を取得
    let syscall_number = unsafe { *((stack_ptr as *const u64).offset(-1)) }; // RAX
    
    // 現在のプロセスIDを取得（デバッグ用）
    let current_pid = unsafe { CPU_DATA.current_process_id };
    
    // デバッグ出力：生のレジスタ値を表示
    crate::println!("DEBUG: Syscall from PID {}, RAX={}", current_pid, syscall_number);
    
    let result = match syscall_number {
        0 => {
            // sys_exit: プロセス終了
            crate::println!("Syscall: sys_exit from PID {}", current_pid);
            
            // プロセスをスケジューラから削除
            use crate::process::scheduler::SCHEDULER;
            let mut sched = SCHEDULER.lock();
            sched.processes.retain(|p| p.id != current_pid);
            
            // 現在のプロセスIDを0にリセット
            unsafe { crate::syscall::CPU_DATA.current_process_id = 0; }
            
            0 // 成功を示す戻り値
        }
        1 => {
            // sys_write: デバッグ出力
            // 引数: RDI=fd, RSI=buf*, RDX=count
            let fd = unsafe { *((stack_ptr as *const u64).offset(2)) }; // RDI
            let buf = unsafe { *((stack_ptr as *const u64).offset(4)) }; // RSI  
            let count = unsafe { *((stack_ptr as *const u64).offset(5)) }; // RDX
            
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
            let target_pid = unsafe { *((stack_ptr as *const u64).offset(2)) }; // RDI
            
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
            let channel_id = unsafe { *((stack_ptr as *const u64).offset(2)) }; // RDI
            let msg_type = unsafe { *((stack_ptr as *const u64).offset(4)) } as u32; // RSI
            let data_ptr = unsafe { *((stack_ptr as *const u64).offset(5)) }; // RDX
            let data_len = unsafe { *((stack_ptr as *const u64).offset(6)) }; // R10
            
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
            let channel_id = unsafe { *((stack_ptr as *const u64).offset(2)) }; // RDI
            let buffer_ptr = unsafe { *((stack_ptr as *const u64).offset(4)) }; // RSI
            let buffer_size = unsafe { *((stack_ptr as *const u64).offset(5)) }; // RDX
            
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
            crate::println!("Unknown syscall {} from PID {}", syscall_number, current_pid);
            -1i64 // エラー
        }
    };

    // 結果をu64として返す（負の値は符号拡張される）
    result as u64
}
use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor};
use x86_64::structures::gdt::SegmentSelector;

use lazy_static::lazy_static;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        // Ring 3 -> Ring0 遷移スタック
        tss.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            stack_start + STACK_SIZE
        };

        // スタックオーバーフローやダブルフォルトなどの例外処理用にスタックを設定
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            let stack_end = stack_start + STACK_SIZE;
            stack_end
        };
        tss
    };
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        // カーネルモード用のセグメント
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());

        // ユーザーモード用のセグメント
        // Data Segmentはスタックやヒープに使用
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());

        // TSS
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));
        (gdt, Selectors {
            code_selector,
            data_selector,
            user_code_selector,
            user_data_selector,
            tss_selector,
        })
    };
}

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

// カーネル特権スタックの最上部アドレスを返す。
pub fn kernel_stack_top() -> VirtAddr {
    // TSS.privilege_stack_table[0] を返す
    TSS.privilege_stack_table[0]
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, SS, Segment};

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        SS::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }
}

pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}

// セキュリティ関数：ユーザーモード遷移パラメータを検証
fn validate_user_mode_transition(code_addr: VirtAddr, stack_addr: VirtAddr) -> bool {
    // コードアドレスの検証
    let code_addr_raw = code_addr.as_u64();
    
    // ユーザー空間の範囲チェック（0x400_000 - 0x7FFF_FFFF）
    if code_addr_raw < 0x400_000 || code_addr_raw > 0x7FFF_FFFF {
        crate::println!("SECURITY: Code address {:#x} outside user space range", code_addr_raw);
        return false;
    }
    
    // スタックアドレスの検証
    let stack_addr_raw = stack_addr.as_u64();
    
    // ユーザースタックの範囲チェック（0x600_000 - 0x7FFF_FFFF）
    if stack_addr_raw < 0x600_000 || stack_addr_raw > 0x7FFF_FFFF {
        crate::println!("SECURITY: Stack address {:#x} outside user space range", stack_addr_raw);
        return false;
    }
    
    // アライメントチェック（16バイト境界）
    if code_addr_raw % 16 != 0 || stack_addr_raw % 16 != 0 {
        crate::println!("SECURITY: Addresses not properly aligned");
        return false;
    }
    
    // スタックがコード領域と重ならないことを確認
    let stack_bottom = stack_addr_raw - 4096; // 4KBスタックを仮定
    if stack_bottom <= code_addr_raw && code_addr_raw <= stack_addr_raw {
        crate::println!("SECURITY: Stack and code regions overlap");
        return false;
    }
    
    crate::println!("SECURITY: User mode transition validation passed");
    true
}

// セキュリティ関数：現在の特権レベルを検証
fn validate_current_privilege_level() {
    // 簡略化された特権レベル検証
    // TODO: 完全な特権レベルチェックを実装する
    // 現在はデバッグ出力のみで、実際の検証は後で追加
    
    crate::println!("SECURITY: Privilege level validation (simplified)");
    crate::println!("SECURITY: Assuming kernel mode for now");
}

// セキュリティ関数：スタック保護を設定
fn setup_stack_protection(stack_addr: VirtAddr) {
    let stack_top = stack_addr.as_u64();
    let stack_bottom = stack_top - 4096; // 4KBスタック
    
    // スタックの境界を記録（将来的な保護のため）
    crate::println!("SECURITY: Stack protection setup");
    crate::println!("  Stack range: {:#x} - {:#x}", stack_bottom, stack_top);
    
    // 実際のスタック保護ページを設定
    // TODO: ページテーブル操作が必要で、現在はデバッグ出力のみ
    // 将来的にはguardページを設定してスタックオーバーフローを検出
    
    // スタックオーバーフロー検出のための境界チェックを強化
    if stack_bottom < 0x400_000 {
        crate::println!("SECURITY: Stack bottom too low - potential corruption");
    }
}

// セキュリティ関数：セキュアなRFLAGSを設定
fn setup_secure_rflags() -> u64 {
    // 基本RFLAGS設定
    let mut rflags = 0x202u64; // 割り込み有効フラグ
    
    // セキュリティ関連のフラグを設定
    rflags |= 0x1; // Carry Flag (クリア)
    rflags &= !(0x4); // Parity Flag (クリア)
    rflags &= !(0x8); // Auxiliary Carry Flag (クリア)
    rflags &= !(0x10); // Zero Flag (クリア)
    rflags &= !(0x20); // Sign Flag (クリア)
    
    // IOPLフィールドをクリア（ユーザーモードでのIO操作を制限）
    rflags &= !(0x3000); // IOPL = 0
    
    crate::println!("SECURITY: Secure RFLAGS configured: {:#x}", rflags);
    rflags
}

// ユーザーモード突入
pub unsafe fn jump_to_user_mode(code_addr: VirtAddr, stack_addr: VirtAddr) -> ! {
    // セキュリティ検証を実行
    if !validate_user_mode_transition(code_addr, stack_addr) {
        crate::println!("SECURITY ERROR: Invalid user mode transition parameters");
        crate::println!("  code_addr: {:#x}, stack_addr: {:#x}", code_addr.as_u64(), stack_addr.as_u64());
        panic!("Security violation in user mode transition");
    }

    let selectors = get_selectors();

    // セレクタに特権レベル3（RPL=3）を設定
    let data_selector = (selectors.user_data_selector.0 | 3) as u64;
    let code_selector = (selectors.user_code_selector.0 | 3) as u64;

    // デバッグ出力
    crate::println!("GDT Selectors:");
    crate::println!("  user_code_selector: {} (with RPL3: {})", selectors.user_code_selector.0, code_selector);
    crate::println!("  user_data_selector: {} (with RPL3: {})", selectors.user_data_selector.0, data_selector);
    crate::println!("Jump targets:");
    crate::println!("  code_addr: {:#x}", code_addr.as_u64());
    crate::println!("  stack_addr: {:#x}", stack_addr.as_u64());
    
    // セキュリティ：現在の特権レベルを検証
    validate_current_privilege_level();
    
    // セキュリティ：スタック保護を設定
    setup_stack_protection(stack_addr);
    
    // 割り込みを無効化して安全にジャンプ
    x86_64::instructions::interrupts::disable();
    crate::println!("Interrupts disabled for user mode jump");
    
    // ユーザーモード開始をタイマーに通知
    // 現在のプロセスIDを取得して渡す
    let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };
    crate::timer::start_user_mode(current_pid);

    unsafe {
        // セキュアなRFLAGSを設定
        let secure_rflags = setup_secure_rflags();
        
        crate::println!("Using IRETQ for secure user mode transition...");
        crate::println!("Setting up stack frame for IRETQ:");
        crate::println!("  RIP (code_addr): {:#x}", code_addr.as_u64());
        crate::println!("  RSP (stack_addr): {:#x}", stack_addr.as_u64());
        crate::println!("  RFLAGS: {:#x}", secure_rflags);
        crate::println!("  CS: {} (with RPL3: {})", selectors.user_code_selector.0, code_selector);
        crate::println!("  SS: {} (with RPL3: {})", selectors.user_data_selector.0, data_selector);
        
        // IRETQ用のスタックフレームを手動で構築してジャンプ
        // 順番: SS, RSP, RFLAGS, CS, RIP
        core::arch::asm!(
            "push {stack_sel}",      // SS
            "push {stack_ptr}",      // RSP
            "push {rflags}",         // RFLAGS
            "push {code_sel}",       // CS
            "push {instruction_ptr}", // RIP
            "iretq",                // IRETQでユーザーモードへ
            stack_sel = in(reg) data_selector,
            stack_ptr = in(reg) stack_addr.as_u64(),
            rflags = in(reg) secure_rflags,
            code_sel = in(reg) code_selector,
            instruction_ptr = in(reg) code_addr.as_u64(),
            options(noreturn)
        );
    }
}
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
    use x86_64::instructions::segmentation::set_cs;
    use x86_64::instructions::tables::load_tss;
    use x86_64::registers::segmentation::{SS, Segment};

    GDT.0.load();
    unsafe {
        set_cs(GDT.1.code_selector);
        SS::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }
}

pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}

// ユーザーモード突入
pub unsafe fn jump_to_user_mode(code_addr: VirtAddr, stack_addr: VirtAddr) -> ! {
    use x86_64::instructions::segmentation::{CS, Segment};
    use core::arch::asm;

    let selectors = get_selectors();

    // セレクタに特権レベル3（RPL=3）を設定
    let data_selector = (selectors.user_data_selector.0 | 3) as u64;
    let code_selector = (selectors.user_code_selector.0 | 3) as u64;

    unsafe {
        // IRETQ 用のスタックフレームを手動で構築してジャンプ
        // 順番: SS, RSP, RFLAGS, CS, RIP
        asm!(
            "push {stack_sel}",
            "push {stack_ptr}",
            "push 0x202", // RFLAGS (Interrupt Enableフラグを立てる)
            "push {code_sel}",
            "push {instruction_ptr}",
            "iretq",
            stack_sel = in(reg) data_selector,
            stack_ptr = in(reg) stack_addr.as_u64(),
            code_sel = in(reg) code_selector,
            instruction_ptr = in(reg) code_addr.as_u64(),
            options(noreturn)
        );
    }
}
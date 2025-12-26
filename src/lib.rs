#![no_std]
#![feature(abi_x86_interrupt)]
#[macro_use]
pub mod vga_buffer;
pub mod interrupts;
pub mod gdt;

pub fn init() {
    interrupts::init_idt();
    gdt::init();
}
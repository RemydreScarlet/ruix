#!/usr/bin/env rust

#![no_std]
#![no_main]
#![feature(asm)]

use core::panic::PanicInfo;
use core::arch::asm;

// Syscall numbers
const SYS_GETPID: u64 = 39;
const SYS_FORK: u64 = 57;
const SYS_WAIT4: u64 = 61;
const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    test_syscalls();
    exit(0);
}

fn test_syscalls() {
    // Test getpid
    let pid = getpid();
    print_string("My PID: ");
    print_int(pid);
    print_char('\n');
    
    // Test fork
    let child_pid = fork();
    if child_pid == 0 {
        // Child process
        print_string("Child process, PID: ");
        print_int(getpid());
        print_char('\n');
        exit(42);
    } else {
        // Parent process
        print_string("Parent process, child PID: ");
        print_int(child_pid);
        print_char('\n');
        
        // Test wait4
        let mut status = 0i32;
        let waited_pid = wait4(-1i64 as u64, &mut status);
        
        print_string("Waited for PID: ");
        print_int(waited_pid);
        print_string(", exit status: ");
        print_int(status as u64);
        print_char('\n');
    }
}

// Syscall wrappers
fn getpid() -> i64 {
    let mut result: i64;
    unsafe {
        asm!(
            "mov rax, {}",
            "syscall",
            "mov {}, rax",
            out(reg) result,
            const SYS_GETPID,
        );
    }
    result
}

fn fork() -> i64 {
    let mut result: i64;
    unsafe {
        asm!(
            "mov rax, {}",
            "syscall",
            "mov {}, rax",
            out(reg) result,
            const SYS_FORK,
        );
    }
    result
}

fn wait4(pid: u64, status_ptr: *mut i32) -> i64 {
    let mut result: i64;
    unsafe {
        asm!(
            "mov rax, {}",
            "mov rdi, {}",
            "mov rsi, {}",
            "syscall",
            "mov {}, rax",
            out(reg) result,
            in(reg) pid,
            in(reg) status_ptr,
            const SYS_WAIT4,
        );
    }
    result
}

fn exit(code: i32) -> ! {
    unsafe {
        asm!(
            "mov rax, {}",
            "mov rdi, {}",
            "syscall",
            const SYS_EXIT,
            in(reg) code,
        );
    }
    loop {}
}

// Simple output functions
fn print_string(s: &str) {
    for byte in s.bytes() {
        print_char(byte as char);
    }
}

fn print_int(mut n: u64) {
    if n == 0 {
        print_char('0');
        return;
    }
    
    let mut buf = [0u8; 20];
    let mut i = 19;
    
    while n > 0 {
        buf[i] = ((n % 10) as u8) + b'0';
        n /= 10;
        i -= 1;
    }
    
    for j in (i + 1)..20 {
        print_char(buf[j] as char);
    }
}

fn print_char(c: char) {
    let mut result: i64;
    unsafe {
        asm!(
            "mov rax, {}",
            "mov rdi, {}",
            "mov rsi, {}",
            "mov rdx, {}",
            "syscall",
            "mov {}, rax",
            out(reg) result,
            const SYS_WRITE,
            in(reg) 1i64,      // stdout
            in(reg) &c as *const _ as u64,
            in(reg) 1u64,       // count
        );
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print_string("PANIC: ");
    if let Some(location) = info.location() {
        print_string("file ");
        let file_bytes = location.file().as_bytes();
        for &byte in file_bytes {
            print_char(*byte as char);
        }
        print_string(":");
        print_int(location.line() as u64);
        print_string(" ");
    }
    if let Some(message) = info.message() {
        print_string(message);
    }
    print_char('\n');
    loop {}
}

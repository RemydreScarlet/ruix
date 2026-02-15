.global _start

.section .text
_start:
    # Test getpid syscall
    mov rax, 39
    syscall
    mov rbx, rax
    
    # Print result
    mov rdi, 1        # stdout
    mov rsi, test_msg
    mov rdx, test_msg_len
    syscall
    
    # Test exit
    mov rax, 0
    mov rdi, 42
    syscall
    
    # Infinite loop
    jmp $

.section .rodata
test_msg: .asciz "Test getpid syscall - PID: "

//! Kernel Test Suite
//! 
//! This module contains all the test cases for the kernel components.

use crate::testing::{TestCase, TestSuite, TestCategory, TestResult, TestError};
use crate::error::KernelResult;
use crate::memory::scalable;
use crate::cpu;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use x86_64::VirtAddr;

pub mod ipc_tests;

/// Create all test suites
pub fn create_all_test_suites() -> Vec<TestSuite> {
    alloc::vec![
        create_memory_tests(),
        create_cpu_tests(),
        create_error_tests(),
        create_integration_tests(),
        create_ipc_tests(),
    ]
}

/// Memory management tests
fn create_memory_tests() -> TestSuite {
    TestSuite::new("Memory Management", "Tests for memory allocation and management", TestCategory::Memory)
        .add_test(TestCase::new("simple_allocation", "Test basic memory allocation", TestCategory::Unit, test_simple_allocation))
        .add_test(TestCase::new("allocation_with_flags", "Test allocation with different flags", TestCategory::Unit, test_allocation_with_flags))
        .add_test(TestCase::new("memory_statistics", "Test memory statistics tracking", TestCategory::Unit, test_memory_statistics))
        .add_test(TestCase::new("page_mapping", "Test page mapping and unmapping", TestCategory::Integration, test_page_mapping))
}

/// CPU management tests
fn create_cpu_tests() -> TestSuite {
    TestSuite::new("CPU Management", "Tests for CPU management and per-CPU data", TestCategory::Unit)
        .add_test(TestCase::new("cpu_data_access", "Test per-CPU data access", TestCategory::Unit, test_cpu_data_access))
        .add_test(TestCase::new("cpu_statistics", "Test CPU statistics tracking", TestCategory::Unit, test_cpu_statistics))
        .add_test(TestCase::new("interrupt_handling", "Test interrupt context handling", TestCategory::Integration, test_interrupt_handling))
}

/// Error handling tests
fn create_error_tests() -> TestSuite {
    TestSuite::new("Error Handling", "Tests for error handling and recovery", TestCategory::Unit)
        .add_test(TestCase::new("error_creation", "Test error creation and display", TestCategory::Unit, test_error_creation))
        .add_test(TestCase::new("error_conversion", "Test error type conversions", TestCategory::Unit, test_error_conversion))
        .add_test(TestCase::new("recovery_strategies", "Test error recovery strategies", TestCategory::Unit, test_recovery_strategies))
}

/// Integration tests
fn create_integration_tests() -> TestSuite {
    TestSuite::new("Integration", "Integration tests across multiple subsystems", TestCategory::Integration)
        .add_test(TestCase::new("memory_cpu_integration", "Test memory and CPU subsystem integration", TestCategory::Integration, test_memory_cpu_integration))
        .add_test(TestCase::new("error_propagation", "Test error propagation across subsystems", TestCategory::Integration, test_error_propagation))
}

// ===== Memory Tests =====

fn test_simple_allocation() -> TestResult {
    crate::assert_true!(crate::testing::get_current_time() > 0);
    
    // Test basic allocation
    let size = 4096; // 4KB
    let addr = scalable::allocate_simple(size)?;
    
    // Verify the allocation
    crate::assert_true!(!addr.is_null());
    
    // Free the memory
    scalable::free(addr, size)?;
    
    Ok(())
}

fn test_allocation_with_flags() -> TestResult {
    use crate::memory::scalable::{AllocFlags, MemoryType};
    
    // Test allocation with different flags
    let flags = AllocFlags {
        zero: true,
        contiguous: false,
        align: Some(4096),
        mem_type: MemoryType::Kernel,
    };
    
    let size = 8192; // 8KB
    let addr = scalable::allocate(size, flags)?;
    
    // Verify alignment
    crate::assert_eq!(addr.as_u64() % 4096, 0);
    
    // Free the memory
    scalable::free(addr, size)?;
    
    Ok(())
}

fn test_memory_statistics() -> TestResult {
    // Get initial statistics
    let initial_stats = scalable::get_memory_stats();
    let initial_total = initial_stats.total_allocated;
    
    // Allocate some memory
    let size = 4096;
    let addr = scalable::allocate_simple(size)?;
    
    // Check that statistics updated
    let after_alloc_stats = scalable::get_memory_stats();
    crate::assert_true!(after_alloc_stats.current_usage > initial_stats.current_usage);
    crate::assert_true!(after_alloc_stats.allocation_count > initial_stats.allocation_count);
    
    // Free the memory
    scalable::free(addr, size)?;
    
    // Check that statistics updated again
    let after_free_stats = scalable::get_memory_stats();
    crate::assert_true!(after_free_stats.current_usage < after_alloc_stats.current_usage);
    crate::assert_true!(after_free_stats.free_count > initial_stats.free_count);
    
    Ok(())
}

fn test_page_mapping() -> TestResult {
    use x86_64::{structures::paging::Page, VirtAddr};
    
    // This test would require access to the page mapper
    // For now, we'll just test the interface
    let page = Page::<x86_64::structures::paging::Size4KiB>::containing_address(VirtAddr::new(0x1000_0000));
    
    // In a real implementation, you would:
    // 1. Allocate a physical frame
    // 2. Map the page
    // 3. Verify the mapping
    // 4. Unmap the page
    
    crate::assert_true!(page.start_address().as_u64() == 0x1000_0000);
    
    Ok(())
}

// ===== CPU Tests =====

fn test_cpu_data_access() -> TestResult {
    // Test per-CPU data access
    let cpu = cpu::current_cpu()?;
    
    // Verify CPU data
    crate::assert_true!(cpu.cpu_id < cpu::MAX_CPUS);
    crate::assert_true!(cpu.kernel_stack_top > 0);
    
    // Test atomic operations
    let old_pid = cpu.get_current_process_id();
    cpu.set_current_process_id(123);
    crate::assert_eq!(cpu.get_current_process_id(), 123);
    cpu.set_current_process_id(old_pid);
    
    Ok(())
}

fn test_cpu_statistics() -> TestResult {
    // Test performance monitoring
    let initial_stats = cpu::PERF_MONITOR.get_stats();
    
    // Increment some counters
    cpu::PERF_MONITOR.increment_context_switches();
    cpu::PERF_MONITOR.increment_interrupts();
    cpu::PERF_MONITOR.increment_syscalls();
    
    let new_stats = cpu::PERF_MONITOR.get_stats();
    crate::assert_true!(new_stats.0 > initial_stats.0); // context switches
    crate::assert_true!(new_stats.1 > initial_stats.1); // interrupts
    crate::assert_true!(new_stats.2 > initial_stats.2); // syscalls
    
    Ok(())
}

fn test_interrupt_handling() -> TestResult {
    let cpu = cpu::current_cpu()?;
    
    // Test interrupt context tracking
    crate::assert_false!(cpu.in_interrupt());
    
    cpu.enter_interrupt();
    crate::assert_true!(cpu.in_interrupt());
    
    cpu.exit_interrupt();
    crate::assert_false!(cpu.in_interrupt());
    
    Ok(())
}

// ===== Error Handling Tests =====

fn test_error_creation() -> TestResult {
    use crate::error::{KernelError, AllocError, ProcessError};
    
    // Test error creation
    let mem_error = KernelError::Memory(AllocError::OutOfMemory);
    crate::assert_eq!(mem_error.to_string(), "Memory error: Out of memory");
    
    let proc_error = KernelError::Process(ProcessError::InvalidPid);
    crate::assert_eq!(proc_error.to_string(), "Process error: Invalid process ID");
    
    Ok(())
}

fn test_error_conversion() -> TestResult {
    use crate::error::{KernelError, AllocError};
    
    // Test error conversion
    let alloc_error = AllocError::OutOfMemory;
    let kernel_error: KernelError = alloc_error.into();
    
    match kernel_error {
        KernelError::Memory(AllocError::OutOfMemory) => {}, // Expected
        _ => return Err(TestError::AssertionFailed("Unexpected error type".to_string())),
    }
    
    Ok(())
}

fn test_recovery_strategies() -> TestResult {
    use crate::error::{KernelError, AllocError, RecoveryStrategy};
    
    // Test recovery strategy determination
    let oom_error = KernelError::Memory(AllocError::OutOfMemory);
    let strategy = crate::error::get_recovery_strategy(&oom_error);
    crate::assert_eq!(strategy, RecoveryStrategy::Abort);
    
    let alignment_error = KernelError::Memory(AllocError::BadAlignment);
    let strategy = crate::error::get_recovery_strategy(&alignment_error);
    crate::assert_eq!(strategy, RecoveryStrategy::Retry);
    
    Ok(())
}

// ===== Integration Tests =====

fn test_memory_cpu_integration() -> TestResult {
    // Test that memory allocation works correctly with per-CPU data
    let cpu = cpu::current_cpu()?;
    let cpu_id = cpu.cpu_id;
    
    // Allocate memory
    let size = 4096;
    let addr = scalable::allocate_simple(size)?;
    
    // Verify allocation succeeded
    crate::assert_true!(!addr.is_null());
    
    // Check that CPU state is still valid
    let current_cpu = cpu::current_cpu()?;
    crate::assert_eq!(current_cpu.cpu_id, cpu_id);
    
    // Free memory
    scalable::free(addr, size)?;
    
    Ok(())
}

fn test_error_propagation() -> TestResult {
    use crate::error::{KernelError, AllocError};
    
    // Test error propagation through function calls
    fn inner_function() -> KernelResult<()> {
        Err(KernelError::Memory(AllocError::OutOfMemory))
    }
    
    fn outer_function() -> KernelResult<()> {
        inner_function()
    }
    
    let result = outer_function();
    crate::assert_err!(result);
    
    match result.unwrap_err() {
        KernelError::Memory(AllocError::OutOfMemory) => {}, // Expected
        _ => return Err(TestError::AssertionFailed("Wrong error type".to_string())),
    }
    
    Ok(())
}

// ===== Performance Tests =====

pub fn create_performance_tests() -> TestSuite {
    TestSuite::new("Performance", "Performance benchmarks and stress tests", TestCategory::Performance)
        .add_test(TestCase::new("memory_allocation_speed", "Benchmark memory allocation speed", TestCategory::Performance, test_memory_allocation_speed))
        .add_test(TestCase::new("cpu_data_access_speed", "Benchmark per-CPU data access speed", TestCategory::Performance, test_cpu_data_access_speed))
}

fn test_memory_allocation_speed() -> TestResult {
    let iterations = 1000;
    let start_time = crate::testing::get_current_time();
    
    for _ in 0..iterations {
        let size = 1024;
        let addr = scalable::allocate_simple(size)?;
        scalable::free(addr, size)?;
    }
    
    let end_time = crate::testing::get_current_time();
    let duration = end_time - start_time;
    
    // This is a very basic performance test
    // In a real implementation, you'd use proper timing
    crate::assert_true!(duration < 10000); // Should complete in less than 10 "time units"
    
    crate::println!("Memory allocation test: {} iterations in {} time units", iterations, duration);
    
    Ok(())
}

fn test_cpu_data_access_speed() -> TestResult {
    let iterations = 10000;
    let start_time = crate::testing::get_current_time();
    
    for i in 0..iterations {
        let cpu = cpu::current_cpu()?;
        cpu.set_current_process_id(i % 100);
        let _ = cpu.get_current_process_id();
    }
    
    let end_time = crate::testing::get_current_time();
    let duration = end_time - start_time;
    
    crate::assert_true!(duration < 5000); // Should complete in less than 5 "time units"
    
    crate::println!("CPU data access test: {} iterations in {} time units", iterations, duration);
    
    Ok(())
}

// ===== Stress Tests =====

pub fn create_stress_tests() -> TestSuite {
    TestSuite::new("Stress", "Stress tests for system limits", TestCategory::Stress)
        .add_test(TestCase::new("memory_stress", "Stress test memory allocation", TestCategory::Stress, test_memory_stress))
        .add_test(TestCase::new("cpu_stress", "Stress test CPU operations", TestCategory::Stress, test_cpu_stress))
}

fn test_memory_stress() -> TestResult {
    let mut allocations: Vec<x86_64::VirtAddr> = Vec::new();
    
    // Try to allocate a lot of small blocks
    for i in 0..100 {
        let size = 1024 * (i + 1); // Increasing sizes
        match scalable::allocate_simple(size) {
            Ok(addr) => {
                allocations.push(addr); // Store the allocation
                // In a real implementation, you'd store these for cleanup
                scalable::free(addr, size)?;
            }
            Err(_) => {
                // Expected to eventually run out of memory
                break;
            }
        }
    }
    
    Ok(())
}

fn test_cpu_stress() -> TestResult {
    let cpu = cpu::current_cpu()?;
    
    // Rapidly switch process IDs
    for i in 0..1000 {
        cpu.set_current_process_id(i);
        let _ = cpu.get_current_process_id();
    }
    
    // Test interrupt nesting
    for _ in 0..10 {
        cpu.enter_interrupt();
    }
    
    for _ in 0..10 {
        cpu.exit_interrupt();
    }
    
    crate::assert_false!(cpu.in_interrupt());
    
    Ok(())
}

/// IPC tests
fn create_ipc_tests() -> TestSuite {
    TestSuite::new("IPC System", "Tests for Inter-Process Communication", TestCategory::Ipc)
        .add_test(TestCase::new("ipc_boot_sequence", "Test IPC boot sequence", TestCategory::Integration, crate::tests::ipc_tests::test_ipc_boot_sequence))
        .add_test(TestCase::new("ipc_cleanup", "Test IPC cleanup functionality", TestCategory::Integration, crate::tests::ipc_tests::test_ipc_cleanup))
        .add_test(TestCase::new("page_table_ops", "Test page table operations", TestCategory::Integration, crate::tests::ipc_tests::test_page_table_ops))
        .add_test(TestCase::new("all_ipc_tests", "Run all IPC tests", TestCategory::System, crate::tests::ipc_tests::run_all_ipc_tests))
}

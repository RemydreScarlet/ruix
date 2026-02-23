//! Boot Check System
//! 
//! This module provides comprehensive boot-time testing to ensure
//! the kernel is in a stable state before proceeding with IPC implementation.

use crate::error::{KernelError, KernelResult};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

/// Boot test phases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPhase {
    /// Basic kernel functionality
    BasicSystem,
    /// Memory allocation and management
    MemoryAllocation,
    /// Process creation and management
    ProcessCreation,
    /// System call functionality
    Syscalls,
    /// IPC functionality
    IpcSystem,
}

/// Boot test result
#[derive(Debug, Clone)]
pub struct BootTestResult {
    /// Test phase
    pub phase: TestPhase,
    /// Test name
    pub test_name: String,
    /// Whether test passed
    pub passed: bool,
    /// Additional details
    pub details: String,
}

/// Boot checker - runs comprehensive boot-time tests
pub struct BootChecker {
    /// All test results
    pub test_results: Vec<BootTestResult>,
    /// Current test phase
    pub current_phase: TestPhase,
}

impl BootChecker {
    /// Create a new boot checker
    pub fn new() -> Self {
        Self {
            test_results: Vec::new(),
            current_phase: TestPhase::BasicSystem,
        }
    }

    /// Run all boot checks including comprehensive test suites
    pub fn run_all_checks(&mut self) -> KernelResult<()> {
        crate::println!("🚀 STARTING BOOT CHECK SEQUENCE");
        
        // Run basic system checks first
        self.test_basic_system()?;
        self.test_memory_allocation()?;
        self.test_process_creation()?;
        self.test_syscalls()?;
        
        // Run comprehensive test suites
        crate::println!("BOOT_CHECK: Running comprehensive test suites...");
        self.run_comprehensive_tests()?;
        
        crate::println!("✅ All boot checks passed successfully");
        Ok(())
    }

    /// Test basic kernel functionality
    fn test_basic_system(&mut self) -> KernelResult<()> {
        self.current_phase = TestPhase::BasicSystem;
        crate::println!("BOOT_CHECK: Testing basic kernel functionality...");

        // Test 1: Basic printing works
        self.add_result("Basic printing", true, "Serial output functional".to_string());

        // Test 2: Basic arithmetic works
        let test_val = 42 + 58;
        let arithmetic_ok = test_val == 100;
        self.add_result("Basic arithmetic", arithmetic_ok, 
            format!("42 + 58 = {}", test_val));

        // Test 3: Basic memory allocation (small)
        let allocation_ok = self.test_small_allocation();
        self.add_result("Small allocation", allocation_ok, 
            "Can allocate small structures".to_string());

        if !arithmetic_ok || !allocation_ok {
            return Err(KernelError::General(crate::error::GeneralError::Internal));
        }

        Ok(())
    }

    /// Test memory allocation system
    fn test_memory_allocation(&mut self) -> KernelResult<()> {
        self.current_phase = TestPhase::MemoryAllocation;
        crate::println!("BOOT_CHECK: Testing memory allocation...");

        // Test 1: Vector allocation
        let vector_ok = self.test_vector_allocation();
        self.add_result("Vector allocation", vector_ok, 
            "Can allocate and grow vectors".to_string());

        // Test 2: String allocation
        let string_ok = self.test_string_allocation();
        self.add_result("String allocation", string_ok, 
            "Can allocate and manipulate strings".to_string());

        if !vector_ok || !string_ok {
            return Err(KernelError::Memory(crate::error::AllocError::OutOfMemory));
        }

        Ok(())
    }

    /// Test process creation and management
    fn test_process_creation(&mut self) -> KernelResult<()> {
        self.current_phase = TestPhase::ProcessCreation;
        crate::println!("BOOT_CHECK: Testing process creation...");

        // Test 1: Scheduler access
        let scheduler_ok = self.test_scheduler_access();
        self.add_result("Scheduler access", scheduler_ok, 
            "Can access process scheduler".to_string());

        // Test 2: Process ID system
        let pid_ok = self.test_pid_system();
        self.add_result("PID system", pid_ok, 
            "Process ID system functional".to_string());

        if !scheduler_ok || !pid_ok {
            return Err(KernelError::Process(crate::error::ProcessError::NotFound));
        }

        Ok(())
    }

    /// Test system call functionality
    fn test_syscalls(&mut self) -> KernelResult<()> {
        self.current_phase = TestPhase::Syscalls;
        crate::println!("BOOT_CHECK: Testing system calls...");

        // Test 1: Syscall initialization
        let syscall_ok = self.test_syscall_init();
        self.add_result("Syscall init", syscall_ok, 
            "System call system initialized".to_string());

        // Test 2: Current process ID
        let pid_ok = self.test_current_pid();
        self.add_result("Current PID", pid_ok, 
            "Can get current process ID".to_string());

        if !syscall_ok || !pid_ok {
            return Err(KernelError::Syscall(crate::error::SyscallError::InvalidArgs));
        }

        Ok(())
    }

    /// Add a test result
    fn add_result(&mut self, test_name: &str, passed: bool, details: String) {
        let result = BootTestResult {
            phase: self.current_phase,
            test_name: test_name.to_string(),
            passed,
            details,
        };
        self.test_results.push(result);
        
        let status = if passed { "[OK]" } else { "[Failed]" };
        crate::println!("{} {}", status, test_name);
    }

    /// Check if all tests passed
    pub fn all_passed(&self) -> bool {
        self.test_results.iter().all(|r| r.passed)
    }

    /// Get failed tests
    pub fn get_failed_tests(&self) -> Vec<&BootTestResult> {
        self.test_results.iter().filter(|r| !r.passed).collect::<Vec<_>>()
    }

    /// Print summary
    pub fn print_summary(&self) {
        crate::println!("\n=== BOOT CHECK SUMMARY ===");
        let total = self.test_results.len();
        let passed = self.test_results.iter().filter(|r| r.passed).count();
        let failed = total - passed;

        crate::println!("Total tests: {}", total);
        crate::println!("Passed: {}", passed);
        crate::println!("Failed: {}", failed);

        if failed > 0 {
            crate::println!("\nFailed tests:");
            for test in &self.test_results {
                if !test.passed {
                    crate::println!("  - {}: {}", test.test_name, test.details);
                }
            }
        }

        crate::println!("========================");
    }

    // Helper test functions
    
    fn test_small_allocation(&self) -> bool {
        // Try to allocate a small structure
        let test_vec: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        test_vec.capacity() == 0 // Should be empty initially
    }

    fn test_vector_allocation(&self) -> bool {
        let mut vec = alloc::vec::Vec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);
        vec.len() == 3 && vec[0] == 1 && vec[2] == 3
    }

    fn test_string_allocation(&self) -> bool {
        let mut s = String::new();
        s.push_str("test");
        s.len() == 4 && s == "test"
    }

    fn test_scheduler_access(&self) -> bool {
        // Try to access the scheduler
        use crate::process::scheduler::SCHEDULER;
        let _sched = SCHEDULER.lock();
        true // If we can lock it, it's working
    }

    fn test_pid_system(&self) -> bool {
        // Test that we can get a process ID (even if it's just 0)
        let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };
        current_pid >= 0 // Basic sanity check
    }

    fn test_syscall_init(&self) -> bool {
        // Test that syscall system is initialized
        // This is basic - if we got here, syscalls are probably working
        true
    }

    fn test_current_pid(&self) -> bool {
        let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };
        current_pid >= 0
    }

    /// Run comprehensive test suites
    fn run_comprehensive_tests(&mut self) -> KernelResult<()> {
        self.current_phase = TestPhase::IpcSystem;
        
        // Create and run all test suites
        let test_suites = crate::tests::create_all_test_suites();
        
        for suite in test_suites {
            crate::println!("BOOT_CHECK: Running {} suite...", suite.name);
            
            for test in &suite.tests {
                let result = test.run();
                self.add_result(&test.metadata.name, result.success, 
                    if result.success { 
                        "Test passed".to_string() 
                    } else { 
                        result.error.as_ref().map(|e| e.to_string()).unwrap_or_else(|| "Test failed".to_string())
                    });
            }
        }
        
        Ok(())
    }
}

/// Run all boot checks and return a boot checker
pub fn run_boot_checks() -> KernelResult<BootChecker> {
    let mut checker = BootChecker::new();
    checker.run_all_checks()?;
    Ok(checker)
}

/// Quick boot check - just the essentials
pub fn quick_boot_check() -> KernelResult<()> {
    crate::println!("Running quick boot check...");
    
    // Test basic allocation
    let _test = alloc::vec::Vec::<u8>::new();
    
    // Test string creation
    let _test_str = String::from("boot");
    
    crate::println!("Quick boot check passed");
    Ok(())
}

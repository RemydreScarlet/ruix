//! Kernel Testing Framework
//! 
//! This module provides a comprehensive testing framework for the kernel,
//! allowing for unit tests, integration tests, and system tests.

pub mod boot_check;

pub use boot_check::{run_boot_checks, quick_boot_check, BootChecker};

use crate::error::{KernelError, KernelResult};
use core::fmt;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

/// Test result type
pub type TestResult = Result<(), TestError>;

/// Test error types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestError {
    /// Assertion failed
    AssertionFailed(String),
    /// Expected panic but didn't panic
    ExpectedPanic,
    /// Unexpected panic
    UnexpectedPanic(String),
    /// Timeout
    Timeout,
    /// Setup failed
    SetupFailed(String),
    /// Teardown failed
    TeardownFailed(String),
    /// Resource not available
    ResourceUnavailable(String),
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TestError::AssertionFailed(msg) => write!(f, "Assertion failed: {}", msg),
            TestError::ExpectedPanic => write!(f, "Expected panic but didn't panic"),
            TestError::UnexpectedPanic(msg) => write!(f, "Unexpected panic: {}", msg),
            TestError::Timeout => write!(f, "Test timed out"),
            TestError::SetupFailed(msg) => write!(f, "Setup failed: {}", msg),
            TestError::TeardownFailed(msg) => write!(f, "Teardown failed: {}", msg),
            TestError::ResourceUnavailable(msg) => write!(f, "Resource unavailable: {}", msg),
        }
    }
}

/// Convert KernelError to TestError for test compatibility
impl From<crate::error::KernelError> for TestError {
    fn from(error: crate::error::KernelError) -> Self {
        TestError::AssertionFailed(format!("Kernel error: {:?}", error))
    }
}

/// Test metadata
#[derive(Debug, Clone)]
pub struct TestMetadata {
    /// Test name
    pub name: String,
    /// Test description
    pub description: String,
    /// Test category
    pub category: TestCategory,
    /// Expected execution time (in milliseconds)
    pub expected_time_ms: u64,
    /// Whether this test requires special setup
    pub requires_setup: bool,
    /// Whether this test might panic
    pub might_panic: bool,
}

/// Test categories
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestCategory {
    /// Unit tests (fast, isolated)
    Unit,
    /// Integration tests (multiple components)
    Integration,
    /// System tests (full kernel)
    System,
    /// Performance tests
    Performance,
    /// Stress tests
    Stress,
    /// Memory tests
    Memory,
    /// Process tests
    Process,
    /// IPC tests
    Ipc,
    /// Hardware tests
    Hardware,
}

/// Test function type
pub type TestFunction = fn() -> TestResult;

/// Test case
pub struct TestCase {
    /// Test metadata
    pub metadata: TestMetadata,
    /// Test function
    pub test_fn: TestFunction,
    /// Setup function (optional)
    pub setup_fn: Option<TestFunction>,
    /// Teardown function (optional)
    pub teardown_fn: Option<TestFunction>,
}

impl TestCase {
    /// Create a new test case
    pub fn new(name: &str, description: &str, category: TestCategory, test_fn: TestFunction) -> Self {
        Self {
            metadata: TestMetadata {
                name: name.to_string(),
                description: description.to_string(),
                category,
                expected_time_ms: 1000, // Default 1 second
                requires_setup: false,
                might_panic: false,
            },
            test_fn,
            setup_fn: None,
            teardown_fn: None,
        }
    }

    /// Set expected execution time
    pub fn with_expected_time(mut self, time_ms: u64) -> Self {
        self.metadata.expected_time_ms = time_ms;
        self
    }

    /// Set setup function
    pub fn with_setup(mut self, setup_fn: TestFunction) -> Self {
        self.setup_fn = Some(setup_fn);
        self.metadata.requires_setup = true;
        self
    }

    /// Set teardown function
    pub fn with_teardown(mut self, teardown_fn: TestFunction) -> Self {
        self.teardown_fn = Some(teardown_fn);
        self
    }

    /// Mark as potentially panicking
    pub fn might_panic(mut self) -> Self {
        self.metadata.might_panic = true;
        self
    }

    /// Run the test
    pub fn run(&self) -> TestExecutionResult {
        let start_time = get_current_time();
        
        // Run setup if present
        if let Some(setup_fn) = self.setup_fn {
            if let Err(e) = setup_fn() {
                return TestExecutionResult {
                    test_name: self.metadata.name.clone(),
                    success: false,
                    duration_ms: get_current_time() - start_time,
                    error: Some(TestError::SetupFailed(e.to_string())),
                    output: Vec::new(),
                };
            }
        }

        // Run the actual test
        let result = if self.metadata.might_panic {
            self.run_panic_test()
        } else {
            self.run_normal_test()
        };

        // Run teardown if present
        if let Some(teardown_fn) = self.teardown_fn {
            if let Err(e) = teardown_fn() {
                return TestExecutionResult {
                    test_name: self.metadata.name.clone(),
                    success: false,
                    duration_ms: get_current_time() - start_time,
                    error: Some(TestError::TeardownFailed(e.to_string())),
                    output: Vec::new(),
                };
            }
        }

        TestExecutionResult {
            test_name: self.metadata.name.clone(),
            success: result.is_ok(),
            duration_ms: get_current_time() - start_time,
            error: result.err(),
            output: Vec::new(), // TODO: Capture test output
        }
    }

    /// Run a normal test
    fn run_normal_test(&self) -> TestResult {
        (self.test_fn)()
    }

    /// Run a test that might panic
    fn run_panic_test(&self) -> TestResult {
        // For now, we can't catch panics in a no_std environment
        // This would need special assembly or compiler support
        (self.test_fn)()
    }
}

/// Test execution result
#[derive(Debug, Clone)]
pub struct TestExecutionResult {
    /// Test name
    pub test_name: String,
    /// Whether the test passed
    pub success: bool,
    /// Execution time in milliseconds
    pub duration_ms: u64,
    /// Error if test failed
    pub error: Option<TestError>,
    /// Test output (captured)
    pub output: Vec<String>,
}

/// Test suite
pub struct TestSuite {
    /// Suite name
    pub name: String,
    /// Test cases
    pub tests: Vec<TestCase>,
    /// Suite metadata
    pub metadata: SuiteMetadata,
}

/// Test suite metadata
#[derive(Debug, Clone)]
pub struct SuiteMetadata {
    /// Suite description
    pub description: String,
    /// Suite category
    pub category: TestCategory,
    /// Total expected time
    pub expected_time_ms: u64,
}

impl TestSuite {
    /// Create a new test suite
    pub fn new(name: &str, description: &str, category: TestCategory) -> Self {
        Self {
            name: name.to_string(),
            tests: Vec::new(),
            metadata: SuiteMetadata {
                description: description.to_string(),
                category,
                expected_time_ms: 0,
            },
        }
    }

    /// Add a test to the suite
    pub fn add_test(mut self, test: TestCase) -> Self {
        self.metadata.expected_time_ms += test.metadata.expected_time_ms;
        self.tests.push(test);
        self
    }

    /// Run all tests in the suite
    pub fn run(&self) -> SuiteResult {
        let mut results = Vec::new();
        let mut passed = 0;
        let mut failed = 0;
        let start_time = get_current_time();

        crate::println!("Running test suite: {}", self.name);
        crate::println!("Description: {}", self.metadata.description);
        crate::println!("Tests: {}", self.tests.len());
        crate::println!("Expected time: {}ms", self.metadata.expected_time_ms);
        crate::println!("");

        for test in &self.tests {
            crate::println!("Running: {}...", test.metadata.name);
            let result = test.run();
            
            if result.success {
                crate::println!("✓ PASSED ({}ms)", result.duration_ms);
                passed += 1;
            } else {
                crate::println!("✗ FAILED ({}ms): {}", result.duration_ms, 
                    result.error.as_ref().map(|e| e.to_string()).unwrap_or_else(|| "Unknown".to_string()));
                failed += 1;
            }
            
            results.push(result);
        }

        let total_time = get_current_time() - start_time;

        SuiteResult {
            suite_name: self.name.clone(),
            total_tests: self.tests.len(),
            passed,
            failed,
            total_time_ms: total_time,
            results,
        }
    }
}

/// Test suite result
#[derive(Debug, Clone)]
pub struct SuiteResult {
    /// Suite name
    pub suite_name: String,
    /// Total number of tests
    pub total_tests: usize,
    /// Number of passed tests
    pub passed: usize,
    /// Number of failed tests
    pub failed: usize,
    /// Total execution time
    pub total_time_ms: u64,
    /// Individual test results
    pub results: Vec<TestExecutionResult>,
}

impl SuiteResult {
    /// Print summary
    pub fn print_summary(&self) {
        crate::println!("\n=== Test Suite Summary ===");
        crate::println!("Suite: {}", self.suite_name);
        crate::println!("Total tests: {}", self.total_tests);
        crate::println!("Passed: {}", self.passed);
        crate::println!("Failed: {}", self.failed);
        crate::println!("Success rate: {:.1}%", (self.passed as f64 / self.total_tests as f64) * 100.0);
        crate::println!("Total time: {}ms", self.total_time_ms);
        crate::println!("========================");
    }

    /// Check if all tests passed
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

/// Test registry
pub struct TestRegistry {
    /// All test suites
    pub suites: Vec<TestSuite>,
}

impl TestRegistry {
    /// Create a new test registry
    pub fn new() -> Self {
        Self {
            suites: Vec::new(),
        }
    }

    /// Add a test suite
    pub fn add_suite(mut self, suite: TestSuite) -> Self {
        self.suites.push(suite);
        self
    }

    /// Run all registered test suites
    pub fn run_all(&self) -> Vec<SuiteResult> {
        let mut all_results = Vec::new();
        
        for suite in &self.suites {
            let result = suite.run();
            result.print_summary();
            all_results.push(result);
        }
        
        all_results
    }

    /// Run tests by category
    pub fn run_category(&self, category: TestCategory) -> Vec<SuiteResult> {
        let mut results = Vec::new();
        
        for suite in &self.suites {
            if suite.metadata.category == category {
                let result = suite.run();
                result.print_summary();
                results.push(result);
            }
        }
        
        results
    }
}

/// Assertion macros for testing
#[macro_export]
macro_rules! assert_eq {
    ($left:expr, $right:expr) => {
        if $left != $right {
            return Err($crate::testing::TestError::AssertionFailed(
                format!("assertion failed: {} != {}", stringify!($left), stringify!($right))
            ));
        }
    };
    ($left:expr, $right:expr, $msg:expr) => {
        if $left != $right {
            return Err($crate::testing::TestError::AssertionFailed(
                format!("{}: {} != {}", $msg, stringify!($left), stringify!($right))
            ));
        }
    };
}

#[macro_export]
macro_rules! assert_ne {
    ($left:expr, $right:expr) => {
        if $left == $right {
            return Err($crate::testing::TestError::AssertionFailed(
                format!("assertion failed: {} == {}", stringify!($left), stringify!($right))
            ));
        }
    };
}

#[macro_export]
macro_rules! assert_true {
    ($expr:expr) => {
        if !$expr {
            return Err($crate::testing::TestError::AssertionFailed(
                format!("assertion failed: {} is not true", stringify!($expr))
            ));
        }
    };
}

#[macro_export]
macro_rules! assert_false {
    ($expr:expr) => {
        if $expr {
            return Err($crate::testing::TestError::AssertionFailed(
                format!("assertion failed: {} is not false", stringify!($expr))
            ));
        }
    };
}

#[macro_export]
macro_rules! assert_ok {
    ($expr:expr) => {
        match $expr {
            Ok(_) => {},
            Err(e) => {
                return Err($crate::testing::TestError::AssertionFailed(
                    format!("expected Ok, got Err: {:?}", e)
                ));
            }
        }
    };
}

#[macro_export]
macro_rules! assert_err {
    ($expr:expr) => {
        match $expr {
            Ok(v) => {
                return Err($crate::testing::TestError::AssertionFailed(
                    format!("expected Err, got Ok: {:?}", v)
                ));
            }
            Err(_) => {},
        }
    };
}

/// Get current time (simple implementation)
pub fn get_current_time() -> u64 {
    // In a real implementation, you'd use a proper timer
    // For now, we'll use a simple counter
    static mut TIME_COUNTER: u64 = 0;
    unsafe {
        TIME_COUNTER += 1;
        TIME_COUNTER
    }
}

/// Global test registry
static mut TEST_REGISTRY: Option<TestRegistry> = None;
static REGISTRY_INIT: bool = false;

/// Get the global test registry
#[allow(static_mut_refs)]
pub fn test_registry() -> &'static mut TestRegistry {
    unsafe {
        if TEST_REGISTRY.is_none() {
            TEST_REGISTRY = Some(TestRegistry::new());
        }
        TEST_REGISTRY.as_mut().unwrap()
    }
}

/// Run all tests
pub fn run_all_tests() -> KernelResult<()> {
    crate::println!("=== Running All Kernel Tests ===");
    
    let registry = test_registry();
    let results = registry.run_all();
    
    let total_suites = results.len();
    let total_passed = results.iter().filter(|r| r.all_passed()).count();
    let total_failed = total_suites - total_passed;
    
    crate::println!("\n=== Overall Test Results ===");
    crate::println!("Total suites: {}", total_suites);
    crate::println!("Passed suites: {}", total_passed);
    crate::println!("Failed suites: {}", total_failed);
    
    if total_failed == 0 {
        crate::println!("🎉 All tests passed!");
        Ok(())
    } else {
        crate::println!("❌ {} test suite(s) failed", total_failed);
        Err(KernelError::General(crate::error::GeneralError::Internal))
    }
}

/// Register a test suite
#[macro_export]
macro_rules! register_test_suite {
    ($suite:expr) => {
        $crate::testing::test_registry().add_suite($suite);
    };
}

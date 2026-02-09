//! Ruix OS Error Handling System
//! 
//! This module provides a comprehensive error handling framework for the kernel,
//! ensuring proper error propagation and handling across all subsystems.

use core::fmt;

/// Kernel-wide error type that encompasses all possible error conditions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    /// Memory allocation errors
    Memory(AllocError),
    /// Process management errors
    Process(ProcessError),
    /// System call errors
    Syscall(SyscallError),
    /// IPC communication errors
    Ipc(IpcError),
    /// Hardware/IO errors
    Hardware(HardwareError),
    /// General kernel errors
    General(GeneralError),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            KernelError::Memory(e) => write!(f, "Memory error: {}", e),
            KernelError::Process(e) => write!(f, "Process error: {}", e),
            KernelError::Syscall(e) => write!(f, "Syscall error: {}", e),
            KernelError::Ipc(e) => write!(f, "IPC error: {}", e),
            KernelError::Hardware(e) => write!(f, "Hardware error: {}", e),
            KernelError::General(e) => write!(f, "General error: {}", e),
        }
    }
}

/// Memory allocation and management errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocError {
    /// Out of memory
    OutOfMemory,
    /// Invalid memory alignment
    BadAlignment,
    /// Memory region already in use
    AlreadyInUse,
    /// Invalid memory address
    InvalidAddress,
    /// Permission denied
    PermissionDenied,
}

impl fmt::Display for AllocError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AllocError::OutOfMemory => write!(f, "Out of memory"),
            AllocError::BadAlignment => write!(f, "Bad memory alignment"),
            AllocError::AlreadyInUse => write!(f, "Memory region already in use"),
            AllocError::InvalidAddress => write!(f, "Invalid memory address"),
            AllocError::PermissionDenied => write!(f, "Permission denied"),
        }
    }
}

/// Process management errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    /// Invalid process ID
    InvalidPid,
    /// Process not found
    NotFound,
    /// Process already exists
    AlreadyExists,
    /// Invalid process state
    InvalidState,
    /// Stack allocation failed
    StackAllocationFailed,
    /// Context switch failed
    ContextSwitchFailed,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessError::InvalidPid => write!(f, "Invalid process ID"),
            ProcessError::NotFound => write!(f, "Process not found"),
            ProcessError::AlreadyExists => write!(f, "Process already exists"),
            ProcessError::InvalidState => write!(f, "Invalid process state"),
            ProcessError::StackAllocationFailed => write!(f, "Stack allocation failed"),
            ProcessError::ContextSwitchFailed => write!(f, "Context switch failed"),
        }
    }
}

/// System call errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyscallError {
    /// Invalid system call number
    InvalidNumber,
    /// Invalid arguments
    InvalidArgs,
    /// Permission denied
    PermissionDenied,
    /// Resource not available
    ResourceUnavailable,
    /// Operation not supported
    NotSupported,
    /// Buffer too small
    BufferTooSmall,
}

impl fmt::Display for SyscallError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SyscallError::InvalidNumber => write!(f, "Invalid system call number"),
            SyscallError::InvalidArgs => write!(f, "Invalid arguments"),
            SyscallError::PermissionDenied => write!(f, "Permission denied"),
            SyscallError::ResourceUnavailable => write!(f, "Resource unavailable"),
            SyscallError::NotSupported => write!(f, "Operation not supported"),
            SyscallError::BufferTooSmall => write!(f, "Buffer too small"),
        }
    }
}

/// IPC communication errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcError {
    /// Channel not found
    ChannelNotFound,
    /// Channel already exists
    ChannelExists,
    /// Message too large
    MessageTooLarge,
    /// No message available
    NoMessage,
    /// Invalid channel ID
    InvalidChannelId,
    /// Connection refused
    ConnectionRefused,
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            IpcError::ChannelNotFound => write!(f, "Channel not found"),
            IpcError::ChannelExists => write!(f, "Channel already exists"),
            IpcError::MessageTooLarge => write!(f, "Message too large"),
            IpcError::NoMessage => write!(f, "No message available"),
            IpcError::InvalidChannelId => write!(f, "Invalid channel ID"),
            IpcError::ConnectionRefused => write!(f, "Connection refused"),
        }
    }
}

/// Hardware and IO errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    /// Device not found
    DeviceNotFound,
    /// Device busy
    DeviceBusy,
    /// IO operation failed
    IoFailed,
    /// Invalid port
    InvalidPort,
    /// Timeout
    Timeout,
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HardwareError::DeviceNotFound => write!(f, "Device not found"),
            HardwareError::DeviceBusy => write!(f, "Device busy"),
            HardwareError::IoFailed => write!(f, "IO operation failed"),
            HardwareError::InvalidPort => write!(f, "Invalid port"),
            HardwareError::Timeout => write!(f, "Timeout"),
        }
    }
}

/// General kernel errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneralError {
    /// Invalid operation
    InvalidOperation,
    /// Not implemented
    NotImplemented,
    /// Internal error
    Internal,
    /// Invalid state
    InvalidState,
}

impl fmt::Display for GeneralError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GeneralError::InvalidOperation => write!(f, "Invalid operation"),
            GeneralError::NotImplemented => write!(f, "Not implemented"),
            GeneralError::Internal => write!(f, "Internal error"),
            GeneralError::InvalidState => write!(f, "Invalid state"),
        }
    }
}

/// Result type alias for kernel operations
pub type KernelResult<T> = Result<T, KernelError>;

/// Error conversion macros for easier error handling
#[macro_export]
macro_rules! kerror {
    ($error:expr) => {
        Err($crate::error::KernelError::from($error))
    };
}

#[macro_export]
macro_rules! ktry {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err) => return kerror!(err),
        }
    };
}

/// Error conversion implementations
impl From<AllocError> for KernelError {
    fn from(err: AllocError) -> Self {
        KernelError::Memory(err)
    }
}

impl From<ProcessError> for KernelError {
    fn from(err: ProcessError) -> Self {
        KernelError::Process(err)
    }
}

impl From<SyscallError> for KernelError {
    fn from(err: SyscallError) -> Self {
        KernelError::Syscall(err)
    }
}

impl From<IpcError> for KernelError {
    fn from(err: IpcError) -> Self {
        KernelError::Ipc(err)
    }
}

impl From<HardwareError> for KernelError {
    fn from(err: HardwareError) -> Self {
        KernelError::Hardware(err)
    }
}

impl From<GeneralError> for KernelError {
    fn from(err: GeneralError) -> Self {
        KernelError::General(err)
    }
}

/// Error logging functionality
pub fn log_error(err: &KernelError) {
    crate::println!("KERNEL ERROR: {}", err);
    
    // In a real implementation, you might want to:
    // - Log to a persistent buffer
    // - Send to serial port
    // - Write to debug output
    // - Trigger error handling procedures
    
    match err {
        KernelError::Memory(e) => crate::println!("  Memory subsystem: {}", e),
        KernelError::Process(e) => crate::println!("  Process subsystem: {}", e),
        KernelError::Syscall(e) => crate::println!("  Syscall subsystem: {}", e),
        KernelError::Ipc(e) => crate::println!("  IPC subsystem: {}", e),
        KernelError::Hardware(e) => crate::println!("  Hardware subsystem: {}", e),
        KernelError::General(e) => crate::println!("  General: {}", e),
    }
}

/// Error recovery strategies
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Retry the operation
    Retry,
    /// Skip the operation
    Skip,
    /// Abort the current operation
    Abort,
    /// Reboot the system
    Reboot,
    /// Enter panic mode
    Panic,
}

/// Determine recovery strategy for different error types
pub fn get_recovery_strategy(err: &KernelError) -> RecoveryStrategy {
    match err {
        // Memory errors might be recoverable
        KernelError::Memory(AllocError::OutOfMemory) => RecoveryStrategy::Abort,
        KernelError::Memory(_) => RecoveryStrategy::Retry,
        
        // Process errors usually require abort
        KernelError::Process(_) => RecoveryStrategy::Abort,
        
        // Syscall errors are usually recoverable
        KernelError::Syscall(_) => RecoveryStrategy::Skip,
        
        // IPC errors are usually recoverable
        KernelError::Ipc(_) => RecoveryStrategy::Retry,
        
        // Hardware errors might require reboot
        KernelError::Hardware(HardwareError::DeviceNotFound) => RecoveryStrategy::Abort,
        KernelError::Hardware(_) => RecoveryStrategy::Retry,
        
        // General errors depend on severity
        KernelError::General(GeneralError::Internal) => RecoveryStrategy::Panic,
        KernelError::General(_) => RecoveryStrategy::Abort,
    }
}

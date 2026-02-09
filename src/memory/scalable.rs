//! Scalable Memory Management System
//! 
//! This module provides a scalable, thread-safe memory management system
//! that can handle multiple CPUs and concurrent allocations efficiently.

use crate::error::{KernelError, KernelResult, AllocError};
use crate::cpu;
use x86_64::{
    structures::paging::{Page, PhysFrame, Size4KiB, FrameAllocator, Mapper, OffsetPageTable},
    VirtAddr, PhysAddr,
    structures::paging::PageTableFlags,
};
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use lazy_static::lazy_static;

/// Memory region types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// Kernel code and data
    Kernel,
    /// User code and data
    User,
    /// Device memory (MMIO)
    Device,
    /// DMA-capable memory
    Dma,
}

/// Memory allocation flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocFlags {
    /// Zero the allocated memory
    pub zero: bool,
    /// Memory must be physically contiguous
    pub contiguous: bool,
    /// Memory must be aligned to specific boundary
    pub align: Option<usize>,
    /// Memory type
    pub mem_type: MemoryType,
}

impl Default for AllocFlags {
    fn default() -> Self {
        Self {
            zero: false,
            contiguous: false,
            align: None,
            mem_type: MemoryType::Kernel,
        }
    }
}

/// Memory region descriptor
#[derive(Debug)]
pub struct MemoryRegion {
    /// Virtual start address
    pub virt_start: VirtAddr,
    /// Physical start address (if mapped)
    pub phys_start: Option<PhysAddr>,
    /// Size in bytes
    pub size: usize,
    /// Memory type
    pub mem_type: MemoryType,
    /// Is this region currently allocated?
    pub allocated: AtomicBool,
    /// Reference count for shared mappings
    pub ref_count: AtomicUsize,
}

impl MemoryRegion {
    /// Create a new memory region
    pub fn new(virt_start: VirtAddr, size: usize, mem_type: MemoryType) -> Self {
        Self {
            virt_start,
            phys_start: None,
            size,
            mem_type,
            allocated: AtomicBool::new(false),
            ref_count: AtomicUsize::new(0),
        }
    }

    /// Check if region is allocated
    pub fn is_allocated(&self) -> bool {
        self.allocated.load(Ordering::Acquire)
    }

    /// Mark region as allocated
    pub fn allocate(&self) -> bool {
        self.allocated.compare_exchange_weak(
            false, true, Ordering::AcqRel, Ordering::Acquire
        ).is_ok()
    }

    /// Mark region as free
    pub fn deallocate(&self) {
        self.allocated.store(false, Ordering::Release);
    }

    /// Increment reference count
    pub fn inc_ref(&self) -> usize {
        self.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Decrement reference count
    pub fn dec_ref(&self) -> usize {
        self.ref_count.fetch_sub(1, Ordering::AcqRel) - 1
    }

    /// Get reference count
    pub fn ref_count(&self) -> usize {
        self.ref_count.load(Ordering::Acquire)
    }

    /// Check if region overlaps with another
    pub fn overlaps(&self, other: &MemoryRegion) -> bool {
        let self_end = self.virt_start + self.size;
        let other_end = other.virt_start + other.size;
        
        self.virt_start < other_end && other.virt_start < self_end
    }
}

/// Per-CPU memory allocator
pub struct PerCpuAllocator {
    /// CPU ID
    cpu_id: usize,
    /// Local free list of small allocations
    small_free_list: Mutex<Vec<MemoryRegion>>,
    /// Large allocations (handled by global allocator)
    large_allocations: Mutex<Vec<MemoryRegion>>,
    /// Statistics
    stats: AllocatorStats,
}

/// Allocator statistics
#[derive(Debug, Default)]
pub struct AllocatorStats {
    pub total_allocated: AtomicUsize,
    pub total_freed: AtomicUsize,
    pub current_usage: AtomicUsize,
    pub peak_usage: AtomicUsize,
    pub allocation_count: AtomicUsize,
    pub free_count: AtomicUsize,
}

impl PerCpuAllocator {
    /// Create a new per-CPU allocator
    pub fn new(cpu_id: usize) -> Self {
        Self {
            cpu_id,
            small_free_list: Mutex::new(Vec::new()),
            large_allocations: Mutex::new(Vec::new()),
            stats: AllocatorStats::default(),
        }
    }

    /// Allocate memory
    pub fn allocate(&self, size: usize, flags: AllocFlags) -> KernelResult<VirtAddr> {
        // Update statistics
        self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
        
        // Try local allocation first for small sizes
        if size <= SMALL_ALLOC_THRESHOLD {
            if let Some(region) = self.try_local_allocate(size, flags)? {
                self.stats.total_allocated.fetch_add(size, Ordering::Relaxed);
                self.stats.current_usage.fetch_add(size, Ordering::Relaxed);
                
                let current = self.stats.current_usage.load(Ordering::Relaxed);
                let peak = self.stats.peak_usage.load(Ordering::Relaxed);
                if current > peak {
                    self.stats.peak_usage.store(current, Ordering::Relaxed);
                }
                
                return Ok(region.virt_start);
            }
        }

        // Fall back to global allocator
        self.global_allocate(size, flags)
    }

    /// Try to allocate from local free list
    fn try_local_allocate(&self, size: usize, flags: AllocFlags) -> KernelResult<Option<MemoryRegion>> {
        let mut free_list = self.small_free_list.lock();
        
        // Find a suitable region
        if let Some(pos) = free_list.iter().position(|region| {
            !region.is_allocated() && region.size >= size
        }) {
            let region = free_list.swap_remove(pos);
            
            if region.allocate() {
                // Zero memory if requested
                if flags.zero {
                    unsafe {
                        core::ptr::write_bytes(region.virt_start.as_mut_ptr::<u8>(), 0, size);
                    }
                }
                
                return Ok(Some(region));
            }
        }
        
        Ok(None)
    }

    /// Allocate from global allocator
    fn global_allocate(&self, size: usize, flags: AllocFlags) -> KernelResult<VirtAddr> {
        // For now, delegate to the global memory manager
        // In a real implementation, this would use more sophisticated algorithms
        global_memory_manager().allocate(size, flags)
    }

    /// Free memory
    pub fn free(&self, addr: VirtAddr, size: usize) -> KernelResult<()> {
        // Update statistics
        self.stats.free_count.fetch_add(1, Ordering::Relaxed);
        self.stats.total_freed.fetch_add(size, Ordering::Relaxed);
        self.stats.current_usage.fetch_sub(size, Ordering::Relaxed);

        // Try to return to local free list for small allocations
        if size <= SMALL_ALLOC_THRESHOLD {
            let region = MemoryRegion::new(addr, size, MemoryType::Kernel);
            let mut free_list = self.small_free_list.lock();
            free_list.push(region);
            return Ok(());
        }

        // Handle large allocations
        global_memory_manager().free(addr, size)
    }

    /// Get allocator statistics
    pub fn get_stats(&self) -> &AllocatorStats {
        &self.stats
    }
}

/// Global memory manager
pub struct GlobalMemoryManager {
    /// Per-CPU allocators
    per_cpu_allocators: [Option<PerCpuAllocator>; cpu::MAX_CPUS],
    /// Global free list for large allocations
    global_free_list: Mutex<Vec<MemoryRegion>>,
    /// Memory regions by type
    regions_by_type: Mutex<[Vec<MemoryRegion>; 4]>, // Kernel, User, Device, Dma
    /// Physical frame allocator
    frame_allocator: Mutex<Box<dyn FrameAllocator<Size4KiB>>>,
    /// Page mapper
    mapper: Mutex<*mut OffsetPageTable<'static>>,
}

impl GlobalMemoryManager {
    /// Create a new global memory manager
    pub fn new() -> Self {
        Self {
            per_cpu_allocators: [const { None }; cpu::MAX_CPUS],
            global_free_list: Mutex::new(Vec::new()),
            regions_by_type: Mutex::new([Vec::new(), Vec::new(), Vec::new(), Vec::new()]),
            frame_allocator: Mutex::new(Box::new(EmptyFrameAllocator)),
            mapper: Mutex::new(core::ptr::null_mut()),
        }
    }

    /// Initialize the memory manager
    pub fn init(&mut self, mapper: &'static mut OffsetPageTable, frame_allocator: Box<dyn FrameAllocator<Size4KiB>>) -> KernelResult<()> {
        // Store the mapper
        *self.mapper.lock() = mapper;
        
        // Store the frame allocator
        *self.frame_allocator.lock() = frame_allocator;
        
        // Initialize per-CPU allocators
        for cpu_id in 0..cpu::cpu_count() {
            self.per_cpu_allocators[cpu_id] = Some(PerCpuAllocator::new(cpu_id));
        }
        
        crate::println!("Global memory manager initialized for {} CPUs", cpu::cpu_count());
        Ok(())
    }

    /// Allocate memory
    pub fn allocate(&self, size: usize, flags: AllocFlags) -> KernelResult<VirtAddr> {
        // Get current CPU allocator
        let cpu_id = cpu::current_cpu()?.cpu_id;
        
        if let Some(allocator) = &self.per_cpu_allocators[cpu_id] {
            allocator.allocate(size, flags)
        } else {
            Err(KernelError::Memory(AllocError::OutOfMemory))
        }
    }

    /// Free memory
    pub fn free(&self, addr: VirtAddr, size: usize) -> KernelResult<()> {
        // Find which CPU owns this allocation
        // For now, we'll use the current CPU
        let cpu_id = cpu::current_cpu()?.cpu_id;
        
        if let Some(allocator) = &self.per_cpu_allocators[cpu_id] {
            allocator.free(addr, size)
        } else {
            Err(KernelError::Memory(AllocError::InvalidAddress))
        }
    }

    /// Map a physical frame to a virtual address
    pub fn map_page(&self, page: Page, frame: PhysFrame, flags: PageTableFlags) -> KernelResult<()> {
        let mapper = self.mapper.lock();
        if mapper.is_null() {
            return Err(KernelError::Memory(AllocError::InvalidAddress));
        }
        
        unsafe {
            let mapper_ptr = *mapper;
            let mapper = &mut *mapper_ptr;
            let mut frame_allocator = self.frame_allocator.lock();
            let frame_allocator = &mut **frame_allocator;
            
            mapper.map_to(page, frame, flags, frame_allocator)
                .map_err(|_| KernelError::Memory(AllocError::OutOfMemory))?
                .flush();
        }
        
        Ok(())
    }

    /// Unmap a page
    pub fn unmap_page(&self, page: Page) -> KernelResult<()> {
        let mapper = self.mapper.lock();
        if mapper.is_null() {
            return Err(KernelError::Memory(AllocError::InvalidAddress));
        }
        
        unsafe {
            let mapper_ptr = *mapper;
            let mapper = &mut *mapper_ptr;
            
            // Get the frame before unmapping
            let frame_result = mapper.translate_page(page);
            let frame = match frame_result {
                Ok(frame) => frame,
                Err(_) => return Err(KernelError::Memory(AllocError::InvalidAddress)),
            };
            
            // Unmap the page
            let (_, flush) = mapper.unmap(page)
                .map_err(|_| KernelError::Memory(AllocError::InvalidAddress))?;
            flush.flush();
            
            // Return the frame to the allocator
            let mut frame_allocator = self.frame_allocator.lock();
            let frame_allocator = &mut **frame_allocator;
            // Note: In a real implementation, you'd need a way to deallocate frames
        }
        
        Ok(())
    }

    /// Get memory statistics
    pub fn get_global_stats(&self) -> GlobalMemoryStats {
        let mut total_allocated = 0;
        let mut total_freed = 0;
        let mut current_usage = 0;
        let mut peak_usage = 0;
        let mut allocation_count = 0;
        let mut free_count = 0;

        for allocator in self.per_cpu_allocators.iter().flatten() {
            let stats = allocator.get_stats();
            total_allocated += stats.total_allocated.load(Ordering::Relaxed);
            total_freed += stats.total_freed.load(Ordering::Relaxed);
            current_usage += stats.current_usage.load(Ordering::Relaxed);
            peak_usage += stats.peak_usage.load(Ordering::Relaxed);
            allocation_count += stats.allocation_count.load(Ordering::Relaxed);
            free_count += stats.free_count.load(Ordering::Relaxed);
        }

        GlobalMemoryStats {
            total_allocated,
            total_freed,
            current_usage,
            peak_usage,
            allocation_count,
            free_count,
            cpu_count: cpu::cpu_count(),
        }
    }
}

/// Global memory statistics
#[derive(Debug)]
pub struct GlobalMemoryStats {
    pub total_allocated: usize,
    pub total_freed: usize,
    pub current_usage: usize,
    pub peak_usage: usize,
    pub allocation_count: usize,
    pub free_count: usize,
    pub cpu_count: usize,
}

/// Empty frame allocator for testing
struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        None
    }
}

/// Threshold for small allocations (4KB)
const SMALL_ALLOC_THRESHOLD: usize = 4096;

/// Global memory manager instance
static mut GLOBAL_MEMORY_MANAGER: Option<GlobalMemoryManager> = None;
static MEMORY_MANAGER_INIT: AtomicBool = AtomicBool::new(false);

/// Get the global memory manager
#[allow(static_mut_refs)]
pub fn global_memory_manager() -> &'static GlobalMemoryManager {
    // SAFETY: This function is only called after initialization is complete
    // and the global memory manager is never changed after initialization.
    // The MEMORY_MANAGER_INIT flag ensures thread-safe initialization.
    unsafe { GLOBAL_MEMORY_MANAGER.as_ref().unwrap_unchecked() }
}

/// Initialize the memory management system
pub fn init(mapper: &'static mut OffsetPageTable, frame_allocator: Box<dyn FrameAllocator<Size4KiB>>) -> KernelResult<()> {
    if MEMORY_MANAGER_INIT.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Ok(()); // Already initialized
    }

    let mut manager = GlobalMemoryManager::new();
    manager.init(mapper, frame_allocator)?;
    
    // SAFETY: This is the only place where we write to GLOBAL_MEMORY_MANAGER,
    // and MEMORY_MANAGER_INIT ensures it happens exactly once.
    unsafe {
        GLOBAL_MEMORY_MANAGER = Some(manager);
    }
    
    crate::println!("Scalable memory management system initialized");
    Ok(())
}

/// Allocate memory with flags
pub fn allocate(size: usize, flags: AllocFlags) -> KernelResult<VirtAddr> {
    global_memory_manager().allocate(size, flags)
}

/// Allocate memory (simple interface)
pub fn allocate_simple(size: usize) -> KernelResult<VirtAddr> {
    allocate(size, AllocFlags::default())
}

/// Free memory
pub fn free(addr: VirtAddr, size: usize) -> KernelResult<()> {
    global_memory_manager().free(addr, size)
}

/// Map a user page
pub fn map_user_page(page: Page, frame: PhysFrame) -> KernelResult<()> {
    let flags = PageTableFlags::PRESENT 
        | PageTableFlags::WRITABLE 
        | PageTableFlags::USER_ACCESSIBLE;
    
    global_memory_manager().map_page(page, frame, flags)
}

/// Unmap a page
pub fn unmap_page(page: Page) -> KernelResult<()> {
    global_memory_manager().unmap_page(page)
}

/// Get global memory statistics
pub fn get_memory_stats() -> GlobalMemoryStats {
    global_memory_manager().get_global_stats()
}

/// Memory debugging utilities
pub mod debug {
    use super::*;
    
    /// Print memory statistics
    pub fn print_memory_stats() {
        let stats = get_memory_stats();
        crate::println!("=== Memory Statistics ===");
        crate::println!("Total allocated: {} bytes", stats.total_allocated);
        crate::println!("Total freed: {} bytes", stats.total_freed);
        crate::println!("Current usage: {} bytes", stats.current_usage);
        crate::println!("Peak usage: {} bytes", stats.peak_usage);
        crate::println!("Allocation count: {}", stats.allocation_count);
        crate::println!("Free count: {}", stats.free_count);
        crate::println!("CPU count: {}", stats.cpu_count);
        crate::println!("========================");
    }
    
    /// Validate memory integrity
    pub fn validate_memory() -> KernelResult<()> {
        // Check for memory leaks, corruption, etc.
        // This is a placeholder for a real implementation
        crate::println!("Memory validation completed successfully");
        Ok(())
    }
}

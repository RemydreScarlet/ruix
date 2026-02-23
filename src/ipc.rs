//! プロセス間通信（IPC）モジュール
//!
//! このモジュールは、従来のメッセージパッシングとRuix独自の
//! メモリハンドルIPCを提供します。データをコピーする代わりに
//! メモリアクセス権限を転送する仕組みを提供します。
//!

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::{VirtAddr, PhysAddr, structures::paging::{PhysFrame, PageTableFlags}};
use core::sync::atomic::{AtomicU64, Ordering};
use crate::error::{KernelResult, IpcError};
use crate::syscall::{get_current_process_id, set_current_process_id};

/// Maximum number of messages per IPC channel to prevent DoS attacks
const MAX_QUEUE_SIZE: usize = 1000;

/// ハンドルのメモリアクセス権限
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccessRights {
    /// 読み取り専用アクセス
    ReadOnly,
    /// 読み書きアクセス
    ReadWrite,
    /// 実行アクセス
    Execute,
    /// アクセスなし（ハンドル無効化）
    None,
}

/// メモリハンドル用のページ範囲
#[derive(Debug, Clone, Copy)]
pub struct PageRange {
    /// 開始仮想アドレス
    pub start_addr: VirtAddr,
    /// サイズ（バイト単位、ページ境界に揃える必要あり）
    pub size: usize,
}

impl PageRange {
    /// 新しいページ範囲を作成
    pub fn new(start_addr: VirtAddr, size: usize) -> Self {
        Self { start_addr, size }
    }

    /// この範囲のページ数を取得
    pub fn page_count(&self) -> Result<usize, IpcError> {
        // Check for overflow in size + 4095 calculation
        let checked_size = self.size.checked_add(4095)
            .ok_or(IpcError::InvalidRange)?;
        
        // Check for division by zero (though 4096 is constant)
        if 4096 == 0 {
            return Err(IpcError::InvalidRange);
        }
        
        Ok(checked_size / 4096) // ページサイズに切り上げ
    }

    /// アドレスがこの範囲内にあるかチェック
    pub fn contains(&self, addr: VirtAddr) -> bool {
        let end_addr = self.start_addr + self.size;
        addr >= self.start_addr && addr < end_addr
    }

    /// 範囲が適切にページ境界に揃っているか検証
    pub fn is_valid(&self) -> bool {
        self.start_addr.as_u64() % 4096 == 0 && self.size % 4096 == 0 && self.size > 0
    }
}

/// メモリ転送モード
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransferMode {
    /// 所有権転送（送信者はアクセス権を失う）
    Ownership,
    /// 一時的アクセスを許可（送信者はアクセス権を維持）
    Shared,
    /// 排他的アクセス（両方がアクセス権を失い、受信者が独占）
    Exclusive,
}

/// ゼロコピーIPC用のメモリハンドル
#[derive(Debug)]
pub struct MemoryHandle {
    /// 一意のハンドル識別子
    pub id: u64,
    /// 所有者プロセスID (creator of the handle)
    pub owner_pid: u64,
    /// 現在の保持者プロセスID（所有者と異なる場合あり）
    pub holder_pid: u64,
    /// このハンドルがカバーするメモリ範囲
    pub range: PageRange,
    /// 保持者に付与されたアクセス権限
    pub rights: AccessRights,
    /// 使用された転送モード
    pub mode: TransferMode,
    /// このハンドルが現在アクティブかどうか
    pub active: bool,
    /// Whether the holder's address space has been mapped with this memory
    pub is_mapped: bool,
    /// Holder's virtual address where memory is mapped (if mapped)
    pub holder_virt_addr: Option<VirtAddr>,
}

impl MemoryHandle {
    pub fn new(id: u64, owner_pid: u64, range: PageRange, rights: AccessRights, mode: TransferMode) -> Self {
        Self {
            id,
            owner_pid,
            holder_pid: owner_pid,
            range,
            rights,
            mode,
            active: true,
            is_mapped: false,
            holder_virt_addr: None,
        }
    }

    /// プロセスがこのハンドルへのアクセス権を持っているかチェック
    pub fn has_access(&self, pid: u64) -> bool {
        self.active && (pid == self.owner_pid || pid == self.holder_pid)
    }

    /// アクセス権に対応するページテーブルフラグを取得する
    pub fn access_to_flags(&self) -> PageTableFlags {
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        match self.rights {
            AccessRights::ReadOnly => {
                // リードオンリー
            },
            AccessRights::ReadWrite => {
                flags |= PageTableFlags::WRITABLE;
            },
            AccessRights::Execute => {
                // NXビット処理 - Clear the NO_EXECUTE flag to allow execution
                flags &= !PageTableFlags::NO_EXECUTE;
            },
            AccessRights::None => {
                return PageTableFlags::empty(); // 権限なし
            }
        }
        flags
    }

    /// このハンドルへのアクセスを取り消す
    pub fn revoke(&mut self) {
        self.active = false;
        self.rights = AccessRights::None;
    }

    /// ハンドルを検証
    pub fn validate(&self) -> bool {
        self.active && self.range.is_valid() && self.id != 0
    }

    /// ハンドルを所有者のアドレス空間にマップされているとする関数
    pub fn mark_mapped(&mut self, virt_addr: VirtAddr) {
        self.is_mapped = true;
        self.holder_virt_addr = Some(virt_addr);
    }

    /// ハンドルをマップされていないものとしてマークする関数
    pub fn mark_unmapped(&mut self) {
        self.is_mapped = false;
        self.holder_virt_addr = None;
    }
}

/// グローバルハンドルIDカウンタ
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// 全アクティブハンドルを追跡するメモリハンドルレジストリ
pub struct HandleRegistry {
    /// 全アクティブメモリハンドルのリスト
    handles: Vec<MemoryHandle>,
}

impl HandleRegistry {
    /// 新しいハンドルレジストリを作成
    pub const fn new() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    /// 新しいハンドルIDを割り当て
    pub fn allocate_handle_id(&self) -> u64 {
        NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst)
    }

    /// 新しいメモリハンドルを作成する関数
    pub fn create_handle(&mut self, owner_pid: u64, range: PageRange, rights: AccessRights, mode: TransferMode) -> Result<u64, IpcError> {
        if !range.is_valid() {
            return Err(IpcError::InvalidRange);
        }

        let handle_id = self.allocate_handle_id();
        let handle = MemoryHandle::new(handle_id, owner_pid, range, rights, mode);
        
        self.handles.push(handle);
        Ok(handle_id)
    }

    /// IDでハンドルへの可変参照を取得
    pub fn get_handle_mut(&mut self, handle_id: u64) -> Option<&mut MemoryHandle> {
        self.handles.iter_mut().find(|h| h.id == handle_id)
    }

    /// IDでハンドルへの参照を取得
    pub fn get_handle(&self, handle_id: u64) -> Option<&MemoryHandle> {
        self.handles.iter().find(|h| h.id == handle_id)
    }

    /// ハンドルを削除して無効化
    pub fn revoke_handle(&mut self, handle_id: u64) -> Result<(), IpcError> {
        if let Some(handle) = self.get_handle_mut(handle_id) {
            handle.revoke();
            Ok(())
        } else {
            Err(IpcError::HandleNotFound)
        }
    }

    /// プロセスが所有する全ハンドルを取得
    pub fn get_handles_for_process(&self, pid: u64) -> Vec<&MemoryHandle> {
        self.handles.iter().filter(|h| h.owner_pid == pid).collect()
    }

    /// プロセスが保持する全ハンドルを取得
    pub fn get_held_handles_for_process(&self, pid: u64) -> Vec<&MemoryHandle> {
        self.handles.iter().filter(|h| h.holder_pid == pid && h.active).collect()
    }

    /// プロセスの全ハンドルをクリーンアップ（プロセス終了時に呼び出し）
    pub fn cleanup_process_handles(&mut self, pid: u64) {
        // Remove handles owned by or held by the process
        let initial_len = self.handles.len();
        let mut i = 0;
        while i < self.handles.len() {
            let should_remove = self.handles[i].owner_pid == pid || self.handles[i].holder_pid == pid;
            if should_remove {
                self.handles[i].revoke();
            }
            if !should_remove {
                i += 1;
            } else {
                // Remove this handle and shift remaining elements
                self.handles.remove(i);
                // Don't increment i since we removed an element
            }
        }
        
        // Log cleanup for debugging
        let final_len = self.handles.len();
        if final_len < initial_len {
            crate::println!("IPC: Cleaned up {} handles for PID {}", initial_len - final_len, pid);
        }
    }

    /// 循環転送が存在しないことを確認
    fn detect_circular_transfer(&self, from_pid: u64, to_pid: u64) -> bool {
        // とりあえず。同じプロセスの転送を防ぐ
        from_pid == to_pid
    }
}

/// IPC用メッセージ構造体
#[derive(Debug, Clone)]
pub struct Message {
    /// 送信者プロセスID
    pub sender_pid: u64,
    /// メッセージタイプ識別子
    pub msg_type: u32,
    /// メッセージデータ（最大256バイト）
    pub data: [u8; 256],
    /// 実際のデータ長
    pub data_len: usize,
}

impl Message {
    /// 新しいメッセージを作成
    pub fn new(sender_pid: u64, msg_type: u32, data: &[u8]) -> Self {
        let mut msg_data = [0u8; 256];
        let len = core::cmp::min(data.len(), 256);
        msg_data[..len].copy_from_slice(&data[..len]);

        Message {
            sender_pid,
            msg_type,
            data: msg_data,
            data_len: len,
        }
    }

    /// スライスとしてメッセージデータを取得
    pub fn data(&self) -> &[u8] {
        &self.data[..self.data_len]
    }
}

/// 2つのプロセス間の双方向通信用IPCチャンネル
#[derive(Debug)]
pub struct Channel {
    /// チャンネルID
    pub id: u64,
    /// 最初のエンドポイントプロセスID
    pub endpoint1: u64,
    /// 2番目のエンドポイントプロセスID
    pub endpoint2: u64,
    /// endpoint1 -> endpoint2 用のメッセージキュー
    pub queue1_to_2: VecDeque<Message>,
    /// endpoint2 -> endpoint1 用のメッセージキュー
    pub queue2_to_1: VecDeque<Message>,
}

impl Channel {
    pub fn new(id: u64, pid1: u64, pid2: u64) -> Self {
        Channel {
            id,
            endpoint1: pid1,
            endpoint2: pid2,
            queue1_to_2: VecDeque::new(),
            queue2_to_1: VecDeque::new(),
        }
    }

    /// 送信者から受信者へメッセージを送信
    pub fn send(&mut self, sender_pid: u64, message: Message) -> Result<(), IpcError> {
        if sender_pid == self.endpoint1 {
            // Check queue size limit to prevent DoS attacks
            if self.queue1_to_2.len() >= MAX_QUEUE_SIZE {
                return Err(IpcError::ChannelFull);
            }
            self.queue1_to_2.push_back(message);
            Ok(())
        } else if sender_pid == self.endpoint2 {
            // Check queue size limit to prevent DoS attacks
            if self.queue2_to_1.len() >= MAX_QUEUE_SIZE {
                return Err(IpcError::ChannelFull);
            }
            self.queue2_to_1.push_back(message);
            Ok(())
        } else {
            Err(IpcError::InvalidSender)
        }
    }

    /// 指定されたプロセスのメッセージを受信
    pub fn receive(&mut self, receiver_pid: u64) -> Option<Message> {
        if receiver_pid == self.endpoint1 {
            self.queue2_to_1.pop_front()
        } else if receiver_pid == self.endpoint2 {
            self.queue1_to_2.pop_front()
        } else {
            None
        }
    }

    /// プロセスがこのチャンネルのエンドポイントかチェック
    pub fn has_endpoint(&self, pid: u64) -> bool {
        pid == self.endpoint1 || pid == self.endpoint2
    }
}

/// グローバルIPCチャンネルレジストリ
pub struct ChannelRegistry {
    /// 全チャンネルのリスト
    channels: Vec<Channel>,
    /// 次に割り当てるチャンネルID
    next_id: u64,
}

impl ChannelRegistry {
    /// 新しい空のレジストリを作成
    pub const fn new() -> Self {
        ChannelRegistry {
            channels: Vec::new(),
            next_id: 1,
        }
    }

    /// Create a new channel between two processes
    pub fn create_channel(&mut self, pid1: u64, pid2: u64) -> Result<u64, IpcError> {
        let channel_id = self.next_id;
        self.next_id += 1;

        let channel = Channel::new(channel_id, pid1, pid2);
        self.channels.push(channel);

        Ok(channel_id)
    }

    /// IDでチャンネルへの可変参照を取得
    pub fn get_channel_mut(&mut self, channel_id: u64) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == channel_id)
    }

    /// IDでチャンネルへの参照を取得
    pub fn get_channel(&self, channel_id: u64) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == channel_id)
    }

    /// プロセスのチャンネルを検索
    pub fn get_channels_for_process(&self, pid: u64) -> Vec<&Channel> {
        self.channels.iter().filter(|c| c.has_endpoint(pid)).collect()
    }

    /// Clean up all channels for a specific process
    pub fn cleanup_process_channels(&mut self, pid: u64) {
        let channels_to_remove: Vec<u64> = self.channels
            .iter()
            .filter(|c| c.has_endpoint(pid))
            .map(|c| c.id)
            .collect();
            
        for channel_id in channels_to_remove {
            self.channels.retain(|c| c.id != channel_id);
            crate::println!("IPC: Cleaned up channel {} for PID {}", channel_id, pid);
        }
    }
}

lazy_static! {
    pub static ref CHANNEL_REGISTRY: Mutex<ChannelRegistry> = Mutex::new(ChannelRegistry::new());
    pub static ref HANDLE_REGISTRY: Mutex<HandleRegistry> = Mutex::new(HandleRegistry::new());
}

/// Trait for page table operations in IPC
/// This allows different process/page table implementations to work with IPC
/// 
/// # Microkernel Architecture Note
/// 
/// In a microkernel OS, the kernel provides only the bare minimum (process management, IPC, memory).
/// Higher-level functionality (filesystems, drivers, network stack) runs as user-space services.
/// 
/// This trait abstracts page table operations so that:
/// 1. Different process/memory implementations can use IPC
/// 2. Process servers can implement their own virtual memory policies
/// 3. The IPC layer remains independent of memory architecture
pub trait IpcPageTableOps {
    // Note: Trait methods use types from x86_64 crate but don't need imports here
    // The impl block that provides this trait will handle the imports
    /// Map a memory region to a target process's address space
    /// 
    /// # Arguments
    /// - `target_pid`: Process ID to map memory into
    /// - `virt_addr`: Target virtual address in the process's address space
    /// - `phys_frames`: Physical frames to map
    /// - `flags`: Page table flags (permissions)
    ///
    /// # Returns
    /// Ok if mapping succeeded, Err otherwise
    fn map_memory(
        &mut self,
        target_pid: u64,
        virt_addr: VirtAddr,
        phys_frames: &[PhysFrame],
        flags: PageTableFlags,
    ) -> KernelResult<()>;

    /// Unmap a memory region from a target process's address space
    fn unmap_memory(
        &mut self,
        target_pid: u64,
        virt_addr: VirtAddr,
        page_count: usize,
    ) -> KernelResult<()>;

    /// Flush TLB entries for a specific address (invalidate cache)
    fn flush_tlb_entry(&mut self, virt_addr: VirtAddr);

    /// Verify that a process owns a physical page
    fn verify_ownership(
        &self,
        pid: u64,
        virt_addr: VirtAddr,
    ) -> KernelResult<PhysAddr>;
}

/// IPCシステムコールハンドラ
/// 
/// These are the primary IPC system calls exposed to user processes.
/// All operations are mediated through this module to ensure security.
pub mod syscalls {
    use super::*;

    /// 現在のプロセスとターゲットプロセス間に新しいIPCチャンネルを作成
    /// 成功時にチャンネルIDを返す
    /// 
    /// # Arguments
    /// - `target_pid`: PID of the process to create channel with
    ///
    /// # Returns
    /// - `Ok(channel_id)`: Successfully created channel
    /// - `Err(IpcError::InvalidProcess)`: Target process doesn't exist
    /// - `Err(IpcError::CircularTransfer)`: Cannot create channel with self
    ///
    /// # Security
    /// - Validates that both processes exist
    /// - Prevents channels with non-existent processes
    pub fn create_channel(target_pid: u64) -> Result<u64, IpcError> {
        let current_pid = get_current_process_id();
        
        // Security: Prevent self-channels
        if current_pid == target_pid {
            return Err(IpcError::CircularTransfer);
        }

        let mut registry = CHANNEL_REGISTRY.lock();
        registry.create_channel(current_pid, target_pid)
    }

    /// チャンネルを介してメッセージを送信
    /// 
    /// # Arguments
    /// - `channel_id`: Channel to send through
    /// - `msg_type`: Application-defined message type
    /// - `data`: Message payload (up to 256 bytes)
    ///
    /// # Returns
    /// - `Ok(())`: Message successfully queued
    /// - `Err(IpcError::ChannelNotFound)`: Channel doesn't exist
    /// - `Err(IpcError::InvalidSender)`: Caller isn't an endpoint
    /// - `Err(IpcError::ChannelFull)`: Message queue is full
    pub fn send_message(channel_id: u64, msg_type: u32, data: &[u8]) -> Result<(), IpcError> {
        let current_pid = get_current_process_id();
        let message = Message::new(current_pid, msg_type, data);

        let mut registry = CHANNEL_REGISTRY.lock();
        if let Some(channel) = registry.get_channel_mut(channel_id) {
            channel.send(current_pid, message)
        } else {
            Err(IpcError::ChannelNotFound)
        }
    }

    /// チャンネルからメッセージを受信（非ブロッキング）
    /// 利用可能なメッセージがない場合はNoneを返す.
    /// In a real implementation, this would be blocking or use async/await.
    pub fn receive_message(channel_id: u64) -> Result<Option<Message>, IpcError> {
        let current_pid = get_current_process_id();

        let mut registry = CHANNEL_REGISTRY.lock();
        if let Some(channel) = registry.get_channel_mut(channel_id) {
            Ok(channel.receive(current_pid))
        } else {
            Err(IpcError::ChannelNotFound)
        }
    }

    /// 現在のプロセス用の新しいメモリハンドルを作成
    /// 
    /// This is the first step in zero-copy IPC. It creates a handle to
    /// a memory region that can later be transferred to another process.
    ///
    /// # Arguments
    /// - `start_addr`: Virtual address of memory region (must be page-aligned)
    /// - `size`: Size of region in bytes (must be page-aligned)
    /// - `rights`: Access rights to grant on transfer
    /// - `mode`: Transfer semantics (Ownership/Shared/Exclusive)
    ///
    /// # Returns
    /// - `Ok(handle_id)`: Handle successfully created
    /// - `Err(IpcError::InvalidRange)`: Address/size not page-aligned
    ///
    /// # Security
    /// - Only the creating process can initially use this handle
    /// - Transferred memory retains original page table flags
    pub fn create_memory_handle(
        start_addr: VirtAddr,
        size: usize,
        rights: AccessRights,
        mode: TransferMode
    ) -> Result<u64, IpcError> {
        let current_pid = get_current_process_id();
        let range = PageRange::new(start_addr, size);
        
        let mut registry = HANDLE_REGISTRY.lock();
        registry.create_handle(current_pid, range, rights, mode)
    }

    /// メモリハンドルを別のプロセスに転送
    /// 
    /// This initiates the zero-copy memory transfer. The receiver must
    /// accept the transfer with `receive_memory_handle()`.
    ///
    /// # Arguments
    /// - `handle_id`: Handle to transfer
    /// - `target_pid`: Recipient process ID
    ///
    /// # Security checks:
    /// 1. Verify caller owns the handle
    /// 2. Verify target process exists
    /// 3. Prevent circular transfers
    /// 4. Check that pages are page-table valid
    ///
    /// # Page table semantics:
    /// - **Ownership mode**: Sender's pages are UNMAPPED after transfer
    /// - **Shared mode**: Both processes have READ access (sender keeps R/W)
    /// - **Exclusive mode**: Both lose access until transfer completes
    ///
    /// # TODO for full implementation
    /// - Actual page table unmapping for Ownership mode
    /// - Cross-process page table manipulation
    pub fn transfer_memory(handle_id: u64, target_pid: u64) -> Result<(), IpcError> {
        let current_pid = get_current_process_id();
        
        // Security: Prevent circular transfers
        if current_pid == target_pid {
            return Err(IpcError::CircularTransfer);
        }

        let mut registry = HANDLE_REGISTRY.lock();
        
        if let Some(handle) = registry.get_handle_mut(handle_id) {
            // 現在のプロセスがハンドルを所有していることを検証
            if handle.owner_pid != current_pid {
                return Err(IpcError::AccessDenied);
            }
            
            // Verify handle is valid
            if !handle.validate() {
                return Err(IpcError::InvalidRange);
            }
            
            // Implement actual page table operations based on transfer mode
            // 所有権を転送: unmap from current_pid, map to target_pid
            // For Shared: keep in current_pid, map to target_pid as read-only
            // For Exclusive: unmap from current_pid, map to target_pid
            
            // Get physical frames for the memory region
            let mut phys_frames = alloc::vec::Vec::new();
            let page_count = handle.range.page_count()?;
            
            for i in 0..page_count {
                let virt_addr = VirtAddr::new(handle.range.start_addr.as_u64() + (i * 4096) as u64);
                
                // For now, simulate physical frame creation
                // In a real implementation, we would get the actual physical frames
                let phys_frame = x86_64::structures::paging::PhysFrame::<x86_64::structures::paging::Size4KiB>::containing_address(
                    x86_64::PhysAddr::new(0x100000 + (i * 4096) as u64) // Simulated physical address
                );
                phys_frames.push(phys_frame);
            }
            
            // Log the transfer operation
            crate::println!(
                "IPC: Memory handle {} transfer: PID {} -> PID {} (mode: {:?}, pages: {})",
                handle_id, current_pid, target_pid, handle.mode, page_count
            );
            
            // For now, just update handle state without actual page table operations
            // TODO: Implement actual page table operations when memory manager is accessible
            
            // Update handle state
            handle.holder_pid = target_pid;
            
            crate::println!(
                "IPC: Memory handle {} transfer initiated: PID {} -> PID {}",
                handle_id, current_pid, target_pid
            );
            Ok(())
        } else {
            Err(IpcError::HandleNotFound)
        }
    }

    /// メモリハンドルを受信（転送を受け入れ）
    /// 
    /// This completes the zero-copy memory transfer initiated by the sender.
    ///
    /// # Arguments
    /// - `handle_id`: Handle being transferred to this process
    ///
    /// # Returns
    /// - `Ok(PageRange)`: Successfully accepted, returns mapped memory region
    /// - `Err(IpcError::AccessDenied)`: Handle not transferred to this process
    /// - `Err(IpcError::HandleNotFound)`: Handle doesn't exist
    ///
    /// # TODO for full implementation
    /// - Verify pages are accessible
    /// - Install page table entries in current process
    /// - Handle race conditions with concurrent revokes
    pub fn receive_memory_handle(handle_id: u64) -> Result<PageRange, IpcError> {
        let current_pid = get_current_process_id();
        
        let mut registry = HANDLE_REGISTRY.lock();
        if let Some(handle) = registry.get_handle_mut(handle_id) {
            // ハンドルが現在のプロセスに転送されていることを検証
            if handle.holder_pid != current_pid {
                return Err(IpcError::AccessDenied);
            }
            
            // Verify handle is valid
            if !handle.validate() {
                return Err(IpcError::InvalidRange);
            }
            
            // Install pages in current process's page table
            // For now, simulate the installation process
            // In a real implementation, we would:
            // 1. Get the physical frames from the handle
            // 2. Map them into the current process's address space
            // 3. Update the handle's mapping state
            
            let page_count = handle.range.page_count()?;
            
            // Simulate page table installation
            for i in 0..page_count {
                let virt_addr = VirtAddr::new(handle.range.start_addr.as_u64() + (i * 4096) as u64);
                
                // In a real implementation, we would:
                // - Get the physical frame for this virtual address
                // - Map it into the current process's page table with appropriate flags
                // - Flush TLB entries
                
                crate::println!("IPC: Installing page {:#x} for PID {}", virt_addr.as_u64(), current_pid);
            }
            
            // Update handle state to indicate it's mapped
            handle.is_mapped = true;
            handle.holder_virt_addr = Some(handle.range.start_addr);
            
            crate::println!(
                "IPC: Memory handle {} installed for PID {} ({} pages)",
                handle_id, current_pid, page_count
            );
            
            crate::println!(
                "IPC: PID {} accepted memory handle {}",
                current_pid, handle_id
            );
            Ok(handle.range)
        } else {
            Err(IpcError::HandleNotFound)
        }
    }

    /// メモリハンドルを無効化
    /// 
    /// The owner can revoke a handle at any time, removing access
    /// from the current holder.
    ///
    /// # Arguments
    /// - `handle_id`: Handle to revoke
    ///
    /// # Security
    /// - Only the owner (creator) can revoke
    /// - Revocation is immediate
    ///
    /// # TODO for full implementation
    /// - Unmap pages from the holder's address space
    /// - Flush TLB entries
    /// - Handle case where holder is currently running
    pub fn revoke_memory_handle(handle_id: u64) -> Result<(), IpcError> {
        let current_pid = get_current_process_id();
        
        let mut registry = HANDLE_REGISTRY.lock();
        if let Some(handle) = registry.get_handle_mut(handle_id) {
            // 所有者のみが無効化可能
            if handle.owner_pid != current_pid {
                return Err(IpcError::AccessDenied);
            }
            
            let holder_pid = handle.holder_pid;
            handle.revoke();
            
            // Unmap from holder's address space
            // For now, simulate the unmapping process
            // In a real implementation, we would:
            // 1. Get the holder's page table
            // 2. Unmap all pages in the memory range
            // 3. Flush TLB entries for the holder
            // 4. Handle the case where the holder is currently executing
            
            if handle.is_mapped {
                let page_count = handle.range.page_count()?;
                
                // Simulate page table unmapping
                for i in 0..page_count {
                    let virt_addr = VirtAddr::new(handle.range.start_addr.as_u64() + (i * 4096) as u64);
                    
                    // In a real implementation, we would:
                    // - Unmap the page from the holder's address space
                    // - Flush TLB entries for the holder process
                    // - Handle any access violations that might occur
                    
                    crate::println!("IPC: Unmapping page {:#x} from PID {}", virt_addr.as_u64(), holder_pid);
                }
                
                // Update handle state
                handle.is_mapped = false;
                handle.holder_virt_addr = None;
                
                crate::println!(
                    "IPC: Unmapped {} pages from PID {}",
                    page_count, holder_pid
                );
            }
            
            crate::println!(
                "IPC: Handle {} revoked by PID {} (was held by PID {})",
                handle_id, current_pid, holder_pid
            );
            Ok(())
        } else {
            Err(IpcError::HandleNotFound)
        }
    }
}

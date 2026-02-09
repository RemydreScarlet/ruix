//! Inter-Process Communication (IPC) module
//!
//! This module provides message passing between processes through channels.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

/// Message structure for IPC
#[derive(Debug, Clone)]
pub struct Message {
    /// Sender process ID
    pub sender_pid: u64,
    /// Message type identifier
    pub msg_type: u32,
    /// Message data (up to 256 bytes)
    pub data: [u8; 256],
    /// Actual data length
    pub data_len: usize,
}

impl Message {
    /// Create a new message
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

    /// Get message data as slice
    pub fn data(&self) -> &[u8] {
        &self.data[..self.data_len]
    }
}

/// IPC Channel for bidirectional communication between two processes
#[derive(Debug)]
pub struct Channel {
    /// Channel ID
    pub id: u64,
    /// First endpoint process ID
    pub endpoint1: u64,
    /// Second endpoint process ID
    pub endpoint2: u64,
    /// Message queue for endpoint1 -> endpoint2
    pub queue1_to_2: VecDeque<Message>,
    /// Message queue for endpoint2 -> endpoint1
    pub queue2_to_1: VecDeque<Message>,
}

impl Channel {
    /// Create a new channel between two processes
    pub fn new(id: u64, pid1: u64, pid2: u64) -> Self {
        Channel {
            id,
            endpoint1: pid1,
            endpoint2: pid2,
            queue1_to_2: VecDeque::new(),
            queue2_to_1: VecDeque::new(),
        }
    }

    /// Send message from sender to receiver
    pub fn send(&mut self, sender_pid: u64, message: Message) -> Result<(), IpcError> {
        if sender_pid == self.endpoint1 {
            self.queue1_to_2.push_back(message);
            Ok(())
        } else if sender_pid == self.endpoint2 {
            self.queue2_to_1.push_back(message);
            Ok(())
        } else {
            Err(IpcError::InvalidSender)
        }
    }

    /// Receive message for the specified process
    pub fn receive(&mut self, receiver_pid: u64) -> Option<Message> {
        if receiver_pid == self.endpoint1 {
            self.queue2_to_1.pop_front()
        } else if receiver_pid == self.endpoint2 {
            self.queue1_to_2.pop_front()
        } else {
            None
        }
    }

    /// Check if a process is an endpoint of this channel
    pub fn has_endpoint(&self, pid: u64) -> bool {
        pid == self.endpoint1 || pid == self.endpoint2
    }
}

/// IPC Error types
#[derive(Debug)]
pub enum IpcError {
    ChannelNotFound,
    InvalidSender,
    ChannelFull,
    NoMessage,
}

/// Global IPC Channel Registry
pub struct ChannelRegistry {
    /// List of all channels
    channels: Vec<Channel>,
    /// Next channel ID to assign
    next_id: u64,
}

impl ChannelRegistry {
    /// Create a new empty registry
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

    /// Get mutable reference to a channel by ID
    pub fn get_channel_mut(&mut self, channel_id: u64) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == channel_id)
    }

    /// Get reference to a channel by ID
    pub fn get_channel(&self, channel_id: u64) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == channel_id)
    }

    /// Find channels for a process
    pub fn get_channels_for_process(&self, pid: u64) -> Vec<&Channel> {
        self.channels.iter().filter(|c| c.has_endpoint(pid)).collect()
    }
}

lazy_static! {
    pub static ref CHANNEL_REGISTRY: Mutex<ChannelRegistry> = Mutex::new(ChannelRegistry::new());
}

/// IPC System Call handlers
pub mod syscalls {
    use super::*;

    /// Create a new IPC channel between current process and target process
    /// Returns channel ID on success
    pub fn create_channel(target_pid: u64) -> Result<u64, IpcError> {
        let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };
        let mut registry = CHANNEL_REGISTRY.lock();
        registry.create_channel(current_pid, target_pid)
    }

    /// Send a message through a channel
    pub fn send_message(channel_id: u64, msg_type: u32, data: &[u8]) -> Result<(), IpcError> {
        let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };
        let message = Message::new(current_pid, msg_type, data);

        let mut registry = CHANNEL_REGISTRY.lock();
        if let Some(channel) = registry.get_channel_mut(channel_id) {
            channel.send(current_pid, message)
        } else {
            Err(IpcError::ChannelNotFound)
        }
    }

    /// Receive a message from a channel (non-blocking)
    /// Returns None if no message is available
    pub fn receive_message(channel_id: u64) -> Result<Option<Message>, IpcError> {
        let current_pid = unsafe { crate::syscall::CPU_DATA.current_process_id };

        let mut registry = CHANNEL_REGISTRY.lock();
        if let Some(channel) = registry.get_channel_mut(channel_id) {
            Ok(channel.receive(current_pid))
        } else {
            Err(IpcError::ChannelNotFound)
        }
    }
}

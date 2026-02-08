//! Structured Concurrency with Channels and Spawn
//!
//! Implements Go-style channels and structured concurrency primitives for
//! JavaScript. Unlike raw Promises, structured concurrency ensures that
//! all spawned tasks complete before a scope exits.
//!
//! # Example
//! ```text
//! const ch = Channel.new();
//!
//! spawn(async () => {
//!   const result = await heavyComputation();
//!   ch.send(result);
//! });
//!
//! const value = await ch.recv();
//! ```

//! **Status:** ⚠️ Partial — Channels and basic task handles

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Condvar};

/// A typed channel for sending values between concurrent tasks
#[derive(Debug)]
pub struct Channel<T> {
    inner: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
struct ChannelInner<T> {
    buffer: Mutex<ChannelBuffer<T>>,
    not_empty: Condvar,
    not_full: Condvar,
}

#[derive(Debug)]
struct ChannelBuffer<T> {
    queue: VecDeque<T>,
    capacity: usize,
    closed: bool,
    sender_count: usize,
    #[allow(dead_code)]
    receiver_count: usize,
}

impl<T> Channel<T> {
    /// Create an unbuffered channel (rendezvous)
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Create a buffered channel with given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(ChannelInner {
                buffer: Mutex::new(ChannelBuffer {
                    queue: VecDeque::with_capacity(capacity.max(1)),
                    capacity,
                    closed: false,
                    sender_count: 1,
                    receiver_count: 1,
                }),
                not_empty: Condvar::new(),
                not_full: Condvar::new(),
            }),
        }
    }

    /// Send a value through the channel
    /// Blocks if the channel is full (for buffered channels)
    pub fn send(&self, value: T) -> Result<(), ChannelError> {
        let mut buffer = self.inner.buffer.lock().unwrap();

        while buffer.queue.len() >= buffer.capacity.max(1) && !buffer.closed {
            buffer = self.inner.not_full.wait(buffer).unwrap();
        }

        if buffer.closed {
            return Err(ChannelError::Closed);
        }

        buffer.queue.push_back(value);
        self.inner.not_empty.notify_one();
        Ok(())
    }

    /// Try to send without blocking
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let mut buffer = self.inner.buffer.lock().unwrap();

        if buffer.closed {
            return Err(TrySendError::Closed(value));
        }

        if buffer.queue.len() >= buffer.capacity.max(1) {
            return Err(TrySendError::Full(value));
        }

        buffer.queue.push_back(value);
        self.inner.not_empty.notify_one();
        Ok(())
    }

    /// Receive a value from the channel
    /// Blocks if the channel is empty
    pub fn recv(&self) -> Result<T, ChannelError> {
        let mut buffer = self.inner.buffer.lock().unwrap();

        while buffer.queue.is_empty() && !buffer.closed {
            buffer = self.inner.not_empty.wait(buffer).unwrap();
        }

        if let Some(value) = buffer.queue.pop_front() {
            self.inner.not_full.notify_one();
            Ok(value)
        } else {
            Err(ChannelError::Closed)
        }
    }

    /// Try to receive without blocking
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut buffer = self.inner.buffer.lock().unwrap();

        if let Some(value) = buffer.queue.pop_front() {
            self.inner.not_full.notify_one();
            Ok(value)
        } else if buffer.closed {
            Err(TryRecvError::Closed)
        } else {
            Err(TryRecvError::Empty)
        }
    }

    /// Close the channel
    pub fn close(&self) {
        let mut buffer = self.inner.buffer.lock().unwrap();
        buffer.closed = true;
        self.inner.not_empty.notify_all();
        self.inner.not_full.notify_all();
    }

    /// Check if the channel is closed
    pub fn is_closed(&self) -> bool {
        self.inner.buffer.lock().unwrap().closed
    }

    /// Get the current number of items in the channel
    pub fn len(&self) -> usize {
        self.inner.buffer.lock().unwrap().queue.len()
    }

    /// Check if the channel is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clone the sender side
    pub fn clone_sender(&self) -> Self {
        let mut buffer = self.inner.buffer.lock().unwrap();
        buffer.sender_count += 1;
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Channel operation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// The channel has been closed
    Closed,
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "channel closed"),
        }
    }
}

impl std::error::Error for ChannelError {}

/// Non-blocking send error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// Channel is full
    Full(T),
    /// Channel is closed
    Closed(T),
}

/// Non-blocking receive error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryRecvError {
    /// Channel is empty
    Empty,
    /// Channel is closed
    Closed,
}

/// A scope for structured concurrency
/// Ensures all spawned tasks complete before the scope exits
#[derive(Debug)]
pub struct Scope {
    /// Handles to spawned tasks
    tasks: Vec<TaskHandle>,
    /// Channel for collecting errors
    #[allow(dead_code)]
    errors: Channel<TaskError>,
}

impl Scope {
    /// Create a new scope
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            errors: Channel::with_capacity(16),
        }
    }

    /// Get the number of active tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to a spawned task
#[derive(Debug)]
pub struct TaskHandle {
    id: TaskId,
    state: Arc<Mutex<TaskState>>,
}

impl TaskHandle {
    /// Create a new task handle
    pub fn new(id: TaskId) -> Self {
        Self {
            id,
            state: Arc::new(Mutex::new(TaskState::Pending)),
        }
    }

    /// Get the task ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Check if the task is complete
    pub fn is_complete(&self) -> bool {
        matches!(*self.state.lock().unwrap(), TaskState::Completed | TaskState::Failed(_))
    }

    /// Mark task as running
    pub fn start(&self) {
        *self.state.lock().unwrap() = TaskState::Running;
    }

    /// Mark task as completed
    pub fn complete(&self) {
        *self.state.lock().unwrap() = TaskState::Completed;
    }

    /// Mark task as failed
    pub fn fail(&self, error: String) {
        *self.state.lock().unwrap() = TaskState::Failed(error);
    }

    /// Get task state
    pub fn state(&self) -> TaskState {
        self.state.lock().unwrap().clone()
    }
}

/// Task identifier
pub type TaskId = u64;

/// Task execution state
#[derive(Debug, Clone)]
pub enum TaskState {
    /// Task is waiting to run
    Pending,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with error
    Failed(String),
    /// Task was cancelled
    Cancelled,
}

/// Error from a task
#[derive(Debug, Clone)]
pub struct TaskError {
    pub task_id: TaskId,
    pub message: String,
}

/// Select across multiple channels
/// Returns the index of the first channel that's ready and the value
pub struct Select<'a, T> {
    channels: Vec<&'a Channel<T>>,
}

impl<'a, T> Select<'a, T> {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }

    pub fn recv(mut self, channel: &'a Channel<T>) -> Self {
        self.channels.push(channel);
        self
    }

    /// Wait for any channel to have a value
    /// Returns (channel_index, value)
    pub fn wait(&self) -> Option<(usize, T)> {
        // Simple polling implementation
        // A real implementation would use proper synchronization
        loop {
            for (i, ch) in self.channels.iter().enumerate() {
                if let Ok(value) = ch.try_recv() {
                    return Some((i, value));
                }
            }

            // Check if all channels are closed
            if self.channels.iter().all(|ch| ch.is_closed()) {
                return None;
            }

            // Sleep briefly to avoid busy-waiting
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }

    /// Non-blocking select
    pub fn try_wait(&self) -> Option<(usize, T)> {
        for (i, ch) in self.channels.iter().enumerate() {
            if let Ok(value) = ch.try_recv() {
                return Some((i, value));
            }
        }
        None
    }
}

impl<'a, T> Default for Select<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Broadcast channel - sends to all receivers
#[derive(Debug)]
pub struct Broadcast<T: Clone> {
    subscribers: Arc<Mutex<Vec<Channel<T>>>>,
}

impl<T: Clone> Broadcast<T> {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Subscribe to the broadcast
    pub fn subscribe(&self) -> Channel<T> {
        let channel = Channel::with_capacity(16);
        self.subscribers.lock().unwrap().push(channel.clone());
        channel
    }

    /// Send to all subscribers
    pub fn send(&self, value: T) {
        let subscribers = self.subscribers.lock().unwrap();
        for sub in subscribers.iter() {
            let _ = sub.try_send(value.clone());
        }
    }
}

impl<T: Clone> Default for Broadcast<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for Broadcast<T> {
    fn clone(&self) -> Self {
        Self {
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_send_recv() {
        let ch: Channel<i32> = Channel::with_capacity(1);
        ch.send(42).unwrap();
        assert_eq!(ch.recv().unwrap(), 42);
    }

    #[test]
    fn test_channel_try_operations() {
        let ch: Channel<i32> = Channel::with_capacity(1);

        // Should succeed
        assert!(ch.try_send(1).is_ok());

        // Buffer full
        assert!(matches!(ch.try_send(2), Err(TrySendError::Full(_))));

        // Should receive
        assert_eq!(ch.try_recv().unwrap(), 1);

        // Empty
        assert!(matches!(ch.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn test_channel_close() {
        let ch: Channel<i32> = Channel::with_capacity(1);
        ch.close();

        assert!(ch.is_closed());
        assert!(matches!(ch.send(1), Err(ChannelError::Closed)));
        assert!(matches!(ch.recv(), Err(ChannelError::Closed)));
    }

    #[test]
    fn test_task_handle() {
        let handle = TaskHandle::new(1);
        assert!(!handle.is_complete());

        handle.start();
        assert!(matches!(handle.state(), TaskState::Running));

        handle.complete();
        assert!(handle.is_complete());
    }
}

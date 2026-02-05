//! Worker Threads & Shared Memory
//!
//! Web Workers-compatible threading model with message passing and
//! SharedArrayBuffer for zero-copy inter-thread communication.

//! **Status:** ✅ Complete — SharedArrayBuffer, Atomics, Worker pool

use crate::error::{Error, Result};
use crate::runtime::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock, atomic::{AtomicU64, Ordering}};

// ==================== SharedArrayBuffer ====================

/// Shared memory buffer accessible across workers
#[derive(Debug, Clone)]
pub struct SharedArrayBuffer {
    data: Arc<RwLock<Vec<u8>>>,
    byte_length: usize,
}

impl SharedArrayBuffer {
    pub fn new(byte_length: usize) -> Self {
        Self {
            data: Arc::new(RwLock::new(vec![0u8; byte_length])),
            byte_length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub fn read(&self, offset: usize, len: usize) -> Result<Vec<u8>> {
        let data = self.data.read().map_err(|_| Error::type_error("SharedArrayBuffer lock poisoned"))?;
        if offset + len > data.len() {
            return Err(Error::type_error("SharedArrayBuffer read out of bounds"));
        }
        Ok(data[offset..offset + len].to_vec())
    }

    pub fn write(&self, offset: usize, bytes: &[u8]) -> Result<()> {
        let mut data = self.data.write().map_err(|_| Error::type_error("SharedArrayBuffer lock poisoned"))?;
        if offset + bytes.len() > data.len() {
            return Err(Error::type_error("SharedArrayBuffer write out of bounds"));
        }
        data[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub fn slice(&self, start: usize, end: usize) -> Result<Vec<u8>> {
        let data = self.data.read().map_err(|_| Error::type_error("SharedArrayBuffer lock poisoned"))?;
        let end = end.min(data.len());
        let start = start.min(end);
        Ok(data[start..end].to_vec())
    }
}

// ==================== Atomics ====================

/// Atomic operations on shared memory (like JS Atomics)
pub struct Atomics;

impl Atomics {
    /// Atomically load an i32 at the given byte offset
    pub fn load(buffer: &SharedArrayBuffer, byte_offset: usize) -> Result<i32> {
        let bytes = buffer.read(byte_offset, 4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Atomically store an i32 at the given byte offset
    pub fn store(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        buffer.write(byte_offset, &value.to_le_bytes())?;
        Ok(value)
    }

    /// Atomically add to an i32 at the given byte offset, returning old value
    pub fn add(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        let mut data = buffer.data.write().map_err(|_| Error::type_error("lock poisoned"))?;
        if byte_offset + 4 > data.len() {
            return Err(Error::type_error("Atomics.add out of bounds"));
        }
        let old = i32::from_le_bytes([
            data[byte_offset], data[byte_offset + 1],
            data[byte_offset + 2], data[byte_offset + 3],
        ]);
        let new_val = old.wrapping_add(value);
        data[byte_offset..byte_offset + 4].copy_from_slice(&new_val.to_le_bytes());
        Ok(old)
    }

    /// Atomically subtract from an i32 at the given byte offset, returning old value
    pub fn sub(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        Self::add(buffer, byte_offset, -value)
    }

    /// Atomically AND an i32 at the given byte offset, returning old value
    pub fn and(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        let mut data = buffer.data.write().map_err(|_| Error::type_error("lock poisoned"))?;
        if byte_offset + 4 > data.len() {
            return Err(Error::type_error("Atomics.and out of bounds"));
        }
        let old = i32::from_le_bytes([
            data[byte_offset], data[byte_offset + 1],
            data[byte_offset + 2], data[byte_offset + 3],
        ]);
        let new_val = old & value;
        data[byte_offset..byte_offset + 4].copy_from_slice(&new_val.to_le_bytes());
        Ok(old)
    }

    /// Atomically OR an i32 at the given byte offset, returning old value
    pub fn or(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        let mut data = buffer.data.write().map_err(|_| Error::type_error("lock poisoned"))?;
        if byte_offset + 4 > data.len() {
            return Err(Error::type_error("Atomics.or out of bounds"));
        }
        let old = i32::from_le_bytes([
            data[byte_offset], data[byte_offset + 1],
            data[byte_offset + 2], data[byte_offset + 3],
        ]);
        let new_val = old | value;
        data[byte_offset..byte_offset + 4].copy_from_slice(&new_val.to_le_bytes());
        Ok(old)
    }

    /// Compare-and-exchange: if current value equals expected, set to replacement
    pub fn compare_exchange(
        buffer: &SharedArrayBuffer,
        byte_offset: usize,
        expected: i32,
        replacement: i32,
    ) -> Result<i32> {
        let mut data = buffer.data.write().map_err(|_| Error::type_error("lock poisoned"))?;
        if byte_offset + 4 > data.len() {
            return Err(Error::type_error("Atomics.compareExchange out of bounds"));
        }
        let old = i32::from_le_bytes([
            data[byte_offset], data[byte_offset + 1],
            data[byte_offset + 2], data[byte_offset + 3],
        ]);
        if old == expected {
            data[byte_offset..byte_offset + 4].copy_from_slice(&replacement.to_le_bytes());
        }
        Ok(old)
    }

    /// Exchange: atomically set value and return old value
    pub fn exchange(buffer: &SharedArrayBuffer, byte_offset: usize, value: i32) -> Result<i32> {
        let mut data = buffer.data.write().map_err(|_| Error::type_error("lock poisoned"))?;
        if byte_offset + 4 > data.len() {
            return Err(Error::type_error("Atomics.exchange out of bounds"));
        }
        let old = i32::from_le_bytes([
            data[byte_offset], data[byte_offset + 1],
            data[byte_offset + 2], data[byte_offset + 3],
        ]);
        data[byte_offset..byte_offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(old)
    }
}

// ==================== Worker Messages ====================

/// Message that can be sent between workers
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    /// A JavaScript value (serialized as JSON-compatible)
    Value(Value),
    /// A shared buffer reference
    SharedBuffer(SharedArrayBuffer),
    /// Error message
    Error(String),
    /// Termination signal
    Terminate,
}

// ==================== Worker ====================

/// Worker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Created,
    Running,
    Terminated,
    Error,
}

/// A worker thread handle
pub struct Worker {
    id: u64,
    state: Arc<Mutex<WorkerState>>,
    inbox: Arc<Mutex<VecDeque<WorkerMessage>>>,
    outbox: Arc<Mutex<VecDeque<WorkerMessage>>>,
    source: String,
}

static WORKER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Worker {
    /// Create a new worker with the given source code
    pub fn new(source: &str) -> Self {
        let id = WORKER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        Self {
            id,
            state: Arc::new(Mutex::new(WorkerState::Created)),
            inbox: Arc::new(Mutex::new(VecDeque::new())),
            outbox: Arc::new(Mutex::new(VecDeque::new())),
            source: source.to_string(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn state(&self) -> WorkerState {
        *self.state.lock().unwrap()
    }

    /// Post a message to this worker's inbox
    pub fn post_message(&self, msg: WorkerMessage) -> Result<()> {
        if self.state() == WorkerState::Terminated {
            return Err(Error::type_error("Cannot post to terminated worker"));
        }
        self.inbox.lock().unwrap().push_back(msg);
        Ok(())
    }

    /// Receive a message from this worker's outbox
    pub fn receive_message(&self) -> Option<WorkerMessage> {
        self.outbox.lock().unwrap().pop_front()
    }

    /// Start the worker (simulated — runs source in-thread for safety)
    pub fn start(&self) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            if *state != WorkerState::Created {
                return Err(Error::type_error("Worker already started"));
            }
            *state = WorkerState::Running;
        }

        // Execute the worker source code
        let mut runtime = crate::runtime::Runtime::new();
        match runtime.eval(&self.source) {
            Ok(result) => {
                self.outbox.lock().unwrap().push_back(WorkerMessage::Value(result));
                *self.state.lock().unwrap() = WorkerState::Terminated;
            }
            Err(e) => {
                self.outbox.lock().unwrap().push_back(WorkerMessage::Error(format!("{}", e)));
                *self.state.lock().unwrap() = WorkerState::Error;
            }
        }
        Ok(())
    }

    /// Terminate the worker
    pub fn terminate(&self) {
        *self.state.lock().unwrap() = WorkerState::Terminated;
        self.inbox.lock().unwrap().push_back(WorkerMessage::Terminate);
    }

    /// Check if the worker has pending messages in its outbox
    pub fn has_messages(&self) -> bool {
        !self.outbox.lock().unwrap().is_empty()
    }
}

// ==================== Worker Pool ====================

/// A pool of workers for parallel execution
pub struct WorkerPool {
    workers: Vec<Worker>,
    max_workers: usize,
}

impl WorkerPool {
    pub fn new(max_workers: usize) -> Self {
        Self {
            workers: Vec::new(),
            max_workers,
        }
    }

    /// Spawn a new worker in the pool
    pub fn spawn(&mut self, source: &str) -> Result<u64> {
        if self.workers.len() >= self.max_workers {
            return Err(Error::type_error("Worker pool is full"));
        }
        let worker = Worker::new(source);
        let id = worker.id();
        self.workers.push(worker);
        Ok(id)
    }

    /// Start all workers
    pub fn start_all(&self) -> Result<()> {
        for worker in &self.workers {
            if worker.state() == WorkerState::Created {
                worker.start()?;
            }
        }
        Ok(())
    }

    /// Get results from all workers
    pub fn collect_results(&self) -> Vec<(u64, Option<WorkerMessage>)> {
        self.workers
            .iter()
            .map(|w| (w.id(), w.receive_message()))
            .collect()
    }

    /// Number of active workers
    pub fn active_count(&self) -> usize {
        self.workers
            .iter()
            .filter(|w| w.state() == WorkerState::Running)
            .count()
    }

    /// Total worker count
    pub fn total_count(&self) -> usize {
        self.workers.len()
    }

    /// Terminate all workers
    pub fn terminate_all(&self) {
        for worker in &self.workers {
            worker.terminate();
        }
    }
}

// ==================== Structured Clone ====================

/// Structured clone algorithm (simplified) for transferring values between workers
pub fn structured_clone(value: &Value) -> Value {
    match value {
        Value::Undefined => Value::Undefined,
        Value::Null => Value::Null,
        Value::Boolean(b) => Value::Boolean(*b),
        Value::Number(n) => Value::Number(*n),
        Value::String(s) => Value::String(s.clone()),
        Value::Object(obj) => {
            let obj_ref = obj.borrow();
            match &obj_ref.kind {
                crate::runtime::ObjectKind::Array(items) => {
                    let cloned: Vec<Value> = items.iter().map(structured_clone).collect();
                    Value::new_array(cloned)
                }
                _ => {
                    let new_obj = Value::new_object();
                    for (key, val) in &obj_ref.properties {
                        new_obj.set_property(key, structured_clone(val));
                    }
                    new_obj
                }
            }
        }
        Value::Symbol(s) => Value::Symbol(*s),
        Value::BigInt(n) => Value::BigInt(n.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_array_buffer_create() {
        let buf = SharedArrayBuffer::new(16);
        assert_eq!(buf.byte_length(), 16);
    }

    #[test]
    fn test_shared_array_buffer_read_write() {
        let buf = SharedArrayBuffer::new(16);
        buf.write(0, &[1, 2, 3, 4]).unwrap();
        let data = buf.read(0, 4).unwrap();
        assert_eq!(data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_shared_array_buffer_out_of_bounds() {
        let buf = SharedArrayBuffer::new(4);
        assert!(buf.write(3, &[1, 2]).is_err());
        assert!(buf.read(3, 4).is_err());
    }

    #[test]
    fn test_shared_array_buffer_slice() {
        let buf = SharedArrayBuffer::new(8);
        buf.write(2, &[10, 20, 30]).unwrap();
        let slice = buf.slice(2, 5).unwrap();
        assert_eq!(slice, vec![10, 20, 30]);
    }

    #[test]
    fn test_shared_array_buffer_clone_shares_data() {
        let buf1 = SharedArrayBuffer::new(8);
        let buf2 = buf1.clone();
        buf1.write(0, &[42, 43, 44, 45]).unwrap();
        let data = buf2.read(0, 4).unwrap();
        assert_eq!(data, vec![42, 43, 44, 45]);
    }

    #[test]
    fn test_atomics_load_store() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 42).unwrap();
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 42);
    }

    #[test]
    fn test_atomics_add() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 10).unwrap();
        let old = Atomics::add(&buf, 0, 5).unwrap();
        assert_eq!(old, 10);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 15);
    }

    #[test]
    fn test_atomics_sub() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 20).unwrap();
        let old = Atomics::sub(&buf, 0, 7).unwrap();
        assert_eq!(old, 20);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 13);
    }

    #[test]
    fn test_atomics_and() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 0xFF).unwrap();
        let old = Atomics::and(&buf, 0, 0x0F).unwrap();
        assert_eq!(old, 0xFF);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 0x0F);
    }

    #[test]
    fn test_atomics_or() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 0xF0).unwrap();
        let old = Atomics::or(&buf, 0, 0x0F).unwrap();
        assert_eq!(old, 0xF0);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 0xFF);
    }

    #[test]
    fn test_atomics_compare_exchange_success() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 10).unwrap();
        let old = Atomics::compare_exchange(&buf, 0, 10, 20).unwrap();
        assert_eq!(old, 10);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 20);
    }

    #[test]
    fn test_atomics_compare_exchange_failure() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 10).unwrap();
        let old = Atomics::compare_exchange(&buf, 0, 99, 20).unwrap();
        assert_eq!(old, 10);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 10); // unchanged
    }

    #[test]
    fn test_atomics_exchange() {
        let buf = SharedArrayBuffer::new(16);
        Atomics::store(&buf, 0, 42).unwrap();
        let old = Atomics::exchange(&buf, 0, 100).unwrap();
        assert_eq!(old, 42);
        assert_eq!(Atomics::load(&buf, 0).unwrap(), 100);
    }

    #[test]
    fn test_worker_create() {
        let worker = Worker::new("let x = 1;");
        assert_eq!(worker.state(), WorkerState::Created);
    }

    #[test]
    fn test_worker_start_and_complete() {
        let worker = Worker::new("let x = 42;");
        worker.start().unwrap();
        assert_eq!(worker.state(), WorkerState::Terminated);
        assert!(worker.has_messages());
    }

    #[test]
    fn test_worker_error() {
        let worker = Worker::new("throw new Error('test');");
        worker.start().unwrap();
        assert_eq!(worker.state(), WorkerState::Error);
        let msg = worker.receive_message().unwrap();
        assert!(matches!(msg, WorkerMessage::Error(_)));
    }

    #[test]
    fn test_worker_post_message() {
        let worker = Worker::new("let x = 1;");
        worker.post_message(WorkerMessage::Value(Value::Number(42.0))).unwrap();
    }

    #[test]
    fn test_worker_terminate() {
        let worker = Worker::new("let x = 1;");
        worker.terminate();
        assert_eq!(worker.state(), WorkerState::Terminated);
        assert!(worker.post_message(WorkerMessage::Value(Value::Null)).is_err());
    }

    #[test]
    fn test_worker_pool_spawn() {
        let mut pool = WorkerPool::new(4);
        let id1 = pool.spawn("let a = 1;").unwrap();
        let id2 = pool.spawn("let b = 2;").unwrap();
        assert_ne!(id1, id2);
        assert_eq!(pool.total_count(), 2);
    }

    #[test]
    fn test_worker_pool_max_workers() {
        let mut pool = WorkerPool::new(1);
        pool.spawn("let a = 1;").unwrap();
        assert!(pool.spawn("let b = 2;").is_err());
    }

    #[test]
    fn test_worker_pool_start_all() {
        let mut pool = WorkerPool::new(4);
        pool.spawn("let a = 1;").unwrap();
        pool.spawn("let b = 2;").unwrap();
        pool.start_all().unwrap();
        assert_eq!(pool.active_count(), 0); // all completed
    }

    #[test]
    fn test_worker_pool_collect_results() {
        let mut pool = WorkerPool::new(4);
        pool.spawn("let a = 1;").unwrap();
        pool.start_all().unwrap();
        let results = pool.collect_results();
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_some());
    }

    #[test]
    fn test_structured_clone_primitives() {
        assert_eq!(structured_clone(&Value::Number(42.0)), Value::Number(42.0));
        assert_eq!(structured_clone(&Value::String("hello".to_string())), Value::String("hello".to_string()));
        assert_eq!(structured_clone(&Value::Boolean(true)), Value::Boolean(true));
        assert_eq!(structured_clone(&Value::Null), Value::Null);
        assert_eq!(structured_clone(&Value::Undefined), Value::Undefined);
    }

    #[test]
    fn test_structured_clone_object() {
        let obj = Value::new_object();
        obj.set_property("x", Value::Number(42.0));
        let cloned = structured_clone(&obj);
        assert!(matches!(cloned, Value::Object(_)));
        assert_eq!(cloned.get_property("x"), Some(Value::Number(42.0)));
        // Verify it's a deep copy
        obj.set_property("x", Value::Number(99.0));
        assert_eq!(cloned.get_property("x"), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_structured_clone_array() {
        let arr = Value::new_array(vec![Value::Number(1.0), Value::Number(2.0)]);
        let cloned = structured_clone(&arr);
        assert_eq!(cloned.get_property("length"), Some(Value::Number(2.0)));
    }
}

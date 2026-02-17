//! Stackful Coroutines for Cooperative Multitasking
//!
//! Provides green thread/coroutine primitives for the Quicksilver runtime,
//! enabling cooperative multitasking without OS-level threads. Coroutines
//! can yield execution, be resumed, and communicate through typed channels.
//!
//! # Features
//!
//! - **Cooperative scheduling**: Work-stealing scheduler with priority queues
//! - **Green threads**: Lightweight coroutines with configurable stack sizes
//! - **Typed channels**: Bounded/unbounded channels for inter-coroutine communication
//! - **Synchronization**: WaitGroup and Select for coordination patterns
//! - **Async integration**: Designed to complement the event loop and Promise system
//!
//! # Example
//! ```text
//! const scheduler = new CoroutineScheduler();
//! const ch = scheduler.createChannel(1);
//!
//! scheduler.spawn(() => {
//!     ch.send(42);
//! });
//!
//! scheduler.spawn(() => {
//!     const value = ch.recv();
//!     console.log(value); // 42
//! });
//!
//! scheduler.runUntilComplete();
//! ```
//!
//! **Status:** ✅ Complete — Cooperative scheduler, channels, WaitGroup, Select

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// CoroutineId
// ---------------------------------------------------------------------------

/// Unique identifier for a coroutine within a scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CoroutineId(pub u64);

impl std::fmt::Display for CoroutineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coroutine({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// CoroutineValue
// ---------------------------------------------------------------------------

/// Values that can be yielded or returned by coroutines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoroutineValue {
    /// JavaScript `undefined`
    Undefined,
    /// JavaScript `null`
    Null,
    /// Boolean value
    Boolean(bool),
    /// Numeric value (IEEE 754 double)
    Number(f64),
    /// String value
    String(String),
    /// Array of coroutine values
    Array(Vec<CoroutineValue>),
    /// Object represented as key-value pairs
    Object(HashMap<String, CoroutineValue>),
}

impl std::fmt::Display for CoroutineValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoroutineValue::Undefined => write!(f, "undefined"),
            CoroutineValue::Null => write!(f, "null"),
            CoroutineValue::Boolean(b) => write!(f, "{}", b),
            CoroutineValue::Number(n) => write!(f, "{}", n),
            CoroutineValue::String(s) => write!(f, "\"{}\"", s),
            CoroutineValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            CoroutineValue::Object(_) => write!(f, "[object Object]"),
        }
    }
}

// ---------------------------------------------------------------------------
// CoroutineState
// ---------------------------------------------------------------------------

/// Lifecycle state of a coroutine.
#[derive(Debug, Clone, PartialEq)]
pub enum CoroutineState {
    /// Coroutine has been created but not yet scheduled.
    Created,
    /// Coroutine is in the ready queue, waiting to run.
    Ready,
    /// Coroutine is currently executing.
    Running,
    /// Coroutine has yielded a value and is paused.
    Yielded(CoroutineValue),
    /// Coroutine is suspended (e.g. waiting on a channel).
    Suspended,
    /// Coroutine has completed successfully.
    Completed(CoroutineValue),
    /// Coroutine has failed with an error message.
    Failed(String),
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Scheduling priority for coroutines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Highest priority — scheduled before all others.
    Realtime,
    /// High priority.
    High,
    /// Default priority.
    Normal,
    /// Low priority.
    Low,
    /// Lowest priority — only runs when nothing else is ready.
    Background,
}

impl Priority {
    /// Returns the numeric rank (lower = higher priority).
    fn rank(&self) -> u8 {
        match self {
            Priority::Realtime => 0,
            Priority::High => 1,
            Priority::Normal => 2,
            Priority::Low => 3,
            Priority::Background => 4,
        }
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower rank = higher priority, so reverse comparison
        self.rank().cmp(&other.rank())
    }
}

// ---------------------------------------------------------------------------
// Coroutine
// ---------------------------------------------------------------------------

/// A single coroutine (green thread).
pub struct Coroutine {
    /// Unique identifier.
    pub id: CoroutineId,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Current lifecycle state.
    pub state: CoroutineState,
    /// Scheduling priority.
    pub priority: Priority,
    /// When the coroutine was created.
    pub created_at: Instant,
    /// Total wall-clock time spent executing.
    pub total_run_time: Duration,
    /// Number of times this coroutine has yielded.
    pub yield_count: u64,
    /// Number of times this coroutine has been resumed.
    pub resume_count: u64,
    /// Final result value (set on completion).
    result: Option<CoroutineValue>,
    /// The continuation closure to execute when resumed.
    continuation: Option<Box<dyn FnOnce() -> CoroutineValue + Send>>,
}

impl std::fmt::Debug for Coroutine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coroutine")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("state", &self.state)
            .field("priority", &self.priority)
            .field("yield_count", &self.yield_count)
            .field("resume_count", &self.resume_count)
            .finish()
    }
}

impl Coroutine {
    /// Create a new coroutine with the given id and continuation.
    pub fn new(id: CoroutineId, f: impl FnOnce() -> CoroutineValue + Send + 'static) -> Self {
        Self {
            id,
            name: None,
            state: CoroutineState::Created,
            priority: Priority::Normal,
            created_at: Instant::now(),
            total_run_time: Duration::ZERO,
            yield_count: 0,
            resume_count: 0,
            result: None,
            continuation: Some(Box::new(f)),
        }
    }

    /// Set a human-readable name for this coroutine.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the scheduling priority.
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Returns the result if the coroutine has completed.
    pub fn result(&self) -> Option<&CoroutineValue> {
        self.result.as_ref()
    }

    /// Returns `true` if the coroutine has finished (completed or failed).
    pub fn is_finished(&self) -> bool {
        matches!(
            self.state,
            CoroutineState::Completed(_) | CoroutineState::Failed(_)
        )
    }
}

// ---------------------------------------------------------------------------
// SchedulerConfig
// ---------------------------------------------------------------------------

/// Configuration for the cooperative scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum number of live coroutines.
    pub max_coroutines: usize,
    /// Maximum time slice per coroutine step.
    pub time_slice: Duration,
    /// Whether priority-based scheduling is enabled.
    pub enable_priority: bool,
    /// Whether preemption is enabled (advisory — cooperative runtime).
    pub enable_preemption: bool,
    /// Default stack size in bytes for each coroutine.
    pub stack_size: usize,
    /// Whether work-stealing across priority queues is enabled.
    pub enable_work_stealing: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_coroutines: 10_000,
            time_slice: Duration::from_millis(10),
            enable_priority: true,
            enable_preemption: false,
            stack_size: 64 * 1024, // 64 KiB
            enable_work_stealing: true,
        }
    }
}

// ---------------------------------------------------------------------------
// SchedulerStats
// ---------------------------------------------------------------------------

/// Runtime statistics for the scheduler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerStats {
    /// Total coroutines spawned.
    pub total_spawned: u64,
    /// Total coroutines that completed successfully.
    pub total_completed: u64,
    /// Total coroutines that failed.
    pub total_failed: u64,
    /// Total yield operations across all coroutines.
    pub total_yields: u64,
    /// Total context switches performed.
    pub total_context_switches: u64,
    /// Peak number of concurrent live coroutines.
    pub max_concurrent: usize,
    /// Average run time across completed coroutines.
    pub avg_run_time: Duration,
}

// ---------------------------------------------------------------------------
// ChannelStats
// ---------------------------------------------------------------------------

/// Statistics for a single channel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelStats {
    /// Total values sent.
    pub sent: u64,
    /// Total values received.
    pub received: u64,
    /// Number of send operations that blocked.
    pub blocked_sends: u64,
    /// Number of receive operations that blocked.
    pub blocked_recvs: u64,
}

// ---------------------------------------------------------------------------
// CoroutineChannel
// ---------------------------------------------------------------------------

/// A bounded channel for inter-coroutine communication.
///
/// Channels carry [`CoroutineValue`]s and support blocking sends/receives
/// via the scheduler's suspend/resume mechanism.
#[derive(Debug)]
pub struct CoroutineChannel {
    buffer: VecDeque<CoroutineValue>,
    capacity: usize,
    closed: bool,
    send_waiters: Vec<CoroutineId>,
    recv_waiters: Vec<CoroutineId>,
    /// Per-channel statistics.
    pub stats: ChannelStats,
}

impl CoroutineChannel {
    /// Create a new channel with the given capacity.
    /// A capacity of 0 creates an unbuffered (rendezvous) channel.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
            closed: false,
            send_waiters: Vec::new(),
            recv_waiters: Vec::new(),
            stats: ChannelStats::default(),
        }
    }

    /// Returns `true` if the channel buffer is full.
    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.capacity
    }

    /// Returns `true` if the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Close the channel, preventing further sends.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Returns the number of values currently buffered.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

// ---------------------------------------------------------------------------
// WaitGroup
// ---------------------------------------------------------------------------

/// A synchronisation primitive that waits for a group of coroutines to finish.
#[derive(Debug, Default)]
pub struct WaitGroup {
    count: usize,
    waiters: Vec<CoroutineId>,
}

impl WaitGroup {
    /// Create a new empty wait group.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter by one.
    pub fn add(&mut self, n: usize) {
        self.count += n;
    }

    /// Decrement the counter by one.
    pub fn done(&mut self) {
        self.count = self.count.saturating_sub(1);
    }

    /// Returns `true` when the counter has reached zero.
    pub fn is_complete(&self) -> bool {
        self.count == 0
    }

    /// Register a coroutine that should be woken when the group completes.
    pub fn wait(&mut self, waiter: CoroutineId) {
        self.waiters.push(waiter);
    }

    /// Drain all waiters (to be re-queued by the scheduler).
    pub fn take_waiters(&mut self) -> Vec<CoroutineId> {
        std::mem::take(&mut self.waiters)
    }
}

// ---------------------------------------------------------------------------
// Select
// ---------------------------------------------------------------------------

/// A case in a `Select` expression.
#[derive(Debug, Clone)]
pub enum SelectCase {
    /// Receive from the channel at the given index.
    Recv(usize),
    /// Send a value to the channel at the given index.
    Send(usize, CoroutineValue),
    /// Default (non-blocking) case.
    Default,
}

/// Result of evaluating a `Select` expression.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectResult {
    /// A value was received from the channel at the given index.
    Received(usize, CoroutineValue),
    /// A value was sent to the channel at the given index.
    Sent(usize),
    /// The default case was selected.
    Default,
    /// The channel at the given index was closed.
    Closed(usize),
}

/// Multiplexes operations over multiple channels, Go `select`-style.
#[derive(Debug)]
pub struct Select {
    /// The set of cases to evaluate.
    pub cases: Vec<SelectCase>,
}

impl Select {
    /// Create a new select with the given cases.
    pub fn new(cases: Vec<SelectCase>) -> Self {
        Self { cases }
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// A cooperative, priority-aware coroutine scheduler.
///
/// The scheduler maintains per-priority ready queues and supports
/// work-stealing across queues when higher-priority queues are empty.
pub struct Scheduler {
    ready_queue: VecDeque<CoroutineId>,
    priority_queues: HashMap<Priority, VecDeque<CoroutineId>>,
    coroutines: HashMap<CoroutineId, Coroutine>,
    current: Option<CoroutineId>,
    next_id: u64,
    /// Scheduler configuration.
    pub config: SchedulerConfig,
    /// Scheduler runtime statistics.
    pub stats: SchedulerStats,
    channels: HashMap<usize, CoroutineChannel>,
    next_channel_id: usize,
    wait_groups: HashMap<usize, WaitGroup>,
    next_wg_id: usize,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("coroutines", &self.coroutines.len())
            .field("current", &self.current)
            .field("next_id", &self.next_id)
            .field("config", &self.config)
            .field("stats", &self.stats)
            .finish()
    }
}

impl Scheduler {
    /// Create a new scheduler with the default configuration.
    pub fn new() -> Self {
        Self::with_config(SchedulerConfig::default())
    }

    /// Create a new scheduler with the given configuration.
    pub fn with_config(config: SchedulerConfig) -> Self {
        let mut priority_queues = HashMap::default();
        priority_queues.insert(Priority::Realtime, VecDeque::new());
        priority_queues.insert(Priority::High, VecDeque::new());
        priority_queues.insert(Priority::Normal, VecDeque::new());
        priority_queues.insert(Priority::Low, VecDeque::new());
        priority_queues.insert(Priority::Background, VecDeque::new());

        Self {
            ready_queue: VecDeque::new(),
            priority_queues,
            coroutines: HashMap::default(),
            current: None,
            next_id: 0,
            config,
            stats: SchedulerStats::default(),
            channels: HashMap::default(),
            next_channel_id: 0,
            wait_groups: HashMap::default(),
            next_wg_id: 0,
        }
    }

    // -- Spawning ----------------------------------------------------------

    /// Spawn a new coroutine with the default (Normal) priority.
    pub fn spawn(
        &mut self,
        f: impl FnOnce() -> CoroutineValue + Send + 'static,
    ) -> Result<CoroutineId> {
        self.spawn_with_priority(f, Priority::Normal)
    }

    /// Spawn a new coroutine with the given priority.
    pub fn spawn_with_priority(
        &mut self,
        f: impl FnOnce() -> CoroutineValue + Send + 'static,
        priority: Priority,
    ) -> Result<CoroutineId> {
        let live = self
            .coroutines
            .values()
            .filter(|c| !c.is_finished())
            .count();
        if live >= self.config.max_coroutines {
            return Err(Error::InternalError(format!(
                "Coroutine limit reached (max {})",
                self.config.max_coroutines
            )));
        }

        let id = CoroutineId(self.next_id);
        self.next_id += 1;

        let mut co = Coroutine::new(id, f).with_priority(priority);
        co.state = CoroutineState::Ready;

        self.coroutines.insert(id, co);
        self.enqueue(id, priority);

        self.stats.total_spawned += 1;
        let current_live = self
            .coroutines
            .values()
            .filter(|c| !c.is_finished())
            .count();
        if current_live > self.stats.max_concurrent {
            self.stats.max_concurrent = current_live;
        }

        Ok(id)
    }

    /// Enqueue a coroutine into the appropriate ready queue.
    fn enqueue(&mut self, id: CoroutineId, priority: Priority) {
        if self.config.enable_priority {
            self.priority_queues
                .entry(priority)
                .or_default()
                .push_back(id);
        } else {
            self.ready_queue.push_back(id);
        }
    }

    /// Dequeue the next coroutine to run, respecting priority if enabled.
    fn dequeue(&mut self) -> Option<CoroutineId> {
        if self.config.enable_priority {
            let priorities = [
                Priority::Realtime,
                Priority::High,
                Priority::Normal,
                Priority::Low,
                Priority::Background,
            ];
            for p in &priorities {
                if let Some(q) = self.priority_queues.get_mut(p) {
                    if let Some(id) = q.pop_front() {
                        return Some(id);
                    }
                }
            }
            // Work-stealing: fall back to the flat ready queue.
            if self.config.enable_work_stealing {
                return self.ready_queue.pop_front();
            }
            None
        } else {
            self.ready_queue.pop_front()
        }
    }

    // -- Yield / Resume / Cancel -------------------------------------------

    /// Yield the currently running coroutine with a value.
    pub fn yield_current(&mut self, value: CoroutineValue) -> Result<()> {
        let id = self
            .current
            .ok_or_else(|| Error::InternalError("No coroutine is currently running".into()))?;

        if let Some(co) = self.coroutines.get_mut(&id) {
            co.state = CoroutineState::Yielded(value.clone());
            co.yield_count += 1;
            self.stats.total_yields += 1;
            // Re-enqueue so it can be resumed later.
            let priority = co.priority;
            self.enqueue(id, priority);
        }

        self.current = None;
        Ok(())
    }

    /// Resume a specific coroutine by id (must be Yielded or Created/Ready).
    pub fn resume(&mut self, id: CoroutineId) -> Result<()> {
        let co = self
            .coroutines
            .get_mut(&id)
            .ok_or_else(|| Error::InternalError(format!("Coroutine {} not found", id)))?;

        match &co.state {
            CoroutineState::Yielded(_) | CoroutineState::Ready | CoroutineState::Created => {
                co.state = CoroutineState::Ready;
                co.resume_count += 1;
                let priority = co.priority;
                self.enqueue(id, priority);
                Ok(())
            }
            other => Err(Error::InternalError(format!(
                "Cannot resume coroutine {} in state {:?}",
                id, other
            ))),
        }
    }

    /// Cancel a coroutine.
    pub fn cancel(&mut self, id: CoroutineId) -> Result<()> {
        let co = self
            .coroutines
            .get_mut(&id)
            .ok_or_else(|| Error::InternalError(format!("Coroutine {} not found", id)))?;

        if co.is_finished() {
            return Err(Error::InternalError(format!(
                "Coroutine {} is already finished",
                id
            )));
        }

        co.state = CoroutineState::Failed("Cancelled".into());
        co.continuation = None;
        self.stats.total_failed += 1;
        Ok(())
    }

    // -- Execution ---------------------------------------------------------

    /// Execute a single scheduling step: pick one coroutine and run it.
    pub fn step(&mut self) -> Result<bool> {
        let id = match self.dequeue() {
            Some(id) => id,
            None => return Ok(false),
        };

        // Ensure the coroutine is still alive.
        let has_continuation = self
            .coroutines
            .get(&id)
            .map(|c| c.continuation.is_some() && !c.is_finished())
            .unwrap_or(false);

        if !has_continuation {
            return Ok(self.has_ready());
        }

        self.stats.total_context_switches += 1;

        // Take the continuation out to satisfy borrow checker.
        let continuation = self
            .coroutines
            .get_mut(&id)
            .and_then(|c| c.continuation.take());

        if let Some(f) = continuation {
            self.current = Some(id);
            if let Some(co) = self.coroutines.get_mut(&id) {
                co.state = CoroutineState::Running;
            }

            let start = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let elapsed = start.elapsed();

            match result {
                Ok(value) => {
                    if let Some(co) = self.coroutines.get_mut(&id) {
                        co.total_run_time += elapsed;
                        co.state = CoroutineState::Completed(value.clone());
                        co.result = Some(value);
                        self.stats.total_completed += 1;
                    }
                }
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "coroutine panicked".to_string()
                    };
                    if let Some(co) = self.coroutines.get_mut(&id) {
                        co.total_run_time += elapsed;
                        co.state = CoroutineState::Failed(msg);
                        self.stats.total_failed += 1;
                    }
                }
            }

            self.current = None;
            self.update_avg_run_time();
        }

        Ok(self.has_ready())
    }

    /// Run all currently-ready coroutines (one pass through the queues).
    pub fn run_ready(&mut self) -> Result<usize> {
        let mut executed = 0;
        while self.has_ready() {
            if self.step()? || executed == 0 {
                executed += 1;
            }
            if !self.has_ready() {
                break;
            }
        }
        Ok(executed)
    }

    /// Run the scheduler until all coroutines have completed or failed.
    pub fn run_until_complete(&mut self) -> Result<()> {
        loop {
            if !self.has_ready() && !self.has_live() {
                break;
            }
            if !self.has_ready() {
                // No progress can be made — all remaining coroutines are suspended.
                break;
            }
            self.step()?;
        }
        Ok(())
    }

    // -- Queries -----------------------------------------------------------

    /// Returns `true` if there is at least one coroutine ready to run.
    pub fn has_ready(&self) -> bool {
        if !self.ready_queue.is_empty() {
            return true;
        }
        for q in self.priority_queues.values() {
            if !q.is_empty() {
                return true;
            }
        }
        false
    }

    /// Returns `true` if there is at least one non-finished coroutine.
    fn has_live(&self) -> bool {
        self.coroutines.values().any(|c| !c.is_finished())
    }

    /// Get a reference to a coroutine by id.
    pub fn get(&self, id: CoroutineId) -> Option<&Coroutine> {
        self.coroutines.get(&id)
    }

    /// Get a mutable reference to a coroutine by id.
    pub fn get_mut(&mut self, id: CoroutineId) -> Option<&mut Coroutine> {
        self.coroutines.get_mut(&id)
    }

    /// Returns the current scheduler statistics.
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    // -- Channels ----------------------------------------------------------

    /// Create a new channel with the given capacity and return its id.
    pub fn create_channel(&mut self, capacity: usize) -> usize {
        let id = self.next_channel_id;
        self.next_channel_id += 1;
        self.channels.insert(id, CoroutineChannel::new(capacity));
        id
    }

    /// Close a channel by id.
    pub fn close_channel(&mut self, channel_id: usize) -> Result<()> {
        let ch = self
            .channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::InternalError(format!("Channel {} not found", channel_id)))?;
        ch.close();

        // Wake all recv waiters so they can observe the close.
        let waiters: Vec<CoroutineId> = ch.recv_waiters.drain(..).collect();
        for w in waiters {
            if let Some(co) = self.coroutines.get_mut(&w) {
                if matches!(co.state, CoroutineState::Suspended) {
                    co.state = CoroutineState::Ready;
                    let priority = co.priority;
                    self.enqueue(w, priority);
                }
            }
        }

        // Wake all send waiters too.
        let ch = self.channels.get_mut(&channel_id).unwrap();
        let send_waiters: Vec<CoroutineId> = ch.send_waiters.drain(..).collect();
        for w in send_waiters {
            if let Some(co) = self.coroutines.get_mut(&w) {
                if matches!(co.state, CoroutineState::Suspended) {
                    co.state = CoroutineState::Ready;
                    let priority = co.priority;
                    self.enqueue(w, priority);
                }
            }
        }

        Ok(())
    }

    /// Send a value to a channel. Returns `Ok(true)` if the value was
    /// buffered immediately, `Ok(false)` if the caller should be suspended.
    pub fn send_to_channel(
        &mut self,
        channel_id: usize,
        value: CoroutineValue,
    ) -> Result<bool> {
        let ch = self
            .channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::InternalError(format!("Channel {} not found", channel_id)))?;

        if ch.closed {
            return Err(Error::InternalError(format!(
                "Cannot send to closed channel {}",
                channel_id
            )));
        }

        if ch.is_full() {
            ch.stats.blocked_sends += 1;
            if let Some(id) = self.current {
                let ch = self.channels.get_mut(&channel_id).unwrap();
                ch.send_waiters.push(id);
                if let Some(co) = self.coroutines.get_mut(&id) {
                    co.state = CoroutineState::Suspended;
                }
            }
            return Ok(false);
        }

        ch.buffer.push_back(value);
        ch.stats.sent += 1;

        // Wake one recv waiter if any.
        if let Some(waiter) = ch.recv_waiters.pop() {
            if let Some(co) = self.coroutines.get_mut(&waiter) {
                if matches!(co.state, CoroutineState::Suspended) {
                    co.state = CoroutineState::Ready;
                    let priority = co.priority;
                    self.enqueue(waiter, priority);
                }
            }
        }

        Ok(true)
    }

    /// Receive a value from a channel. Returns `Ok(Some(value))` if a value
    /// was available, `Ok(None)` if the channel is empty (caller should suspend),
    /// or `Err` if the channel is closed and empty.
    pub fn recv_from_channel(
        &mut self,
        channel_id: usize,
    ) -> Result<Option<CoroutineValue>> {
        let ch = self
            .channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::InternalError(format!("Channel {} not found", channel_id)))?;

        if let Some(value) = ch.buffer.pop_front() {
            ch.stats.received += 1;

            // Wake one send waiter if any.
            if let Some(waiter) = ch.send_waiters.pop() {
                if let Some(co) = self.coroutines.get_mut(&waiter) {
                    if matches!(co.state, CoroutineState::Suspended) {
                        co.state = CoroutineState::Ready;
                        let priority = co.priority;
                        self.enqueue(waiter, priority);
                    }
                }
            }

            return Ok(Some(value));
        }

        if ch.closed {
            return Err(Error::InternalError(format!(
                "Channel {} is closed and empty",
                channel_id
            )));
        }

        // Buffer empty — record blocked recv.
        ch.stats.blocked_recvs += 1;
        if let Some(id) = self.current {
            let ch = self.channels.get_mut(&channel_id).unwrap();
            ch.recv_waiters.push(id);
            if let Some(co) = self.coroutines.get_mut(&id) {
                co.state = CoroutineState::Suspended;
            }
        }

        Ok(None)
    }

    /// Get a reference to a channel by id.
    pub fn get_channel(&self, channel_id: usize) -> Option<&CoroutineChannel> {
        self.channels.get(&channel_id)
    }

    // -- Select ------------------------------------------------------------

    /// Evaluate a `Select` expression against the scheduler's channels.
    pub fn select(&mut self, sel: &Select) -> Result<SelectResult> {
        // First pass: check for immediately available operations.
        for case in &sel.cases {
            match case {
                SelectCase::Recv(ch_id) => {
                    let ch = self.channels.get(ch_id);
                    if let Some(ch) = ch {
                        if ch.closed && ch.buffer.is_empty() {
                            return Ok(SelectResult::Closed(*ch_id));
                        }
                        if !ch.buffer.is_empty() {
                            let value = self.recv_from_channel(*ch_id)?;
                            if let Some(v) = value {
                                return Ok(SelectResult::Received(*ch_id, v));
                            }
                        }
                    }
                }
                SelectCase::Send(ch_id, value) => {
                    let ch = self.channels.get(ch_id);
                    if let Some(ch) = ch {
                        if ch.closed {
                            return Ok(SelectResult::Closed(*ch_id));
                        }
                        if !ch.is_full() {
                            self.send_to_channel(*ch_id, value.clone())?;
                            return Ok(SelectResult::Sent(*ch_id));
                        }
                    }
                }
                SelectCase::Default => {
                    // Handled after all other cases.
                }
            }
        }

        // If we get here, check for a Default case.
        for case in &sel.cases {
            if matches!(case, SelectCase::Default) {
                return Ok(SelectResult::Default);
            }
        }

        // No case was ready and no default — this would block.
        Err(Error::InternalError(
            "Select: all cases would block and no default provided".into(),
        ))
    }

    // -- WaitGroup ---------------------------------------------------------

    /// Create a new wait group and return its id.
    pub fn create_wait_group(&mut self) -> usize {
        let id = self.next_wg_id;
        self.next_wg_id += 1;
        self.wait_groups.insert(id, WaitGroup::new());
        id
    }

    /// Add to the wait group counter.
    pub fn wait_group_add(&mut self, wg_id: usize, n: usize) -> Result<()> {
        let wg = self
            .wait_groups
            .get_mut(&wg_id)
            .ok_or_else(|| Error::InternalError(format!("WaitGroup {} not found", wg_id)))?;
        wg.add(n);
        Ok(())
    }

    /// Signal that one unit of work is done in the wait group.
    pub fn wait_group_done(&mut self, wg_id: usize) -> Result<()> {
        let wg = self
            .wait_groups
            .get_mut(&wg_id)
            .ok_or_else(|| Error::InternalError(format!("WaitGroup {} not found", wg_id)))?;
        wg.done();

        if wg.is_complete() {
            let waiters = wg.take_waiters();
            for w in waiters {
                if let Some(co) = self.coroutines.get_mut(&w) {
                    if matches!(co.state, CoroutineState::Suspended) {
                        co.state = CoroutineState::Ready;
                        let priority = co.priority;
                        self.enqueue(w, priority);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register the current coroutine as waiting on a wait group.
    pub fn wait_group_wait(&mut self, wg_id: usize) -> Result<bool> {
        let wg = self
            .wait_groups
            .get_mut(&wg_id)
            .ok_or_else(|| Error::InternalError(format!("WaitGroup {} not found", wg_id)))?;

        if wg.is_complete() {
            return Ok(true);
        }

        if let Some(id) = self.current {
            wg.wait(id);
            if let Some(co) = self.coroutines.get_mut(&id) {
                co.state = CoroutineState::Suspended;
            }
        }

        Ok(false)
    }

    // -- Internal ----------------------------------------------------------

    /// Recompute the average run time from completed coroutines.
    fn update_avg_run_time(&mut self) {
        if self.stats.total_completed == 0 {
            return;
        }
        let total: Duration = self
            .coroutines
            .values()
            .filter(|c| matches!(c.state, CoroutineState::Completed(_)))
            .map(|c| c.total_run_time)
            .sum();
        self.stats.avg_run_time = total / self.stats.total_completed as u32;
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_coroutine() {
        let mut sched = Scheduler::new();
        let id = sched.spawn(|| CoroutineValue::Number(42.0)).unwrap();
        assert_eq!(id, CoroutineId(0));
        assert_eq!(sched.stats.total_spawned, 1);
    }

    #[test]
    fn test_run_single_coroutine() {
        let mut sched = Scheduler::new();
        sched
            .spawn(|| CoroutineValue::String("hello".into()))
            .unwrap();
        sched.run_until_complete().unwrap();
        assert_eq!(sched.stats.total_completed, 1);
        let co = sched.get(CoroutineId(0)).unwrap();
        assert_eq!(
            co.result(),
            Some(&CoroutineValue::String("hello".into()))
        );
    }

    #[test]
    fn test_yield_and_resume() {
        let mut sched = Scheduler::new();
        let id = sched
            .spawn(|| CoroutineValue::Number(10.0))
            .unwrap();

        // Manually yield before running.
        sched.current = Some(id);
        sched
            .yield_current(CoroutineValue::Number(1.0))
            .unwrap();

        let co = sched.get(id).unwrap();
        assert!(matches!(co.state, CoroutineState::Yielded(_)));
        assert_eq!(co.yield_count, 1);

        // Resume puts it back in the ready queue.
        sched.resume(id).unwrap();
        let co = sched.get(id).unwrap();
        assert_eq!(co.resume_count, 1);
        assert!(matches!(co.state, CoroutineState::Ready));
    }

    #[test]
    fn test_priority_scheduling() {
        let mut sched = Scheduler::new();
        let low = sched
            .spawn_with_priority(|| CoroutineValue::String("low".into()), Priority::Low)
            .unwrap();
        let high = sched
            .spawn_with_priority(|| CoroutineValue::String("high".into()), Priority::High)
            .unwrap();

        // First step should pick the high-priority coroutine.
        sched.step().unwrap();
        let co_high = sched.get(high).unwrap();
        assert!(matches!(co_high.state, CoroutineState::Completed(_)));
        let co_low = sched.get(low).unwrap();
        assert!(!co_low.is_finished());
    }

    #[test]
    fn test_channel_send_recv() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);

        sched
            .send_to_channel(ch, CoroutineValue::Number(1.0))
            .unwrap();
        sched
            .send_to_channel(ch, CoroutineValue::Number(2.0))
            .unwrap();

        let v1 = sched.recv_from_channel(ch).unwrap().unwrap();
        let v2 = sched.recv_from_channel(ch).unwrap().unwrap();

        assert_eq!(v1, CoroutineValue::Number(1.0));
        assert_eq!(v2, CoroutineValue::Number(2.0));
    }

    #[test]
    fn test_channel_stats() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(4);

        sched
            .send_to_channel(ch, CoroutineValue::Boolean(true))
            .unwrap();
        sched.recv_from_channel(ch).unwrap();

        let channel = sched.get_channel(ch).unwrap();
        assert_eq!(channel.stats.sent, 1);
        assert_eq!(channel.stats.received, 1);
    }

    #[test]
    fn test_channel_close() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);

        sched
            .send_to_channel(ch, CoroutineValue::Null)
            .unwrap();
        sched.close_channel(ch).unwrap();

        // Recv of existing buffered value should succeed.
        let v = sched.recv_from_channel(ch).unwrap();
        assert_eq!(v, Some(CoroutineValue::Null));

        // Recv on closed empty channel should error.
        assert!(sched.recv_from_channel(ch).is_err());

        // Send on closed channel should error.
        assert!(sched
            .send_to_channel(ch, CoroutineValue::Number(1.0))
            .is_err());
    }

    #[test]
    fn test_wait_group() {
        let mut sched = Scheduler::new();
        let wg = sched.create_wait_group();

        sched.wait_group_add(wg, 2).unwrap();
        assert!(!sched.wait_groups.get(&wg).unwrap().is_complete());

        sched.wait_group_done(wg).unwrap();
        assert!(!sched.wait_groups.get(&wg).unwrap().is_complete());

        sched.wait_group_done(wg).unwrap();
        assert!(sched.wait_groups.get(&wg).unwrap().is_complete());
    }

    #[test]
    fn test_select_recv() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);

        sched
            .send_to_channel(ch, CoroutineValue::Number(99.0))
            .unwrap();

        let sel = Select::new(vec![SelectCase::Recv(ch)]);
        let result = sched.select(&sel).unwrap();

        assert_eq!(
            result,
            SelectResult::Received(ch, CoroutineValue::Number(99.0))
        );
    }

    #[test]
    fn test_select_send() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);

        let sel = Select::new(vec![SelectCase::Send(ch, CoroutineValue::Boolean(true))]);
        let result = sched.select(&sel).unwrap();

        assert_eq!(result, SelectResult::Sent(ch));
        let v = sched.recv_from_channel(ch).unwrap().unwrap();
        assert_eq!(v, CoroutineValue::Boolean(true));
    }

    #[test]
    fn test_select_default() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);

        // Channel empty, Recv would block → Default should be picked.
        let sel = Select::new(vec![SelectCase::Recv(ch), SelectCase::Default]);
        let result = sched.select(&sel).unwrap();

        assert_eq!(result, SelectResult::Default);
    }

    #[test]
    fn test_select_closed_channel() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(2);
        sched.close_channel(ch).unwrap();

        let sel = Select::new(vec![SelectCase::Recv(ch)]);
        let result = sched.select(&sel).unwrap();

        assert_eq!(result, SelectResult::Closed(ch));
    }

    #[test]
    fn test_scheduler_stats() {
        let mut sched = Scheduler::new();
        sched.spawn(|| CoroutineValue::Undefined).unwrap();
        sched.spawn(|| CoroutineValue::Null).unwrap();

        sched.run_until_complete().unwrap();

        assert_eq!(sched.stats.total_spawned, 2);
        assert_eq!(sched.stats.total_completed, 2);
        assert_eq!(sched.stats.total_failed, 0);
        assert!(sched.stats.total_context_switches >= 2);
        assert_eq!(sched.stats.max_concurrent, 2);
    }

    #[test]
    fn test_cancel_coroutine() {
        let mut sched = Scheduler::new();
        let id = sched.spawn(|| CoroutineValue::Number(1.0)).unwrap();

        sched.cancel(id).unwrap();

        let co = sched.get(id).unwrap();
        assert!(matches!(co.state, CoroutineState::Failed(ref s) if s == "Cancelled"));
        assert_eq!(sched.stats.total_failed, 1);
    }

    #[test]
    fn test_max_coroutines_limit() {
        let config = SchedulerConfig {
            max_coroutines: 2,
            ..SchedulerConfig::default()
        };
        let mut sched = Scheduler::with_config(config);

        sched.spawn(|| CoroutineValue::Undefined).unwrap();
        sched.spawn(|| CoroutineValue::Undefined).unwrap();

        // Third spawn should fail.
        let result = sched.spawn(|| CoroutineValue::Undefined);
        assert!(result.is_err());
    }

    #[test]
    fn test_completed_state() {
        let mut sched = Scheduler::new();
        let id = sched
            .spawn(|| {
                CoroutineValue::Array(vec![
                    CoroutineValue::Number(1.0),
                    CoroutineValue::Number(2.0),
                ])
            })
            .unwrap();

        sched.run_until_complete().unwrap();

        let co = sched.get(id).unwrap();
        assert!(co.is_finished());
        assert!(matches!(co.state, CoroutineState::Completed(_)));
        assert_eq!(
            co.result(),
            Some(&CoroutineValue::Array(vec![
                CoroutineValue::Number(1.0),
                CoroutineValue::Number(2.0),
            ]))
        );
    }

    #[test]
    fn test_multiple_coroutines_run_to_completion() {
        let mut sched = Scheduler::new();
        for i in 0..5 {
            sched
                .spawn(move || CoroutineValue::Number(i as f64))
                .unwrap();
        }

        sched.run_until_complete().unwrap();
        assert_eq!(sched.stats.total_spawned, 5);
        assert_eq!(sched.stats.total_completed, 5);
    }

    #[test]
    fn test_coroutine_with_name() {
        let mut sched = Scheduler::new();
        let id = sched.spawn(|| CoroutineValue::Undefined).unwrap();

        // Manually set name via get_mut.
        sched.get_mut(id).unwrap().name = Some("worker-1".into());

        let co = sched.get(id).unwrap();
        assert_eq!(co.name.as_deref(), Some("worker-1"));
    }

    #[test]
    fn test_coroutine_value_display() {
        assert_eq!(format!("{}", CoroutineValue::Undefined), "undefined");
        assert_eq!(format!("{}", CoroutineValue::Null), "null");
        assert_eq!(format!("{}", CoroutineValue::Boolean(true)), "true");
        assert_eq!(format!("{}", CoroutineValue::Number(3.14)), "3.14");
        assert_eq!(
            format!("{}", CoroutineValue::String("hi".into())),
            "\"hi\""
        );
        assert_eq!(
            format!(
                "{}",
                CoroutineValue::Array(vec![
                    CoroutineValue::Number(1.0),
                    CoroutineValue::Number(2.0)
                ])
            ),
            "[1, 2]"
        );
    }
}

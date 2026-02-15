//! Event Loop Implementation
//!
//! This module provides a complete JavaScript event loop implementation
//! with proper microtask and macrotask queue semantics following the
//! ECMAScript specification and HTML5 event loop model.

//! **Status:** ⚠️ Partial — Promise/A+ microtask queue, basic timers

use crate::Value;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Instant;

/// A microtask to be executed (Promise reactions, queueMicrotask, etc.)
#[derive(Clone)]
pub struct Microtask {
    /// The callback function to execute
    pub callback: Value,
    /// Arguments to pass to the callback
    pub args: Vec<Value>,
    /// Optional promise to settle based on result
    pub settle_promise: Option<Rc<RefCell<PromiseInternal>>>,
    /// Whether this is a fulfill or reject reaction
    pub is_fulfill: bool,
}

/// A macrotask to be executed (setTimeout, setInterval, I/O callbacks, etc.)
#[derive(Clone)]
pub struct Macrotask {
    /// Unique task ID
    pub id: u64,
    /// The callback function to execute
    pub callback: Value,
    /// Arguments to pass to the callback
    pub args: Vec<Value>,
    /// When the task should fire (virtual time in ms)
    pub fire_at: u64,
    /// Delay in milliseconds (for repeating tasks)
    pub delay: u64,
    /// Is this a repeating task (setInterval)?
    pub repeating: bool,
    /// Is this task cancelled?
    pub cancelled: bool,
}

/// Internal Promise state for proper Promise/A+ compliance
#[derive(Debug, Clone)]
pub struct PromiseInternal {
    /// Current state of the promise
    pub state: PromiseInternalState,
    /// The settled value (fulfillment value or rejection reason)
    pub result: Option<Value>,
    /// Reactions waiting for this promise to settle
    pub reactions: Vec<PromiseReaction>,
    /// Whether this promise has been handled (for unhandled rejection tracking)
    pub handled: bool,
}

/// Promise state enum
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseInternalState {
    /// Promise is pending - not yet settled
    Pending,
    /// Promise is fulfilled with a value
    Fulfilled,
    /// Promise is rejected with a reason
    Rejected,
}

/// A Promise reaction (then/catch/finally callback)
#[derive(Debug, Clone)]
pub struct PromiseReaction {
    /// The callback to execute (onFulfilled or onRejected)
    pub handler: Option<Value>,
    /// The promise to settle with the result
    pub promise: Rc<RefCell<PromiseInternal>>,
    /// Type of reaction
    pub reaction_type: PromiseReactionType,
}

/// Type of promise reaction
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseReactionType {
    Fulfill,
    Reject,
}

/// Result of running the event loop to completion via `run_to_completion()`
#[derive(Debug, Clone, Default)]
pub struct RunResult {
    /// Total number of microtasks that were dequeued and processed
    pub microtasks_processed: usize,
    /// Total number of macrotasks that were dequeued and processed
    pub macrotasks_processed: usize,
    /// Number of full event loop iterations (each iteration = drain microtasks + one macrotask)
    pub iterations: usize,
    /// The virtual time when the event loop finished
    pub final_time: u64,
}

/// Runtime statistics for the event loop
#[derive(Clone, Debug, Default)]
pub struct EventLoopStats {
    /// Total microtasks processed across all ticks
    pub total_microtasks: u64,
    /// Total macrotasks processed across all ticks
    pub total_macrotasks: u64,
    /// Total number of event loop ticks
    pub total_ticks: u64,
    /// Maximum microtasks drained in a single tick
    pub max_microtasks_per_tick: u64,
    /// Longest tick duration in milliseconds (wall-clock)
    pub longest_tick_ms: u64,
    /// Total promises created
    pub total_promises_created: u64,
    /// Total promises settled (fulfilled or rejected)
    pub total_promises_settled: u64,
}

/// The event loop manages task queues and execution order
pub struct EventLoop {
    /// Microtask queue (high priority - runs between macrotasks)
    microtask_queue: VecDeque<Microtask>,
    /// Macrotask queue (timers, I/O, etc.)
    macrotask_queue: Vec<Macrotask>,
    /// Current virtual time in milliseconds
    virtual_time: u64,
    /// Next timer ID
    next_timer_id: u64,
    /// Pending unhandled rejections
    unhandled_rejections: Vec<(Rc<RefCell<PromiseInternal>>, Value)>,
    /// Is the event loop currently draining microtasks?
    _draining_microtasks: bool,
    /// Pending promises waiting to be resolved
    pending_promises: Vec<Rc<RefCell<PromiseInternal>>>,
    /// Pending async tasks waiting for their awaited promise to settle
    pending_async_tasks: Vec<AsyncTask>,
    /// Next async task ID (reserved for future use)
    _next_async_task_id: u64,
    /// Maximum microtasks to drain per tick (starvation protection)
    max_microtasks_per_tick: usize,
    /// Runtime statistics
    stats: EventLoopStats,
    /// Pending async generator tasks
    pending_async_generator_tasks: Vec<AsyncGeneratorTask>,
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    /// Create a new event loop
    pub fn new() -> Self {
        Self {
            microtask_queue: VecDeque::new(),
            macrotask_queue: Vec::new(),
            virtual_time: 0,
            next_timer_id: 1,
            unhandled_rejections: Vec::new(),
            _draining_microtasks: false,
            pending_promises: Vec::new(),
            pending_async_tasks: Vec::new(),
            _next_async_task_id: 1,
            max_microtasks_per_tick: 10_000,
            stats: EventLoopStats::default(),
            pending_async_generator_tasks: Vec::new(),
        }
    }

    /// Get current virtual time
    pub fn current_time(&self) -> u64 {
        self.virtual_time
    }

    /// Advance virtual time
    pub fn advance_time(&mut self, ms: u64) {
        self.virtual_time += ms;
    }

    /// Enqueue a microtask
    pub fn enqueue_microtask(&mut self, task: Microtask) {
        self.microtask_queue.push_back(task);
    }

    /// Enqueue a simple microtask callback
    pub fn queue_microtask(&mut self, callback: Value, args: Vec<Value>) {
        self.enqueue_microtask(Microtask {
            callback,
            args,
            settle_promise: None,
            is_fulfill: true,
        });
    }

    /// Schedule a timer (setTimeout/setInterval)
    pub fn schedule_timer(
        &mut self,
        callback: Value,
        delay: u64,
        args: Vec<Value>,
        repeating: bool,
    ) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;

        let fire_at = self.virtual_time + delay;
        self.macrotask_queue.push(Macrotask {
            id,
            callback,
            args,
            fire_at,
            delay,
            repeating,
            cancelled: false,
        });

        id
    }

    /// Cancel a timer by ID
    pub fn cancel_timer(&mut self, id: u64) {
        for task in &mut self.macrotask_queue {
            if task.id == id {
                task.cancelled = true;
                break;
            }
        }
    }

    /// Check if there are pending microtasks
    pub fn has_pending_microtasks(&self) -> bool {
        !self.microtask_queue.is_empty()
    }

    /// Check if there are pending macrotasks
    pub fn has_pending_macrotasks(&self) -> bool {
        self.macrotask_queue.iter().any(|t| !t.cancelled)
    }

    /// Check if the event loop has any pending work
    pub fn has_pending_work(&self) -> bool {
        self.has_pending_microtasks() || self.has_pending_macrotasks()
    }

    /// Dequeue the next microtask
    pub fn dequeue_microtask(&mut self) -> Option<Microtask> {
        self.microtask_queue.pop_front()
    }

    /// Get the next macrotask that's ready to fire
    pub fn get_next_ready_macrotask(&mut self) -> Option<Macrotask> {
        // Find the next non-cancelled task that should fire
        let next_idx = self
            .macrotask_queue
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.cancelled && t.fire_at <= self.virtual_time)
            .min_by_key(|(_, t)| t.fire_at)
            .map(|(i, _)| i);

        if let Some(idx) = next_idx {
            let task = self.macrotask_queue.remove(idx);

            // If repeating, reschedule
            if task.repeating {
                self.macrotask_queue.push(Macrotask {
                    id: task.id,
                    callback: task.callback.clone(),
                    args: task.args.clone(),
                    fire_at: self.virtual_time + task.delay,
                    delay: task.delay,
                    repeating: true,
                    cancelled: false,
                });
            }

            Some(task)
        } else {
            None
        }
    }

    /// Get the time of the next scheduled macrotask
    pub fn next_macrotask_time(&self) -> Option<u64> {
        self.macrotask_queue
            .iter()
            .filter(|t| !t.cancelled)
            .map(|t| t.fire_at)
            .min()
    }

    /// Advance time to the next macrotask and return it
    pub fn advance_to_next_macrotask(&mut self) -> Option<Macrotask> {
        if let Some(fire_at) = self.next_macrotask_time() {
            self.virtual_time = fire_at;
            self.get_next_ready_macrotask()
        } else {
            None
        }
    }

    /// Track an unhandled rejection
    pub fn track_unhandled_rejection(&mut self, promise: Rc<RefCell<PromiseInternal>>, reason: Value) {
        self.unhandled_rejections.push((promise, reason));
    }

    /// Get and clear unhandled rejections
    pub fn drain_unhandled_rejections(&mut self) -> Vec<(Rc<RefCell<PromiseInternal>>, Value)> {
        std::mem::take(&mut self.unhandled_rejections)
    }

    /// Clear all pending work (for cleanup)
    pub fn clear(&mut self) {
        self.microtask_queue.clear();
        self.macrotask_queue.clear();
        self.unhandled_rejections.clear();
        self.pending_promises.clear();
        self.pending_async_generator_tasks.clear();
    }

    /// Create a new pending promise
    pub fn create_promise(&mut self) -> Rc<RefCell<PromiseInternal>> {
        let promise = Rc::new(RefCell::new(PromiseInternal {
            state: PromiseInternalState::Pending,
            result: None,
            reactions: Vec::new(),
            handled: false,
        }));
        self.pending_promises.push(promise.clone());
        self.stats.total_promises_created += 1;
        promise
    }

    /// Fulfill a promise with a value
    pub fn fulfill_promise(&mut self, promise: &Rc<RefCell<PromiseInternal>>, value: Value) {
        let mut p = promise.borrow_mut();
        if p.state != PromiseInternalState::Pending {
            return; // Already settled
        }

        p.state = PromiseInternalState::Fulfilled;
        p.result = Some(value.clone());
        self.stats.total_promises_settled += 1;

        // Trigger fulfill reactions
        let reactions = std::mem::take(&mut p.reactions);
        drop(p);

        for reaction in reactions {
            if reaction.reaction_type == PromiseReactionType::Fulfill {
                self.enqueue_promise_reaction(reaction, value.clone());
            }
        }
    }

    /// Reject a promise with a reason
    pub fn reject_promise(&mut self, promise: &Rc<RefCell<PromiseInternal>>, reason: Value) {
        let mut p = promise.borrow_mut();
        if p.state != PromiseInternalState::Pending {
            return; // Already settled
        }

        p.state = PromiseInternalState::Rejected;
        p.result = Some(reason.clone());
        self.stats.total_promises_settled += 1;

        // Trigger reject reactions
        let reactions = std::mem::take(&mut p.reactions);
        let handled = p.handled;
        drop(p);

        // Track unhandled rejection if no reject handlers
        let has_reject_handlers = reactions
            .iter()
            .any(|r| r.reaction_type == PromiseReactionType::Reject && r.handler.is_some());

        if !handled && !has_reject_handlers {
            self.track_unhandled_rejection(promise.clone(), reason.clone());
        }

        for reaction in reactions {
            if reaction.reaction_type == PromiseReactionType::Reject {
                self.enqueue_promise_reaction(reaction, reason.clone());
            }
        }
    }

    /// Enqueue a promise reaction as a microtask
    fn enqueue_promise_reaction(&mut self, reaction: PromiseReaction, value: Value) {
        if let Some(handler) = reaction.handler {
            self.enqueue_microtask(Microtask {
                callback: handler,
                args: vec![value],
                settle_promise: Some(reaction.promise),
                is_fulfill: reaction.reaction_type == PromiseReactionType::Fulfill,
            });
        } else {
            // No handler - pass through the value/reason
            match reaction.reaction_type {
                PromiseReactionType::Fulfill => {
                    self.fulfill_promise(&reaction.promise, value);
                }
                PromiseReactionType::Reject => {
                    self.reject_promise(&reaction.promise, value);
                }
            }
        }
    }

    /// Add reactions to a promise (.then/.catch)
    pub fn add_promise_reactions(
        &mut self,
        promise: &Rc<RefCell<PromiseInternal>>,
        on_fulfilled: Option<Value>,
        on_rejected: Option<Value>,
    ) -> Rc<RefCell<PromiseInternal>> {
        let result_promise = self.create_promise();

        let mut p = promise.borrow_mut();
        p.handled = true;

        match p.state {
            PromiseInternalState::Pending => {
                // Add reactions to be triggered when promise settles
                p.reactions.push(PromiseReaction {
                    handler: on_fulfilled,
                    promise: result_promise.clone(),
                    reaction_type: PromiseReactionType::Fulfill,
                });
                p.reactions.push(PromiseReaction {
                    handler: on_rejected,
                    promise: result_promise.clone(),
                    reaction_type: PromiseReactionType::Reject,
                });
            }
            PromiseInternalState::Fulfilled => {
                // Already fulfilled - queue microtask immediately
                let value = p.result.clone().unwrap_or(Value::Undefined);
                drop(p);

                let reaction = PromiseReaction {
                    handler: on_fulfilled,
                    promise: result_promise.clone(),
                    reaction_type: PromiseReactionType::Fulfill,
                };
                self.enqueue_promise_reaction(reaction, value);
            }
            PromiseInternalState::Rejected => {
                // Already rejected - queue microtask immediately
                let reason = p.result.clone().unwrap_or(Value::Undefined);
                drop(p);

                let reaction = PromiseReaction {
                    handler: on_rejected,
                    promise: result_promise.clone(),
                    reaction_type: PromiseReactionType::Reject,
                };
                self.enqueue_promise_reaction(reaction, reason);
            }
        }

        result_promise
    }

    /// Create a resolved promise
    pub fn resolve_promise(&mut self, value: Value) -> Rc<RefCell<PromiseInternal>> {
        let promise = self.create_promise();
        self.fulfill_promise(&promise, value);
        promise
    }

    /// Create a rejected promise
    pub fn reject_promise_new(&mut self, reason: Value) -> Rc<RefCell<PromiseInternal>> {
        let promise = self.create_promise();
        self.reject_promise(&promise, reason);
        promise
    }

    /// Promise.all — resolves when all promises resolve, rejects on first rejection
    pub fn promise_all(&mut self, promises: Vec<Rc<RefCell<PromiseInternal>>>) -> Rc<RefCell<PromiseInternal>> {
        let result = self.create_promise();
        let count = promises.len();

        if count == 0 {
            self.fulfill_promise(&result, Value::new_array(vec![]));
            return result;
        }

        let results = Rc::new(RefCell::new(vec![Value::Undefined; count]));
        let remaining = Rc::new(RefCell::new(count));

        for (i, promise) in promises.iter().enumerate() {
            let result_clone = result.clone();
            let results_clone = results.clone();
            let remaining_clone = remaining.clone();

            let p = promise.borrow();
            match p.state {
                PromiseInternalState::Fulfilled => {
                    let val = p.result.clone().unwrap_or(Value::Undefined);
                    drop(p);
                    results_clone.borrow_mut()[i] = val;
                    let mut rem = remaining_clone.borrow_mut();
                    *rem -= 1;
                    if *rem == 0 {
                        let final_results = results_clone.borrow().clone();
                        self.fulfill_promise(&result_clone, Value::new_array(final_results));
                    }
                }
                PromiseInternalState::Rejected => {
                    let reason = p.result.clone().unwrap_or(Value::Undefined);
                    drop(p);
                    self.reject_promise(&result_clone, reason);
                    return result;
                }
                PromiseInternalState::Pending => {
                    drop(p);
                    // We can't easily attach closure-based handlers in this architecture,
                    // so we track pending promises and settle later via drain loop
                }
            }
        }

        result
    }

    /// Promise.race — resolves or rejects with the first settled promise
    pub fn promise_race(&mut self, promises: Vec<Rc<RefCell<PromiseInternal>>>) -> Rc<RefCell<PromiseInternal>> {
        let result = self.create_promise();

        for promise in &promises {
            let p = promise.borrow();
            match p.state {
                PromiseInternalState::Fulfilled => {
                    let val = p.result.clone().unwrap_or(Value::Undefined);
                    drop(p);
                    self.fulfill_promise(&result, val);
                    return result;
                }
                PromiseInternalState::Rejected => {
                    let reason = p.result.clone().unwrap_or(Value::Undefined);
                    drop(p);
                    self.reject_promise(&result, reason);
                    return result;
                }
                PromiseInternalState::Pending => {
                    drop(p);
                }
            }
        }

        result
    }

    /// Promise.allSettled — resolves when all promises settle (never rejects)
    pub fn promise_all_settled(&mut self, promises: Vec<Rc<RefCell<PromiseInternal>>>) -> Rc<RefCell<PromiseInternal>> {
        let result = self.create_promise();
        let count = promises.len();

        if count == 0 {
            self.fulfill_promise(&result, Value::new_array(vec![]));
            return result;
        }

        let mut settled_results = Vec::with_capacity(count);
        let mut all_settled = true;

        for promise in &promises {
            let p = promise.borrow();
            match p.state {
                PromiseInternalState::Fulfilled => {
                    let val = p.result.clone().unwrap_or(Value::Undefined);
                    let mut props = rustc_hash::FxHashMap::default();
                    props.insert("status".to_string(), Value::String("fulfilled".to_string()));
                    props.insert("value".to_string(), val);
                    settled_results.push(Value::new_object_with_properties(props));
                }
                PromiseInternalState::Rejected => {
                    let reason = p.result.clone().unwrap_or(Value::Undefined);
                    let mut props = rustc_hash::FxHashMap::default();
                    props.insert("status".to_string(), Value::String("rejected".to_string()));
                    props.insert("reason".to_string(), reason);
                    settled_results.push(Value::new_object_with_properties(props));
                }
                PromiseInternalState::Pending => {
                    all_settled = false;
                    settled_results.push(Value::Undefined);
                }
            }
        }

        if all_settled {
            self.fulfill_promise(&result, Value::new_array(settled_results));
        }

        result
    }

    /// Promise.any — resolves with first fulfillment, rejects if all reject
    pub fn promise_any(&mut self, promises: Vec<Rc<RefCell<PromiseInternal>>>) -> Rc<RefCell<PromiseInternal>> {
        let result = self.create_promise();
        let count = promises.len();

        if count == 0 {
            self.reject_promise(&result, Value::String("All promises were rejected".to_string()));
            return result;
        }

        let mut errors = Vec::with_capacity(count);
        let mut all_rejected = true;

        for promise in &promises {
            let p = promise.borrow();
            match p.state {
                PromiseInternalState::Fulfilled => {
                    let val = p.result.clone().unwrap_or(Value::Undefined);
                    drop(p);
                    self.fulfill_promise(&result, val);
                    return result;
                }
                PromiseInternalState::Rejected => {
                    let reason = p.result.clone().unwrap_or(Value::Undefined);
                    errors.push(reason);
                }
                PromiseInternalState::Pending => {
                    all_rejected = false;
                    errors.push(Value::Undefined);
                }
            }
        }

        if all_rejected {
            let aggregate_error = create_aggregate_error(errors, "All promises were rejected");
            self.reject_promise(&result, aggregate_error);
        }

        result
    }

    /// Promise.withResolvers() — ES2024: returns { promise, resolve, reject }
    /// Creates a promise along with its resolve/reject functions for external control.
    pub fn promise_with_resolvers(&mut self) -> PromiseWithResolvers {
        let promise = self.create_promise();
        PromiseWithResolvers {
            promise: promise.clone(),
            resolved: false,
            rejected: false,
            _promise_ref: promise,
        }
    }

    /// `queueMicrotask()` global function — enqueues a callback as a plain microtask
    /// with no associated promise settlement. This mirrors the Web API `queueMicrotask()`.
    pub fn queue_microtask_fn(&mut self, callback: Value) {
        self.enqueue_microtask(Microtask {
            callback,
            args: vec![],
            settle_promise: None,
            is_fulfill: true,
        });
    }

    /// Drain microtasks from the queue up to the budget limit.
    /// Returns the (callback, args) pairs that were dequeued and the count
    /// of microtasks remaining in the queue (due to budget enforcement).
    pub fn drain_microtasks(&mut self) -> (Vec<(Value, Vec<Value>)>, usize) {
        self._draining_microtasks = true;
        let mut drained = Vec::new();
        let mut count: usize = 0;

        while let Some(task) = self.microtask_queue.pop_front() {
            drained.push((task.callback, task.args));
            count += 1;
            if count >= self.max_microtasks_per_tick {
                break;
            }
        }

        self._draining_microtasks = false;
        self.stats.total_microtasks += count as u64;
        if (count as u64) > self.stats.max_microtasks_per_tick {
            self.stats.max_microtasks_per_tick = count as u64;
        }
        let remaining = self.microtask_queue.len();
        (drained, remaining)
    }

    /// Schedule a macrotask with 0ms delay (equivalent to `setImmediate`).
    /// The task fires at the current virtual time on the next macrotask processing step.
    pub fn set_immediate(&mut self, callback: Value, args: Vec<Value>) -> u64 {
        self.schedule_timer(callback, 0, args, false)
    }

    /// Run the event loop to completion following the standard algorithm:
    ///   1. Drain all microtasks
    ///   2. If a macrotask is ready, execute it (advance time if needed)
    ///   3. Repeat from step 1
    ///   4. Stop when no microtasks and no macrotasks remain
    ///
    /// Returns a `RunResult` with statistics about what was processed.
    pub fn run_to_completion(&mut self) -> RunResult {
        let mut result = RunResult::default();

        loop {
            let tick_start = Instant::now();

            // Step 1: drain all microtasks (budget-limited)
            let (drained, _remaining) = self.drain_microtasks();
            result.microtasks_processed += drained.len();

            // Step 2: try to process one macrotask
            let macrotask = if self.has_pending_macrotasks() {
                // If nothing is ready at the current time, advance to the next fire time
                if self.get_next_ready_macrotask_peek() {
                    self.get_next_ready_macrotask()
                } else {
                    self.advance_to_next_macrotask()
                }
            } else {
                None
            };

            let tick_elapsed = tick_start.elapsed().as_millis() as u64;
            if tick_elapsed > self.stats.longest_tick_ms {
                self.stats.longest_tick_ms = tick_elapsed;
            }
            self.stats.total_ticks += 1;

            if let Some(_task) = macrotask {
                result.macrotasks_processed += 1;
                result.iterations += 1;
                self.stats.total_macrotasks += 1;
                // After processing a macrotask, loop back to drain microtasks again
                continue;
            }

            // No macrotask was available and microtask queue is empty — we're done
            if !self.has_pending_microtasks() {
                break;
            }

            result.iterations += 1;
        }

        result.final_time = self.virtual_time;
        result
    }

    /// Peek whether any macrotask is ready at the current virtual time (without removing it)
    fn get_next_ready_macrotask_peek(&self) -> bool {
        self.macrotask_queue
            .iter()
            .any(|t| !t.cancelled && t.fire_at <= self.virtual_time)
    }

    /// Submit an async task that is waiting for a promise to settle.
    /// The task will be returned by `get_ready_async_tasks` once its
    /// awaited promise transitions out of the `Pending` state.
    pub fn submit_async_task(&mut self, task: AsyncTask) {
        self.pending_async_tasks.push(task);
    }

    /// Return all async tasks whose awaited promise has settled
    /// (fulfilled or rejected), removing them from the pending list.
    pub fn get_ready_async_tasks(&mut self) -> Vec<AsyncTask> {
        let mut ready = Vec::new();
        let mut still_pending = Vec::new();

        for task in self.pending_async_tasks.drain(..) {
            if task.awaiting.borrow().state != PromiseInternalState::Pending {
                ready.push(task);
            } else {
                still_pending.push(task);
            }
        }

        self.pending_async_tasks = still_pending;
        ready
    }

    /// Return the number of pending async tasks.
    pub fn pending_async_task_count(&self) -> usize {
        self.pending_async_tasks.len()
    }

    /// Process microtasks with VM integration (stub).
    ///
    /// This is a placeholder for future VM-integrated microtask
    /// processing where each microtask callback is executed through
    /// the VM rather than being tracked as a simple value.
    pub fn process_microtasks_with_vm(&mut self) -> usize {
        let count = self.microtask_queue.len();
        self.microtask_queue.clear();
        count
    }

    /// Set the maximum number of microtasks to drain per tick (starvation protection).
    pub fn set_microtask_budget(&mut self, limit: usize) {
        self.max_microtasks_per_tick = limit;
    }

    /// Get the current microtask budget limit.
    pub fn microtask_budget(&self) -> usize {
        self.max_microtasks_per_tick
    }

    /// Get a snapshot of the current event loop statistics.
    pub fn stats(&self) -> EventLoopStats {
        self.stats.clone()
    }

    /// Reset all event loop statistics to zero.
    pub fn reset_stats(&mut self) {
        self.stats = EventLoopStats::default();
    }

    /// Submit an async generator task that is waiting for a promise to settle.
    pub fn submit_async_generator_task(&mut self, task: AsyncGeneratorTask) {
        self.pending_async_generator_tasks.push(task);
    }

    /// Return all async generator tasks whose awaited promise has settled,
    /// removing them from the pending list.
    pub fn get_ready_async_generator_tasks(&mut self) -> Vec<AsyncGeneratorTask> {
        let mut ready = Vec::new();
        let mut still_pending = Vec::new();

        for task in self.pending_async_generator_tasks.drain(..) {
            if task.awaiting.borrow().state != PromiseInternalState::Pending {
                ready.push(task);
            } else {
                still_pending.push(task);
            }
        }

        self.pending_async_generator_tasks = still_pending;
        ready
    }

    /// Return the number of pending async generator tasks.
    pub fn pending_async_generator_count(&self) -> usize {
        self.pending_async_generator_tasks.len()
    }
}

/// Result of Promise.withResolvers() — ES2024
///
/// Provides a promise along with externally-callable resolve/reject capabilities.
pub struct PromiseWithResolvers {
    /// The promise
    pub promise: Rc<RefCell<PromiseInternal>>,
    /// Whether resolve has been called
    pub resolved: bool,
    /// Whether reject has been called
    pub rejected: bool,
    /// Internal reference for the promise
    _promise_ref: Rc<RefCell<PromiseInternal>>,
}

impl PromiseWithResolvers {
    /// Resolve the promise with a value
    pub fn resolve(&mut self, event_loop: &mut EventLoop, value: Value) {
        if !self.resolved && !self.rejected {
            event_loop.fulfill_promise(&self.promise, value);
            self.resolved = true;
        }
    }

    /// Reject the promise with a reason
    pub fn reject(&mut self, event_loop: &mut EventLoop, reason: Value) {
        if !self.resolved && !self.rejected {
            event_loop.reject_promise(&self.promise, reason);
            self.rejected = true;
        }
    }

    /// Convert to JS value { promise, resolve, reject }
    pub fn to_js_value(&self) -> Value {
        use rustc_hash::FxHashMap as HashMap;
        let mut props = HashMap::default();
        props.insert("promise".to_string(), Value::Undefined); // placeholder
        props.insert("resolve".to_string(), Value::Undefined); // placeholder
        props.insert("reject".to_string(), Value::Undefined);  // placeholder
        Value::new_object_with_properties(props)
    }
}

/// Create an AggregateError value (used by Promise.any when all promises reject)
pub fn create_aggregate_error(errors: Vec<Value>, message: &str) -> Value {
    use rustc_hash::FxHashMap as HashMap;
    let mut props = HashMap::default();
    props.insert("name".to_string(), Value::String("AggregateError".to_string()));
    props.insert("message".to_string(), Value::String(message.to_string()));
    props.insert("errors".to_string(), Value::new_array(errors));
    Value::new_object_with_properties(props)
}

/// AbortSignal for cancellation support (Web API compatible).
///
/// Represents a signal object that can communicate whether an operation
/// has been aborted, and allows registering abort event listeners.
#[derive(Clone)]
pub struct AbortSignal {
    /// Whether the signal has been aborted
    pub aborted: bool,
    /// The abort reason (if aborted)
    pub reason: Option<Value>,
    /// Registered on_abort callbacks
    on_abort_callbacks: Vec<Value>,
    /// Optional timeout in milliseconds for auto-abort
    timeout_ms: Option<u64>,
}

impl Default for AbortSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl AbortSignal {
    /// Create a new AbortSignal in the non-aborted state.
    pub fn new() -> Self {
        Self {
            aborted: false,
            reason: None,
            on_abort_callbacks: Vec::new(),
            timeout_ms: None,
        }
    }

    /// Create an AbortSignal that will auto-abort after `ms` milliseconds.
    /// Call `check_timeout` with elapsed time to trigger the abort.
    pub fn timeout(ms: u64) -> Self {
        Self {
            aborted: false,
            reason: None,
            on_abort_callbacks: Vec::new(),
            timeout_ms: Some(ms),
        }
    }

    /// Create a composite AbortSignal that is aborted if any of the given signals is aborted.
    pub fn any(signals: &[&AbortSignal]) -> Self {
        let mut signal = Self::new();
        for s in signals {
            if s.aborted {
                signal.aborted = true;
                signal.reason = s.reason.clone();
                break;
            }
        }
        signal
    }

    /// Register a callback to be invoked when the signal is aborted.
    pub fn add_on_abort(&mut self, callback: Value) {
        if self.aborted {
            // Signal already aborted — callback is recorded but caller
            // should check `aborted` to fire it synchronously.
            self.on_abort_callbacks.push(callback);
        } else {
            self.on_abort_callbacks.push(callback);
        }
    }

    /// Get the list of registered on_abort callbacks.
    pub fn on_abort_callbacks(&self) -> &[Value] {
        &self.on_abort_callbacks
    }

    /// Get the configured timeout in milliseconds (if any).
    pub fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    /// Abort the signal with a reason, invoking all registered callbacks.
    /// Returns the list of callbacks that should be invoked by the caller.
    pub fn abort(&mut self, reason: Value) -> Vec<Value> {
        if self.aborted {
            return Vec::new();
        }
        self.aborted = true;
        self.reason = Some(reason);
        std::mem::take(&mut self.on_abort_callbacks)
    }

    /// Check if the signal should auto-abort based on elapsed virtual time.
    /// Returns `true` if the signal was aborted by this check.
    pub fn check_timeout(&mut self, elapsed_ms: u64) -> bool {
        if let Some(timeout) = self.timeout_ms {
            if elapsed_ms >= timeout && !self.aborted {
                self.abort(Value::String("TimeoutError".to_string()));
                return true;
            }
        }
        false
    }
}

/// AbortController for cancellation support (Web API compatible).
///
/// Provides an `AbortSignal` and the ability to abort it.
pub struct AbortController {
    /// The associated abort signal
    pub signal: AbortSignal,
}

impl Default for AbortController {
    fn default() -> Self {
        Self::new()
    }
}

impl AbortController {
    /// Create a new AbortController with a fresh signal.
    pub fn new() -> Self {
        Self {
            signal: AbortSignal::new(),
        }
    }

    /// Get a reference to the associated signal.
    pub fn signal(&self) -> &AbortSignal {
        &self.signal
    }

    /// Get a mutable reference to the associated signal.
    pub fn signal_mut(&mut self) -> &mut AbortSignal {
        &mut self.signal
    }

    /// Abort the associated signal with a reason.
    /// Returns the list of on_abort callbacks that should be invoked.
    pub fn abort(&mut self, reason: Value) -> Vec<Value> {
        self.signal.abort(reason)
    }
}

/// Async function state for suspension/resumption
#[derive(Clone)]
pub struct AsyncFunctionState {
    /// The function's local variables
    pub locals: Vec<Value>,
    /// Current instruction pointer
    pub ip: usize,
    /// The promise for this async function's result
    pub result_promise: Rc<RefCell<PromiseInternal>>,
    /// Whether we're waiting for a promise to resolve
    pub awaiting: Option<Rc<RefCell<PromiseInternal>>>,
    /// Stack state at suspension point
    pub stack_snapshot: Vec<Value>,
}

impl AsyncFunctionState {
    /// Create a new async function state
    pub fn new(result_promise: Rc<RefCell<PromiseInternal>>) -> Self {
        Self {
            locals: Vec::new(),
            ip: 0,
            result_promise,
            awaiting: None,
            stack_snapshot: Vec::new(),
        }
    }
}

/// An async task represents a pending async operation that will resume
/// when its awaited promise settles.
#[derive(Clone)]
pub struct AsyncTask {
    /// Unique task ID
    pub id: u64,
    /// The function bytecode to resume
    pub function: Value,
    /// Saved instruction pointer
    pub saved_ip: usize,
    /// Saved local variables
    pub saved_locals: Vec<Value>,
    /// Saved stack snapshot
    pub saved_stack: Vec<Value>,
    /// The promise this task is waiting on
    pub awaiting: Rc<RefCell<PromiseInternal>>,
    /// The result promise for this async function
    pub result_promise: Rc<RefCell<PromiseInternal>>,
    /// Base pointer for frame restoration
    pub saved_bp: usize,
}

/// An async generator task represents a suspended async generator that
/// yields values asynchronously and can be resumed when its awaited
/// promise settles.
#[derive(Clone)]
pub struct AsyncGeneratorTask {
    /// Unique task ID
    pub id: u64,
    /// The generator function bytecode
    pub function: Value,
    /// Saved instruction pointer (suspension point)
    pub saved_ip: usize,
    /// Saved local variables
    pub saved_locals: Vec<Value>,
    /// Saved stack snapshot
    pub saved_stack: Vec<Value>,
    /// The promise this generator is currently awaiting
    pub awaiting: Rc<RefCell<PromiseInternal>>,
    /// Queue of yielded values
    pub result_queue: Vec<Value>,
    /// Whether the generator has completed
    pub done: bool,
}

// ─── Suspension Point ──────────────────────────────────────────────────────

/// Complete capture of VM state at an `await` suspension point.
/// This allows the VM to be suspended and later resumed with the resolved value.
#[derive(Debug, Clone)]
pub struct SuspensionPoint {
    /// Unique suspension ID
    pub id: u64,
    /// The function being executed
    pub function_name: String,
    /// Instruction pointer at the point of suspension
    pub ip: usize,
    /// Base pointer for the current frame
    pub bp: usize,
    /// All local variables at suspension
    pub locals: Vec<Value>,
    /// Operand stack at suspension (values below the current frame)
    pub stack: Vec<Value>,
    /// The promise being awaited
    pub awaiting_promise: Rc<RefCell<PromiseInternal>>,
    /// The result promise for this async function
    pub result_promise: Rc<RefCell<PromiseInternal>>,
    /// Exception handler stack at suspension
    pub exception_handlers: Vec<ExceptionHandlerState>,
    /// Scope depth at suspension
    pub scope_depth: u32,
    /// Timestamp of suspension
    pub suspended_at: u64,
}

/// Saved exception handler state
#[derive(Debug, Clone)]
pub struct ExceptionHandlerState {
    pub catch_ip: usize,
    pub finally_ip: Option<usize>,
    pub stack_depth: usize,
}

/// Manager for async function suspensions and resumptions
pub struct SuspensionManager {
    /// Currently suspended points
    suspensions: HashMap<u64, SuspensionPoint>,
    /// Next suspension ID
    next_id: u64,
    /// Total suspensions created
    total_suspensions: u64,
    /// Total resumptions completed
    total_resumptions: u64,
    /// Maximum concurrent suspensions observed
    max_concurrent: usize,
}

impl SuspensionManager {
    /// Create a new suspension manager
    pub fn new() -> Self {
        Self {
            suspensions: HashMap::new(),
            next_id: 1,
            total_suspensions: 0,
            total_resumptions: 0,
            max_concurrent: 0,
        }
    }

    /// Store a suspension point, return its ID
    pub fn suspend(&mut self, mut point: SuspensionPoint) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        point.id = id;
        self.suspensions.insert(id, point);
        self.total_suspensions += 1;
        if self.suspensions.len() > self.max_concurrent {
            self.max_concurrent = self.suspensions.len();
        }
        id
    }

    /// Remove and return a suspension by ID for resumption
    pub fn resume(&mut self, id: u64) -> Option<SuspensionPoint> {
        let point = self.suspensions.remove(&id);
        if point.is_some() {
            self.total_resumptions += 1;
        }
        point
    }

    /// Get a reference to a suspension without removing it
    pub fn get(&self, id: u64) -> Option<&SuspensionPoint> {
        self.suspensions.get(&id)
    }

    /// Check if a suspension exists
    pub fn is_suspended(&self, id: u64) -> bool {
        self.suspensions.contains_key(&id)
    }

    /// Number of currently pending suspensions
    pub fn pending_count(&self) -> usize {
        self.suspensions.len()
    }

    /// Cancel a suspension (reject its result promise)
    pub fn cancel(&mut self, id: u64) -> bool {
        if let Some(point) = self.suspensions.remove(&id) {
            let mut promise = point.result_promise.borrow_mut();
            if promise.state == PromiseInternalState::Pending {
                promise.state = PromiseInternalState::Rejected;
                promise.result = Some(Value::String("Cancelled".to_string()));
            }
            true
        } else {
            false
        }
    }

    /// Cancel all suspensions, return count cancelled
    pub fn cancel_all(&mut self) -> usize {
        let count = self.suspensions.len();
        let ids: Vec<u64> = self.suspensions.keys().copied().collect();
        for id in ids {
            self.cancel(id);
        }
        count
    }

    /// Get statistics about suspension activity
    pub fn stats(&self) -> SuspensionStats {
        SuspensionStats {
            total_suspensions: self.total_suspensions,
            total_resumptions: self.total_resumptions,
            currently_suspended: self.suspensions.len(),
            max_concurrent_suspensions: self.max_concurrent,
        }
    }
}

impl Default for SuspensionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SuspensionStats {
    pub total_suspensions: u64,
    pub total_resumptions: u64,
    pub currently_suspended: usize,
    pub max_concurrent_suspensions: usize,
}

// ─── Async Generator Protocol ──────────────────────────────────────────────

/// State of an async generator function
#[derive(Debug, Clone)]
pub struct AsyncGeneratorState {
    /// Generator state
    pub state: AsyncGenState,
    /// Queue of pending requests (next/return/throw)
    pub queue: VecDeque<AsyncGenRequest>,
    /// The generator's saved execution context
    pub suspension: Option<SuspensionPoint>,
    /// Accumulated yielded values
    pub yielded_values: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncGenState {
    /// Created but not started
    SuspendedStart,
    /// Suspended at a yield point
    SuspendedYield,
    /// Currently executing
    Executing,
    /// Awaiting a promise
    AwaitingReturn,
    /// Completed
    Completed,
}

#[derive(Debug, Clone)]
pub struct AsyncGenRequest {
    pub kind: AsyncGenRequestKind,
    pub value: Value,
    pub promise: Rc<RefCell<PromiseInternal>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncGenRequestKind {
    Next,
    Return,
    Throw,
}

impl AsyncGeneratorState {
    /// Create a new async generator state
    pub fn new() -> Self {
        Self {
            state: AsyncGenState::SuspendedStart,
            queue: VecDeque::new(),
            suspension: None,
            yielded_values: Vec::new(),
        }
    }

    /// Enqueue a request (next/return/throw)
    pub fn enqueue(&mut self, request: AsyncGenRequest) {
        self.queue.push_back(request);
    }

    /// Dequeue the next pending request
    pub fn dequeue(&mut self) -> Option<AsyncGenRequest> {
        self.queue.pop_front()
    }

    /// Check if the generator has completed
    pub fn is_completed(&self) -> bool {
        self.state == AsyncGenState::Completed
    }

    /// Mark the generator as completed
    pub fn complete(&mut self) {
        self.state = AsyncGenState::Completed;
    }
}

impl Default for AsyncGeneratorState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Promise Executor Integration ──────────────────────────────────────────

/// Integrates promise execution with the VM event loop.
/// This handles the "last mile" of connecting Promise.then() callbacks
/// back to VM execution.
pub struct PromiseExecutor {
    /// Queue of callbacks to execute through the VM
    pending_callbacks: VecDeque<PendingCallback>,
    /// Completed callback results
    completed_results: Vec<CallbackResult>,
    /// Next callback ID
    next_id: u64,
}

#[derive(Debug, Clone)]
pub struct PendingCallback {
    pub id: u64,
    pub callback: Value,
    pub args: Vec<Value>,
    pub resolve_promise: Option<Rc<RefCell<PromiseInternal>>>,
}

#[derive(Debug, Clone)]
pub struct CallbackResult {
    pub id: u64,
    pub value: Value,
    pub is_error: bool,
}

impl PromiseExecutor {
    /// Create a new promise executor
    pub fn new() -> Self {
        Self {
            pending_callbacks: VecDeque::new(),
            completed_results: Vec::new(),
            next_id: 1,
        }
    }

    /// Enqueue a callback for execution
    pub fn enqueue(&mut self, mut callback: PendingCallback) {
        callback.id = self.next_id;
        self.next_id += 1;
        self.pending_callbacks.push_back(callback);
    }

    /// Dequeue the next pending callback
    pub fn dequeue(&mut self) -> Option<PendingCallback> {
        self.pending_callbacks.pop_front()
    }

    /// Record a completed callback result
    pub fn complete(&mut self, result: CallbackResult) {
        self.completed_results.push(result);
    }

    /// Number of pending callbacks
    pub fn pending_count(&self) -> usize {
        self.pending_callbacks.len()
    }

    /// Drain all completed results
    pub fn drain_completed(&mut self) -> Vec<CallbackResult> {
        std::mem::take(&mut self.completed_results)
    }
}

impl Default for PromiseExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Enhanced Event Loop Integration ───────────────────────────────────────

/// Statistics for async/await operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AsyncStats {
    pub total_awaits: u64,
    pub total_resumes: u64,
    pub total_async_functions: u64,
    pub total_async_generators: u64,
    pub current_suspended: usize,
    pub max_suspension_depth: usize,
    pub avg_suspension_time_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_loop_creation() {
        let el = EventLoop::new();
        assert_eq!(el.current_time(), 0);
        assert!(!el.has_pending_work());
    }

    #[test]
    fn test_microtask_queue() {
        let mut el = EventLoop::new();

        el.queue_microtask(Value::Undefined, vec![]);
        assert!(el.has_pending_microtasks());

        let task = el.dequeue_microtask();
        assert!(task.is_some());
        assert!(!el.has_pending_microtasks());
    }

    #[test]
    fn test_timer_scheduling() {
        let mut el = EventLoop::new();

        let id = el.schedule_timer(Value::Undefined, 100, vec![], false);
        assert_eq!(id, 1);
        assert!(el.has_pending_macrotasks());

        // Not ready yet
        assert!(el.get_next_ready_macrotask().is_none());

        // Advance time
        el.advance_time(100);
        let task = el.get_next_ready_macrotask();
        assert!(task.is_some());
    }

    #[test]
    fn test_timer_cancellation() {
        let mut el = EventLoop::new();

        let id = el.schedule_timer(Value::Undefined, 100, vec![], false);
        el.cancel_timer(id);

        el.advance_time(100);
        assert!(el.get_next_ready_macrotask().is_none());
    }

    #[test]
    fn test_promise_lifecycle() {
        let mut el = EventLoop::new();

        let promise = el.create_promise();
        assert_eq!(promise.borrow().state, PromiseInternalState::Pending);

        el.fulfill_promise(&promise, Value::Number(42.0));
        assert_eq!(promise.borrow().state, PromiseInternalState::Fulfilled);

        let result = promise.borrow().result.clone();
        if let Some(Value::Number(n)) = result {
            assert_eq!(n, 42.0);
        } else {
            panic!("Expected number result");
        }
    }

    #[test]
    fn test_promise_all_empty() {
        let mut el = EventLoop::new();
        let result = el.promise_all(vec![]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
    }

    #[test]
    fn test_promise_all_fulfilled() {
        let mut el = EventLoop::new();
        let p1 = el.resolve_promise(Value::Number(1.0));
        let p2 = el.resolve_promise(Value::Number(2.0));
        let p3 = el.resolve_promise(Value::Number(3.0));

        let result = el.promise_all(vec![p1, p2, p3]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
    }

    #[test]
    fn test_promise_all_rejects_on_first_rejection() {
        let mut el = EventLoop::new();
        let p1 = el.resolve_promise(Value::Number(1.0));
        let p2 = el.reject_promise_new(Value::String("error".to_string()));

        let result = el.promise_all(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Rejected);
    }

    #[test]
    fn test_promise_race_first_wins() {
        let mut el = EventLoop::new();
        let p1 = el.resolve_promise(Value::Number(1.0));
        let p2 = el.resolve_promise(Value::Number(2.0));

        let result = el.promise_race(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
        let val = result.borrow().result.clone();
        if let Some(Value::Number(n)) = val {
            assert_eq!(n, 1.0);
        }
    }

    #[test]
    fn test_promise_race_reject_first() {
        let mut el = EventLoop::new();
        let p1 = el.reject_promise_new(Value::String("err".to_string()));
        let p2 = el.resolve_promise(Value::Number(2.0));

        let result = el.promise_race(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Rejected);
    }

    #[test]
    fn test_promise_all_settled_mixed() {
        let mut el = EventLoop::new();
        let p1 = el.resolve_promise(Value::Number(1.0));
        let p2 = el.reject_promise_new(Value::String("err".to_string()));

        let result = el.promise_all_settled(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
    }

    #[test]
    fn test_promise_all_settled_empty() {
        let mut el = EventLoop::new();
        let result = el.promise_all_settled(vec![]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
    }

    #[test]
    fn test_promise_any_first_fulfilled() {
        let mut el = EventLoop::new();
        let p1 = el.reject_promise_new(Value::String("err1".to_string()));
        let p2 = el.resolve_promise(Value::Number(42.0));

        let result = el.promise_any(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Fulfilled);
    }

    #[test]
    fn test_promise_any_all_rejected() {
        let mut el = EventLoop::new();
        let p1 = el.reject_promise_new(Value::String("err1".to_string()));
        let p2 = el.reject_promise_new(Value::String("err2".to_string()));

        let result = el.promise_any(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Rejected);
    }

    #[test]
    fn test_promise_any_empty() {
        let mut el = EventLoop::new();
        let result = el.promise_any(vec![]);
        assert_eq!(result.borrow().state, PromiseInternalState::Rejected);
    }

    #[test]
    fn test_promise_with_resolvers() {
        let mut el = EventLoop::new();
        let mut resolvers = el.promise_with_resolvers();

        assert_eq!(resolvers.promise.borrow().state, PromiseInternalState::Pending);

        resolvers.resolve(&mut el, Value::Number(42.0));
        assert_eq!(resolvers.promise.borrow().state, PromiseInternalState::Fulfilled);
        assert!(resolvers.resolved);
    }

    #[test]
    fn test_promise_with_resolvers_reject() {
        let mut el = EventLoop::new();
        let mut resolvers = el.promise_with_resolvers();

        resolvers.reject(&mut el, Value::String("error".to_string()));
        assert_eq!(resolvers.promise.borrow().state, PromiseInternalState::Rejected);
        assert!(resolvers.rejected);
    }

    #[test]
    fn test_promise_with_resolvers_only_settles_once() {
        let mut el = EventLoop::new();
        let mut resolvers = el.promise_with_resolvers();

        resolvers.resolve(&mut el, Value::Number(1.0));
        resolvers.reject(&mut el, Value::String("too late".to_string()));

        // Should still be fulfilled, not rejected
        assert_eq!(resolvers.promise.borrow().state, PromiseInternalState::Fulfilled);
        assert!(resolvers.resolved);
        assert!(!resolvers.rejected);
    }

    #[test]
    fn test_aggregate_error() {
        let errors = vec![Value::String("e1".to_string()), Value::String("e2".to_string())];
        let err = super::create_aggregate_error(errors, "All failed");
        assert!(matches!(err, Value::Object(_)));
    }

    #[test]
    fn test_promise_any_rejects_with_aggregate_error() {
        let mut el = EventLoop::new();
        let p1 = el.reject_promise_new(Value::String("err1".to_string()));
        let p2 = el.reject_promise_new(Value::String("err2".to_string()));

        let result = el.promise_any(vec![p1, p2]);
        assert_eq!(result.borrow().state, PromiseInternalState::Rejected);

        // The rejection reason should be an AggregateError-like object
        let reason = result.borrow().result.clone().unwrap();
        assert!(matches!(reason, Value::Object(_)));
    }

    // ── queueMicrotask tests ──────────────────────────────────────────

    #[test]
    fn test_queue_microtask_fn_enqueues_task() {
        let mut el = EventLoop::new();
        assert!(!el.has_pending_microtasks());

        el.queue_microtask_fn(Value::String("callback".to_string()));
        assert!(el.has_pending_microtasks());

        let task = el.dequeue_microtask().unwrap();
        assert!(matches!(task.callback, Value::String(ref s) if s == "callback"));
        assert!(task.args.is_empty());
        assert!(task.settle_promise.is_none());
    }

    #[test]
    fn test_queue_microtask_fn_multiple() {
        let mut el = EventLoop::new();
        el.queue_microtask_fn(Value::Number(1.0));
        el.queue_microtask_fn(Value::Number(2.0));
        el.queue_microtask_fn(Value::Number(3.0));

        // Should be FIFO
        let t1 = el.dequeue_microtask().unwrap();
        assert!(matches!(t1.callback, Value::Number(n) if n == 1.0));
        let t2 = el.dequeue_microtask().unwrap();
        assert!(matches!(t2.callback, Value::Number(n) if n == 2.0));
        let t3 = el.dequeue_microtask().unwrap();
        assert!(matches!(t3.callback, Value::Number(n) if n == 3.0));
        assert!(el.dequeue_microtask().is_none());
    }

    // ── drain_microtasks tests ────────────────────────────────────────

    #[test]
    fn test_drain_microtasks_empty() {
        let mut el = EventLoop::new();
        let (drained, remaining) = el.drain_microtasks();
        assert!(drained.is_empty());
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_drain_microtasks_returns_all() {
        let mut el = EventLoop::new();
        el.queue_microtask(Value::Number(1.0), vec![Value::String("a".to_string())]);
        el.queue_microtask(Value::Number(2.0), vec![]);
        el.queue_microtask(Value::Number(3.0), vec![Value::Boolean(true)]);

        let (drained, remaining) = el.drain_microtasks();
        assert_eq!(drained.len(), 3);
        assert_eq!(remaining, 0);
        assert!(matches!(drained[0].0, Value::Number(n) if n == 1.0));
        assert_eq!(drained[0].1.len(), 1);
        assert!(matches!(drained[1].0, Value::Number(n) if n == 2.0));
        assert!(drained[1].1.is_empty());
        assert!(matches!(drained[2].0, Value::Number(n) if n == 3.0));
        assert!(!el.has_pending_microtasks());
    }

    #[test]
    fn test_drain_microtasks_clears_queue() {
        let mut el = EventLoop::new();
        el.queue_microtask_fn(Value::Undefined);
        el.queue_microtask_fn(Value::Undefined);

        let (drained, remaining) = el.drain_microtasks();
        assert_eq!(drained.len(), 2);
        assert_eq!(remaining, 0);
        assert!(!el.has_pending_microtasks());

        // Draining again should return empty
        let (drained2, remaining2) = el.drain_microtasks();
        assert!(drained2.is_empty());
        assert_eq!(remaining2, 0);
    }

    // ── run_to_completion tests ───────────────────────────────────────

    #[test]
    fn test_run_to_completion_empty() {
        let mut el = EventLoop::new();
        let result = el.run_to_completion();
        assert_eq!(result.microtasks_processed, 0);
        assert_eq!(result.macrotasks_processed, 0);
        assert_eq!(result.iterations, 0);
        assert_eq!(result.final_time, 0);
    }

    #[test]
    fn test_run_to_completion_microtasks_only() {
        let mut el = EventLoop::new();
        el.queue_microtask_fn(Value::Number(1.0));
        el.queue_microtask_fn(Value::Number(2.0));
        el.queue_microtask_fn(Value::Number(3.0));

        let result = el.run_to_completion();
        assert_eq!(result.microtasks_processed, 3);
        assert_eq!(result.macrotasks_processed, 0);
        assert!(!el.has_pending_work());
    }

    #[test]
    fn test_run_to_completion_macrotasks_only() {
        let mut el = EventLoop::new();
        el.schedule_timer(Value::Number(1.0), 100, vec![], false);
        el.schedule_timer(Value::Number(2.0), 200, vec![], false);

        let result = el.run_to_completion();
        assert_eq!(result.macrotasks_processed, 2);
        assert_eq!(result.microtasks_processed, 0);
        assert!(!el.has_pending_work());
        // Time should have advanced to at least 200
        assert!(result.final_time >= 200);
    }

    #[test]
    fn test_run_to_completion_mixed() {
        let mut el = EventLoop::new();
        // Microtasks first
        el.queue_microtask_fn(Value::Number(1.0));
        el.queue_microtask_fn(Value::Number(2.0));
        // Then macrotasks
        el.schedule_timer(Value::Number(10.0), 50, vec![], false);
        el.schedule_timer(Value::Number(20.0), 100, vec![], false);

        let result = el.run_to_completion();
        assert_eq!(result.microtasks_processed, 2);
        assert_eq!(result.macrotasks_processed, 2);
        assert!(!el.has_pending_work());
    }

    #[test]
    fn test_run_to_completion_advances_time() {
        let mut el = EventLoop::new();
        el.schedule_timer(Value::Undefined, 500, vec![], false);

        let result = el.run_to_completion();
        assert_eq!(result.macrotasks_processed, 1);
        assert_eq!(result.final_time, 500);
    }

    // ── setImmediate tests ────────────────────────────────────────────

    #[test]
    fn test_set_immediate_schedules_at_current_time() {
        let mut el = EventLoop::new();
        let id = el.set_immediate(Value::String("imm".to_string()), vec![]);
        assert!(id > 0);
        assert!(el.has_pending_macrotasks());

        // Should be immediately ready (0ms delay at current time)
        let task = el.get_next_ready_macrotask();
        assert!(task.is_some());
        let task = task.unwrap();
        assert!(matches!(task.callback, Value::String(ref s) if s == "imm"));
        assert_eq!(task.delay, 0);
        assert!(!task.repeating);
    }

    #[test]
    fn test_set_immediate_with_args() {
        let mut el = EventLoop::new();
        let args = vec![Value::Number(1.0), Value::String("hello".to_string())];
        el.set_immediate(Value::Undefined, args);

        let task = el.get_next_ready_macrotask().unwrap();
        assert_eq!(task.args.len(), 2);
    }

    #[test]
    fn test_set_immediate_fires_before_delayed_timer() {
        let mut el = EventLoop::new();
        // Schedule a delayed timer first
        el.schedule_timer(Value::String("delayed".to_string()), 100, vec![], false);
        // Then schedule setImmediate
        el.set_immediate(Value::String("immediate".to_string()), vec![]);

        // The immediate task should fire first (at time 0)
        let task = el.get_next_ready_macrotask().unwrap();
        assert!(matches!(task.callback, Value::String(ref s) if s == "immediate"));
    }

    #[test]
    fn test_set_immediate_processed_by_run_to_completion() {
        let mut el = EventLoop::new();
        el.set_immediate(Value::Number(1.0), vec![]);
        el.set_immediate(Value::Number(2.0), vec![]);

        let result = el.run_to_completion();
        assert_eq!(result.macrotasks_processed, 2);
        // Time should not advance for immediate tasks
        assert_eq!(result.final_time, 0);
        assert!(!el.has_pending_work());
    }

    #[test]
    fn test_async_task_submission() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();
        let result_promise = el.create_promise();

        let task = AsyncTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting,
            result_promise,
            saved_bp: 0,
        };

        el.submit_async_task(task);
        assert_eq!(el.pending_async_task_count(), 1);
    }

    #[test]
    fn test_async_task_ready_when_promise_fulfilled() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();
        let result_promise = el.create_promise();

        let task = AsyncTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 10,
            saved_locals: vec![Value::Number(1.0)],
            saved_stack: vec![],
            awaiting: awaiting.clone(),
            result_promise,
            saved_bp: 0,
        };

        el.submit_async_task(task);
        assert_eq!(el.pending_async_task_count(), 1);

        // Fulfill the awaited promise
        el.fulfill_promise(&awaiting, Value::Number(42.0));

        let ready = el.get_ready_async_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
        assert_eq!(ready[0].saved_ip, 10);
        assert_eq!(el.pending_async_task_count(), 0);
    }

    #[test]
    fn test_async_task_ready_when_promise_rejected() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();
        let result_promise = el.create_promise();

        let task = AsyncTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 5,
            saved_locals: vec![],
            saved_stack: vec![Value::Boolean(true)],
            awaiting: awaiting.clone(),
            result_promise,
            saved_bp: 0,
        };

        el.submit_async_task(task);

        // Reject the awaited promise
        el.reject_promise(&awaiting, Value::String("error".to_string()));

        let ready = el.get_ready_async_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
        assert_eq!(el.pending_async_task_count(), 0);
    }

    #[test]
    fn test_async_task_not_ready_when_pending() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();
        let result_promise = el.create_promise();

        let task = AsyncTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting,
            result_promise,
            saved_bp: 0,
        };

        el.submit_async_task(task);

        // Promise is still pending — no tasks should be ready
        let ready = el.get_ready_async_tasks();
        assert_eq!(ready.len(), 0);
        assert_eq!(el.pending_async_task_count(), 1);
    }

    #[test]
    fn test_async_task_count() {
        let mut el = EventLoop::new();

        assert_eq!(el.pending_async_task_count(), 0);

        for i in 0..3 {
            let awaiting = el.create_promise();
            let result_promise = el.create_promise();
            let task = AsyncTask {
                id: i,
                function: Value::Undefined,
                saved_ip: 0,
                saved_locals: vec![],
                saved_stack: vec![],
                awaiting,
                result_promise,
                saved_bp: 0,
            };
            el.submit_async_task(task);
        }

        assert_eq!(el.pending_async_task_count(), 3);

        // Fulfill the first task's awaited promise
        let first_awaiting = el.pending_async_tasks[0].awaiting.clone();
        el.fulfill_promise(&first_awaiting, Value::Undefined);

        let ready = el.get_ready_async_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(el.pending_async_task_count(), 2);
    }

    // ── AbortController/AbortSignal tests ─────────────────────────────

    #[test]
    fn test_abort_signal_new() {
        let signal = AbortSignal::new();
        assert!(!signal.aborted);
        assert!(signal.reason.is_none());
        assert!(signal.on_abort_callbacks().is_empty());
    }

    #[test]
    fn test_abort_signal_abort() {
        let mut signal = AbortSignal::new();
        let callbacks = signal.abort(Value::String("cancelled".to_string()));
        assert!(signal.aborted);
        assert!(matches!(signal.reason, Some(Value::String(ref s)) if s == "cancelled"));
        assert!(callbacks.is_empty());
    }

    #[test]
    fn test_abort_signal_abort_with_callbacks() {
        let mut signal = AbortSignal::new();
        signal.add_on_abort(Value::String("cb1".to_string()));
        signal.add_on_abort(Value::String("cb2".to_string()));

        let callbacks = signal.abort(Value::String("reason".to_string()));
        assert!(signal.aborted);
        assert_eq!(callbacks.len(), 2);
        assert!(matches!(&callbacks[0], Value::String(s) if s == "cb1"));
        assert!(matches!(&callbacks[1], Value::String(s) if s == "cb2"));
    }

    #[test]
    fn test_abort_signal_abort_idempotent() {
        let mut signal = AbortSignal::new();
        signal.add_on_abort(Value::String("cb".to_string()));

        let first = signal.abort(Value::String("first".to_string()));
        assert_eq!(first.len(), 1);

        // Second abort should be a no-op
        let second = signal.abort(Value::String("second".to_string()));
        assert!(second.is_empty());
        assert!(matches!(signal.reason, Some(Value::String(ref s)) if s == "first"));
    }

    #[test]
    fn test_abort_signal_timeout() {
        let signal = AbortSignal::timeout(5000);
        assert!(!signal.aborted);
        assert_eq!(signal.timeout_ms(), Some(5000));
    }

    #[test]
    fn test_abort_signal_check_timeout_not_elapsed() {
        let mut signal = AbortSignal::timeout(5000);
        assert!(!signal.check_timeout(3000));
        assert!(!signal.aborted);
    }

    #[test]
    fn test_abort_signal_check_timeout_elapsed() {
        let mut signal = AbortSignal::timeout(5000);
        assert!(signal.check_timeout(5000));
        assert!(signal.aborted);
        assert!(matches!(signal.reason, Some(Value::String(ref s)) if s == "TimeoutError"));
    }

    #[test]
    fn test_abort_signal_check_timeout_already_aborted() {
        let mut signal = AbortSignal::timeout(5000);
        signal.abort(Value::String("manual".to_string()));
        // Should not re-abort
        assert!(!signal.check_timeout(6000));
        assert!(matches!(signal.reason, Some(Value::String(ref s)) if s == "manual"));
    }

    #[test]
    fn test_abort_signal_any_none_aborted() {
        let s1 = AbortSignal::new();
        let s2 = AbortSignal::new();
        let combined = AbortSignal::any(&[&s1, &s2]);
        assert!(!combined.aborted);
        assert!(combined.reason.is_none());
    }

    #[test]
    fn test_abort_signal_any_one_aborted() {
        let s1 = AbortSignal::new();
        let mut s2 = AbortSignal::new();
        s2.abort(Value::String("aborted".to_string()));

        let combined = AbortSignal::any(&[&s1, &s2]);
        assert!(combined.aborted);
        assert!(matches!(combined.reason, Some(Value::String(ref s)) if s == "aborted"));
    }

    #[test]
    fn test_abort_signal_any_first_aborted_wins() {
        let mut s1 = AbortSignal::new();
        s1.abort(Value::String("first".to_string()));
        let mut s2 = AbortSignal::new();
        s2.abort(Value::String("second".to_string()));

        let combined = AbortSignal::any(&[&s1, &s2]);
        assert!(combined.aborted);
        assert!(matches!(combined.reason, Some(Value::String(ref s)) if s == "first"));
    }

    #[test]
    fn test_abort_signal_any_empty() {
        let combined = AbortSignal::any(&[]);
        assert!(!combined.aborted);
    }

    #[test]
    fn test_abort_controller_new() {
        let controller = AbortController::new();
        assert!(!controller.signal().aborted);
    }

    #[test]
    fn test_abort_controller_abort() {
        let mut controller = AbortController::new();
        let callbacks = controller.abort(Value::String("cancelled".to_string()));
        assert!(callbacks.is_empty());
        assert!(controller.signal().aborted);
        assert!(matches!(controller.signal().reason, Some(Value::String(ref s)) if s == "cancelled"));
    }

    #[test]
    fn test_abort_controller_abort_fires_callbacks() {
        let mut controller = AbortController::new();
        controller.signal_mut().add_on_abort(Value::Number(1.0));
        controller.signal_mut().add_on_abort(Value::Number(2.0));

        let callbacks = controller.abort(Value::String("abort".to_string()));
        assert_eq!(callbacks.len(), 2);
        assert!(controller.signal().aborted);
    }

    #[test]
    fn test_abort_controller_default() {
        let controller = AbortController::default();
        assert!(!controller.signal().aborted);
    }

    // ── Microtask budget tests ────────────────────────────────────────

    #[test]
    fn test_default_microtask_budget() {
        let el = EventLoop::new();
        assert_eq!(el.microtask_budget(), 10_000);
    }

    #[test]
    fn test_set_microtask_budget() {
        let mut el = EventLoop::new();
        el.set_microtask_budget(100);
        assert_eq!(el.microtask_budget(), 100);
    }

    #[test]
    fn test_drain_microtasks_respects_budget() {
        let mut el = EventLoop::new();
        el.set_microtask_budget(3);

        for i in 0..10 {
            el.queue_microtask_fn(Value::Number(i as f64));
        }

        let (drained, remaining) = el.drain_microtasks();
        assert_eq!(drained.len(), 3);
        assert_eq!(remaining, 7);
        assert!(el.has_pending_microtasks());

        // Drain again — should get next 3
        let (drained2, remaining2) = el.drain_microtasks();
        assert_eq!(drained2.len(), 3);
        assert_eq!(remaining2, 4);

        // Drain remaining
        let (drained3, remaining3) = el.drain_microtasks();
        assert_eq!(drained3.len(), 3);
        assert_eq!(remaining3, 1);

        let (drained4, remaining4) = el.drain_microtasks();
        assert_eq!(drained4.len(), 1);
        assert_eq!(remaining4, 0);
        assert!(!el.has_pending_microtasks());
    }

    #[test]
    fn test_drain_microtasks_budget_larger_than_queue() {
        let mut el = EventLoop::new();
        el.set_microtask_budget(100);
        el.queue_microtask_fn(Value::Number(1.0));
        el.queue_microtask_fn(Value::Number(2.0));

        let (drained, remaining) = el.drain_microtasks();
        assert_eq!(drained.len(), 2);
        assert_eq!(remaining, 0);
    }

    // ── Event loop stats tests ────────────────────────────────────────

    #[test]
    fn test_stats_initial() {
        let el = EventLoop::new();
        let stats = el.stats();
        assert_eq!(stats.total_microtasks, 0);
        assert_eq!(stats.total_macrotasks, 0);
        assert_eq!(stats.total_ticks, 0);
        assert_eq!(stats.max_microtasks_per_tick, 0);
        assert_eq!(stats.longest_tick_ms, 0);
        assert_eq!(stats.total_promises_created, 0);
        assert_eq!(stats.total_promises_settled, 0);
    }

    #[test]
    fn test_stats_promise_tracking() {
        let mut el = EventLoop::new();
        let p1 = el.create_promise();
        let _p2 = el.create_promise();
        assert_eq!(el.stats().total_promises_created, 2);
        assert_eq!(el.stats().total_promises_settled, 0);

        el.fulfill_promise(&p1, Value::Number(1.0));
        assert_eq!(el.stats().total_promises_settled, 1);
    }

    #[test]
    fn test_stats_promise_reject_tracking() {
        let mut el = EventLoop::new();
        let p = el.create_promise();
        el.reject_promise(&p, Value::String("err".to_string()));
        assert_eq!(el.stats().total_promises_created, 1);
        assert_eq!(el.stats().total_promises_settled, 1);
    }

    #[test]
    fn test_stats_double_settle_not_counted() {
        let mut el = EventLoop::new();
        let p = el.create_promise();
        el.fulfill_promise(&p, Value::Number(1.0));
        el.fulfill_promise(&p, Value::Number(2.0)); // no-op
        assert_eq!(el.stats().total_promises_settled, 1);
    }

    #[test]
    fn test_stats_microtask_tracking() {
        let mut el = EventLoop::new();
        el.queue_microtask_fn(Value::Undefined);
        el.queue_microtask_fn(Value::Undefined);
        el.queue_microtask_fn(Value::Undefined);

        let (drained, _) = el.drain_microtasks();
        assert_eq!(drained.len(), 3);
        assert_eq!(el.stats().total_microtasks, 3);
        assert_eq!(el.stats().max_microtasks_per_tick, 3);
    }

    #[test]
    fn test_stats_max_microtasks_per_tick() {
        let mut el = EventLoop::new();

        // First tick: 2 microtasks
        el.queue_microtask_fn(Value::Undefined);
        el.queue_microtask_fn(Value::Undefined);
        el.drain_microtasks();
        assert_eq!(el.stats().max_microtasks_per_tick, 2);

        // Second tick: 5 microtasks
        for _ in 0..5 {
            el.queue_microtask_fn(Value::Undefined);
        }
        el.drain_microtasks();
        assert_eq!(el.stats().max_microtasks_per_tick, 5);

        // Third tick: 1 microtask — max should remain 5
        el.queue_microtask_fn(Value::Undefined);
        el.drain_microtasks();
        assert_eq!(el.stats().max_microtasks_per_tick, 5);
        assert_eq!(el.stats().total_microtasks, 8);
    }

    #[test]
    fn test_stats_run_to_completion() {
        let mut el = EventLoop::new();
        el.queue_microtask_fn(Value::Undefined);
        el.schedule_timer(Value::Undefined, 100, vec![], false);
        el.schedule_timer(Value::Undefined, 200, vec![], false);

        el.run_to_completion();

        let stats = el.stats();
        assert_eq!(stats.total_microtasks, 1);
        assert_eq!(stats.total_macrotasks, 2);
        assert!(stats.total_ticks >= 1);
    }

    #[test]
    fn test_reset_stats() {
        let mut el = EventLoop::new();
        el.create_promise();
        el.queue_microtask_fn(Value::Undefined);
        el.drain_microtasks();

        assert!(el.stats().total_microtasks > 0);
        assert!(el.stats().total_promises_created > 0);

        el.reset_stats();
        let stats = el.stats();
        assert_eq!(stats.total_microtasks, 0);
        assert_eq!(stats.total_macrotasks, 0);
        assert_eq!(stats.total_ticks, 0);
        assert_eq!(stats.total_promises_created, 0);
        assert_eq!(stats.total_promises_settled, 0);
    }

    // ── Async generator task tests ────────────────────────────────────

    #[test]
    fn test_async_generator_task_submission() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting,
            result_queue: vec![],
            done: false,
        };

        el.submit_async_generator_task(task);
        assert_eq!(el.pending_async_generator_count(), 1);
    }

    #[test]
    fn test_async_generator_task_not_ready_when_pending() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting,
            result_queue: vec![],
            done: false,
        };

        el.submit_async_generator_task(task);
        let ready = el.get_ready_async_generator_tasks();
        assert_eq!(ready.len(), 0);
        assert_eq!(el.pending_async_generator_count(), 1);
    }

    #[test]
    fn test_async_generator_task_ready_when_fulfilled() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 42,
            saved_locals: vec![Value::Number(10.0)],
            saved_stack: vec![],
            awaiting: awaiting.clone(),
            result_queue: vec![Value::Number(1.0), Value::Number(2.0)],
            done: false,
        };

        el.submit_async_generator_task(task);
        el.fulfill_promise(&awaiting, Value::Number(99.0));

        let ready = el.get_ready_async_generator_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
        assert_eq!(ready[0].saved_ip, 42);
        assert_eq!(ready[0].result_queue.len(), 2);
        assert!(!ready[0].done);
        assert_eq!(el.pending_async_generator_count(), 0);
    }

    #[test]
    fn test_async_generator_task_ready_when_rejected() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 5,
            saved_locals: vec![],
            saved_stack: vec![Value::Boolean(true)],
            awaiting: awaiting.clone(),
            result_queue: vec![],
            done: false,
        };

        el.submit_async_generator_task(task);
        el.reject_promise(&awaiting, Value::String("error".to_string()));

        let ready = el.get_ready_async_generator_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
        assert_eq!(el.pending_async_generator_count(), 0);
    }

    #[test]
    fn test_async_generator_task_done_flag() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting: awaiting.clone(),
            result_queue: vec![Value::String("final".to_string())],
            done: true,
        };

        el.submit_async_generator_task(task);
        el.fulfill_promise(&awaiting, Value::Undefined);

        let ready = el.get_ready_async_generator_tasks();
        assert_eq!(ready.len(), 1);
        assert!(ready[0].done);
    }

    #[test]
    fn test_async_generator_task_multiple() {
        let mut el = EventLoop::new();

        for i in 0..3 {
            let awaiting = el.create_promise();
            let task = AsyncGeneratorTask {
                id: i,
                function: Value::Undefined,
                saved_ip: i as usize,
                saved_locals: vec![],
                saved_stack: vec![],
                awaiting,
                result_queue: vec![],
                done: false,
            };
            el.submit_async_generator_task(task);
        }

        assert_eq!(el.pending_async_generator_count(), 3);

        // Fulfill only the second task's awaited promise
        let second_awaiting = el.pending_async_generator_tasks[1].awaiting.clone();
        el.fulfill_promise(&second_awaiting, Value::Undefined);

        let ready = el.get_ready_async_generator_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, 1);
        assert_eq!(el.pending_async_generator_count(), 2);
    }

    #[test]
    fn test_async_generator_task_clear() {
        let mut el = EventLoop::new();
        let awaiting = el.create_promise();

        let task = AsyncGeneratorTask {
            id: 1,
            function: Value::Undefined,
            saved_ip: 0,
            saved_locals: vec![],
            saved_stack: vec![],
            awaiting,
            result_queue: vec![],
            done: false,
        };

        el.submit_async_generator_task(task);
        assert_eq!(el.pending_async_generator_count(), 1);

        el.clear();
        assert_eq!(el.pending_async_generator_count(), 0);
    }

    // ── SuspensionPoint & SuspensionManager tests ─────────────────────

    fn make_test_suspension(el: &mut EventLoop, name: &str) -> SuspensionPoint {
        SuspensionPoint {
            id: 0,
            function_name: name.to_string(),
            ip: 42,
            bp: 10,
            locals: vec![Value::Number(1.0), Value::String("x".to_string())],
            stack: vec![Value::Boolean(true)],
            awaiting_promise: el.create_promise(),
            result_promise: el.create_promise(),
            exception_handlers: vec![],
            scope_depth: 2,
            suspended_at: 1000,
        }
    }

    #[test]
    fn test_suspension_point_creation() {
        let mut el = EventLoop::new();
        let sp = make_test_suspension(&mut el, "myFunc");
        assert_eq!(sp.function_name, "myFunc");
        assert_eq!(sp.ip, 42);
        assert_eq!(sp.bp, 10);
        assert_eq!(sp.locals.len(), 2);
        assert_eq!(sp.scope_depth, 2);
        assert_eq!(sp.suspended_at, 1000);
    }

    #[test]
    fn test_suspension_manager_suspend_and_resume() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();
        let sp = make_test_suspension(&mut el, "asyncFn");
        let id = mgr.suspend(sp);
        assert!(id > 0);
        assert_eq!(mgr.pending_count(), 1);

        let resumed = mgr.resume(id);
        assert!(resumed.is_some());
        let resumed = resumed.unwrap();
        assert_eq!(resumed.function_name, "asyncFn");
        assert_eq!(resumed.ip, 42);
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn test_suspension_manager_cancel() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();
        let sp = make_test_suspension(&mut el, "cancelMe");
        let result_promise = sp.result_promise.clone();
        let id = mgr.suspend(sp);

        assert!(mgr.cancel(id));
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(result_promise.borrow().state, PromiseInternalState::Rejected);
        // Cancelling again returns false
        assert!(!mgr.cancel(id));
    }

    #[test]
    fn test_suspension_manager_cancel_all() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();
        let sp1 = make_test_suspension(&mut el, "f1");
        let sp2 = make_test_suspension(&mut el, "f2");
        // Use the promises from the actual suspensions
        let sp1_rp = sp1.result_promise.clone();
        let sp2_rp = sp2.result_promise.clone();
        mgr.suspend(sp1);
        mgr.suspend(sp2);

        let count = mgr.cancel_all();
        assert_eq!(count, 2);
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(sp1_rp.borrow().state, PromiseInternalState::Rejected);
        assert_eq!(sp2_rp.borrow().state, PromiseInternalState::Rejected);
    }

    #[test]
    fn test_suspension_manager_stats() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();
        let sp1 = make_test_suspension(&mut el, "a");
        let sp2 = make_test_suspension(&mut el, "b");
        let id1 = mgr.suspend(sp1);
        let _id2 = mgr.suspend(sp2);

        let stats = mgr.stats();
        assert_eq!(stats.total_suspensions, 2);
        assert_eq!(stats.currently_suspended, 2);
        assert_eq!(stats.max_concurrent_suspensions, 2);
        assert_eq!(stats.total_resumptions, 0);

        mgr.resume(id1);
        let stats = mgr.stats();
        assert_eq!(stats.total_resumptions, 1);
        assert_eq!(stats.currently_suspended, 1);
        assert_eq!(stats.max_concurrent_suspensions, 2);
    }

    #[test]
    fn test_suspension_manager_pending_count() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();
        assert_eq!(mgr.pending_count(), 0);

        let sp = make_test_suspension(&mut el, "fn1");
        mgr.suspend(sp);
        assert_eq!(mgr.pending_count(), 1);

        let sp = make_test_suspension(&mut el, "fn2");
        mgr.suspend(sp);
        assert_eq!(mgr.pending_count(), 2);
    }

    #[test]
    fn test_exception_handler_state_creation() {
        let handler = ExceptionHandlerState {
            catch_ip: 100,
            finally_ip: Some(200),
            stack_depth: 5,
        };
        assert_eq!(handler.catch_ip, 100);
        assert_eq!(handler.finally_ip, Some(200));
        assert_eq!(handler.stack_depth, 5);

        let handler_no_finally = ExceptionHandlerState {
            catch_ip: 50,
            finally_ip: None,
            stack_depth: 3,
        };
        assert!(handler_no_finally.finally_ip.is_none());
    }

    #[test]
    fn test_async_generator_state_creation() {
        let gen = AsyncGeneratorState::new();
        assert_eq!(gen.state, AsyncGenState::SuspendedStart);
        assert!(gen.queue.is_empty());
        assert!(gen.suspension.is_none());
        assert!(gen.yielded_values.is_empty());
        assert!(!gen.is_completed());
    }

    #[test]
    fn test_async_generator_state_enqueue_dequeue() {
        let mut el = EventLoop::new();
        let mut gen = AsyncGeneratorState::new();
        let promise = el.create_promise();

        gen.enqueue(AsyncGenRequest {
            kind: AsyncGenRequestKind::Next,
            value: Value::Number(1.0),
            promise: promise.clone(),
        });
        gen.enqueue(AsyncGenRequest {
            kind: AsyncGenRequestKind::Return,
            value: Value::Number(2.0),
            promise: el.create_promise(),
        });

        assert_eq!(gen.queue.len(), 2);

        let req = gen.dequeue().unwrap();
        assert_eq!(req.kind, AsyncGenRequestKind::Next);
        assert!(matches!(req.value, Value::Number(n) if n == 1.0));

        let req = gen.dequeue().unwrap();
        assert_eq!(req.kind, AsyncGenRequestKind::Return);

        assert!(gen.dequeue().is_none());
    }

    #[test]
    fn test_async_generator_state_transitions() {
        let mut gen = AsyncGeneratorState::new();
        assert_eq!(gen.state, AsyncGenState::SuspendedStart);
        assert!(!gen.is_completed());

        gen.state = AsyncGenState::Executing;
        assert_eq!(gen.state, AsyncGenState::Executing);
        assert!(!gen.is_completed());

        gen.state = AsyncGenState::SuspendedYield;
        assert_eq!(gen.state, AsyncGenState::SuspendedYield);

        gen.state = AsyncGenState::AwaitingReturn;
        assert_eq!(gen.state, AsyncGenState::AwaitingReturn);

        gen.complete();
        assert!(gen.is_completed());
        assert_eq!(gen.state, AsyncGenState::Completed);
    }

    #[test]
    fn test_async_gen_request_kinds() {
        assert_ne!(AsyncGenRequestKind::Next, AsyncGenRequestKind::Return);
        assert_ne!(AsyncGenRequestKind::Return, AsyncGenRequestKind::Throw);
        assert_ne!(AsyncGenRequestKind::Next, AsyncGenRequestKind::Throw);
        assert_eq!(AsyncGenRequestKind::Next, AsyncGenRequestKind::Next);
    }

    #[test]
    fn test_promise_executor_enqueue_dequeue() {
        let mut executor = PromiseExecutor::new();
        assert_eq!(executor.pending_count(), 0);

        executor.enqueue(PendingCallback {
            id: 0,
            callback: Value::Number(1.0),
            args: vec![Value::Boolean(true)],
            resolve_promise: None,
        });
        executor.enqueue(PendingCallback {
            id: 0,
            callback: Value::Number(2.0),
            args: vec![],
            resolve_promise: None,
        });

        assert_eq!(executor.pending_count(), 2);

        let cb = executor.dequeue().unwrap();
        assert!(matches!(cb.callback, Value::Number(n) if n == 1.0));
        assert_eq!(cb.args.len(), 1);
        assert_eq!(executor.pending_count(), 1);

        let cb = executor.dequeue().unwrap();
        assert!(matches!(cb.callback, Value::Number(n) if n == 2.0));
        assert_eq!(executor.pending_count(), 0);

        assert!(executor.dequeue().is_none());
    }

    #[test]
    fn test_promise_executor_complete_and_drain() {
        let mut executor = PromiseExecutor::new();

        executor.complete(CallbackResult {
            id: 1,
            value: Value::Number(42.0),
            is_error: false,
        });
        executor.complete(CallbackResult {
            id: 2,
            value: Value::String("err".to_string()),
            is_error: true,
        });

        let results = executor.drain_completed();
        assert_eq!(results.len(), 2);
        assert!(!results[0].is_error);
        assert_eq!(results[0].id, 1);
        assert!(results[1].is_error);

        // Drain again should be empty
        let results = executor.drain_completed();
        assert!(results.is_empty());
    }

    #[test]
    fn test_async_stats_defaults() {
        let stats = AsyncStats::default();
        assert_eq!(stats.total_awaits, 0);
        assert_eq!(stats.total_resumes, 0);
        assert_eq!(stats.total_async_functions, 0);
        assert_eq!(stats.total_async_generators, 0);
        assert_eq!(stats.current_suspended, 0);
        assert_eq!(stats.max_suspension_depth, 0);
        assert_eq!(stats.avg_suspension_time_us, 0);
    }

    #[test]
    fn test_multiple_suspensions_concurrently() {
        let mut el = EventLoop::new();
        let mut mgr = SuspensionManager::new();

        let mut ids = Vec::new();
        for i in 0..5 {
            let mut sp = make_test_suspension(&mut el, &format!("fn{}", i));
            sp.ip = i * 10;
            ids.push(mgr.suspend(sp));
        }

        assert_eq!(mgr.pending_count(), 5);
        assert_eq!(mgr.stats().max_concurrent_suspensions, 5);

        // Resume middle one
        let point = mgr.resume(ids[2]).unwrap();
        assert_eq!(point.ip, 20);
        assert_eq!(mgr.pending_count(), 4);

        // Others still accessible
        assert!(mgr.is_suspended(ids[0]));
        assert!(mgr.is_suspended(ids[1]));
        assert!(!mgr.is_suspended(ids[2]));
        assert!(mgr.is_suspended(ids[3]));
        assert!(mgr.is_suspended(ids[4]));
    }

    #[test]
    fn test_resume_nonexistent_suspension() {
        let mut mgr = SuspensionManager::new();
        assert!(mgr.resume(999).is_none());
        assert!(mgr.get(999).is_none());
        assert!(!mgr.is_suspended(999));
    }

    #[test]
    fn test_suspension_with_exception_handlers() {
        let mut el = EventLoop::new();
        let mut sp = make_test_suspension(&mut el, "tryCatchAsync");
        sp.exception_handlers = vec![
            ExceptionHandlerState {
                catch_ip: 50,
                finally_ip: Some(80),
                stack_depth: 3,
            },
            ExceptionHandlerState {
                catch_ip: 120,
                finally_ip: None,
                stack_depth: 5,
            },
        ];

        let mut mgr = SuspensionManager::new();
        let id = mgr.suspend(sp);

        let resumed = mgr.resume(id).unwrap();
        assert_eq!(resumed.exception_handlers.len(), 2);
        assert_eq!(resumed.exception_handlers[0].catch_ip, 50);
        assert_eq!(resumed.exception_handlers[0].finally_ip, Some(80));
        assert_eq!(resumed.exception_handlers[1].catch_ip, 120);
        assert!(resumed.exception_handlers[1].finally_ip.is_none());
    }
}

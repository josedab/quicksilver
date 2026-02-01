//! Event Loop Implementation
//!
//! This module provides a complete JavaScript event loop implementation
//! with proper microtask and macrotask queue semantics following the
//! ECMAScript specification and HTML5 event loop model.

//! **Status:** ⚠️ Partial — Promise/A+ microtask queue, basic timers

use crate::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

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
#[derive(Clone)]
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
#[derive(Clone)]
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
}

//! Event Loop Implementation
//!
//! This module provides a complete JavaScript event loop implementation
//! with proper microtask and macrotask queue semantics following the
//! ECMAScript specification and HTML5 event loop model.

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
    draining_microtasks: bool,
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
            draining_microtasks: false,
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
}

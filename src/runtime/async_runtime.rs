//! Async Runtime Support
//!
//! This module provides support for async functions and proper await semantics.
//! It implements suspension/resumption of async functions and integration with
//! the event loop.

use super::value::{Function, Object, ObjectKind, PromiseState, Value};
use crate::error::{Error, Result};
use crate::event_loop::{EventLoop, PromiseInternal};
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// State of a suspended async function
#[derive(Clone)]
pub struct SuspendedAsyncFunction {
    /// The async function being executed
    pub function: Rc<RefCell<Function>>,
    /// Saved instruction pointer
    pub ip: usize,
    /// Saved local variables
    pub locals: Vec<Value>,
    /// Saved stack
    pub stack: Vec<Value>,
    /// The promise that will be resolved with the function's return value
    pub result_promise: Rc<RefCell<PromiseInternal>>,
    /// The promise we're currently awaiting (if any)
    pub awaited_promise: Option<Value>,
    /// Base pointer for the function's stack frame
    pub bp: usize,
}

impl SuspendedAsyncFunction {
    /// Create a new suspended async function state
    pub fn new(
        function: Rc<RefCell<Function>>,
        result_promise: Rc<RefCell<PromiseInternal>>,
    ) -> Self {
        Self {
            function,
            ip: 0,
            locals: Vec::new(),
            stack: Vec::new(),
            result_promise,
            awaited_promise: None,
            bp: 0,
        }
    }

    /// Save the current execution state
    pub fn save_state(&mut self, ip: usize, locals: Vec<Value>, stack: Vec<Value>, bp: usize) {
        self.ip = ip;
        self.locals = locals;
        self.stack = stack;
        self.bp = bp;
    }

    /// Check if the awaited promise is ready
    pub fn is_ready(&self) -> bool {
        if let Some(ref promise) = self.awaited_promise {
            if let Value::Object(obj) = promise {
                let obj_ref = obj.borrow();
                if let ObjectKind::Promise { state, .. } = &obj_ref.kind {
                    return *state != PromiseState::Pending;
                }
            }
        }
        true // No promise to await, or not a promise
    }

    /// Get the resolved value of the awaited promise
    pub fn get_awaited_value(&self) -> Result<Value> {
        if let Some(ref promise) = self.awaited_promise {
            if let Value::Object(obj) = promise {
                let obj_ref = obj.borrow();
                if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                    match state {
                        PromiseState::Fulfilled => {
                            return Ok(value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined));
                        }
                        PromiseState::Rejected => {
                            let reason = value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined);
                            return Err(Error::InternalError(format!(
                                "Uncaught (in promise): {}",
                                reason.to_js_string()
                            )));
                        }
                        PromiseState::Pending => {
                            return Err(Error::InternalError(
                                "Cannot get value of pending promise".to_string(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(Value::Undefined)
    }
}

/// Async function executor that manages suspended async functions
pub struct AsyncExecutor {
    /// Queue of suspended async functions waiting to be resumed
    suspended: Vec<SuspendedAsyncFunction>,
    /// Currently executing async function (if any)
    _current: Option<SuspendedAsyncFunction>,
}

impl Default for AsyncExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncExecutor {
    /// Create a new async executor
    pub fn new() -> Self {
        Self {
            suspended: Vec::new(),
            _current: None,
        }
    }

    /// Start executing an async function
    pub fn start_async_function(
        &mut self,
        function: Rc<RefCell<Function>>,
        result_promise: Rc<RefCell<PromiseInternal>>,
    ) -> SuspendedAsyncFunction {
        SuspendedAsyncFunction::new(function, result_promise)
    }

    /// Suspend the current async function, waiting for a promise
    pub fn suspend(&mut self, mut state: SuspendedAsyncFunction, awaited_promise: Value) {
        state.awaited_promise = Some(awaited_promise);
        self.suspended.push(state);
    }

    /// Get the next async function that's ready to resume
    pub fn get_ready_function(&mut self) -> Option<SuspendedAsyncFunction> {
        // Find a function that's ready (awaited promise is resolved)
        let ready_idx = self.suspended.iter().position(|f| f.is_ready());
        ready_idx.map(|idx| self.suspended.remove(idx))
    }

    /// Check if there are any suspended async functions
    pub fn has_suspended(&self) -> bool {
        !self.suspended.is_empty()
    }

    /// Get the number of suspended functions
    pub fn suspended_count(&self) -> usize {
        self.suspended.len()
    }
}

/// Result of executing one step of an async function
pub enum AsyncStepResult {
    /// Function completed with a value
    Completed(Value),
    /// Function is suspended waiting for a promise
    Suspended(SuspendedAsyncFunction),
    /// Function threw an error
    Error(Error),
}

/// Create a promise for an async function result
pub fn create_async_function_promise(event_loop: &mut EventLoop) -> (Value, Rc<RefCell<PromiseInternal>>) {
    let internal = event_loop.create_promise();

    // Create the external Promise value
    let obj = Rc::new(RefCell::new(Object {
        kind: ObjectKind::Promise {
            state: PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
        cached_shape_id: None,
    }));

    (Value::Object(obj), internal)
}

/// Resolve an async function's promise with a value
pub fn resolve_async_promise(
    event_loop: &mut EventLoop,
    promise: &Rc<RefCell<PromiseInternal>>,
    value: Value,
) {
    event_loop.fulfill_promise(promise, value);
}

/// Reject an async function's promise with a reason
pub fn reject_async_promise(
    event_loop: &mut EventLoop,
    promise: &Rc<RefCell<PromiseInternal>>,
    reason: Value,
) {
    event_loop.reject_promise(promise, reason);
}

/// Check if a value is a thenable (has .then method)
pub fn is_thenable(value: &Value) -> bool {
    if let Value::Object(obj) = value {
        let obj_ref = obj.borrow();
        // Check for Promise
        if matches!(obj_ref.kind, ObjectKind::Promise { .. }) {
            return true;
        }
        // Check for .then property that's a function
        if let Some(then_prop) = obj_ref.get_property("then") {
            if let Value::Object(then_obj) = &then_prop {
                let then_ref = then_obj.borrow();
                return matches!(
                    then_ref.kind,
                    ObjectKind::Function(_)
                        | ObjectKind::NativeFunction { .. }
                        | ObjectKind::BoundFunction { .. }
                );
            }
        }
    }
    false
}

/// Resolve a value, wrapping non-thenables in a resolved promise
pub fn resolve_value(event_loop: &mut EventLoop, value: Value) -> Value {
    if is_thenable(&value) {
        value
    } else {
        // Wrap in a resolved promise
        super::promise::create_resolved_promise_el(event_loop, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Chunk;

    fn create_test_function() -> Rc<RefCell<Function>> {
        Rc::new(RefCell::new(Function {
            name: Some("test".to_string()),
            chunk: Chunk::new(),
            upvalues: Vec::new(),
            is_async: true,
            is_generator: false,
        }))
    }

    #[test]
    fn test_async_executor_creation() {
        let executor = AsyncExecutor::new();
        assert!(!executor.has_suspended());
        assert_eq!(executor.suspended_count(), 0);
    }

    #[test]
    fn test_suspended_async_function() {
        let mut event_loop = EventLoop::new();
        let function = create_test_function();
        let promise = event_loop.create_promise();

        let mut state = SuspendedAsyncFunction::new(function, promise);
        assert!(state.is_ready()); // No awaited promise yet

        // Save some state
        state.save_state(10, vec![Value::Number(1.0)], vec![Value::Number(2.0)], 5);
        assert_eq!(state.ip, 10);
        assert_eq!(state.bp, 5);
    }

    #[test]
    fn test_is_thenable() {
        // Regular values are not thenable
        assert!(!is_thenable(&Value::Number(42.0)));
        assert!(!is_thenable(&Value::String("hello".to_string())));
        assert!(!is_thenable(&Value::Undefined));

        // Promise is thenable
        let promise = super::super::promise::create_pending_promise_simple();
        assert!(is_thenable(&promise));
    }
}

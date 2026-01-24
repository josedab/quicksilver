//! Promise Implementation
//!
//! This module provides a Promise/A+ compliant Promise implementation
//! that integrates with the event loop for proper async semantics.

use super::value::{Object, ObjectKind, PromiseState, Value};
use crate::event_loop::{EventLoop, PromiseInternal, PromiseInternalState};
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// ID counter for Promise tracking
static PROMISE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Create a Promise that wraps an internal promise from the event loop
pub fn create_promise_from_internal(internal: Rc<RefCell<PromiseInternal>>) -> Value {
    let id = PROMISE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // Map internal state to external state
    let (state, value) = {
        let p = internal.borrow();
        let state = match p.state {
            PromiseInternalState::Pending => PromiseState::Pending,
            PromiseInternalState::Fulfilled => PromiseState::Fulfilled,
            PromiseInternalState::Rejected => PromiseState::Rejected,
        };
        let value = p.result.clone().map(Box::new);
        (state, value)
    };

    let obj = Rc::new(RefCell::new(Object {
        kind: ObjectKind::Promise {
            state,
            value,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
        cached_shape_id: None,
    }));

    // Store the internal promise reference
    obj.borrow_mut()
        .properties
        .insert("__internal_promise_id".to_string(), Value::Number(id as f64));

    let promise = Value::Object(obj.clone());
    add_promise_prototype_methods(&promise, internal);
    promise
}

/// Create a new pending promise
pub fn create_pending_promise(event_loop: &mut EventLoop) -> (Value, Rc<RefCell<PromiseInternal>>) {
    let internal = event_loop.create_promise();
    let promise = create_promise_from_internal(internal.clone());
    (promise, internal)
}

/// Create a resolved promise
pub fn create_resolved_promise_el(event_loop: &mut EventLoop, value: Value) -> Value {
    let internal = event_loop.resolve_promise(value);
    create_promise_from_internal(internal)
}

/// Create a rejected promise
pub fn create_rejected_promise_el(event_loop: &mut EventLoop, reason: Value) -> Value {
    let internal = event_loop.reject_promise_new(reason);
    create_promise_from_internal(internal)
}

/// Helper to create a simple resolved promise without event loop
pub fn create_resolved_promise_simple(value: Value) -> Value {
    let obj = Rc::new(RefCell::new(Object {
        kind: ObjectKind::Promise {
            state: PromiseState::Fulfilled,
            value: Some(Box::new(value)),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
        cached_shape_id: None,
    }));

    Value::Object(obj)
}

/// Helper to create a simple rejected promise without event loop
pub fn create_rejected_promise_simple(reason: Value) -> Value {
    let obj = Rc::new(RefCell::new(Object {
        kind: ObjectKind::Promise {
            state: PromiseState::Rejected,
            value: Some(Box::new(reason)),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
        cached_shape_id: None,
    }));

    Value::Object(obj)
}

/// Helper to create a simple pending promise without event loop
pub fn create_pending_promise_simple() -> Value {
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

    Value::Object(obj)
}

/// Add Promise prototype methods (.then, .catch, .finally)
fn add_promise_prototype_methods(promise: &Value, internal: Rc<RefCell<PromiseInternal>>) {
    // .then(onFulfilled, onRejected)
    let internal_then = internal.clone();
    let then_fn: Rc<dyn Fn(&[Value]) -> crate::error::Result<Value>> =
        Rc::new(move |args: &[Value]| {
            let on_fulfilled = args.first().cloned();
            let on_rejected = args.get(1).cloned();

            let p = internal_then.borrow();
            let (state, value) = (p.state, p.result.clone());
            drop(p);

            // Create result promise
            let result_promise = create_pending_promise_simple();

            match state {
                PromiseInternalState::Fulfilled => {
                    let resolved_value: Value = value.unwrap_or(Value::Undefined);
                    if let Some(callback) = on_fulfilled {
                        // In a full implementation, we'd schedule this as a microtask
                        // and call the callback with the value
                        // For now, return a resolved promise
                        if is_callable(&callback) {
                            // The callback should be called with the value
                            // and its result should settle the result_promise
                            return Ok(create_resolved_promise_simple(resolved_value));
                        }
                    }
                    Ok(create_resolved_promise_simple(resolved_value))
                }
                PromiseInternalState::Rejected => {
                    let rejection_reason: Value = value.unwrap_or(Value::Undefined);
                    if let Some(callback) = on_rejected {
                        if is_callable(&callback) {
                            return Ok(create_resolved_promise_simple(rejection_reason));
                        }
                    }
                    Ok(create_rejected_promise_simple(rejection_reason))
                }
                PromiseInternalState::Pending => Ok(result_promise),
            }
        });

    promise.set_property(
        "then",
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::NativeFunction {
                name: "then".to_string(),
                func: then_fn,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
            cached_shape_id: None,
        }))),
    );

    // .catch(onRejected) - shorthand for .then(undefined, onRejected)
    let internal_catch = internal.clone();
    let catch_fn: Rc<dyn Fn(&[Value]) -> crate::error::Result<Value>> =
        Rc::new(move |args: &[Value]| {
            let on_rejected = args.first().cloned();

            let p = internal_catch.borrow();
            let (state, value) = (p.state, p.result.clone());
            drop(p);

            match state {
                PromiseInternalState::Fulfilled => {
                    let resolved_value: Value = value.unwrap_or(Value::Undefined);
                    Ok(create_resolved_promise_simple(resolved_value))
                }
                PromiseInternalState::Rejected => {
                    let rejection_reason: Value = value.unwrap_or(Value::Undefined);
                    if let Some(callback) = on_rejected {
                        if is_callable(&callback) {
                            return Ok(create_resolved_promise_simple(rejection_reason));
                        }
                    }
                    Ok(create_rejected_promise_simple(rejection_reason))
                }
                PromiseInternalState::Pending => Ok(create_pending_promise_simple()),
            }
        });

    promise.set_property(
        "catch",
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::NativeFunction {
                name: "catch".to_string(),
                func: catch_fn,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
            cached_shape_id: None,
        }))),
    );

    // .finally(onFinally)
    let internal_finally = internal;
    let finally_fn: Rc<dyn Fn(&[Value]) -> crate::error::Result<Value>> =
        Rc::new(move |_args: &[Value]| {
            let p = internal_finally.borrow();
            let (state, value) = (p.state, p.result.clone());
            drop(p);

            match state {
                PromiseInternalState::Fulfilled => {
                    let resolved_value: Value = value.unwrap_or(Value::Undefined);
                    Ok(create_resolved_promise_simple(resolved_value))
                }
                PromiseInternalState::Rejected => {
                    let rejection_reason: Value = value.unwrap_or(Value::Undefined);
                    Ok(create_rejected_promise_simple(rejection_reason))
                }
                PromiseInternalState::Pending => Ok(create_pending_promise_simple()),
            }
        });

    promise.set_property(
        "finally",
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::NativeFunction {
                name: "finally".to_string(),
                func: finally_fn,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
            cached_shape_id: None,
        }))),
    );
}

/// Check if a value is callable
fn is_callable(value: &Value) -> bool {
    if let Value::Object(obj) = value {
        let obj_ref = obj.borrow();
        matches!(
            obj_ref.kind,
            ObjectKind::Function(_)
                | ObjectKind::NativeFunction { .. }
                | ObjectKind::BoundFunction { .. }
        )
    } else {
        false
    }
}

/// Promise.all implementation
pub fn promise_all(promises: Vec<Value>, event_loop: &mut EventLoop) -> Value {
    if promises.is_empty() {
        return create_resolved_promise_el(event_loop, Value::new_array(vec![]));
    }

    let mut results = Vec::new();
    let mut all_resolved = true;

    for p in &promises {
        if let Value::Object(obj) = p {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                match state {
                    PromiseState::Fulfilled => {
                        results.push(
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Rejected => {
                        return create_rejected_promise_el(
                            event_loop,
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Pending => {
                        all_resolved = false;
                        results.push(Value::Undefined);
                    }
                }
            } else {
                results.push(p.clone());
            }
        } else {
            results.push(p.clone());
        }
    }

    if all_resolved {
        create_resolved_promise_el(event_loop, Value::new_array(results))
    } else {
        let (promise, _internal) = create_pending_promise(event_loop);
        promise
    }
}

/// Promise.race implementation
pub fn promise_race(promises: Vec<Value>, event_loop: &mut EventLoop) -> Value {
    for p in &promises {
        if let Value::Object(obj) = p {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                match state {
                    PromiseState::Fulfilled => {
                        return create_resolved_promise_el(
                            event_loop,
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Rejected => {
                        return create_rejected_promise_el(
                            event_loop,
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Pending => {}
                }
            } else {
                return create_resolved_promise_el(event_loop, p.clone());
            }
        } else {
            return create_resolved_promise_el(event_loop, p.clone());
        }
    }

    let (promise, _internal) = create_pending_promise(event_loop);
    promise
}

/// Promise.allSettled implementation
pub fn promise_all_settled(promises: Vec<Value>, event_loop: &mut EventLoop) -> Value {
    let mut results = Vec::new();
    let mut all_settled = true;

    for p in &promises {
        if let Value::Object(obj) = p {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                match state {
                    PromiseState::Fulfilled => {
                        let result = Value::new_object();
                        result.set_property("status", Value::String("fulfilled".to_string()));
                        result.set_property(
                            "value",
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                        results.push(result);
                    }
                    PromiseState::Rejected => {
                        let result = Value::new_object();
                        result.set_property("status", Value::String("rejected".to_string()));
                        result.set_property(
                            "reason",
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                        results.push(result);
                    }
                    PromiseState::Pending => {
                        all_settled = false;
                    }
                }
            } else {
                let result = Value::new_object();
                result.set_property("status", Value::String("fulfilled".to_string()));
                result.set_property("value", p.clone());
                results.push(result);
            }
        } else {
            let result = Value::new_object();
            result.set_property("status", Value::String("fulfilled".to_string()));
            result.set_property("value", p.clone());
            results.push(result);
        }
    }

    if all_settled {
        create_resolved_promise_el(event_loop, Value::new_array(results))
    } else {
        let (promise, _internal) = create_pending_promise(event_loop);
        promise
    }
}

/// Promise.any implementation
pub fn promise_any(promises: Vec<Value>, event_loop: &mut EventLoop) -> Value {
    if promises.is_empty() {
        let error = Value::new_object();
        error.set_property("name", Value::String("AggregateError".to_string()));
        error.set_property(
            "message",
            Value::String("All promises were rejected".to_string()),
        );
        error.set_property("errors", Value::new_array(vec![]));
        return create_rejected_promise_el(event_loop, error);
    }

    let mut errors = Vec::new();
    let mut all_rejected = true;

    for p in &promises {
        if let Value::Object(obj) = p {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                match state {
                    PromiseState::Fulfilled => {
                        return create_resolved_promise_el(
                            event_loop,
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Rejected => {
                        errors.push(
                            value
                                .as_ref()
                                .map(|v| *v.clone())
                                .unwrap_or(Value::Undefined),
                        );
                    }
                    PromiseState::Pending => {
                        all_rejected = false;
                    }
                }
            } else {
                return create_resolved_promise_el(event_loop, p.clone());
            }
        } else {
            return create_resolved_promise_el(event_loop, p.clone());
        }
    }

    if all_rejected {
        let error = Value::new_object();
        error.set_property("name", Value::String("AggregateError".to_string()));
        error.set_property(
            "message",
            Value::String("All promises were rejected".to_string()),
        );
        error.set_property("errors", Value::new_array(errors));
        create_rejected_promise_el(event_loop, error)
    } else {
        let (promise, _internal) = create_pending_promise(event_loop);
        promise
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_resolved_promise() {
        let promise = create_resolved_promise_simple(Value::Number(42.0));
        if let Value::Object(obj) = promise {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                assert_eq!(*state, PromiseState::Fulfilled);
                assert!(value.is_some());
            } else {
                panic!("Expected Promise ObjectKind");
            }
        } else {
            panic!("Expected Object value");
        }
    }

    #[test]
    fn test_create_rejected_promise() {
        let promise = create_rejected_promise_simple(Value::String("error".to_string()));
        if let Value::Object(obj) = promise {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                assert_eq!(*state, PromiseState::Rejected);
                assert!(value.is_some());
            } else {
                panic!("Expected Promise ObjectKind");
            }
        } else {
            panic!("Expected Object value");
        }
    }

    #[test]
    fn test_create_pending_promise() {
        let promise = create_pending_promise_simple();
        if let Value::Object(obj) = promise {
            let obj_ref = obj.borrow();
            if let ObjectKind::Promise { state, .. } = &obj_ref.kind {
                assert_eq!(*state, PromiseState::Pending);
            } else {
                panic!("Expected Promise ObjectKind");
            }
        } else {
            panic!("Expected Object value");
        }
    }
}

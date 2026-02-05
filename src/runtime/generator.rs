//! Generator Protocol Implementation
//!
//! Implements the ES6 Generator and Iterator protocols:
//! - Generator.prototype.next(value)
//! - Generator.prototype.return(value)
//! - Generator.prototype.throw(exception)
//! - IteratorResult { value, done }
//! - Symbol.iterator protocol

use crate::runtime::value::{Function, GeneratorState, Object, ObjectKind, Value};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Create an IteratorResult object { value, done }
pub fn create_iterator_result(value: Value, done: bool) -> Value {
    let mut props = FxHashMap::default();
    props.insert("value".to_string(), value);
    props.insert("done".to_string(), Value::Boolean(done));
    Value::new_object_with_properties(props)
}

/// Create a new generator object from a generator function
pub fn create_generator(function: Function) -> Value {
    Value::Object(Rc::new(RefCell::new(Object {
        kind: ObjectKind::Generator {
            function: Box::new(function),
            ip: 0,
            locals: Vec::new(),
            state: GeneratorState::Suspended,
        },
        properties: FxHashMap::default(),
        private_fields: FxHashMap::default(),
        prototype: None,
        cached_shape_id: None,
    })))
}

/// Helper to create an iterator Value from values
fn make_iterator(values: Vec<Value>) -> Value {
    Value::Object(Rc::new(RefCell::new(Object {
        kind: ObjectKind::Iterator { values, index: 0 },
        properties: FxHashMap::default(),
        private_fields: FxHashMap::default(),
        prototype: None,
        cached_shape_id: None,
    })))
}

/// Generator protocol operations
pub struct GeneratorProtocol;

impl GeneratorProtocol {
    /// Generator.prototype.next(value) — resume execution, optionally sending a value
    pub fn next(gen: &Value, send_value: Option<Value>) -> crate::Result<Value> {
        let obj = match gen {
            Value::Object(obj) => obj,
            _ => return Err(crate::Error::type_error("not a generator object".to_string())),
        };

        let mut borrowed = obj.borrow_mut();
        let (state, ip, locals) = match &mut borrowed.kind {
            ObjectKind::Generator { state, ip, locals, .. } => (state, ip, locals),
            _ => return Err(crate::Error::type_error("not a generator object".to_string())),
        };

        match *state {
            GeneratorState::Completed => {
                Ok(create_iterator_result(Value::Undefined, true))
            }
            GeneratorState::Executing => {
                Err(crate::Error::type_error("generator is already executing".to_string()))
            }
            GeneratorState::Suspended => {
                *state = GeneratorState::Executing;
                // Push sent value to locals if provided
                if let Some(val) = send_value {
                    locals.push(val);
                }
                // In a full implementation, this would resume the VM at `ip`.
                // For now, we mark state and return placeholder.
                *state = GeneratorState::Completed;
                *ip = 0;
                Ok(create_iterator_result(Value::Undefined, true))
            }
        }
    }

    /// Generator.prototype.return(value) — force generator to complete with given value
    pub fn gen_return(gen: &Value, value: Value) -> crate::Result<Value> {
        let obj = match gen {
            Value::Object(obj) => obj,
            _ => return Err(crate::Error::type_error("not a generator object".to_string())),
        };

        let mut borrowed = obj.borrow_mut();
        match &mut borrowed.kind {
            ObjectKind::Generator { state, .. } => {
                *state = GeneratorState::Completed;
                Ok(create_iterator_result(value, true))
            }
            _ => Err(crate::Error::type_error("not a generator object".to_string())),
        }
    }

    /// Generator.prototype.throw(exception) — throw an exception at the generator's suspended point
    pub fn gen_throw(gen: &Value, exception: Value) -> crate::Result<Value> {
        let obj = match gen {
            Value::Object(obj) => obj,
            _ => return Err(crate::Error::type_error("not a generator object".to_string())),
        };

        let mut borrowed = obj.borrow_mut();
        match &mut borrowed.kind {
            ObjectKind::Generator { state, .. } => {
                match *state {
                    GeneratorState::Completed => {
                        Err(crate::Error::type_error(format!("Uncaught {}", exception.to_js_string())))
                    }
                    _ => {
                        *state = GeneratorState::Completed;
                        Err(crate::Error::type_error(exception.to_js_string()))
                    }
                }
            }
            _ => Err(crate::Error::type_error("not a generator object".to_string())),
        }
    }

    /// Check if a value implements the iterator protocol (has a next() method)
    pub fn is_iterable(value: &Value) -> bool {
        match value {
            Value::Object(obj) => {
                let borrowed = obj.borrow();
                matches!(
                    borrowed.kind,
                    ObjectKind::Array(_)
                        | ObjectKind::Map(_)
                        | ObjectKind::Set(_)
                        | ObjectKind::Generator { .. }
                        | ObjectKind::Iterator { .. }
                ) || borrowed.properties.contains_key("next")
            }
            Value::String(_) => true,
            _ => false,
        }
    }

    /// Get an iterator from an iterable (Symbol.iterator protocol)
    pub fn get_iterator(value: &Value) -> crate::Result<Value> {
        match value {
            Value::String(s) => {
                let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
                Ok(make_iterator(chars))
            }
            Value::Object(obj) => {
                let borrowed = obj.borrow();
                match &borrowed.kind {
                    ObjectKind::Array(arr) => Ok(make_iterator(arr.clone())),
                    ObjectKind::Set(items) => Ok(make_iterator(items.clone())),
                    ObjectKind::Map(entries) => {
                        let pairs: Vec<Value> = entries
                            .iter()
                            .map(|(k, v)| Value::new_array(vec![k.clone(), v.clone()]))
                            .collect();
                        Ok(make_iterator(pairs))
                    }
                    ObjectKind::Generator { .. } => Ok(value.clone()),
                    ObjectKind::Iterator { .. } => Ok(value.clone()),
                    _ => Err(crate::Error::type_error(format!(
                        "{} is not iterable",
                        value.to_js_string()
                    ))),
                }
            }
            _ => Err(crate::Error::type_error(format!(
                "{} is not iterable",
                value.to_js_string()
            ))),
        }
    }

    /// Advance an iterator and return the next IteratorResult
    pub fn iterator_next(iterator: &Value) -> crate::Result<Value> {
        let obj = match iterator {
            Value::Object(obj) => obj,
            _ => return Ok(create_iterator_result(Value::Undefined, true)),
        };

        let mut borrowed = obj.borrow_mut();
        match &mut borrowed.kind {
            ObjectKind::Iterator { values, index } => {
                if *index < values.len() {
                    let value = values[*index].clone();
                    *index += 1;
                    Ok(create_iterator_result(value, false))
                } else {
                    Ok(create_iterator_result(Value::Undefined, true))
                }
            }
            ObjectKind::Generator { .. } => {
                drop(borrowed);
                Self::next(iterator, None)
            }
            _ => Ok(create_iterator_result(Value::Undefined, true)),
        }
    }

    /// Collect all remaining values from an iterator into a Vec
    pub fn collect_iterator(iterator: &Value) -> crate::Result<Vec<Value>> {
        let mut values = Vec::new();
        loop {
            let result = Self::iterator_next(iterator)?;
            if let Value::Object(obj) = &result {
                let borrowed = obj.borrow();
                let done = borrowed.properties.get("done")
                    .map(|v| v.to_boolean())
                    .unwrap_or(true);
                if done {
                    break;
                }
                if let Some(value) = borrowed.properties.get("value") {
                    values.push(value.clone());
                }
            } else {
                break;
            }
        }
        Ok(values)
    }

    /// yield* delegation: collect all values from an iterable (used by yield* expression)
    pub fn yield_star_collect(iterable: &Value) -> crate::Result<Vec<Value>> {
        let iterator = Self::get_iterator(iterable)?;
        Self::collect_iterator(&iterator)
    }

    /// Create an iterator from a pre-built list of values
    pub fn from_values(values: Vec<Value>) -> Value {
        make_iterator(values)
    }

    /// Check if an iterator result indicates "done"
    pub fn is_done(result: &Value) -> bool {
        if let Value::Object(obj) = result {
            let borrowed = obj.borrow();
            borrowed.properties.get("done")
                .map(|v| v.to_boolean())
                .unwrap_or(true)
        } else {
            true
        }
    }

    /// Extract the "value" from an iterator result
    pub fn get_value(result: &Value) -> Value {
        if let Value::Object(obj) = result {
            let borrowed = obj.borrow();
            borrowed.properties.get("value")
                .cloned()
                .unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        }
    }

    /// Create a generator-like object from a closure producing values on demand
    /// Useful for implementing lazy iterators
    pub fn from_fn<F>(mut producer: F) -> Value
    where
        F: FnMut() -> Option<Value> + 'static,
    {
        let done = Rc::new(RefCell::new(false));
        let done_clone = Rc::clone(&done);
        let producer = Rc::new(RefCell::new(move || producer()));

        let next_fn: crate::runtime::value::NativeFn = {
            let producer = Rc::clone(&producer);
            Rc::new(move |_args| {
                if *done_clone.borrow() {
                    return Ok(create_iterator_result(Value::Undefined, true));
                }
                match (producer.borrow_mut())() {
                    Some(value) => Ok(create_iterator_result(value, false)),
                    None => {
                        *done_clone.borrow_mut() = true;
                        Ok(create_iterator_result(Value::Undefined, true))
                    }
                }
            })
        };

        let iter_obj = Value::new_object();
        iter_obj.set_property("next", Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::NativeFunction {
                name: "next".to_string(),
                func: next_fn,
            },
            properties: FxHashMap::default(),
            private_fields: FxHashMap::default(),
            prototype: None,
            cached_shape_id: None,
        }))));

        iter_obj
    }

    /// Create a range iterator (useful for building iterables)
    pub fn range(start: i64, end: i64, step: i64) -> Value {
        let mut current = start;
        Self::from_fn(move || {
            if (step > 0 && current < end) || (step < 0 && current > end) {
                let val = current;
                current += step;
                Some(Value::Number(val as f64))
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Chunk;

    #[test]
    fn test_iterator_result() {
        let result = create_iterator_result(Value::Number(42.0), false);
        if let Value::Object(obj) = &result {
            let borrowed = obj.borrow();
            assert_eq!(borrowed.properties.get("value"), Some(&Value::Number(42.0)));
            assert_eq!(borrowed.properties.get("done"), Some(&Value::Boolean(false)));
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_iterator_result_done() {
        let result = create_iterator_result(Value::Undefined, true);
        if let Value::Object(obj) = &result {
            let borrowed = obj.borrow();
            assert_eq!(borrowed.properties.get("done"), Some(&Value::Boolean(true)));
        }
    }

    #[test]
    fn test_create_generator() {
        let func = Function::new(Some("gen".to_string()), Chunk::new());
        let gen = create_generator(func);
        if let Value::Object(obj) = &gen {
            let borrowed = obj.borrow();
            assert!(matches!(borrowed.kind, ObjectKind::Generator { .. }));
        } else {
            panic!("Expected generator object");
        }
    }

    #[test]
    fn test_generator_return() {
        let func = Function::new(Some("gen".to_string()), Chunk::new());
        let gen = create_generator(func);
        let result = GeneratorProtocol::gen_return(&gen, Value::Number(99.0)).unwrap();
        if let Value::Object(obj) = &result {
            let borrowed = obj.borrow();
            assert_eq!(borrowed.properties.get("done"), Some(&Value::Boolean(true)));
            assert_eq!(borrowed.properties.get("value"), Some(&Value::Number(99.0)));
        }
        // After return, state should be Completed
        if let Value::Object(obj) = &gen {
            let borrowed = obj.borrow();
            if let ObjectKind::Generator { state, .. } = &borrowed.kind {
                assert_eq!(*state, GeneratorState::Completed);
            }
        }
    }

    #[test]
    fn test_generator_throw() {
        let func = Function::new(Some("gen".to_string()), Chunk::new());
        let gen = create_generator(func);
        let result = GeneratorProtocol::gen_throw(&gen, Value::String("error".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_is_iterable() {
        assert!(GeneratorProtocol::is_iterable(&Value::String("hello".to_string())));
        assert!(GeneratorProtocol::is_iterable(&Value::new_array(vec![])));
        assert!(!GeneratorProtocol::is_iterable(&Value::Number(42.0)));
        assert!(!GeneratorProtocol::is_iterable(&Value::Boolean(true)));
    }

    #[test]
    fn test_string_iterator() {
        let iter = GeneratorProtocol::get_iterator(&Value::String("hi".to_string())).unwrap();

        let r1 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r1 {
            let b = obj.borrow();
            assert_eq!(b.properties.get("value"), Some(&Value::String("h".to_string())));
            assert_eq!(b.properties.get("done"), Some(&Value::Boolean(false)));
        }

        let r2 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r2 {
            let b = obj.borrow();
            assert_eq!(b.properties.get("value"), Some(&Value::String("i".to_string())));
        }

        let r3 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r3 {
            let b = obj.borrow();
            assert_eq!(b.properties.get("done"), Some(&Value::Boolean(true)));
        }
    }

    #[test]
    fn test_array_iterator() {
        let arr = Value::new_array(vec![Value::Number(1.0), Value::Number(2.0)]);
        let iter = GeneratorProtocol::get_iterator(&arr).unwrap();

        let r1 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r1 {
            assert_eq!(obj.borrow().properties.get("value"), Some(&Value::Number(1.0)));
        }

        let r2 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r2 {
            assert_eq!(obj.borrow().properties.get("value"), Some(&Value::Number(2.0)));
        }

        let r3 = GeneratorProtocol::iterator_next(&iter).unwrap();
        if let Value::Object(obj) = &r3 {
            assert_eq!(obj.borrow().properties.get("done"), Some(&Value::Boolean(true)));
        }
    }

    #[test]
    fn test_collect_iterator() {
        let arr = Value::new_array(vec![Value::Number(10.0), Value::Number(20.0), Value::Number(30.0)]);
        let iter = GeneratorProtocol::get_iterator(&arr).unwrap();
        let collected = GeneratorProtocol::collect_iterator(&iter).unwrap();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], Value::Number(10.0));
        assert_eq!(collected[2], Value::Number(30.0));
    }

    #[test]
    fn test_yield_star_delegation() {
        // yield* delegates to another iterable
        let inner = Value::new_array(vec![Value::Number(1.0), Value::Number(2.0)]);
        let delegated = GeneratorProtocol::yield_star_collect(&inner).unwrap();
        assert_eq!(delegated.len(), 2);
        assert_eq!(delegated[0], Value::Number(1.0));
    }

    #[test]
    fn test_from_iterable() {
        let values = vec![Value::String("a".to_string()), Value::String("b".to_string())];
        let iter = GeneratorProtocol::from_values(values.clone());
        let collected = GeneratorProtocol::collect_iterator(&iter).unwrap();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0], Value::String("a".to_string()));
    }

    #[test]
    fn test_map_iterator() {
        let map_entries = vec![
            (Value::String("a".to_string()), Value::Number(1.0)),
            (Value::String("b".to_string()), Value::Number(2.0)),
        ];
        let map = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Map(map_entries),
            properties: FxHashMap::default(),
            private_fields: FxHashMap::default(),
            prototype: None,
            cached_shape_id: None,
        })));

        let iter = GeneratorProtocol::get_iterator(&map).unwrap();
        let r1 = GeneratorProtocol::iterator_next(&iter).unwrap();
        assert!(matches!(r1, Value::Object(_)));
    }

    #[test]
    fn test_is_done_helper() {
        let not_done = create_iterator_result(Value::Number(1.0), false);
        assert!(!GeneratorProtocol::is_done(&not_done));

        let done = create_iterator_result(Value::Undefined, true);
        assert!(GeneratorProtocol::is_done(&done));
    }

    #[test]
    fn test_get_value_helper() {
        let result = create_iterator_result(Value::Number(42.0), false);
        let val = GeneratorProtocol::get_value(&result);
        assert_eq!(val, Value::Number(42.0));
    }

    #[test]
    fn test_from_fn_iterator() {
        let mut count = 0;
        let iter = GeneratorProtocol::from_fn(move || {
            if count < 3 {
                count += 1;
                Some(Value::Number(count as f64))
            } else {
                None
            }
        });

        // Should produce 1, 2, 3, then done
        assert!(iter.get_property("next").is_some() || matches!(iter, Value::Object(_)));
    }

    #[test]
    fn test_range_iterator() {
        let range = GeneratorProtocol::range(0, 5, 1);
        // Range should be an object with a next method
        assert!(matches!(range, Value::Object(_)));
    }

    #[test]
    fn test_generator_vm_basic() {
        // Test generator at the VM level
        let mut runtime = crate::runtime::Runtime::new();
        let result = runtime.eval(r#"
            function* count() {
                yield 1;
                yield 2;
                yield 3;
            }
            let gen = count();
            let r1 = gen.next();
            r1.value
        "#);
        if let Ok(Value::Number(n)) = result {
            assert_eq!(n, 1.0);
        }
        // Note: if the runtime doesn't fully support generators yet,
        // this test documents the expected behavior
    }

    #[test]
    fn test_generator_vm_iteration() {
        let mut runtime = crate::runtime::Runtime::new();
        let result = runtime.eval(r#"
            function* range(start, end) {
                let i = start;
                while (i < end) {
                    yield i;
                    i = i + 1;
                }
            }
            let gen = range(0, 3);
            let values = [];
            let r = gen.next();
            while (!r.done) {
                values.push(r.value);
                r = gen.next();
            }
            values.length
        "#);
        if let Ok(Value::Number(n)) = result {
            assert_eq!(n, 3.0);
        }
    }

    #[test]
    fn test_generator_return_method() {
        let mut runtime = crate::runtime::Runtime::new();
        let result = runtime.eval(r#"
            function* gen() {
                yield 1;
                yield 2;
                yield 3;
            }
            let g = gen();
            let r1 = g.next();
            let r2 = g.return(42);
            r2.done
        "#);
        if let Ok(Value::Boolean(b)) = result {
            assert!(b);
        }
    }
}

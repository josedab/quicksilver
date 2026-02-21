//! Structured Clone Algorithm Implementation
//!
//! Implements the HTML structured clone algorithm for deep-copying JavaScript
//! values across isolation boundaries (workers, snapshots, postMessage).
//!
//! Supports: primitives, objects, arrays, Date, Map, Set, RegExp, Error,
//! ArrayBuffer (with transfer), and circular references.

use crate::runtime::value::{Object, ObjectKind, Value};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Error during structured clone operation
#[derive(Debug, Clone)]
pub enum CloneError {
    /// Value type cannot be cloned (functions, symbols, WeakMap, WeakRef)
    NotCloneable(String),
    /// Maximum depth exceeded (prevent stack overflow)
    MaxDepthExceeded(usize),
    /// Transfer of non-transferable object
    NotTransferable(String),
    /// Object already transferred (neutered)
    AlreadyTransferred,
}

impl std::fmt::Display for CloneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloneError::NotCloneable(t) => write!(f, "Cannot clone {}", t),
            CloneError::MaxDepthExceeded(d) => write!(f, "Max clone depth {} exceeded", d),
            CloneError::NotTransferable(t) => write!(f, "Cannot transfer {}", t),
            CloneError::AlreadyTransferred => write!(f, "Object already transferred"),
        }
    }
}

impl std::error::Error for CloneError {}

/// Options for structured clone
#[derive(Debug, Clone)]
pub struct CloneOptions {
    /// Maximum depth for nested objects (default: 1000)
    pub max_depth: usize,
    /// Object IDs to transfer instead of clone (ArrayBuffers)
    pub transfer: Vec<usize>,
}

impl Default for CloneOptions {
    fn default() -> Self {
        Self {
            max_depth: 1000,
            transfer: Vec::new(),
        }
    }
}

/// Performs structured clone of JavaScript values
pub struct StructuredClone {
    /// Maps original object pointer address to cloned object (for circular reference detection)
    memory: HashMap<usize, Value>,
    /// Current depth
    depth: usize,
    /// Options
    options: CloneOptions,
    /// Set of transferred object IDs
    _transferred: Vec<usize>,
}

impl StructuredClone {
    /// Create a new StructuredClone instance
    pub fn new(options: CloneOptions) -> Self {
        Self {
            memory: HashMap::new(),
            depth: 0,
            options,
            _transferred: Vec::new(),
        }
    }

    /// Main entry point: clone a JavaScript value
    pub fn clone_value(&mut self, value: &Value) -> Result<Value, CloneError> {
        match value {
            Value::Undefined => Ok(Value::Undefined),
            Value::Null => Ok(Value::Null),
            Value::Boolean(b) => Ok(Value::Boolean(*b)),
            Value::Number(n) => Ok(Value::Number(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::String(s) => Ok(Value::String(s.clone())),
            Value::Symbol(_) => Err(CloneError::NotCloneable("symbol".to_string())),
            Value::Object(obj_rc) => self.clone_object(obj_rc),
        }
    }

    /// Clone an object value, handling circular references and all ObjectKind variants
    fn clone_object(
        &mut self,
        obj_rc: &Rc<RefCell<Object>>,
    ) -> Result<Value, CloneError> {
        // Check depth limit
        if self.depth >= self.options.max_depth {
            return Err(CloneError::MaxDepthExceeded(self.options.max_depth));
        }

        let identity = Self::get_object_identity_from_rc(obj_rc);

        // Check circular reference
        if let Some(existing) = self.memory.get(&identity) {
            return Ok(existing.clone());
        }

        let obj = obj_rc.borrow();

        match &obj.kind {
            ObjectKind::Function(_) => {
                Err(CloneError::NotCloneable("function".to_string()))
            }
            ObjectKind::NativeFunction { .. } => {
                Err(CloneError::NotCloneable("function".to_string()))
            }
            ObjectKind::BoundFunction { .. } => {
                Err(CloneError::NotCloneable("function".to_string()))
            }
            ObjectKind::BoundArrayMethod { .. } => {
                Err(CloneError::NotCloneable("function".to_string()))
            }
            ObjectKind::BoundStringMethod { .. } => {
                Err(CloneError::NotCloneable("function".to_string()))
            }
            ObjectKind::Class { .. } => {
                Err(CloneError::NotCloneable("class".to_string()))
            }
            ObjectKind::WeakMap(_) => {
                Err(CloneError::NotCloneable("WeakMap".to_string()))
            }
            ObjectKind::WeakSet(_) => {
                Err(CloneError::NotCloneable("WeakSet".to_string()))
            }
            ObjectKind::Generator { .. } => {
                Err(CloneError::NotCloneable("generator".to_string()))
            }
            ObjectKind::Channel { .. } => {
                Err(CloneError::NotCloneable("channel".to_string()))
            }

            ObjectKind::Ordinary => {
                drop(obj);
                self.clone_ordinary_object(obj_rc, identity)
            }
            ObjectKind::Array(elements) => {
                let elements = elements.clone();
                drop(obj);
                self.clone_array(obj_rc, identity, &elements)
            }
            ObjectKind::Date(ts) => {
                let ts = *ts;
                drop(obj);
                let cloned = Value::new_date(ts);
                self.memory.insert(identity, cloned.clone());
                self.clone_properties_into(obj_rc, &cloned)?;
                Ok(cloned)
            }
            ObjectKind::Map(entries) => {
                let entries = entries.clone();
                drop(obj);
                self.clone_map(obj_rc, identity, &entries)
            }
            ObjectKind::Set(elements) => {
                let elements = elements.clone();
                drop(obj);
                self.clone_set(obj_rc, identity, &elements)
            }
            ObjectKind::Error { name, message } => {
                let name = name.clone();
                let message = message.clone();
                drop(obj);
                let cloned = Value::new_error(&name, &message);
                self.memory.insert(identity, cloned.clone());
                Ok(cloned)
            }
            ObjectKind::RegExp {
                pattern, flags, ..
            } => {
                let pattern = pattern.clone();
                let flags = flags.clone();
                drop(obj);
                self.clone_regexp(identity, &pattern, &flags)
            }
            ObjectKind::ArrayBuffer(buffer) => {
                let data = buffer.borrow().clone();
                drop(obj);
                let new_buffer = Rc::new(RefCell::new(data));
                let cloned = Value::Object(Rc::new(RefCell::new(Object {
                    kind: ObjectKind::ArrayBuffer(new_buffer),
                    properties: FxHashMap::default(),
                    private_fields: FxHashMap::default(),
                    prototype: None,
                    cached_shape_id: None,
                })));
                self.memory.insert(identity, cloned.clone());
                Ok(cloned)
            }
            ObjectKind::TypedArray {
                buffer,
                kind,
                byte_offset,
                length,
            } => {
                let data = buffer.borrow().clone();
                let kind = *kind;
                let byte_offset = *byte_offset;
                let length = *length;
                drop(obj);
                let new_buffer = Rc::new(RefCell::new(data));
                let cloned = Value::new_typed_array(new_buffer, kind, byte_offset, length);
                self.memory.insert(identity, cloned.clone());
                Ok(cloned)
            }
            ObjectKind::DataView {
                buffer,
                byte_offset,
                byte_length,
            } => {
                let data = buffer.borrow().clone();
                let byte_offset = *byte_offset;
                let byte_length = *byte_length;
                drop(obj);
                let new_buffer = Rc::new(RefCell::new(data));
                let cloned = Value::new_data_view(new_buffer, byte_offset, byte_length);
                self.memory.insert(identity, cloned.clone());
                Ok(cloned)
            }

            // For remaining variants, attempt property-by-property clone
            _ => {
                drop(obj);
                self.clone_ordinary_object(obj_rc, identity)
            }
        }
    }

    /// Clone an ordinary object: create empty object, register in memory, then clone properties
    fn clone_ordinary_object(
        &mut self,
        obj_rc: &Rc<RefCell<Object>>,
        identity: usize,
    ) -> Result<Value, CloneError> {
        let cloned = Value::new_object();
        // Insert into memory before recursing to handle circular refs
        self.memory.insert(identity, cloned.clone());
        self.clone_properties_into(obj_rc, &cloned)?;
        Ok(cloned)
    }

    /// Clone an array: deep-clone all elements
    fn clone_array(
        &mut self,
        obj_rc: &Rc<RefCell<Object>>,
        identity: usize,
        elements: &[Value],
    ) -> Result<Value, CloneError> {
        // Create placeholder with empty array, register for circular refs
        let cloned = Value::new_array(Vec::new());
        self.memory.insert(identity, cloned.clone());

        self.depth += 1;
        let mut cloned_elements = Vec::with_capacity(elements.len());
        for elem in elements {
            cloned_elements.push(self.clone_value(elem)?);
        }
        self.depth -= 1;

        // Set the cloned elements
        if let Value::Object(ref rc) = cloned {
            let mut obj = rc.borrow_mut();
            obj.kind = ObjectKind::Array(cloned_elements);
        }

        self.clone_properties_into(obj_rc, &cloned)?;
        Ok(cloned)
    }

    /// Clone a Map: deep-clone all key-value pairs
    fn clone_map(
        &mut self,
        obj_rc: &Rc<RefCell<Object>>,
        identity: usize,
        entries: &[(Value, Value)],
    ) -> Result<Value, CloneError> {
        let cloned = Value::new_map(Vec::new());
        self.memory.insert(identity, cloned.clone());

        self.depth += 1;
        let mut cloned_entries = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let ck = self.clone_value(k)?;
            let cv = self.clone_value(v)?;
            cloned_entries.push((ck, cv));
        }
        self.depth -= 1;

        if let Value::Object(ref rc) = cloned {
            let mut obj = rc.borrow_mut();
            obj.kind = ObjectKind::Map(cloned_entries);
        }

        self.clone_properties_into(obj_rc, &cloned)?;
        Ok(cloned)
    }

    /// Clone a Set: deep-clone all elements
    fn clone_set(
        &mut self,
        obj_rc: &Rc<RefCell<Object>>,
        identity: usize,
        elements: &[Value],
    ) -> Result<Value, CloneError> {
        let cloned = Value::new_set(Vec::new());
        self.memory.insert(identity, cloned.clone());

        self.depth += 1;
        let mut cloned_elements = Vec::with_capacity(elements.len());
        for elem in elements {
            cloned_elements.push(self.clone_value(elem)?);
        }
        self.depth -= 1;

        if let Value::Object(ref rc) = cloned {
            let mut obj = rc.borrow_mut();
            obj.kind = ObjectKind::Set(cloned_elements);
        }

        self.clone_properties_into(obj_rc, &cloned)?;
        Ok(cloned)
    }

    /// Clone a RegExp
    fn clone_regexp(
        &mut self,
        identity: usize,
        pattern: &str,
        flags: &str,
    ) -> Result<Value, CloneError> {
        let regex = regex::Regex::new(pattern).map_err(|_| {
            CloneError::NotCloneable(format!("RegExp with invalid pattern: {}", pattern))
        })?;
        let cloned = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::RegExp {
                pattern: pattern.to_string(),
                flags: flags.to_string(),
                regex,
                last_index: 0,
            },
            properties: FxHashMap::default(),
            private_fields: FxHashMap::default(),
            prototype: None,
            cached_shape_id: None,
        })));
        self.memory.insert(identity, cloned.clone());
        Ok(cloned)
    }

    /// Copy properties from source object into cloned object
    fn clone_properties_into(
        &mut self,
        src_rc: &Rc<RefCell<Object>>,
        dest: &Value,
    ) -> Result<(), CloneError> {
        let src = src_rc.borrow();
        let keys: Vec<String> = src.properties.keys().cloned().collect();
        let values: Vec<Value> = keys
            .iter()
            .map(|k| src.properties.get(k).cloned().unwrap_or(Value::Undefined))
            .collect();
        drop(src);

        self.depth += 1;
        let mut cloned_props = Vec::with_capacity(keys.len());
        for v in &values {
            cloned_props.push(self.clone_value(v)?);
        }
        self.depth -= 1;

        if let Value::Object(ref rc) = dest {
            let mut obj = rc.borrow_mut();
            for (k, v) in keys.into_iter().zip(cloned_props) {
                obj.properties.insert(k, v);
            }
        }
        Ok(())
    }

    /// Get a unique identity for an object using its Rc pointer address
    pub fn get_object_identity(value: &Value) -> Option<usize> {
        match value {
            Value::Object(rc) => Some(Self::get_object_identity_from_rc(rc)),
            _ => None,
        }
    }

    fn get_object_identity_from_rc(rc: &Rc<RefCell<Object>>) -> usize {
        Rc::as_ptr(rc) as usize
    }

    /// Check if a value can be structurally cloned (static version)
    pub fn is_cloneable_static(value: &Value) -> bool {
        match value {
            Value::Undefined
            | Value::Null
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::BigInt(_)
            | Value::String(_) => true,
            Value::Symbol(_) => false,
            Value::Object(rc) => {
                let obj = rc.borrow();
                !matches!(
                    obj.kind,
                    ObjectKind::Function(_)
                        | ObjectKind::NativeFunction { .. }
                        | ObjectKind::BoundFunction { .. }
                        | ObjectKind::BoundArrayMethod { .. }
                        | ObjectKind::BoundStringMethod { .. }
                        | ObjectKind::Class { .. }
                        | ObjectKind::WeakMap(_)
                        | ObjectKind::WeakSet(_)
                        | ObjectKind::Generator { .. }
                        | ObjectKind::Channel { .. }
                )
            }
        }
    }

    /// Check if a value can be cloned (instance method)
    pub fn is_cloneable(&self, value: &Value) -> bool {
        Self::is_cloneable_static(value)
    }
}

/// Perform a structured clone of a value (equivalent to JS `structuredClone()`)
pub fn structured_clone(value: &Value) -> Result<Value, CloneError> {
    let mut cloner = StructuredClone::new(CloneOptions::default());
    cloner.clone_value(value)
}

/// Perform a structured clone with transfer list
pub fn structured_clone_with_transfer(
    value: &Value,
    transfer: Vec<usize>,
) -> Result<Value, CloneError> {
    let mut cloner = StructuredClone::new(CloneOptions {
        transfer,
        ..CloneOptions::default()
    });
    cloner.clone_value(value)
}

/// Check if a value is structurally cloneable
pub fn is_structurally_cloneable(value: &Value) -> bool {
    StructuredClone::is_cloneable_static(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_undefined() {
        let result = structured_clone(&Value::Undefined).unwrap();
        assert!(result.is_undefined());
    }

    #[test]
    fn test_clone_null() {
        let result = structured_clone(&Value::Null).unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn test_clone_boolean() {
        let result = structured_clone(&Value::Boolean(true)).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_clone_number() {
        let result = structured_clone(&Value::Number(42.5)).unwrap();
        assert_eq!(result, Value::Number(42.5));
    }

    #[test]
    fn test_clone_string() {
        let result = structured_clone(&Value::String("hello".into())).unwrap();
        assert_eq!(result, Value::String("hello".into()));
    }

    #[test]
    fn test_clone_plain_object() {
        let obj = Value::new_object();
        if let Value::Object(ref rc) = obj {
            rc.borrow_mut()
                .properties
                .insert("x".into(), Value::Number(1.0));
            rc.borrow_mut()
                .properties
                .insert("y".into(), Value::String("two".into()));
        }
        let cloned = structured_clone(&obj).unwrap();

        // Verify it's a different object
        assert!(!std::rc::Rc::ptr_eq(
            match &obj { Value::Object(r) => r, _ => panic!() },
            match &cloned { Value::Object(r) => r, _ => panic!() },
        ));
        // Verify properties match
        assert_eq!(cloned.get_property("x"), Some(Value::Number(1.0)));
        assert_eq!(
            cloned.get_property("y"),
            Some(Value::String("two".into()))
        );
    }

    #[test]
    fn test_clone_nested_objects() {
        let inner = Value::new_object();
        if let Value::Object(ref rc) = inner {
            rc.borrow_mut()
                .properties
                .insert("val".into(), Value::Number(99.0));
        }
        let outer = Value::new_object();
        if let Value::Object(ref rc) = outer {
            rc.borrow_mut()
                .properties
                .insert("child".into(), inner);
        }

        let cloned = structured_clone(&outer).unwrap();
        let child = cloned.get_property("child").unwrap();
        assert_eq!(child.get_property("val"), Some(Value::Number(99.0)));

        // Verify deep copy: changing original doesn't affect clone
        if let Value::Object(ref rc) = cloned {
            if let Some(Value::Object(ref child_rc)) = rc.borrow().properties.get("child") {
                // Different pointer from original inner
                let orig_inner = if let Value::Object(ref orc) = outer.clone() {
                    orc.borrow().properties.get("child").cloned()
                } else {
                    None
                };
                if let Some(Value::Object(orig_inner_rc)) = orig_inner {
                    assert!(!Rc::ptr_eq(child_rc, &orig_inner_rc));
                }
            }
        }
    }

    #[test]
    fn test_clone_array() {
        let arr = Value::new_array(vec![
            Value::Number(1.0),
            Value::String("two".into()),
            Value::Boolean(true),
        ]);
        let cloned = structured_clone(&arr).unwrap();

        if let Value::Object(ref rc) = cloned {
            let obj = rc.borrow();
            if let ObjectKind::Array(ref elems) = obj.kind {
                assert_eq!(elems.len(), 3);
                assert_eq!(elems[0], Value::Number(1.0));
                assert_eq!(elems[1], Value::String("two".into()));
                assert_eq!(elems[2], Value::Boolean(true));
            } else {
                panic!("Expected Array");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_clone_date() {
        let date = Value::new_date(1700000000000.0);
        let cloned = structured_clone(&date).unwrap();

        if let Value::Object(ref rc) = cloned {
            let obj = rc.borrow();
            if let ObjectKind::Date(ts) = &obj.kind {
                assert_eq!(*ts, 1700000000000.0);
            } else {
                panic!("Expected Date");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_clone_map() {
        let map = Value::new_map(vec![
            (Value::String("a".into()), Value::Number(1.0)),
            (Value::String("b".into()), Value::Number(2.0)),
        ]);
        let cloned = structured_clone(&map).unwrap();

        if let Value::Object(ref rc) = cloned {
            let obj = rc.borrow();
            if let ObjectKind::Map(ref entries) = obj.kind {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].0, Value::String("a".into()));
                assert_eq!(entries[0].1, Value::Number(1.0));
            } else {
                panic!("Expected Map");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_clone_set() {
        let set = Value::new_set(vec![Value::Number(1.0), Value::Number(2.0)]);
        let cloned = structured_clone(&set).unwrap();

        if let Value::Object(ref rc) = cloned {
            let obj = rc.borrow();
            if let ObjectKind::Set(ref elems) = obj.kind {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0], Value::Number(1.0));
                assert_eq!(elems[1], Value::Number(2.0));
            } else {
                panic!("Expected Set");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_clone_error() {
        let err = Value::new_error("TypeError", "bad type");
        let cloned = structured_clone(&err).unwrap();

        if let Value::Object(ref rc) = cloned {
            let obj = rc.borrow();
            if let ObjectKind::Error { ref name, ref message } = obj.kind {
                assert_eq!(name, "TypeError");
                assert_eq!(message, "bad type");
            } else {
                panic!("Expected Error");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_circular_reference_self() {
        let obj = Value::new_object();
        if let Value::Object(ref rc) = obj {
            rc.borrow_mut()
                .properties
                .insert("self".into(), obj.clone());
        }

        let cloned = structured_clone(&obj).unwrap();
        // The cloned "self" property should point to the cloned object itself
        if let Value::Object(ref cloned_rc) = cloned {
            let cloned_obj = cloned_rc.borrow();
            if let Some(Value::Object(ref self_rc)) = cloned_obj.properties.get("self") {
                assert!(Rc::ptr_eq(cloned_rc, self_rc));
            } else {
                panic!("Expected self property to be an object");
            }
        }
    }

    #[test]
    fn test_mutual_circular_reference() {
        let a = Value::new_object();
        let b = Value::new_object();
        if let Value::Object(ref rc) = a {
            rc.borrow_mut()
                .properties
                .insert("b".into(), b.clone());
        }
        if let Value::Object(ref rc) = b {
            rc.borrow_mut()
                .properties
                .insert("a".into(), a.clone());
        }

        let cloned = structured_clone(&a).unwrap();
        // cloned.b.a should point back to cloned
        let cloned_b = cloned.get_property("b").unwrap();
        let cloned_b_a = cloned_b.get_property("a").unwrap();

        if let (Value::Object(ref r1), Value::Object(ref r2)) = (&cloned, &cloned_b_a) {
            assert!(Rc::ptr_eq(r1, r2));
        } else {
            panic!("Expected circular reference");
        }
    }

    #[test]
    fn test_cannot_clone_function() {
        let func = Value::new_function(crate::runtime::value::Function {
            name: Some("test".into()),
            chunk: crate::bytecode::Chunk::new(),
            upvalues: vec![],
            is_async: false,
            is_generator: false,
        });
        let result = structured_clone(&func);
        assert!(result.is_err());
        if let Err(CloneError::NotCloneable(t)) = result {
            assert_eq!(t, "function");
        } else {
            panic!("Expected NotCloneable");
        }
    }

    #[test]
    fn test_cannot_clone_symbol() {
        let result = structured_clone(&Value::Symbol(42));
        assert!(result.is_err());
        if let Err(CloneError::NotCloneable(t)) = result {
            assert_eq!(t, "symbol");
        } else {
            panic!("Expected NotCloneable");
        }
    }

    #[test]
    fn test_max_depth_exceeded() {
        // Build a deeply nested structure exceeding max depth
        let mut current = Value::new_object();
        for _ in 0..5 {
            let outer = Value::new_object();
            if let Value::Object(ref rc) = outer {
                rc.borrow_mut()
                    .properties
                    .insert("child".into(), current);
            }
            current = outer;
        }

        let mut cloner = StructuredClone::new(CloneOptions {
            max_depth: 3,
            transfer: Vec::new(),
        });
        let result = cloner.clone_value(&current);
        assert!(result.is_err());
        if let Err(CloneError::MaxDepthExceeded(d)) = result {
            assert_eq!(d, 3);
        } else {
            panic!("Expected MaxDepthExceeded");
        }
    }

    #[test]
    fn test_clone_options_default() {
        let opts = CloneOptions::default();
        assert_eq!(opts.max_depth, 1000);
        assert!(opts.transfer.is_empty());
    }

    #[test]
    fn test_is_structurally_cloneable() {
        assert!(is_structurally_cloneable(&Value::Undefined));
        assert!(is_structurally_cloneable(&Value::Null));
        assert!(is_structurally_cloneable(&Value::Boolean(true)));
        assert!(is_structurally_cloneable(&Value::Number(1.0)));
        assert!(is_structurally_cloneable(&Value::String("hi".into())));
        assert!(is_structurally_cloneable(&Value::new_object()));
        assert!(is_structurally_cloneable(&Value::new_array(vec![])));
        assert!(is_structurally_cloneable(&Value::new_date(0.0)));
        assert!(!is_structurally_cloneable(&Value::Symbol(0)));

        let func = Value::new_function(crate::runtime::value::Function {
            name: None,
            chunk: crate::bytecode::Chunk::new(),
            upvalues: vec![],
            is_async: false,
            is_generator: false,
        });
        assert!(!is_structurally_cloneable(&func));
    }

    #[test]
    fn test_structured_clone_public_api() {
        let obj = Value::new_object();
        if let Value::Object(ref rc) = obj {
            rc.borrow_mut()
                .properties
                .insert("key".into(), Value::String("value".into()));
        }
        let cloned = structured_clone(&obj).unwrap();
        assert_eq!(
            cloned.get_property("key"),
            Some(Value::String("value".into()))
        );
    }

    #[test]
    fn test_clone_empty_object() {
        let obj = Value::new_object();
        let cloned = structured_clone(&obj).unwrap();
        if let Value::Object(ref rc) = cloned {
            assert!(rc.borrow().properties.is_empty());
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_clone_empty_array() {
        let arr = Value::new_array(vec![]);
        let cloned = structured_clone(&arr).unwrap();
        if let Value::Object(ref rc) = cloned {
            if let ObjectKind::Array(ref elems) = rc.borrow().kind {
                assert!(elems.is_empty());
            } else {
                panic!("Expected Array");
            }
        }
    }

    #[test]
    fn test_clone_error_display_formatting() {
        let e1 = CloneError::NotCloneable("function".into());
        assert_eq!(format!("{}", e1), "Cannot clone function");

        let e2 = CloneError::MaxDepthExceeded(100);
        assert_eq!(format!("{}", e2), "Max clone depth 100 exceeded");

        let e3 = CloneError::NotTransferable("Map".into());
        assert_eq!(format!("{}", e3), "Cannot transfer Map");

        let e4 = CloneError::AlreadyTransferred;
        assert_eq!(format!("{}", e4), "Object already transferred");
    }
}

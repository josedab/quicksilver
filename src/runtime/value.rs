//! JavaScript value types
//!
//! This module defines the runtime representation of JavaScript values.

use crate::bytecode::Chunk;
use crate::error::Result;
use num_bigint::BigInt;
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// Format a Unix timestamp as ISO 8601 string
fn format_timestamp(secs: i64, _nsecs: u32) -> String {
    // Simple date formatting without external dependencies
    // Unix epoch is January 1, 1970
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year, month, day from days since epoch
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
            year, month, day, hours, minutes, seconds)
}

/// Convert days since Unix epoch to year/month/day
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Simplified algorithm for date calculation
    let remaining_days = days + 719468; // days from year 0 to epoch

    let era = if remaining_days >= 0 { remaining_days } else { remaining_days - 146096 } / 146097;
    let doe = remaining_days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (year, m as u32, d as u32)
}

/// Type alias for native function implementations
pub type NativeFn = Rc<dyn Fn(&[Value]) -> Result<Value>>;

/// A JavaScript value
#[derive(Clone)]
pub enum Value {
    /// undefined
    Undefined,
    /// null
    Null,
    /// Boolean value
    Boolean(bool),
    /// Number (IEEE 754 double)
    Number(f64),
    /// BigInt (arbitrary precision integer)
    BigInt(BigInt),
    /// String
    String(String),
    /// Object (includes arrays, functions, etc.)
    Object(Rc<RefCell<Object>>),
    /// Symbol
    Symbol(u64),
}

impl Value {
    /// Check if value is undefined
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Check if value is nullish (null or undefined)
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// Convert to boolean (truthiness)
    pub fn to_boolean(&self) -> bool {
        use num_traits::Zero;
        match self {
            Value::Undefined | Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::BigInt(n) => !n.is_zero(),
            Value::String(s) => !s.is_empty(),
            Value::Object(_) => true,
            Value::Symbol(_) => true,
        }
    }

    /// Convert to number
    pub fn to_number(&self) -> f64 {
        use num_traits::ToPrimitive;
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Boolean(true) => 1.0,
            Value::Boolean(false) => 0.0,
            Value::Number(n) => *n,
            Value::BigInt(n) => n.to_f64().unwrap_or(f64::INFINITY),
            Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            Value::Object(_) => f64::NAN, // Should call valueOf/toString
            Value::Symbol(_) => f64::NAN,
        }
    }

    /// Convert to JavaScript string representation
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(true) => "true".to_string(),
            Value::Boolean(false) => "false".to_string(),
            Value::Number(n) => {
                if n.is_nan() {
                    "NaN".to_string()
                } else if n.is_infinite() {
                    if *n > 0.0 {
                        "Infinity".to_string()
                    } else {
                        "-Infinity".to_string()
                    }
                } else if *n == 0.0 {
                    "0".to_string()
                } else {
                    format!("{}", n)
                }
            }
            Value::BigInt(n) => n.to_string(),
            Value::String(s) => s.clone(),
            Value::Object(obj) => {
                let obj = obj.borrow();
                match &obj.kind {
                    ObjectKind::Array(arr) => {
                        let elements: Vec<String> = arr.iter().map(|v| v.to_js_string()).collect();
                        elements.join(",")
                    }
                    ObjectKind::Function(_) => "[Function]".to_string(),
                    ObjectKind::NativeFunction { name, .. } => format!("[Native: {}]", name),
                    ObjectKind::URL { href, .. } => href.clone(),
                    ObjectKind::URLSearchParams { params } => {
                        params
                            .iter()
                            .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
                            .collect::<Vec<_>>()
                            .join("&")
                    }
                    _ => "[object Object]".to_string(),
                }
            }
            Value::Symbol(id) => format!("Symbol({})", id),
        }
    }

    /// Get the typeof string
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object", // Historical quirk
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::BigInt(_) => "bigint",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::Object(obj) => {
                let obj = obj.borrow();
                match &obj.kind {
                    ObjectKind::Function(_)
                    | ObjectKind::NativeFunction { .. }
                    | ObjectKind::BoundArrayMethod { .. }
                    | ObjectKind::BoundStringMethod { .. } => "function",
                    _ => "object",
                }
            }
        }
    }

    /// Strict equality (===)
    pub fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => {
                if a.is_nan() || b.is_nan() {
                    false
                } else {
                    a == b
                }
            }
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            _ => false,
        }
    }

    /// Abstract equality (==)
    pub fn equals(&self, other: &Value) -> bool {
        use num_traits::ToPrimitive;
        match (self, other) {
            // Same types use strict equality
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => {
                if a.is_nan() || b.is_nan() {
                    false
                } else {
                    a == b
                }
            }
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Symbol(a), Value::Symbol(b)) => a == b,

            // null == undefined
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => true,

            // Number comparisons
            (Value::Number(_), Value::String(_)) => {
                self.strict_equals(&Value::Number(other.to_number()))
            }
            (Value::String(_), Value::Number(_)) => {
                Value::Number(self.to_number()).strict_equals(other)
            }

            // BigInt == Number (compare as numbers when possible)
            (Value::BigInt(n), Value::Number(m)) => {
                if m.is_nan() || m.is_infinite() || m.fract() != 0.0 {
                    false
                } else {
                    n.to_f64().map(|x| x == *m).unwrap_or(false)
                }
            }
            (Value::Number(n), Value::BigInt(m)) => {
                if n.is_nan() || n.is_infinite() || n.fract() != 0.0 {
                    false
                } else {
                    m.to_f64().map(|x| x == *n).unwrap_or(false)
                }
            }

            // Boolean comparisons
            (Value::Boolean(b), _) => Value::Number(if *b { 1.0 } else { 0.0 }).equals(other),
            (_, Value::Boolean(b)) => self.equals(&Value::Number(if *b { 1.0 } else { 0.0 })),

            _ => false,
        }
    }

    /// Create a new BigInt value from a string
    pub fn new_bigint(s: &str) -> Option<Value> {
        // Strip the 'n' suffix if present
        let s = s.strip_suffix('n').unwrap_or(s);
        s.parse::<BigInt>().ok().map(Value::BigInt)
    }

    /// Create a new BigInt value from an i64
    pub fn bigint_from_i64(n: i64) -> Value {
        Value::BigInt(BigInt::from(n))
    }

    /// Create a new object value
    pub fn new_object() -> Value {
        Value::Object(Rc::new(RefCell::new(Object::new())))
    }

    /// Create a new object value with properties
    pub fn new_object_with_properties(properties: HashMap<String, Value>) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Ordinary,
            properties,
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new array value
    pub fn new_array(elements: Vec<Value>) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Array(elements),
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new error value
    pub fn new_error(error_type: &str, message: &str) -> Value {
        let mut properties = HashMap::default();
        properties.insert("name".to_string(), Value::String(error_type.to_string()));
        properties.insert("message".to_string(), Value::String(message.to_string()));
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: error_type.to_string(),
                message: message.to_string(),
            },
            properties,
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new function value
    pub fn new_function(func: Function) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Function(func),
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new class value
    pub fn new_class(name: String, constructor: Option<Value>) -> Value {
        Self::new_class_with_super(name, constructor, None)
    }

    /// Create a new class value with optional superclass
    pub fn new_class_with_super(
        name: String,
        constructor: Option<Value>,
        super_class: Option<Value>,
    ) -> Value {
        // Extract the constructor function if provided
        let ctor_func = constructor.and_then(|v| {
            if let Value::Object(obj) = v {
                let obj_ref = obj.borrow();
                if let ObjectKind::Function(f) = &obj_ref.kind {
                    return Some(Box::new(f.clone()));
                }
            }
            None
        });

        // Box the superclass if provided
        let super_boxed = super_class.map(Box::new);

        // If there's a superclass, inherit its prototype
        let mut prototype = HashMap::default();
        if let Some(ref sc) = super_boxed {
            if let Value::Object(obj) = sc.as_ref() {
                let obj_ref = obj.borrow();
                if let ObjectKind::Class {
                    prototype: super_proto,
                    ..
                } = &obj_ref.kind
                {
                    // Copy superclass prototype methods
                    for (k, v) in super_proto {
                        prototype.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Class {
                name,
                constructor: ctor_func,
                prototype,
                super_class: super_boxed,
                getters: HashMap::default(),
                setters: HashMap::default(),
                static_getters: HashMap::default(),
                static_setters: HashMap::default(),
                instance_fields: HashMap::default(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new class value with prototype methods, getters, setters, and static members
    pub fn new_class_with_prototype(
        name: String,
        constructor: Option<Value>,
        prototype_methods: HashMap<String, Value>,
        getters: HashMap<String, Value>,
        setters: HashMap<String, Value>,
        static_methods: HashMap<String, Value>,
        static_getters: HashMap<String, Value>,
        static_setters: HashMap<String, Value>,
    ) -> Value {
        Self::new_class_with_prototype_and_fields(
            name,
            constructor,
            prototype_methods,
            getters,
            setters,
            static_methods,
            static_getters,
            static_setters,
            HashMap::default(),
        )
    }

    /// Create a new class value with prototype methods, getters, setters, static members, and instance fields
    pub fn new_class_with_prototype_and_fields(
        name: String,
        constructor: Option<Value>,
        prototype_methods: HashMap<String, Value>,
        getters: HashMap<String, Value>,
        setters: HashMap<String, Value>,
        static_methods: HashMap<String, Value>,
        static_getters: HashMap<String, Value>,
        static_setters: HashMap<String, Value>,
        instance_fields: HashMap<String, Value>,
    ) -> Value {
        // Extract the constructor function if provided
        let ctor_func = constructor.and_then(|v| {
            if let Value::Object(obj) = v {
                let obj_ref = obj.borrow();
                if let ObjectKind::Function(f) = &obj_ref.kind {
                    return Some(Box::new(f.clone()));
                }
            }
            None
        });

        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Class {
                name,
                constructor: ctor_func,
                prototype: prototype_methods,
                super_class: None,
                getters,
                setters,
                static_getters,
                static_setters,
                instance_fields,
            },
            properties: static_methods, // Static methods stored in properties
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new ArrayBuffer with the given byte length
    pub fn new_array_buffer(byte_length: usize) -> Value {
        let buffer = Rc::new(RefCell::new(vec![0u8; byte_length]));
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::ArrayBuffer(buffer),
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new TypedArray from an ArrayBuffer
    pub fn new_typed_array(
        buffer: Rc<RefCell<Vec<u8>>>,
        kind: TypedArrayKind,
        byte_offset: usize,
        length: usize,
    ) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::TypedArray {
                buffer,
                kind,
                byte_offset,
                length,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new TypedArray with its own buffer
    pub fn new_typed_array_with_length(kind: TypedArrayKind, length: usize) -> Value {
        let byte_length = length * kind.bytes_per_element();
        let buffer = Rc::new(RefCell::new(vec![0u8; byte_length]));
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::TypedArray {
                buffer,
                kind,
                byte_offset: 0,
                length,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Create a new DataView
    pub fn new_data_view(buffer: Rc<RefCell<Vec<u8>>>, byte_offset: usize, byte_length: usize) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::DataView {
                buffer,
                byte_offset,
                byte_length,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })))
    }

    /// Get property from object
    pub fn get_property(&self, key: &str) -> Option<Value> {
        match self {
            Value::Object(obj_rc) => {
                let obj = obj_rc.borrow();

                // Check if this is an array and we're accessing a method
                if let ObjectKind::Array(_) = &obj.kind {
                    // Known array methods - return bound methods
                    const ARRAY_METHODS: &[&str] = &[
                        "push", "pop", "shift", "unshift", "indexOf", "includes",
                        "join", "reverse", "slice", "concat", "map", "filter",
                        "forEach", "reduce", "reduceRight", "find", "findIndex",
                        "some", "every", "flat", "flatMap", "sort", "splice",
                        "fill", "copyWithin", "entries", "keys", "values", "at",
                        "toReversed", "toSorted", "toSpliced", "with",
                    ];
                    if ARRAY_METHODS.contains(&key) {
                        drop(obj);
                        return Some(Value::Object(Rc::new(RefCell::new(Object {
                            kind: ObjectKind::BoundArrayMethod {
                                receiver: obj_rc.clone(),
                                method: key.to_string(),
                            },
                            properties: HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None,
                        }))));
                    }
                }

                obj.get_property(key)
            }
            Value::String(s) => {
                // Known string methods - return bound methods
                const STRING_METHODS: &[&str] = &[
                    "charAt", "charCodeAt", "codePointAt", "concat", "endsWith",
                    "includes", "indexOf", "lastIndexOf", "localeCompare", "match",
                    "matchAll", "normalize", "padEnd", "padStart", "repeat", "replace",
                    "replaceAll", "search", "slice", "split", "startsWith", "substring",
                    "toLowerCase", "toUpperCase", "trim", "trimEnd", "trimStart",
                    "valueOf", "toString", "at",
                ];
                if STRING_METHODS.contains(&key) {
                    return Some(Value::Object(Rc::new(RefCell::new(Object {
                        kind: ObjectKind::BoundStringMethod {
                            receiver: s.clone(),
                            method: key.to_string(),
                        },
                        properties: HashMap::default(),
                        private_fields: HashMap::default(),
                        prototype: None,
                    }))));
                }

                // String properties
                match key {
                    "length" => Some(Value::Number(s.chars().count() as f64)),
                    _ => {
                        if let Ok(idx) = key.parse::<usize>() {
                            s.chars().nth(idx).map(|c| Value::String(c.to_string()))
                        } else {
                            None
                        }
                    }
                }
            }
            _ => None,
        }
    }

    /// Set property on object
    pub fn set_property(&self, key: &str, value: Value) -> bool {
        match self {
            Value::Object(obj) => {
                let mut obj = obj.borrow_mut();
                obj.set_property(key, value);
                true
            }
            _ => false,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.strict_equals(other)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undefined => write!(f, "undefined"),
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::BigInt(n) => write!(f, "{}n", n),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Object(obj) => {
                let obj = obj.borrow();
                match &obj.kind {
                    ObjectKind::Ordinary => write!(f, "{{...}}"),
                    ObjectKind::Array(arr) => write!(f, "{:?}", arr),
                    ObjectKind::Function(func) => {
                        write!(
                            f,
                            "[Function: {}]",
                            func.name.as_deref().unwrap_or("anonymous")
                        )
                    }
                    ObjectKind::NativeFunction { name, .. } => {
                        write!(f, "[Native: {}]", name)
                    }
                    ObjectKind::Class { name, .. } => write!(f, "[Class: {}]", name),
                    ObjectKind::Error { name, message } => write!(f, "{}: {}", name, message),
                    ObjectKind::Promise { .. } => write!(f, "[Promise]"),
                    ObjectKind::Iterator { .. } => write!(f, "[Iterator]"),
                    ObjectKind::SpreadMarker(arr) => write!(f, "[Spread: {:?}]", arr),
                    ObjectKind::Date(ts) => {
                        // Format as ISO string
                        let secs = (*ts / 1000.0) as i64;
                        let nsecs = ((*ts % 1000.0) * 1_000_000.0) as u32;
                        write!(f, "{}", format_timestamp(secs, nsecs))
                    }
                    ObjectKind::Map(entries) => write!(f, "Map({})", entries.len()),
                    ObjectKind::Set(items) => write!(f, "Set({})", items.len()),
                    ObjectKind::WeakMap(entries) => write!(f, "WeakMap({})", entries.iter().filter(|(k, _)| k.upgrade().is_some()).count()),
                    ObjectKind::WeakSet(items) => write!(f, "WeakSet({})", items.iter().filter(|w| w.upgrade().is_some()).count()),
                    ObjectKind::RegExp { pattern, flags, .. } => write!(f, "/{}/{}", pattern, flags),
                    ObjectKind::Generator { state, .. } => write!(f, "[Generator: {:?}]", state),
                    ObjectKind::Proxy { revoked, .. } => if *revoked { write!(f, "[Proxy (revoked)]") } else { write!(f, "[Proxy]") },
                    ObjectKind::ArrayBuffer(buf) => write!(f, "ArrayBuffer({})", buf.borrow().len()),
                    ObjectKind::TypedArray { kind, length, .. } => write!(f, "{}({})", kind.name(), length),
                    ObjectKind::DataView { byte_length, .. } => write!(f, "DataView({})", byte_length),
                    ObjectKind::BoundArrayMethod { method, .. } => write!(f, "[BoundArrayMethod: {}]", method),
                    ObjectKind::BoundStringMethod { method, .. } => write!(f, "[BoundStringMethod: {}]", method),
                    ObjectKind::BoundFunction { .. } => write!(f, "[BoundFunction]"),
                    ObjectKind::URL { href, .. } => write!(f, "URL {{ {} }}", href),
                    ObjectKind::URLSearchParams { params } => write!(f, "URLSearchParams({})", params.len()),
                    ObjectKind::Channel { capacity, .. } => write!(f, "Channel(capacity={})", capacity),
                }
            }
            Value::Symbol(id) => write!(f, "Symbol({})", id),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_js_string())
    }
}

/// JavaScript object
#[derive(Clone)]
pub struct Object {
    /// Object kind
    pub kind: ObjectKind,
    /// Properties
    pub properties: HashMap<String, Value>,
    /// Private fields (for class instances)
    pub private_fields: HashMap<String, Value>,
    /// Prototype
    pub prototype: Option<Rc<RefCell<Object>>>,
}

impl Object {
    /// Create a new ordinary object
    pub fn new() -> Self {
        Self {
            kind: ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }
    }

    /// Get a property
    pub fn get_property(&self, key: &str) -> Option<Value> {
        // Check own properties
        if let Some(value) = self.properties.get(key) {
            return Some(value.clone());
        }

        // Check array elements
        if let ObjectKind::Array(arr) = &self.kind {
            if key == "length" {
                return Some(Value::Number(arr.len() as f64));
            }
            if let Ok(idx) = key.parse::<usize>() {
                return arr.get(idx).cloned();
            }
        }

        // Check ArrayBuffer properties
        if let ObjectKind::ArrayBuffer(buffer) = &self.kind {
            if key == "byteLength" {
                return Some(Value::Number(buffer.borrow().len() as f64));
            }
        }

        // Check TypedArray elements
        if let ObjectKind::TypedArray { buffer, kind, byte_offset, length } = &self.kind {
            if key == "length" {
                return Some(Value::Number(*length as f64));
            }
            if key == "byteLength" {
                return Some(Value::Number((*length * kind.bytes_per_element()) as f64));
            }
            if key == "byteOffset" {
                return Some(Value::Number(*byte_offset as f64));
            }
            if key == "BYTES_PER_ELEMENT" {
                return Some(Value::Number(kind.bytes_per_element() as f64));
            }
            if let Ok(idx) = key.parse::<usize>() {
                if idx < *length {
                    let buf = buffer.borrow();
                    let elem_size = kind.bytes_per_element();
                    let offset = byte_offset + idx * elem_size;
                    if offset + elem_size <= buf.len() {
                        let value = match kind {
                            TypedArrayKind::Int8 => buf[offset] as i8 as f64,
                            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => buf[offset] as f64,
                            TypedArrayKind::Int16 => {
                                i16::from_le_bytes([buf[offset], buf[offset + 1]]) as f64
                            }
                            TypedArrayKind::Uint16 => {
                                u16::from_le_bytes([buf[offset], buf[offset + 1]]) as f64
                            }
                            TypedArrayKind::Int32 => {
                                i32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]) as f64
                            }
                            TypedArrayKind::Uint32 => {
                                u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]) as f64
                            }
                            TypedArrayKind::Float32 => {
                                f32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]) as f64
                            }
                            TypedArrayKind::Float64 => {
                                f64::from_le_bytes([
                                    buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3],
                                    buf[offset + 4], buf[offset + 5], buf[offset + 6], buf[offset + 7],
                                ])
                            }
                        };
                        return Some(Value::Number(value));
                    }
                }
                return Some(Value::Undefined);
            }
        }

        // Check DataView properties
        if let ObjectKind::DataView { byte_offset, byte_length, .. } = &self.kind {
            if key == "byteLength" {
                return Some(Value::Number(*byte_length as f64));
            }
            if key == "byteOffset" {
                return Some(Value::Number(*byte_offset as f64));
            }
        }

        // Check URL properties
        if let ObjectKind::URL {
            href,
            protocol,
            username,
            password,
            host,
            hostname,
            port,
            pathname,
            search,
            hash,
        } = &self.kind
        {
            match key {
                "href" => return Some(Value::String(href.clone())),
                "protocol" => return Some(Value::String(protocol.clone())),
                "username" => return Some(Value::String(username.clone())),
                "password" => return Some(Value::String(password.clone())),
                "host" => return Some(Value::String(host.clone())),
                "hostname" => return Some(Value::String(hostname.clone())),
                "port" => return Some(Value::String(port.clone())),
                "pathname" => return Some(Value::String(pathname.clone())),
                "search" => return Some(Value::String(search.clone())),
                "hash" => return Some(Value::String(hash.clone())),
                "toString" | "toJSON" => {
                    // Return the href when toString/toJSON is called
                    return Some(Value::String(href.clone()));
                }
                _ => {}
            }
        }

        // Check URLSearchParams properties
        if let ObjectKind::URLSearchParams { params } = &self.kind {
            match key {
                "size" => return Some(Value::Number(params.len() as f64)),
                _ => {}
            }
        }

        // Check class prototype methods (for instances of classes)
        if let ObjectKind::Class { prototype, .. } = &self.kind {
            if let Some(value) = prototype.get(key) {
                return Some(value.clone());
            }
        }

        // Check prototype chain
        if let Some(proto) = &self.prototype {
            let proto_ref = proto.borrow();
            // If prototype is a class, only check its internal prototype methods (not static properties)
            if let ObjectKind::Class { prototype, super_class, .. } = &proto_ref.kind {
                if let Some(value) = prototype.get(key) {
                    return Some(value.clone());
                }
                // Check superclass prototype if present
                if let Some(super_cls) = super_class {
                    if let Value::Object(super_obj) = super_cls.as_ref() {
                        let super_ref = super_obj.borrow();
                        if let ObjectKind::Class { prototype: super_proto, .. } = &super_ref.kind {
                            if let Some(value) = super_proto.get(key) {
                                return Some(value.clone());
                            }
                        }
                    }
                }
                // For class prototypes, don't check properties (those are static members)
                return None;
            }
            // For non-class prototypes, continue with normal lookup
            return proto_ref.get_property(key);
        }

        None
    }

    /// Set a property
    pub fn set_property(&mut self, key: &str, value: Value) {
        // Handle array elements
        if let ObjectKind::Array(arr) = &mut self.kind {
            if let Ok(idx) = key.parse::<usize>() {
                if idx >= arr.len() {
                    arr.resize(idx + 1, Value::Undefined);
                }
                arr[idx] = value;
                return;
            }
        }

        // Handle TypedArray element assignment
        if let ObjectKind::TypedArray { buffer, kind, byte_offset, length } = &mut self.kind {
            if let Ok(idx) = key.parse::<usize>() {
                if idx < *length {
                    let num = match &value {
                        Value::Number(n) => *n,
                        _ => value.to_number(),
                    };
                    let mut buf = buffer.borrow_mut();
                    let elem_size = kind.bytes_per_element();
                    let offset = *byte_offset + idx * elem_size;
                    if offset + elem_size <= buf.len() {
                        match kind {
                            TypedArrayKind::Int8 => {
                                buf[offset] = num as i8 as u8;
                            }
                            TypedArrayKind::Uint8 => {
                                buf[offset] = num as u8;
                            }
                            TypedArrayKind::Uint8Clamped => {
                                // Clamp to 0-255
                                let clamped = if num < 0.0 { 0u8 }
                                    else if num > 255.0 { 255u8 }
                                    else { num.round() as u8 };
                                buf[offset] = clamped;
                            }
                            TypedArrayKind::Int16 => {
                                let bytes = (num as i16).to_le_bytes();
                                buf[offset] = bytes[0];
                                buf[offset + 1] = bytes[1];
                            }
                            TypedArrayKind::Uint16 => {
                                let bytes = (num as u16).to_le_bytes();
                                buf[offset] = bytes[0];
                                buf[offset + 1] = bytes[1];
                            }
                            TypedArrayKind::Int32 => {
                                let bytes = (num as i32).to_le_bytes();
                                buf[offset..offset + 4].copy_from_slice(&bytes);
                            }
                            TypedArrayKind::Uint32 => {
                                let bytes = (num as u32).to_le_bytes();
                                buf[offset..offset + 4].copy_from_slice(&bytes);
                            }
                            TypedArrayKind::Float32 => {
                                let bytes = (num as f32).to_le_bytes();
                                buf[offset..offset + 4].copy_from_slice(&bytes);
                            }
                            TypedArrayKind::Float64 => {
                                let bytes = num.to_le_bytes();
                                buf[offset..offset + 8].copy_from_slice(&bytes);
                            }
                        }
                    }
                }
                return;
            }
        }

        self.properties.insert(key.to_string(), value);
    }

    /// Check if object has own property
    pub fn has_own_property(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }
}

impl Default for Object {
    fn default() -> Self {
        Self::new()
    }
}

/// Object kind
#[derive(Clone)]
pub enum ObjectKind {
    /// Ordinary object
    Ordinary,
    /// Array object
    Array(Vec<Value>),
    /// Function object
    Function(Function),
    /// Native function
    NativeFunction { name: String, func: NativeFn },
    /// Class object
    Class {
        name: String,
        constructor: Option<Box<Function>>,
        prototype: HashMap<String, Value>,
        /// Superclass reference for inheritance
        super_class: Option<Box<Value>>,
        /// Getter methods (property name -> getter function)
        getters: HashMap<String, Value>,
        /// Setter methods (property name -> setter function)
        setters: HashMap<String, Value>,
        /// Static getter methods
        static_getters: HashMap<String, Value>,
        /// Static setter methods
        static_setters: HashMap<String, Value>,
        /// Instance fields with default values (for field initializers)
        /// Keys starting with # are private fields
        instance_fields: HashMap<String, Value>,
    },
    /// Error object
    Error { name: String, message: String },
    /// Promise object
    Promise {
        state: PromiseState,
        value: Option<Box<Value>>,
        /// Callbacks to run when fulfilled
        on_fulfilled: Vec<Value>,
        /// Callbacks to run when rejected
        on_rejected: Vec<Value>,
    },
    /// Iterator object
    Iterator { values: Vec<Value>, index: usize },
    /// Spread marker for function calls
    SpreadMarker(Vec<Value>),
    /// Date object
    Date(f64),
    /// Map object
    Map(Vec<(Value, Value)>),
    /// Set object
    Set(Vec<Value>),
    /// WeakMap object (uses weak references for keys)
    WeakMap(Vec<(std::rc::Weak<RefCell<Object>>, Value)>),
    /// WeakSet object (uses weak references for values)
    WeakSet(Vec<std::rc::Weak<RefCell<Object>>>),
    /// RegExp object
    RegExp {
        /// The regex pattern
        pattern: String,
        /// Flags: global (g), ignore case (i), multiline (m), dotall (s), unicode (u), sticky (y)
        flags: String,
        /// Compiled regex (using Rust regex crate)
        regex: regex::Regex,
        /// Last index for global/sticky matching
        last_index: usize,
    },
    /// Generator object
    Generator {
        /// The generator function
        function: Box<Function>,
        /// Current instruction pointer
        ip: usize,
        /// Local variables saved between yields
        locals: Vec<Value>,
        /// Generator state
        state: GeneratorState,
    },
    /// Proxy object (ES6 metaprogramming)
    Proxy {
        /// The target object being proxied
        target: Box<Value>,
        /// The handler object with trap functions
        handler: Box<Value>,
        /// Is this proxy revoked?
        revoked: bool,
    },
    /// ArrayBuffer - raw binary data
    ArrayBuffer(Rc<RefCell<Vec<u8>>>),
    /// TypedArray - view into ArrayBuffer
    TypedArray {
        /// The underlying buffer
        buffer: Rc<RefCell<Vec<u8>>>,
        /// Type of elements
        kind: TypedArrayKind,
        /// Byte offset into buffer
        byte_offset: usize,
        /// Number of elements
        length: usize,
    },
    /// DataView - arbitrary access to ArrayBuffer
    DataView {
        /// The underlying buffer
        buffer: Rc<RefCell<Vec<u8>>>,
        /// Byte offset into buffer
        byte_offset: usize,
        /// Byte length of view
        byte_length: usize,
    },
    /// Bound array method - captures array and method name for deferred calls
    BoundArrayMethod {
        /// The array this method is bound to
        receiver: Rc<RefCell<Object>>,
        /// The method name (push, pop, map, etc.)
        method: String,
    },
    /// Bound string method - captures string and method name for deferred calls
    BoundStringMethod {
        /// The string this method is bound to
        receiver: String,
        /// The method name
        method: String,
    },
    /// Bound function - created by Function.prototype.bind()
    BoundFunction {
        /// The target function to call
        target: Box<Value>,
        /// The bound 'this' value
        bound_this: Box<Value>,
        /// Pre-filled arguments
        bound_args: Vec<Value>,
    },
    /// URL object
    URL {
        /// The full URL href
        href: String,
        /// Protocol (e.g., "https:")
        protocol: String,
        /// Username
        username: String,
        /// Password
        password: String,
        /// Host (hostname + port)
        host: String,
        /// Hostname only
        hostname: String,
        /// Port (empty string if not specified)
        port: String,
        /// Pathname (e.g., "/path/to/file")
        pathname: String,
        /// Search/query string including "?" (e.g., "?key=value")
        search: String,
        /// Hash including "#" (e.g., "#section")
        hash: String,
    },
    /// URLSearchParams object
    URLSearchParams {
        /// Query parameters as key-value pairs
        params: Vec<(String, String)>,
    },
    /// Channel object for concurrency
    Channel {
        /// The underlying channel (sends/receives Value)
        channel: std::sync::Arc<crate::concurrency::Channel<Value>>,
        /// Channel capacity (0 = unbuffered)
        capacity: usize,
    },
}

/// Generator state
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeneratorState {
    /// Generator has not started
    Suspended,
    /// Generator is currently executing
    Executing,
    /// Generator has completed
    Completed,
}

/// TypedArray element type
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypedArrayKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
}

impl TypedArrayKind {
    /// Get the number of bytes per element
    pub fn bytes_per_element(&self) -> usize {
        match self {
            TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
            TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
            TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
            TypedArrayKind::Float64 => 8,
        }
    }

    /// Get the type name
    pub fn name(&self) -> &'static str {
        match self {
            TypedArrayKind::Int8 => "Int8Array",
            TypedArrayKind::Uint8 => "Uint8Array",
            TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayKind::Int16 => "Int16Array",
            TypedArrayKind::Uint16 => "Uint16Array",
            TypedArrayKind::Int32 => "Int32Array",
            TypedArrayKind::Uint32 => "Uint32Array",
            TypedArrayKind::Float32 => "Float32Array",
            TypedArrayKind::Float64 => "Float64Array",
        }
    }
}

/// Promise state
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// JavaScript function
#[derive(Clone)]
pub struct Function {
    /// Function name
    pub name: Option<String>,
    /// Bytecode chunk
    pub chunk: Chunk,
    /// Captured upvalues
    pub upvalues: Vec<Rc<RefCell<Value>>>,
    /// Is this an async function?
    pub is_async: bool,
    /// Is this a generator?
    pub is_generator: bool,
}

impl Function {
    /// Create a new function
    pub fn new(name: Option<String>, chunk: Chunk) -> Self {
        Self {
            name,
            chunk,
            upvalues: Vec::new(),
            is_async: false,
            is_generator: false,
        }
    }
}

/// URL encode a string (percent-encoding)
pub fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ' ' => result.push('+'),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}

/// URL decode a string (percent-decoding)
pub fn url_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                hex_to_nibble(bytes[i + 1]),
                hex_to_nibble(bytes[i + 2]),
            ) {
                result.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

fn hex_to_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_equality() {
        assert!(Value::Undefined.strict_equals(&Value::Undefined));
        assert!(Value::Null.strict_equals(&Value::Null));
        assert!(Value::Boolean(true).strict_equals(&Value::Boolean(true)));
        assert!(Value::Number(42.0).strict_equals(&Value::Number(42.0)));
        assert!(
            Value::String("hello".to_string()).strict_equals(&Value::String("hello".to_string()))
        );

        // NaN is not equal to itself
        assert!(!Value::Number(f64::NAN).strict_equals(&Value::Number(f64::NAN)));
    }

    #[test]
    fn test_value_conversion() {
        assert!(Value::Boolean(true).to_boolean());
        assert!(!Value::Boolean(false).to_boolean());
        assert!(!Value::Number(0.0).to_boolean());
        assert!(Value::Number(1.0).to_boolean());
        assert!(!Value::String("".to_string()).to_boolean());
        assert!(Value::String("hello".to_string()).to_boolean());

        assert_eq!(Value::Number(42.0).to_number(), 42.0);
        assert_eq!(Value::String("42".to_string()).to_number(), 42.0);
    }

    #[test]
    fn test_type_of() {
        assert_eq!(Value::Undefined.type_of(), "undefined");
        assert_eq!(Value::Null.type_of(), "object");
        assert_eq!(Value::Boolean(true).type_of(), "boolean");
        assert_eq!(Value::Number(42.0).type_of(), "number");
        assert_eq!(Value::String("hello".to_string()).type_of(), "string");
    }

    #[test]
    fn test_array_operations() {
        let arr = Value::new_array(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(arr.get_property("length"), Some(Value::Number(2.0)));
        assert_eq!(arr.get_property("0"), Some(Value::Number(1.0)));
    }
}

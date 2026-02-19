//! Rust FFI Bridge
//!
//! Enables seamless interoperability between Rust and JavaScript:
//! - Call Rust functions from JavaScript
//! - Pass JavaScript callbacks to Rust
//! - Share data between Rust and JS with automatic conversion
//!
//! # Example
//! ```text
//! // In Rust
//! let mut runtime = Runtime::new();
//! runtime.register_fn("compute", |args: &[Value]| {
//!     let n = args[0].as_number().unwrap_or(0.0) as i64;
//!     Ok(Value::Number((n * 2) as f64))
//! });
//!
//! // In JavaScript
//! const result = compute(21); // Returns 42
//! ```

//! **Status:** ⚠️ Partial — Foreign function interface

pub mod native_bridge;

use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

/// Error type for FFI operations
#[derive(Debug, Clone)]
pub enum FfiError {
    TypeError(String),
    ConversionError(String),
    CallError(String),
    NotFound(String),
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeError(msg) => write!(f, "type error: {}", msg),
            Self::ConversionError(msg) => write!(f, "conversion error: {}", msg),
            Self::CallError(msg) => write!(f, "call error: {}", msg),
            Self::NotFound(msg) => write!(f, "not found: {}", msg),
        }
    }
}

impl std::error::Error for FfiError {}

/// Result type for FFI operations
pub type FfiResult<T> = Result<T, FfiError>;

/// A Rust function that can be called from JavaScript
pub type RustFn = Arc<dyn Fn(&[Value]) -> FfiResult<Value> + Send + Sync>;

/// Registry of Rust functions callable from JavaScript
#[derive(Default)]
pub struct FfiRegistry {
    functions: HashMap<String, RustFn>,
    modules: HashMap<String, FfiModule>,
}

impl FfiRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a Rust function
    pub fn register<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&[Value]) -> FfiResult<Value> + Send + Sync + 'static,
    {
        self.functions.insert(name.to_string(), Arc::new(func));
    }

    /// Register a module of functions
    pub fn register_module(&mut self, name: &str, module: FfiModule) {
        self.modules.insert(name.to_string(), module);
    }

    /// Get a registered function
    pub fn get(&self, name: &str) -> Option<&RustFn> {
        self.functions.get(name)
    }

    /// Get a function from a module
    pub fn get_module_fn(&self, module: &str, name: &str) -> Option<&RustFn> {
        self.modules.get(module).and_then(|m| m.functions.get(name))
    }

    /// Call a registered function
    pub fn call(&self, name: &str, args: &[Value]) -> FfiResult<Value> {
        if let Some(func) = self.functions.get(name) {
            func(args)
        } else {
            Err(FfiError::NotFound(format!("function '{}' not found", name)))
        }
    }

    /// List all registered functions
    pub fn list_functions(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }

    /// List all registered modules
    pub fn list_modules(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }
}

/// A module containing multiple FFI functions
#[derive(Default)]
pub struct FfiModule {
    name: String,
    functions: HashMap<String, RustFn>,
}

impl FfiModule {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            functions: HashMap::default(),
        }
    }

    /// Add a function to the module
    pub fn function<F>(mut self, name: &str, func: F) -> Self
    where
        F: Fn(&[Value]) -> FfiResult<Value> + Send + Sync + 'static,
    {
        self.functions.insert(name.to_string(), Arc::new(func));
        self
    }

    /// Get module name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a function from this module
    pub fn get(&self, name: &str) -> Option<&RustFn> {
        self.functions.get(name)
    }
}

/// Trait for converting Rust types to JavaScript values
pub trait IntoJsValue {
    fn into_js_value(self) -> Value;
}

impl IntoJsValue for () {
    fn into_js_value(self) -> Value {
        Value::Undefined
    }
}

impl IntoJsValue for bool {
    fn into_js_value(self) -> Value {
        Value::Boolean(self)
    }
}

impl IntoJsValue for i32 {
    fn into_js_value(self) -> Value {
        Value::Number(self as f64)
    }
}

impl IntoJsValue for i64 {
    fn into_js_value(self) -> Value {
        Value::Number(self as f64)
    }
}

impl IntoJsValue for f64 {
    fn into_js_value(self) -> Value {
        Value::Number(self)
    }
}

impl IntoJsValue for String {
    fn into_js_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoJsValue for &str {
    fn into_js_value(self) -> Value {
        Value::String(self.to_string())
    }
}

impl<T: IntoJsValue> IntoJsValue for Option<T> {
    fn into_js_value(self) -> Value {
        match self {
            Some(v) => v.into_js_value(),
            None => Value::Null,
        }
    }
}

impl<T: IntoJsValue> IntoJsValue for Vec<T> {
    fn into_js_value(self) -> Value {
        Value::new_array(self.into_iter().map(|v| v.into_js_value()).collect())
    }
}

/// Trait for converting JavaScript values to Rust types
pub trait FromJsValue: Sized {
    fn from_js_value(value: &Value) -> FfiResult<Self>;
}

impl FromJsValue for () {
    fn from_js_value(_value: &Value) -> FfiResult<Self> {
        Ok(())
    }
}

impl FromJsValue for bool {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Boolean(b) => Ok(*b),
            Value::Number(n) => Ok(*n != 0.0),
            Value::String(s) => Ok(!s.is_empty()),
            Value::Null | Value::Undefined => Ok(false),
            _ => Ok(true),
        }
    }
}

impl FromJsValue for i32 {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Number(n) => Ok(*n as i32),
            Value::String(s) => s.parse().map_err(|_| {
                FfiError::ConversionError(format!("cannot convert '{}' to i32", s))
            }),
            Value::Boolean(b) => Ok(if *b { 1 } else { 0 }),
            _ => Err(FfiError::TypeError("expected number".to_string())),
        }
    }
}

impl FromJsValue for i64 {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Number(n) => Ok(*n as i64),
            Value::String(s) => s.parse().map_err(|_| {
                FfiError::ConversionError(format!("cannot convert '{}' to i64", s))
            }),
            Value::Boolean(b) => Ok(if *b { 1 } else { 0 }),
            _ => Err(FfiError::TypeError("expected number".to_string())),
        }
    }
}

impl FromJsValue for f64 {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Number(n) => Ok(*n),
            Value::String(s) => s.parse().map_err(|_| {
                FfiError::ConversionError(format!("cannot convert '{}' to f64", s))
            }),
            Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
            _ => Err(FfiError::TypeError("expected number".to_string())),
        }
    }
}

impl FromJsValue for String {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::String(s) => Ok(s.clone()),
            Value::Number(n) => Ok(n.to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Undefined => Ok("undefined".to_string()),
            _ => Ok(format!("{:?}", value)),
        }
    }
}

impl<T: FromJsValue> FromJsValue for Option<T> {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Null | Value::Undefined => Ok(None),
            _ => Ok(Some(T::from_js_value(value)?)),
        }
    }
}

impl<T: FromJsValue> FromJsValue for Vec<T> {
    fn from_js_value(value: &Value) -> FfiResult<Self> {
        match value {
            Value::Object(obj) => {
                let borrowed = obj.borrow();
                if let crate::runtime::ObjectKind::Array(arr) = &borrowed.kind {
                    arr.iter().map(T::from_js_value).collect()
                } else {
                    Err(FfiError::TypeError("expected array".to_string()))
                }
            }
            _ => Err(FfiError::TypeError("expected array".to_string())),
        }
    }
}

/// Helper macro to create FFI functions with type conversion
#[macro_export]
macro_rules! ffi_fn {
    // No args, returns T
    (|| -> $ret:ty $body:block) => {
        |_args: &[$crate::runtime::Value]| -> $crate::ffi::FfiResult<$crate::runtime::Value> {
            let result: $ret = $body;
            Ok($crate::ffi::IntoJsValue::into_js_value(result))
        }
    };

    // One arg
    (|$arg:ident: $t:ty| -> $ret:ty $body:block) => {
        |args: &[$crate::runtime::Value]| -> $crate::ffi::FfiResult<$crate::runtime::Value> {
            let $arg: $t = $crate::ffi::FromJsValue::from_js_value(
                args.get(0).unwrap_or(&$crate::runtime::Value::Undefined)
            )?;
            let result: $ret = $body;
            Ok($crate::ffi::IntoJsValue::into_js_value(result))
        }
    };

    // Two args
    (|$arg1:ident: $t1:ty, $arg2:ident: $t2:ty| -> $ret:ty $body:block) => {
        |args: &[$crate::runtime::Value]| -> $crate::ffi::FfiResult<$crate::runtime::Value> {
            let $arg1: $t1 = $crate::ffi::FromJsValue::from_js_value(
                args.get(0).unwrap_or(&$crate::runtime::Value::Undefined)
            )?;
            let $arg2: $t2 = $crate::ffi::FromJsValue::from_js_value(
                args.get(1).unwrap_or(&$crate::runtime::Value::Undefined)
            )?;
            let result: $ret = $body;
            Ok($crate::ffi::IntoJsValue::into_js_value(result))
        }
    };
}

/// Standard library functions that can be exposed to JavaScript
pub mod stdlib {
    use super::*;

    /// Create a module with common utility functions
    pub fn utils_module() -> FfiModule {
        FfiModule::new("utils")
            .function("uuid", |_args| {
                // Simple UUID v4 generation
                use std::time::{SystemTime, UNIX_EPOCH};
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let uuid = format!(
                    "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
                    (timestamp >> 96) as u32,
                    (timestamp >> 80) as u16,
                    (timestamp >> 68) as u16 & 0x0fff,
                    ((timestamp >> 52) as u16 & 0x3fff) | 0x8000,
                    timestamp as u64 & 0xffffffffffff
                );
                Ok(Value::String(uuid))
            })
            .function("hash", |args| {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                let mut hasher = DefaultHasher::new();
                input.hash(&mut hasher);
                Ok(Value::Number(hasher.finish() as f64))
            })
            .function("base64_encode", |args| {
                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                let encoded = base64_encode(input.as_bytes());
                Ok(Value::String(encoded))
            })
            .function("base64_decode", |args| {
                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                match base64_decode(&input) {
                    Ok(bytes) => Ok(Value::String(String::from_utf8_lossy(&bytes).to_string())),
                    Err(e) => Err(FfiError::ConversionError(e)),
                }
            })
    }

    /// Create a module with crypto functions
    pub fn crypto_module() -> FfiModule {
        FfiModule::new("crypto")
            .function("random_bytes", |args| {
                let len = i32::from_js_value(args.first().unwrap_or(&Value::Number(16.0)))? as usize;
                let bytes: Vec<Value> = (0..len)
                    .map(|_| Value::Number((rand::random::<u8>()) as f64))
                    .collect();
                Ok(Value::new_array(bytes))
            })
            .function("random_int", |args| {
                let min = i64::from_js_value(args.first().unwrap_or(&Value::Number(0.0)))?;
                let max = i64::from_js_value(args.get(1).unwrap_or(&Value::Number(100.0)))?;
                let value = min + (rand::random::<u64>() % (max - min) as u64) as i64;
                Ok(Value::Number(value as f64))
            })
    }

    /// Create a module with string manipulation functions
    pub fn string_module() -> FfiModule {
        FfiModule::new("string")
            .function("reverse", |args| {
                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                Ok(Value::String(input.chars().rev().collect()))
            })
            .function("capitalize", |args| {
                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                let mut chars = input.chars();
                let result = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(Value::String(result))
            })
            .function("words", |args| {
                let input = String::from_js_value(args.first().unwrap_or(&Value::Undefined))?;
                let words: Vec<Value> = input
                    .split_whitespace()
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                Ok(Value::new_array(words))
            })
    }

    // Simple base64 implementation
    fn base64_encode(data: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();

        for chunk in data.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
            let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

            result.push(ALPHABET[b0 >> 2] as char);
            result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

            if chunk.len() > 1 {
                result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
            } else {
                result.push('=');
            }

            if chunk.len() > 2 {
                result.push(ALPHABET[b2 & 0x3f] as char);
            } else {
                result.push('=');
            }
        }

        result
    }

    fn base64_decode(data: &str) -> Result<Vec<u8>, String> {
        const DECODE_TABLE: [i8; 128] = [
            -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 62, -1, -1, -1, 63,
            52, 53, 54, 55, 56, 57, 58, 59, 60, 61, -1, -1, -1, -1, -1, -1,
            -1,  0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14,
            15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, -1, -1, -1, -1, -1,
            -1, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
            41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, -1, -1, -1, -1, -1,
        ];

        let data = data.trim_end_matches('=');
        let mut result = Vec::new();

        for chunk in data.as_bytes().chunks(4) {
            let mut buf = [0u8; 4];
            for (i, &b) in chunk.iter().enumerate() {
                if b >= 128 || DECODE_TABLE[b as usize] < 0 {
                    return Err(format!("invalid base64 character: {}", b as char));
                }
                buf[i] = DECODE_TABLE[b as usize] as u8;
            }

            result.push((buf[0] << 2) | (buf[1] >> 4));
            if chunk.len() > 2 {
                result.push((buf[1] << 4) | (buf[2] >> 2));
            }
            if chunk.len() > 3 {
                result.push((buf[2] << 6) | buf[3]);
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_registry() {
        let mut registry = FfiRegistry::new();

        registry.register("double", |args| {
            let n = f64::from_js_value(args.get(0).unwrap_or(&Value::Number(0.0)))?;
            Ok(Value::Number(n * 2.0))
        });

        let result = registry.call("double", &[Value::Number(21.0)]).unwrap();
        assert!(matches!(result, Value::Number(n) if n == 42.0));
    }

    #[test]
    fn test_ffi_module() {
        let module = FfiModule::new("math")
            .function("square", |args| {
                let n = f64::from_js_value(args.get(0).unwrap_or(&Value::Number(0.0)))?;
                Ok(Value::Number(n * n))
            });

        let func = module.get("square").unwrap();
        let result = func(&[Value::Number(5.0)]).unwrap();
        assert!(matches!(result, Value::Number(n) if n == 25.0));
    }

    #[test]
    fn test_type_conversion() {
        // To JS
        assert!(matches!(42i32.into_js_value(), Value::Number(n) if n == 42.0));
        assert!(matches!("hello".into_js_value(), Value::String(s) if s == "hello"));
        assert!(matches!(true.into_js_value(), Value::Boolean(true)));

        // From JS
        let n: i32 = FromJsValue::from_js_value(&Value::Number(42.0)).unwrap();
        assert_eq!(n, 42);

        let s: String = FromJsValue::from_js_value(&Value::String("hello".to_string())).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_stdlib_utils() {
        let utils = stdlib::utils_module();

        // Test hash
        let hash_fn = utils.get("hash").unwrap();
        let result = hash_fn(&[Value::String("test".to_string())]).unwrap();
        assert!(matches!(result, Value::Number(_)));

        // Test base64
        let encode_fn = utils.get("base64_encode").unwrap();
        let result = encode_fn(&[Value::String("hello".to_string())]).unwrap();
        assert!(matches!(result, Value::String(s) if s == "aGVsbG8="));
    }
}

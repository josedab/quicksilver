//! Native FFI Bridge
//!
//! Provides dynamic library loading and native function calling via a safe
//! simulated interface. Supports type marshaling between JavaScript values
//! and C-compatible native types, permission-based security, and call tracking.
//!
//! # Example
//! ```text
//! let mut bridge = NativeBridge::new();
//! bridge.load_library("math", "/usr/lib/libm.so")?;
//! bridge.register_symbol("math", "sqrt", FunctionSignature {
//!     name: "sqrt".into(),
//!     params: vec![NativeType::Float64],
//!     return_type: NativeType::Float64,
//!     calling_convention: CallingConvention::C,
//!     is_variadic: false,
//! })?;
//! let result = bridge.call("math", "sqrt", &[NativeValue::Float64(4.0)])?;
//! ```

use crate::error::{Error, Result};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// C-compatible type representation for FFI signatures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NativeType {
    Void,
    Bool,
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Float32,
    Float64,
    Pointer(Box<NativeType>),
    CString,
    Struct(Vec<(String, NativeType)>),
    Array(Box<NativeType>, usize),
    FunctionPointer(Box<FunctionSignature>),
}

impl NativeType {
    /// Returns the size in bytes of this type.
    pub fn size_of(&self) -> usize {
        match self {
            NativeType::Void => 0,
            NativeType::Bool | NativeType::Int8 | NativeType::Uint8 => 1,
            NativeType::Int16 | NativeType::Uint16 => 2,
            NativeType::Int32 | NativeType::Uint32 | NativeType::Float32 => 4,
            NativeType::Int64 | NativeType::Uint64 | NativeType::Float64 => 8,
            NativeType::Pointer(_) | NativeType::CString | NativeType::FunctionPointer(_) => 8,
            NativeType::Struct(fields) => fields.iter().map(|(_, t)| t.size_of()).sum(),
            NativeType::Array(t, len) => t.size_of() * len,
        }
    }

    /// Returns true if this is an integer type.
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            NativeType::Int8
                | NativeType::Uint8
                | NativeType::Int16
                | NativeType::Uint16
                | NativeType::Int32
                | NativeType::Uint32
                | NativeType::Int64
                | NativeType::Uint64
        )
    }

    /// Returns true if this is a floating point type.
    pub fn is_float(&self) -> bool {
        matches!(self, NativeType::Float32 | NativeType::Float64)
    }
}

/// Calling convention for native functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum CallingConvention {
    #[default]
    C,
    Stdcall,
    Fastcall,
    System,
}


/// Describes a native function's type signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<NativeType>,
    pub return_type: NativeType,
    pub calling_convention: CallingConvention,
    pub is_variadic: bool,
}

impl FunctionSignature {
    /// Validate that given arguments match this signature.
    pub fn validate_args(&self, args: &[NativeValue]) -> Result<()> {
        if self.is_variadic {
            if args.len() < self.params.len() {
                return Err(Error::InternalError(format!(
                    "expected at least {} arguments for '{}', got {}",
                    self.params.len(),
                    self.name,
                    args.len()
                )));
            }
        } else if args.len() != self.params.len() {
            return Err(Error::InternalError(format!(
                "expected {} arguments for '{}', got {}",
                self.params.len(),
                self.name,
                args.len()
            )));
        }

        for (i, (arg, param_type)) in args.iter().zip(self.params.iter()).enumerate() {
            if !arg.is_compatible_with(param_type) {
                return Err(Error::InternalError(format!(
                    "argument {} for '{}': type mismatch, expected {:?}",
                    i, self.name, param_type
                )));
            }
        }

        Ok(())
    }
}

/// Runtime value for FFI calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NativeValue {
    Void,
    Bool(bool),
    Int8(i8),
    Uint8(u8),
    Int16(i16),
    Uint16(u16),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Float32(f32),
    Float64(f64),
    String(String),
    Pointer(usize),
    Struct(Vec<(String, NativeValue)>),
    Array(Vec<NativeValue>),
    Null,
}

impl NativeValue {
    /// Check if this value is compatible with a given native type.
    pub fn is_compatible_with(&self, target: &NativeType) -> bool {
        matches!(
            (self, target),
            (NativeValue::Void, NativeType::Void)
                | (NativeValue::Bool(_), NativeType::Bool)
                | (NativeValue::Int8(_), NativeType::Int8)
                | (NativeValue::Uint8(_), NativeType::Uint8)
                | (NativeValue::Int16(_), NativeType::Int16)
                | (NativeValue::Uint16(_), NativeType::Uint16)
                | (NativeValue::Int32(_), NativeType::Int32)
                | (NativeValue::Uint32(_), NativeType::Uint32)
                | (NativeValue::Int64(_), NativeType::Int64)
                | (NativeValue::Uint64(_), NativeType::Uint64)
                | (NativeValue::Float32(_), NativeType::Float32)
                | (NativeValue::Float64(_), NativeType::Float64)
                | (NativeValue::String(_), NativeType::CString)
                | (NativeValue::Pointer(_), NativeType::Pointer(_))
                | (NativeValue::Struct(_), NativeType::Struct(_))
                | (NativeValue::Array(_), NativeType::Array(_, _))
                | (NativeValue::Null, NativeType::Pointer(_))
        )
    }

    /// Convert this value to f64, if numeric.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            NativeValue::Int8(v) => Some(*v as f64),
            NativeValue::Uint8(v) => Some(*v as f64),
            NativeValue::Int16(v) => Some(*v as f64),
            NativeValue::Uint16(v) => Some(*v as f64),
            NativeValue::Int32(v) => Some(*v as f64),
            NativeValue::Uint32(v) => Some(*v as f64),
            NativeValue::Int64(v) => Some(*v as f64),
            NativeValue::Uint64(v) => Some(*v as f64),
            NativeValue::Float32(v) => Some(*v as f64),
            NativeValue::Float64(v) => Some(*v),
            NativeValue::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
}

/// A resolved function symbol in a loaded library.
#[derive(Debug, Clone)]
pub struct NativeSymbol {
    pub name: String,
    pub signature: FunctionSignature,
    pub address: usize,
    pub call_count: u64,
}

/// Represents a loaded native library.
#[derive(Debug, Clone)]
pub struct NativeLibrary {
    pub name: String,
    pub path: String,
    pub symbols: HashMap<String, NativeSymbol>,
    pub loaded: bool,
    pub load_time: Instant,
}

impl NativeLibrary {
    /// Create a new loaded library.
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            symbols: HashMap::default(),
            loaded: true,
            load_time: Instant::now(),
        }
    }

    /// Register a symbol in this library.
    pub fn register_symbol(&mut self, name: &str, signature: FunctionSignature, address: usize) {
        self.symbols.insert(
            name.to_string(),
            NativeSymbol {
                name: name.to_string(),
                signature,
                address,
                call_count: 0,
            },
        );
    }

    /// Look up a symbol by name.
    pub fn get_symbol(&self, name: &str) -> Option<&NativeSymbol> {
        self.symbols.get(name)
    }

    /// Get a mutable reference to a symbol.
    pub fn get_symbol_mut(&mut self, name: &str) -> Option<&mut NativeSymbol> {
        self.symbols.get_mut(name)
    }

    /// List all symbol names in this library.
    pub fn symbol_names(&self) -> Vec<&str> {
        self.symbols.keys().map(|s| s.as_str()).collect()
    }
}

/// Security layer for FFI operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiPermissions {
    /// If `Some`, only these library names may be loaded. `None` means all allowed.
    pub allowed_libraries: Option<Vec<String>>,
    /// Symbols that are always blocked from being called.
    pub blocked_symbols: Vec<String>,
    /// Maximum number of FFI calls per second (0 = unlimited).
    pub max_calls_per_second: u64,
    /// Whether callbacks from native code to JS are allowed.
    pub allow_callbacks: bool,
    /// Whether pointer arithmetic operations are allowed.
    pub allow_pointer_arithmetic: bool,
}

impl Default for FfiPermissions {
    fn default() -> Self {
        Self {
            allowed_libraries: None,
            blocked_symbols: Vec::new(),
            max_calls_per_second: 0,
            allow_callbacks: true,
            allow_pointer_arithmetic: false,
        }
    }
}

impl FfiPermissions {
    /// Create a restrictive permission set.
    pub fn restrictive() -> Self {
        Self {
            allowed_libraries: Some(Vec::new()),
            blocked_symbols: Vec::new(),
            max_calls_per_second: 100,
            allow_callbacks: false,
            allow_pointer_arithmetic: false,
        }
    }

    /// Check if a library is allowed to be loaded.
    pub fn is_library_allowed(&self, name: &str) -> bool {
        match &self.allowed_libraries {
            None => true,
            Some(allowed) => allowed.iter().any(|a| a == name),
        }
    }

    /// Check if a symbol is blocked.
    pub fn is_symbol_blocked(&self, name: &str) -> bool {
        self.blocked_symbols.iter().any(|b| b == name)
    }
}

/// Record of a single FFI call for auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiCallRecord {
    pub library: String,
    pub symbol: String,
    pub arg_count: usize,
    pub success: bool,
    pub duration_us: u64,
    pub timestamp_ms: u64,
}

/// Aggregate statistics for FFI calls.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FfiStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_duration_us: u64,
    pub libraries_loaded: u64,
    pub symbols_resolved: u64,
}

/// Type marshaler for converting between JS values and native values.
pub struct TypeMarshaler;

impl TypeMarshaler {
    /// Convert a JSON value to a native value given the target type.
    pub fn js_to_native(value: &serde_json::Value, target: &NativeType) -> Result<NativeValue> {
        match target {
            NativeType::Void => Ok(NativeValue::Void),
            NativeType::Bool => match value {
                serde_json::Value::Bool(b) => Ok(NativeValue::Bool(*b)),
                serde_json::Value::Number(n) => Ok(NativeValue::Bool(n.as_f64().unwrap_or(0.0) != 0.0)),
                _ => Err(Error::InternalError("expected boolean".to_string())),
            },
            NativeType::Int8 => Self::extract_int(value, "i8").map(|n| NativeValue::Int8(n as i8)),
            NativeType::Uint8 => Self::extract_int(value, "u8").map(|n| NativeValue::Uint8(n as u8)),
            NativeType::Int16 => Self::extract_int(value, "i16").map(|n| NativeValue::Int16(n as i16)),
            NativeType::Uint16 => Self::extract_int(value, "u16").map(|n| NativeValue::Uint16(n as u16)),
            NativeType::Int32 => Self::extract_int(value, "i32").map(|n| NativeValue::Int32(n as i32)),
            NativeType::Uint32 => Self::extract_int(value, "u32").map(|n| NativeValue::Uint32(n as u32)),
            NativeType::Int64 => Self::extract_int(value, "i64").map(NativeValue::Int64),
            NativeType::Uint64 => Self::extract_int(value, "u64").map(|n| NativeValue::Uint64(n as u64)),
            NativeType::Float32 => Self::extract_float(value, "f32").map(|n| NativeValue::Float32(n as f32)),
            NativeType::Float64 => Self::extract_float(value, "f64").map(NativeValue::Float64),
            NativeType::CString => match value {
                serde_json::Value::String(s) => Ok(NativeValue::String(s.clone())),
                _ => Err(Error::InternalError("expected string".to_string())),
            },
            NativeType::Pointer(_) => match value {
                serde_json::Value::Number(n) => Ok(NativeValue::Pointer(n.as_u64().unwrap_or(0) as usize)),
                serde_json::Value::Null => Ok(NativeValue::Null),
                _ => Err(Error::InternalError("expected pointer (number or null)".to_string())),
            },
            NativeType::Struct(fields) => {
                let obj = value
                    .as_object()
                    .ok_or_else(|| Error::InternalError("expected object for struct".to_string()))?;
                let mut result = Vec::new();
                for (name, field_type) in fields {
                    let field_val = obj
                        .get(name)
                        .unwrap_or(&serde_json::Value::Null);
                    result.push((name.clone(), Self::js_to_native(field_val, field_type)?));
                }
                Ok(NativeValue::Struct(result))
            }
            NativeType::Array(elem_type, len) => {
                let arr = value
                    .as_array()
                    .ok_or_else(|| Error::InternalError("expected array".to_string()))?;
                if arr.len() != *len {
                    return Err(Error::InternalError(format!(
                        "expected array of length {}, got {}",
                        len,
                        arr.len()
                    )));
                }
                let elems: Result<Vec<_>> = arr.iter().map(|v| Self::js_to_native(v, elem_type)).collect();
                Ok(NativeValue::Array(elems?))
            }
            NativeType::FunctionPointer(_) => match value {
                serde_json::Value::Number(n) => Ok(NativeValue::Pointer(n.as_u64().unwrap_or(0) as usize)),
                _ => Err(Error::InternalError("expected function pointer address".to_string())),
            },
        }
    }

    /// Convert a native value back to a JSON value.
    pub fn native_to_js(value: &NativeValue) -> serde_json::Value {
        match value {
            NativeValue::Void => serde_json::Value::Null,
            NativeValue::Bool(b) => serde_json::Value::Bool(*b),
            NativeValue::Int8(v) => serde_json::json!(*v),
            NativeValue::Uint8(v) => serde_json::json!(*v),
            NativeValue::Int16(v) => serde_json::json!(*v),
            NativeValue::Uint16(v) => serde_json::json!(*v),
            NativeValue::Int32(v) => serde_json::json!(*v),
            NativeValue::Uint32(v) => serde_json::json!(*v),
            NativeValue::Int64(v) => serde_json::json!(*v),
            NativeValue::Uint64(v) => serde_json::json!(*v),
            NativeValue::Float32(v) => serde_json::json!(*v),
            NativeValue::Float64(v) => serde_json::json!(*v),
            NativeValue::String(s) => serde_json::Value::String(s.clone()),
            NativeValue::Pointer(addr) => serde_json::json!(*addr),
            NativeValue::Struct(fields) => {
                let mut map = serde_json::Map::new();
                for (name, val) in fields {
                    map.insert(name.clone(), Self::native_to_js(val));
                }
                serde_json::Value::Object(map)
            }
            NativeValue::Array(elems) => {
                serde_json::Value::Array(elems.iter().map(Self::native_to_js).collect())
            }
            NativeValue::Null => serde_json::Value::Null,
        }
    }

    fn extract_int(value: &serde_json::Value, type_name: &str) -> Result<i64> {
        match value {
            serde_json::Value::Number(n) => n
                .as_i64()
                .or_else(|| n.as_f64().map(|f| f as i64))
                .ok_or_else(|| Error::InternalError(format!("cannot convert to {}", type_name))),
            _ => Err(Error::InternalError(format!("expected number for {}", type_name))),
        }
    }

    fn extract_float(value: &serde_json::Value, type_name: &str) -> Result<f64> {
        match value {
            serde_json::Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| Error::InternalError(format!("cannot convert to {}", type_name))),
            _ => Err(Error::InternalError(format!("expected number for {}", type_name))),
        }
    }
}

/// Main interface for loading native libraries and calling native functions.
pub struct NativeBridge {
    libraries: HashMap<String, NativeLibrary>,
    permissions: FfiPermissions,
    type_cache: HashMap<String, NativeType>,
    call_log: Vec<FfiCallRecord>,
    stats: FfiStats,
}

impl NativeBridge {
    /// Create a new bridge with default (permissive) permissions.
    pub fn new() -> Self {
        Self {
            libraries: HashMap::default(),
            permissions: FfiPermissions::default(),
            type_cache: HashMap::default(),
            call_log: Vec::new(),
            stats: FfiStats::default(),
        }
    }

    /// Create a new bridge with the given permissions.
    pub fn with_permissions(permissions: FfiPermissions) -> Self {
        Self {
            libraries: HashMap::default(),
            permissions,
            type_cache: HashMap::default(),
            call_log: Vec::new(),
            stats: FfiStats::default(),
        }
    }

    /// Load a library by name and path.
    pub fn load_library(&mut self, name: &str, path: &str) -> Result<()> {
        if !self.permissions.is_library_allowed(name) {
            return Err(Error::InternalError(format!(
                "library '{}' is not allowed by FFI permissions",
                name
            )));
        }

        let lib = NativeLibrary::new(name, path);
        self.libraries.insert(name.to_string(), lib);
        self.stats.libraries_loaded += 1;
        Ok(())
    }

    /// Unload a library by name.
    pub fn unload_library(&mut self, name: &str) -> Result<()> {
        self.libraries
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| Error::InternalError(format!("library '{}' not loaded", name)))
    }

    /// Register a symbol in a loaded library.
    pub fn register_symbol(
        &mut self,
        library: &str,
        name: &str,
        signature: FunctionSignature,
    ) -> Result<()> {
        if self.permissions.is_symbol_blocked(name) {
            return Err(Error::InternalError(format!(
                "symbol '{}' is blocked by FFI permissions",
                name
            )));
        }

        let lib = self
            .libraries
            .get_mut(library)
            .ok_or_else(|| Error::InternalError(format!("library '{}' not loaded", library)))?;

        // Simulated address based on symbol name hash
        let address = {
            let mut h: usize = 0x1000;
            for b in name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as usize);
            }
            h
        };

        lib.register_symbol(name, signature, address);
        self.stats.symbols_resolved += 1;
        Ok(())
    }

    /// Call a native function by library and symbol name (simulated).
    pub fn call(
        &mut self,
        library: &str,
        symbol: &str,
        args: &[NativeValue],
    ) -> Result<NativeValue> {
        if self.permissions.is_symbol_blocked(symbol) {
            return Err(Error::InternalError(format!(
                "symbol '{}' is blocked by FFI permissions",
                symbol
            )));
        }

        let start = Instant::now();

        let lib = self
            .libraries
            .get_mut(library)
            .ok_or_else(|| Error::InternalError(format!("library '{}' not loaded", library)))?;

        if !lib.loaded {
            return Err(Error::InternalError(format!(
                "library '{}' is not loaded",
                library
            )));
        }

        let sym = lib
            .get_symbol_mut(symbol)
            .ok_or_else(|| {
                Error::InternalError(format!("symbol '{}' not found in '{}'", symbol, library))
            })?;

        sym.signature.validate_args(args)?;
        sym.call_count += 1;

        // Simulated call: return default value matching the return type
        let result = Self::simulate_call(&sym.signature.return_type, args);

        let duration = start.elapsed();
        let duration_us = duration.as_micros() as u64;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let record = FfiCallRecord {
            library: library.to_string(),
            symbol: symbol.to_string(),
            arg_count: args.len(),
            success: true,
            duration_us,
            timestamp_ms,
        };
        self.call_log.push(record);
        self.stats.total_calls += 1;
        self.stats.successful_calls += 1;
        self.stats.total_duration_us += duration_us;

        Ok(result)
    }

    /// Get the call log.
    pub fn call_log(&self) -> &[FfiCallRecord] {
        &self.call_log
    }

    /// Get aggregate statistics.
    pub fn stats(&self) -> &FfiStats {
        &self.stats
    }

    /// Get a reference to a loaded library.
    pub fn get_library(&self, name: &str) -> Option<&NativeLibrary> {
        self.libraries.get(name)
    }

    /// List loaded library names.
    pub fn library_names(&self) -> Vec<&str> {
        self.libraries.keys().map(|s| s.as_str()).collect()
    }

    /// Get current permissions.
    pub fn permissions(&self) -> &FfiPermissions {
        &self.permissions
    }

    /// Cache a named type for reuse.
    pub fn cache_type(&mut self, name: &str, native_type: NativeType) {
        self.type_cache.insert(name.to_string(), native_type);
    }

    /// Look up a cached type by name.
    pub fn get_cached_type(&self, name: &str) -> Option<&NativeType> {
        self.type_cache.get(name)
    }

    /// Simulate a native call by returning a sensible default for the return type.
    fn simulate_call(return_type: &NativeType, args: &[NativeValue]) -> NativeValue {
        match return_type {
            NativeType::Void => NativeValue::Void,
            NativeType::Bool => NativeValue::Bool(true),
            NativeType::Int8 => NativeValue::Int8(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as i8),
            NativeType::Uint8 => NativeValue::Uint8(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as u8),
            NativeType::Int16 => NativeValue::Int16(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as i16),
            NativeType::Uint16 => NativeValue::Uint16(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as u16),
            NativeType::Int32 => NativeValue::Int32(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as i32),
            NativeType::Uint32 => NativeValue::Uint32(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as u32),
            NativeType::Int64 => NativeValue::Int64(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as i64),
            NativeType::Uint64 => NativeValue::Uint64(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as u64),
            NativeType::Float32 => NativeValue::Float32(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0) as f32),
            NativeType::Float64 => NativeValue::Float64(args.first().and_then(|a| a.as_f64()).unwrap_or(0.0)),
            NativeType::CString => NativeValue::String(String::new()),
            NativeType::Pointer(_) => NativeValue::Pointer(0),
            NativeType::Struct(fields) => {
                NativeValue::Struct(
                    fields
                        .iter()
                        .map(|(name, t)| (name.clone(), Self::simulate_call(t, &[])))
                        .collect(),
                )
            }
            NativeType::Array(elem, len) => {
                NativeValue::Array((0..*len).map(|_| Self::simulate_call(elem, &[])).collect())
            }
            NativeType::FunctionPointer(_) => NativeValue::Pointer(0),
        }
    }
}

impl Default for NativeBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    fn make_signature(name: &str, params: Vec<NativeType>, ret: NativeType) -> FunctionSignature {
        FunctionSignature {
            name: name.to_string(),
            params,
            return_type: ret,
            calling_convention: CallingConvention::C,
            is_variadic: false,
        }
    }

    #[test]
    fn test_load_library() {
        let mut bridge = NativeBridge::new();
        assert!(bridge.load_library("math", "/usr/lib/libm.so").is_ok());
        assert!(bridge.get_library("math").is_some());
        assert_eq!(bridge.stats().libraries_loaded, 1);
    }

    #[test]
    fn test_unload_library() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        assert!(bridge.unload_library("math").is_ok());
        assert!(bridge.get_library("math").is_none());
        assert!(bridge.unload_library("math").is_err());
    }

    #[test]
    fn test_register_symbol() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        let sig = make_signature("sqrt", vec![NativeType::Float64], NativeType::Float64);
        assert!(bridge.register_symbol("math", "sqrt", sig).is_ok());

        let lib = bridge.get_library("math").unwrap();
        assert!(lib.get_symbol("sqrt").is_some());
        assert_eq!(bridge.stats().symbols_resolved, 1);
    }

    #[test]
    fn test_register_symbol_unknown_library() {
        let mut bridge = NativeBridge::new();
        let sig = make_signature("foo", vec![], NativeType::Void);
        assert!(bridge.register_symbol("unknown", "foo", sig).is_err());
    }

    #[test]
    fn test_call_native_function() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        let sig = make_signature("sqrt", vec![NativeType::Float64], NativeType::Float64);
        bridge.register_symbol("math", "sqrt", sig).unwrap();

        let result = bridge.call("math", "sqrt", &[NativeValue::Float64(4.0)]).unwrap();
        assert!(matches!(result, NativeValue::Float64(v) if v == 4.0));
        assert_eq!(bridge.stats().total_calls, 1);
        assert_eq!(bridge.stats().successful_calls, 1);
    }

    #[test]
    fn test_call_arg_count_mismatch() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        let sig = make_signature("sqrt", vec![NativeType::Float64], NativeType::Float64);
        bridge.register_symbol("math", "sqrt", sig).unwrap();

        let result = bridge.call("math", "sqrt", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_call_type_mismatch() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        let sig = make_signature("sqrt", vec![NativeType::Float64], NativeType::Float64);
        bridge.register_symbol("math", "sqrt", sig).unwrap();

        let result = bridge.call("math", "sqrt", &[NativeValue::String("bad".into())]);
        assert!(result.is_err());
    }

    #[test]
    fn test_permissions_block_library() {
        let perms = FfiPermissions {
            allowed_libraries: Some(vec!["safe_lib".to_string()]),
            ..Default::default()
        };
        let mut bridge = NativeBridge::with_permissions(perms);
        assert!(bridge.load_library("safe_lib", "/lib/safe.so").is_ok());
        assert!(bridge.load_library("evil_lib", "/lib/evil.so").is_err());
    }

    #[test]
    fn test_permissions_block_symbol() {
        let perms = FfiPermissions {
            blocked_symbols: vec!["system".to_string()],
            ..Default::default()
        };
        let mut bridge = NativeBridge::with_permissions(perms);
        bridge.load_library("libc", "/lib/libc.so").unwrap();
        let sig = make_signature("system", vec![NativeType::CString], NativeType::Int32);
        assert!(bridge.register_symbol("libc", "system", sig).is_err());
    }

    #[test]
    fn test_permissions_block_symbol_call() {
        let perms = FfiPermissions {
            blocked_symbols: vec!["exec".to_string()],
            ..Default::default()
        };
        let mut bridge = NativeBridge::with_permissions(perms);
        bridge.load_library("libc", "/lib/libc.so").unwrap();
        let result = bridge.call("libc", "exec", &[NativeValue::String("cmd".into())]);
        assert!(result.is_err());
    }

    #[test]
    fn test_call_log() {
        let mut bridge = NativeBridge::new();
        bridge.load_library("math", "/usr/lib/libm.so").unwrap();
        let sig = make_signature("abs", vec![NativeType::Int32], NativeType::Int32);
        bridge.register_symbol("math", "abs", sig).unwrap();

        bridge.call("math", "abs", &[NativeValue::Int32(-5)]).unwrap();
        assert_eq!(bridge.call_log().len(), 1);
        assert_eq!(bridge.call_log()[0].symbol, "abs");
        assert!(bridge.call_log()[0].success);
    }

    #[test]
    fn test_type_marshaling_js_to_native() {
        let val = serde_json::json!(42);
        let result = TypeMarshaler::js_to_native(&val, &NativeType::Int32).unwrap();
        assert!(matches!(result, NativeValue::Int32(42)));

        let val = serde_json::json!(3.14);
        let result = TypeMarshaler::js_to_native(&val, &NativeType::Float64).unwrap();
        assert!(matches!(result, NativeValue::Float64(v) if (v - 3.14).abs() < f64::EPSILON));

        let val = serde_json::json!("hello");
        let result = TypeMarshaler::js_to_native(&val, &NativeType::CString).unwrap();
        assert!(matches!(result, NativeValue::String(ref s) if s == "hello"));

        let val = serde_json::json!(true);
        let result = TypeMarshaler::js_to_native(&val, &NativeType::Bool).unwrap();
        assert!(matches!(result, NativeValue::Bool(true)));
    }

    #[test]
    fn test_type_marshaling_native_to_js() {
        let val = TypeMarshaler::native_to_js(&NativeValue::Int32(42));
        assert_eq!(val, serde_json::json!(42));

        let val = TypeMarshaler::native_to_js(&NativeValue::Float64(3.14));
        assert_eq!(val, serde_json::json!(3.14));

        let val = TypeMarshaler::native_to_js(&NativeValue::String("hello".into()));
        assert_eq!(val, serde_json::json!("hello"));

        let val = TypeMarshaler::native_to_js(&NativeValue::Null);
        assert!(val.is_null());
    }

    #[test]
    fn test_type_marshaling_roundtrip() {
        let cases: Vec<(serde_json::Value, NativeType)> = vec![
            (serde_json::json!(42), NativeType::Int32),
            (serde_json::json!(255), NativeType::Uint8),
            (serde_json::json!(3.14), NativeType::Float64),
            (serde_json::json!(true), NativeType::Bool),
            (serde_json::json!("test"), NativeType::CString),
        ];

        for (js_val, native_type) in cases {
            let native = TypeMarshaler::js_to_native(&js_val, &native_type).unwrap();
            let back = TypeMarshaler::native_to_js(&native);
            assert_eq!(js_val, back, "roundtrip failed for {:?}", native_type);
        }
    }

    #[test]
    fn test_type_marshaling_struct() {
        let struct_type = NativeType::Struct(vec![
            ("x".to_string(), NativeType::Int32),
            ("y".to_string(), NativeType::Int32),
        ]);
        let val = serde_json::json!({"x": 10, "y": 20});
        let native = TypeMarshaler::js_to_native(&val, &struct_type).unwrap();

        if let NativeValue::Struct(fields) = &native {
            assert_eq!(fields.len(), 2);
            assert!(matches!(fields[0], (ref n, NativeValue::Int32(10)) if n == "x"));
            assert!(matches!(fields[1], (ref n, NativeValue::Int32(20)) if n == "y"));
        } else {
            panic!("expected struct");
        }

        let back = TypeMarshaler::native_to_js(&native);
        assert_eq!(back, serde_json::json!({"x": 10, "y": 20}));
    }

    #[test]
    fn test_native_type_size_of() {
        assert_eq!(NativeType::Void.size_of(), 0);
        assert_eq!(NativeType::Bool.size_of(), 1);
        assert_eq!(NativeType::Int32.size_of(), 4);
        assert_eq!(NativeType::Float64.size_of(), 8);
        assert_eq!(NativeType::Pointer(Box::new(NativeType::Void)).size_of(), 8);
        assert_eq!(NativeType::Array(Box::new(NativeType::Int32), 4).size_of(), 16);
    }

    #[test]
    fn test_native_value_compatibility() {
        assert!(NativeValue::Int32(1).is_compatible_with(&NativeType::Int32));
        assert!(NativeValue::Float64(1.0).is_compatible_with(&NativeType::Float64));
        assert!(NativeValue::String("a".into()).is_compatible_with(&NativeType::CString));
        assert!(NativeValue::Null.is_compatible_with(&NativeType::Pointer(Box::new(NativeType::Void))));
        assert!(!NativeValue::String("a".into()).is_compatible_with(&NativeType::Int32));
        assert!(!NativeValue::Int32(1).is_compatible_with(&NativeType::Float64));
    }

    #[test]
    fn test_variadic_signature() {
        let sig = FunctionSignature {
            name: "printf".to_string(),
            params: vec![NativeType::CString],
            return_type: NativeType::Int32,
            calling_convention: CallingConvention::C,
            is_variadic: true,
        };
        // Variadic: extra args beyond params are allowed
        assert!(sig.validate_args(&[NativeValue::String("fmt".into()), NativeValue::Int32(42)]).is_ok());
        // But must have at least the required params
        assert!(sig.validate_args(&[]).is_err());
    }

    #[test]
    fn test_type_cache() {
        let mut bridge = NativeBridge::new();
        let point_type = NativeType::Struct(vec![
            ("x".to_string(), NativeType::Float64),
            ("y".to_string(), NativeType::Float64),
        ]);
        bridge.cache_type("Point", point_type.clone());
        assert_eq!(bridge.get_cached_type("Point"), Some(&point_type));
        assert_eq!(bridge.get_cached_type("Unknown"), None);
    }

    #[test]
    fn test_restrictive_permissions() {
        let perms = FfiPermissions::restrictive();
        assert!(!perms.is_library_allowed("anything"));
        assert_eq!(perms.max_calls_per_second, 100);
        assert!(!perms.allow_callbacks);
        assert!(!perms.allow_pointer_arithmetic);
    }

    #[test]
    fn test_library_symbol_names() {
        let mut lib = NativeLibrary::new("test", "/lib/test.so");
        let sig1 = make_signature("foo", vec![], NativeType::Void);
        let sig2 = make_signature("bar", vec![], NativeType::Void);
        lib.register_symbol("foo", sig1, 0x1000);
        lib.register_symbol("bar", sig2, 0x2000);

        let names = lib.symbol_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
    }
}

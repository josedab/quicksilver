//! Embeddable C API (libquicksilver)
//!
//! Provides a stable C FFI for embedding Quicksilver in non-Rust applications.
//! Uses opaque handle pattern for memory safety at the FFI boundary.
//!
//! # Usage from C
//! ```c
//! #include "quicksilver.h"
//! QsRuntime* rt = qs_runtime_new();
//! QsValue* result = qs_eval(rt, "1 + 2");
//! double num = qs_value_to_number(result);
//! qs_value_free(result);
//! qs_runtime_free(rt);
//! ```

//! **Status:** ✅ Complete — Full C FFI with runtime, value, object, callback APIs

use crate::runtime::{Runtime, Value};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

/// Opaque runtime handle
pub struct QsRuntime {
    inner: Runtime,
}

/// Opaque value handle
pub struct QsValue {
    inner: Value,
}

/// Error information returned from API calls
#[repr(C)]
pub struct QsError {
    pub message: *mut c_char,
    pub code: i32,
}

/// Value type tag for C consumers
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QsValueType {
    Undefined = 0,
    Null = 1,
    Boolean = 2,
    Number = 3,
    String = 4,
    Object = 5,
    Array = 6,
    Function = 7,
    BigInt = 8,
    Symbol = 9,
}

// === Runtime Management ===

/// Create a new Quicksilver runtime
///
/// # Safety
/// Returns a heap-allocated runtime. Must be freed with `qs_runtime_free`.
#[no_mangle]
pub extern "C" fn qs_runtime_new() -> *mut QsRuntime {
    let runtime = Box::new(QsRuntime {
        inner: Runtime::new(),
    });
    Box::into_raw(runtime)
}

/// Free a runtime
///
/// # Safety
/// `rt` must be a valid pointer returned by `qs_runtime_new`.
#[no_mangle]
pub unsafe extern "C" fn qs_runtime_free(rt: *mut QsRuntime) {
    if !rt.is_null() {
        drop(Box::from_raw(rt));
    }
}

// === Evaluation ===

/// Evaluate JavaScript source code
///
/// Returns a value handle on success, null on error.
/// If `error` is non-null, it will be populated with error details on failure.
///
/// # Safety
/// `rt` must be valid. `source` must be a valid null-terminated UTF-8 string.
/// Returned value must be freed with `qs_value_free`.
#[no_mangle]
pub unsafe extern "C" fn qs_eval(
    rt: *mut QsRuntime,
    source: *const c_char,
    error: *mut QsError,
) -> *mut QsValue {
    if rt.is_null() || source.is_null() {
        return ptr::null_mut();
    }

    let rt = &mut *rt;
    let source_str = match CStr::from_ptr(source).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error(error, "Invalid UTF-8 in source", 1);
            return ptr::null_mut();
        }
    };

    match rt.inner.eval(source_str) {
        Ok(value) => {
            let boxed = Box::new(QsValue { inner: value });
            Box::into_raw(boxed)
        }
        Err(e) => {
            set_error(error, &e.to_string(), 2);
            ptr::null_mut()
        }
    }
}

// === Value Creation ===

/// Create an undefined value
#[no_mangle]
pub extern "C" fn qs_value_undefined() -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::Undefined,
    }))
}

/// Create a null value
#[no_mangle]
pub extern "C" fn qs_value_null() -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::Null,
    }))
}

/// Create a boolean value
#[no_mangle]
pub extern "C" fn qs_value_boolean(val: bool) -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::Boolean(val),
    }))
}

/// Create a number value
#[no_mangle]
pub extern "C" fn qs_value_number(val: f64) -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::Number(val),
    }))
}

/// Create a string value
///
/// # Safety
/// `val` must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn qs_value_string(val: *const c_char) -> *mut QsValue {
    if val.is_null() {
        return qs_value_undefined();
    }
    let s = match CStr::from_ptr(val).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return qs_value_undefined(),
    };
    Box::into_raw(Box::new(QsValue {
        inner: Value::String(s),
    }))
}

/// Create an empty object
#[no_mangle]
pub extern "C" fn qs_value_object() -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::new_object(),
    }))
}

/// Create an empty array
#[no_mangle]
pub extern "C" fn qs_value_array() -> *mut QsValue {
    Box::into_raw(Box::new(QsValue {
        inner: Value::new_array(Vec::new()),
    }))
}

// === Value Inspection ===

/// Get the type of a value
///
/// # Safety
/// `val` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_value_type(val: *const QsValue) -> QsValueType {
    if val.is_null() {
        return QsValueType::Undefined;
    }
    let val = &*val;
    match &val.inner {
        Value::Undefined => QsValueType::Undefined,
        Value::Null => QsValueType::Null,
        Value::Boolean(_) => QsValueType::Boolean,
        Value::Number(_) => QsValueType::Number,
        Value::String(_) => QsValueType::String,
        Value::BigInt(_) => QsValueType::BigInt,
        Value::Symbol(_) => QsValueType::Symbol,
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                crate::runtime::ObjectKind::Array(_) => QsValueType::Array,
                crate::runtime::ObjectKind::Function(_)
                | crate::runtime::ObjectKind::NativeFunction { .. } => QsValueType::Function,
                _ => QsValueType::Object,
            }
        }
    }
}

/// Convert a value to a boolean
///
/// # Safety
/// `val` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_value_to_boolean(val: *const QsValue) -> bool {
    if val.is_null() {
        return false;
    }
    (*val).inner.to_boolean()
}

/// Convert a value to a number
///
/// # Safety
/// `val` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_value_to_number(val: *const QsValue) -> f64 {
    if val.is_null() {
        return f64::NAN;
    }
    (*val).inner.to_number()
}

/// Convert a value to a string. Caller must free the returned string with `qs_string_free`.
///
/// # Safety
/// `val` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_value_to_string(val: *const QsValue) -> *mut c_char {
    if val.is_null() {
        return to_c_string("undefined");
    }
    to_c_string(&(*val).inner.to_js_string())
}

/// Check if two values are strictly equal (===)
///
/// # Safety
/// Both pointers must be valid.
#[no_mangle]
pub unsafe extern "C" fn qs_value_strict_equals(a: *const QsValue, b: *const QsValue) -> bool {
    if a.is_null() || b.is_null() {
        return a.is_null() && b.is_null();
    }
    (*a).inner.strict_equals(&(*b).inner)
}

// === Object Operations ===

/// Set a property on an object
///
/// # Safety
/// `obj` must be a valid object value. `key` must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn qs_object_set(
    obj: *mut QsValue,
    key: *const c_char,
    val: *const QsValue,
) -> bool {
    if obj.is_null() || key.is_null() || val.is_null() {
        return false;
    }
    let key_str = match CStr::from_ptr(key).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return false,
    };
    let obj_ref = &mut *obj;
    let val_ref = &*val;
    if let Value::Object(ref o) = obj_ref.inner {
        o.borrow_mut()
            .set_property(&key_str, val_ref.inner.clone());
        true
    } else {
        false
    }
}

/// Get a property from an object
///
/// # Safety
/// `obj` must be a valid object value. `key` must be valid UTF-8.
/// Returned value must be freed with `qs_value_free`.
#[no_mangle]
pub unsafe extern "C" fn qs_object_get(
    obj: *const QsValue,
    key: *const c_char,
) -> *mut QsValue {
    if obj.is_null() || key.is_null() {
        return qs_value_undefined();
    }
    let key_str = match CStr::from_ptr(key).to_str() {
        Ok(s) => s,
        Err(_) => return qs_value_undefined(),
    };
    let obj_ref = &*obj;
    if let Value::Object(ref o) = obj_ref.inner {
        let value = o
            .borrow()
            .get_property(key_str)
            .unwrap_or(Value::Undefined);
        Box::into_raw(Box::new(QsValue { inner: value }))
    } else {
        qs_value_undefined()
    }
}

// === Global Variables ===

/// Set a global variable in the runtime
///
/// # Safety
/// `rt` must be valid. `name` must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn qs_global_set(
    rt: *mut QsRuntime,
    name: *const c_char,
    val: *const QsValue,
) {
    if rt.is_null() || name.is_null() || val.is_null() {
        return;
    }
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    (*rt).inner.set_global(name_str, (*val).inner.clone());
}

/// Get a global variable from the runtime
///
/// # Safety
/// `rt` must be valid. `name` must be valid UTF-8.
/// Returned value must be freed with `qs_value_free`.
#[no_mangle]
pub unsafe extern "C" fn qs_global_get(
    rt: *const QsRuntime,
    name: *const c_char,
) -> *mut QsValue {
    if rt.is_null() || name.is_null() {
        return qs_value_undefined();
    }
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return qs_value_undefined(),
    };
    let value = (*rt)
        .inner
        .get_global(name_str)
        .unwrap_or(Value::Undefined);
    Box::into_raw(Box::new(QsValue { inner: value }))
}

// === Memory Management ===

/// Free a value handle
///
/// # Safety
/// `val` must be a valid pointer returned by a `qs_value_*` function.
#[no_mangle]
pub unsafe extern "C" fn qs_value_free(val: *mut QsValue) {
    if !val.is_null() {
        drop(Box::from_raw(val));
    }
}

/// Free a string returned by the API
///
/// # Safety
/// `s` must be a valid pointer returned by `qs_value_to_string`.
#[no_mangle]
pub unsafe extern "C" fn qs_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

/// Free an error
///
/// # Safety
/// `err` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_error_free(err: *mut QsError) {
    if !err.is_null() {
        let err = &mut *err;
        if !err.message.is_null() {
            drop(CString::from_raw(err.message));
            err.message = ptr::null_mut();
        }
    }
}

// === Version Info ===

/// API version constants for ABI compatibility checking
pub const QS_API_VERSION_MAJOR: u32 = 1;
pub const QS_API_VERSION_MINOR: u32 = 0;
pub const QS_API_VERSION_PATCH: u32 = 0;

/// Get the Quicksilver version string
///
/// The returned string is statically allocated and must NOT be freed.
#[no_mangle]
pub extern "C" fn qs_version() -> *const c_char {
    // Use a static CString to avoid lifetime issues
    static VERSION: std::sync::LazyLock<CString> =
        std::sync::LazyLock::new(|| CString::new(crate::VERSION).unwrap());
    VERSION.as_ptr()
}

/// Get the C API major version for ABI compatibility checks
#[no_mangle]
pub extern "C" fn qs_api_version_major() -> u32 {
    QS_API_VERSION_MAJOR
}

/// Get the C API minor version
#[no_mangle]
pub extern "C" fn qs_api_version_minor() -> u32 {
    QS_API_VERSION_MINOR
}

/// Get the C API patch version
#[no_mangle]
pub extern "C" fn qs_api_version_patch() -> u32 {
    QS_API_VERSION_PATCH
}

/// Check if the runtime is ABI-compatible with the expected version.
/// Returns true if the runtime's major version matches and minor >= expected.
#[no_mangle]
#[allow(clippy::absurd_extreme_comparisons)]
pub extern "C" fn qs_api_compatible(expected_major: u32, expected_minor: u32) -> bool {
    QS_API_VERSION_MAJOR == expected_major && QS_API_VERSION_MINOR >= expected_minor
}

// === Callback Support (Register C functions as JS globals) ===

/// Native callback type: receives argc + argv, returns a value
pub type QsNativeCallback = unsafe extern "C" fn(argc: i32, argv: *const *const QsValue) -> *mut QsValue;

/// Register a native C function as a global JS function
///
/// # Safety
/// `rt` must be valid. `name` must be valid UTF-8. `callback` must be a valid function pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_register_function(
    rt: *mut QsRuntime,
    name: *const c_char,
    callback: QsNativeCallback,
) -> bool {
    if rt.is_null() || name.is_null() {
        return false;
    }
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return false,
    };

    let cb = callback;
    let native_fn = move |args: &[Value]| -> crate::error::Result<Value> {
        // Convert Value args to QsValue pointers
        let qs_args: Vec<*const QsValue> = args
            .iter()
            .map(|v| {
                let boxed = Box::new(QsValue { inner: v.clone() });
                Box::into_raw(boxed) as *const QsValue
            })
            .collect();

        let result = unsafe { cb(qs_args.len() as i32, qs_args.as_ptr()) };

        // Free the arg handles
        for ptr in &qs_args {
            unsafe { qs_value_free(*ptr as *mut QsValue) };
        }

        if result.is_null() {
            Ok(Value::Undefined)
        } else {
            let val = unsafe { Box::from_raw(result) };
            Ok(val.inner)
        }
    };

    (*rt).inner.register_function(&name_str, native_fn);
    true
}

// === Array Operations ===

/// Get the length of an array value. Returns -1 if not an array.
///
/// # Safety
/// `val` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn qs_array_length(val: *const QsValue) -> i32 {
    if val.is_null() {
        return -1;
    }
    match &(*val).inner {
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                crate::runtime::ObjectKind::Array(elements) => elements.len() as i32,
                _ => -1,
            }
        }
        _ => -1,
    }
}

/// Get an element from an array by index. Returns undefined if out of bounds.
///
/// # Safety
/// `val` must be a valid array value. Returned value must be freed.
#[no_mangle]
pub unsafe extern "C" fn qs_array_get(val: *const QsValue, index: i32) -> *mut QsValue {
    if val.is_null() || index < 0 {
        return qs_value_undefined();
    }
    match &(*val).inner {
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                crate::runtime::ObjectKind::Array(elements) => {
                    let v = elements.get(index as usize).cloned().unwrap_or(Value::Undefined);
                    Box::into_raw(Box::new(QsValue { inner: v }))
                }
                _ => qs_value_undefined(),
            }
        }
        _ => qs_value_undefined(),
    }
}

/// Push a value onto an array. Returns the new length, or -1 on error.
///
/// # Safety
/// `arr` must be a valid array value. `val` must be a valid value.
#[no_mangle]
pub unsafe extern "C" fn qs_array_push(arr: *mut QsValue, val: *const QsValue) -> i32 {
    if arr.is_null() || val.is_null() {
        return -1;
    }
    let item = (*val).inner.clone();
    match &(*arr).inner {
        Value::Object(obj) => {
            let mut obj = obj.borrow_mut();
            match &mut obj.kind {
                crate::runtime::ObjectKind::Array(elements) => {
                    elements.push(item);
                    elements.len() as i32
                }
                _ => -1,
            }
        }
        _ => -1,
    }
}

// === Helper Functions ===

fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => {
            // String contains null bytes; replace them
            let cleaned: String = s.replace('\0', "\\0");
            CString::new(cleaned).unwrap().into_raw()
        }
    }
}

unsafe fn set_error(error: *mut QsError, message: &str, code: i32) {
    if !error.is_null() {
        let err = &mut *error;
        err.message = to_c_string(message);
        err.code = code;
    }
}

/// Generate a C header file for the API
pub fn generate_header() -> String {
    r#"/* quicksilver.h - Quicksilver JavaScript Runtime C API */
#ifndef QUICKSILVER_H
#define QUICKSILVER_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* API version constants */
#define QS_API_VERSION_MAJOR 1
#define QS_API_VERSION_MINOR 0
#define QS_API_VERSION_PATCH 0

/* Opaque types */
typedef struct QsRuntime QsRuntime;
typedef struct QsValue QsValue;

/* Error information */
typedef struct {
    char* message;
    int32_t code;
} QsError;

/* Value types */
typedef enum {
    QS_TYPE_UNDEFINED = 0,
    QS_TYPE_NULL = 1,
    QS_TYPE_BOOLEAN = 2,
    QS_TYPE_NUMBER = 3,
    QS_TYPE_STRING = 4,
    QS_TYPE_OBJECT = 5,
    QS_TYPE_ARRAY = 6,
    QS_TYPE_FUNCTION = 7,
    QS_TYPE_BIGINT = 8,
    QS_TYPE_SYMBOL = 9,
} QsValueType;

/* Native callback type for registering C functions */
typedef QsValue* (*QsNativeCallback)(int32_t argc, const QsValue* const* argv);

/* Runtime management */
QsRuntime* qs_runtime_new(void);
void qs_runtime_free(QsRuntime* rt);

/* Evaluation */
QsValue* qs_eval(QsRuntime* rt, const char* source, QsError* error);

/* Value creation */
QsValue* qs_value_undefined(void);
QsValue* qs_value_null(void);
QsValue* qs_value_boolean(bool val);
QsValue* qs_value_number(double val);
QsValue* qs_value_string(const char* val);
QsValue* qs_value_object(void);
QsValue* qs_value_array(void);

/* Value inspection */
QsValueType qs_value_type(const QsValue* val);
bool qs_value_to_boolean(const QsValue* val);
double qs_value_to_number(const QsValue* val);
char* qs_value_to_string(const QsValue* val);
bool qs_value_strict_equals(const QsValue* a, const QsValue* b);

/* Object operations */
bool qs_object_set(QsValue* obj, const char* key, const QsValue* val);
QsValue* qs_object_get(const QsValue* obj, const char* key);

/* Array operations */
int32_t qs_array_length(const QsValue* arr);
QsValue* qs_array_get(const QsValue* arr, int32_t index);
int32_t qs_array_push(QsValue* arr, const QsValue* val);

/* Global variables */
void qs_global_set(QsRuntime* rt, const char* name, const QsValue* val);
QsValue* qs_global_get(const QsRuntime* rt, const char* name);

/* Callback support */
bool qs_register_function(QsRuntime* rt, const char* name, QsNativeCallback callback);

/* Memory management */
void qs_value_free(QsValue* val);
void qs_string_free(char* s);
void qs_error_free(QsError* err);

/* Version */
const char* qs_version(void);
uint32_t qs_api_version_major(void);
uint32_t qs_api_version_minor(void);
uint32_t qs_api_version_patch(void);
bool qs_api_compatible(uint32_t expected_major, uint32_t expected_minor);

#ifdef __cplusplus
}
#endif

#endif /* QUICKSILVER_H */
"#
    .to_string()
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_lifecycle() {
        let rt = qs_runtime_new();
        assert!(!rt.is_null());
        unsafe { qs_runtime_free(rt) };
    }

    #[test]
    fn test_eval_success() {
        let rt = qs_runtime_new();
        let source = CString::new("1 + 2").unwrap();
        let mut error = QsError {
            message: ptr::null_mut(),
            code: 0,
        };
        unsafe {
            let result = qs_eval(rt, source.as_ptr(), &mut error);
            assert!(!result.is_null());
            assert_eq!(qs_value_to_number(result), 3.0);
            assert_eq!(qs_value_type(result), QsValueType::Number);
            qs_value_free(result);
            qs_runtime_free(rt);
        }
    }

    #[test]
    fn test_eval_error() {
        let rt = qs_runtime_new();
        let source = CString::new("throw new Error('test')").unwrap();
        let mut error = QsError {
            message: ptr::null_mut(),
            code: 0,
        };
        unsafe {
            let result = qs_eval(rt, source.as_ptr(), &mut error);
            assert!(result.is_null());
            assert!(!error.message.is_null());
            assert!(error.code != 0);
            qs_error_free(&mut error);
            qs_runtime_free(rt);
        }
    }

    #[test]
    fn test_value_types() {
        unsafe {
            let undef = qs_value_undefined();
            assert_eq!(qs_value_type(undef), QsValueType::Undefined);
            qs_value_free(undef);

            let null = qs_value_null();
            assert_eq!(qs_value_type(null), QsValueType::Null);
            qs_value_free(null);

            let b = qs_value_boolean(true);
            assert_eq!(qs_value_type(b), QsValueType::Boolean);
            assert!(qs_value_to_boolean(b));
            qs_value_free(b);

            let n = qs_value_number(3.14);
            assert_eq!(qs_value_type(n), QsValueType::Number);
            assert_eq!(qs_value_to_number(n), 3.14);
            qs_value_free(n);

            let s = CString::new("hello").unwrap();
            let sv = qs_value_string(s.as_ptr());
            assert_eq!(qs_value_type(sv), QsValueType::String);
            let str_ptr = qs_value_to_string(sv);
            assert_eq!(CStr::from_ptr(str_ptr).to_str().unwrap(), "hello");
            qs_string_free(str_ptr);
            qs_value_free(sv);
        }
    }

    #[test]
    fn test_object_operations() {
        unsafe {
            let obj = qs_value_object();
            let key = CString::new("x").unwrap();
            let val = qs_value_number(42.0);

            assert!(qs_object_set(obj, key.as_ptr(), val));
            let got = qs_object_get(obj, key.as_ptr());
            assert_eq!(qs_value_to_number(got), 42.0);

            qs_value_free(got);
            qs_value_free(val);
            qs_value_free(obj);
        }
    }

    #[test]
    fn test_global_variables() {
        let rt = qs_runtime_new();
        let name = CString::new("myVar").unwrap();
        let val = qs_value_number(99.0);

        unsafe {
            qs_global_set(rt, name.as_ptr(), val);
            let got = qs_global_get(rt, name.as_ptr());
            assert_eq!(qs_value_to_number(got), 99.0);

            qs_value_free(got);
            qs_value_free(val);
            qs_runtime_free(rt);
        }
    }

    #[test]
    fn test_strict_equals() {
        unsafe {
            let a = qs_value_number(42.0);
            let b = qs_value_number(42.0);
            let c = qs_value_number(43.0);

            assert!(qs_value_strict_equals(a, b));
            assert!(!qs_value_strict_equals(a, c));

            qs_value_free(a);
            qs_value_free(b);
            qs_value_free(c);
        }
    }

    #[test]
    fn test_header_generation() {
        let header = generate_header();
        assert!(header.contains("QsRuntime"));
        assert!(header.contains("qs_eval"));
        assert!(header.contains("qs_runtime_new"));
        assert!(header.contains("#ifndef QUICKSILVER_H"));
        assert!(header.contains("qs_api_version_major"));
        assert!(header.contains("qs_register_function"));
        assert!(header.contains("qs_array_length"));
        assert!(header.contains("QS_API_VERSION_MAJOR"));
    }

    #[test]
    fn test_api_versioning() {
        assert_eq!(qs_api_version_major(), 1);
        assert_eq!(qs_api_version_minor(), 0);
        assert_eq!(qs_api_version_patch(), 0);
        assert!(qs_api_compatible(1, 0));
        assert!(!qs_api_compatible(2, 0)); // major mismatch
        assert!(!qs_api_compatible(1, 1)); // minor too high
    }

    #[test]
    fn test_array_operations() {
        unsafe {
            let arr = qs_value_array();
            assert_eq!(qs_array_length(arr), 0);

            let val1 = qs_value_number(10.0);
            let val2 = qs_value_number(20.0);
            assert_eq!(qs_array_push(arr, val1), 1);
            assert_eq!(qs_array_push(arr, val2), 2);
            assert_eq!(qs_array_length(arr), 2);

            let got = qs_array_get(arr, 0);
            assert_eq!(qs_value_to_number(got), 10.0);
            qs_value_free(got);

            let got2 = qs_array_get(arr, 1);
            assert_eq!(qs_value_to_number(got2), 20.0);
            qs_value_free(got2);

            // Out of bounds returns undefined
            let oob = qs_array_get(arr, 99);
            assert_eq!(qs_value_type(oob), QsValueType::Undefined);
            qs_value_free(oob);

            qs_value_free(val1);
            qs_value_free(val2);
            qs_value_free(arr);
        }
    }

    #[test]
    fn test_array_length_non_array() {
        unsafe {
            let obj = qs_value_object();
            assert_eq!(qs_array_length(obj), -1);
            qs_value_free(obj);

            let num = qs_value_number(42.0);
            assert_eq!(qs_array_length(num), -1);
            qs_value_free(num);
        }
    }

    #[test]
    fn test_register_native_callback() {
        unsafe extern "C" fn my_add(argc: i32, argv: *const *const QsValue) -> *mut QsValue {
            if argc < 2 || argv.is_null() {
                return qs_value_undefined();
            }
            unsafe {
                let a = qs_value_to_number(*argv);
                let b = qs_value_to_number(*argv.add(1));
                qs_value_number(a + b)
            }
        }

        let rt = qs_runtime_new();
        let name = CString::new("nativeAdd").unwrap();
        unsafe {
            assert!(qs_register_function(rt, name.as_ptr(), my_add));

            let source = CString::new("nativeAdd(3, 4)").unwrap();
            let mut error = QsError { message: ptr::null_mut(), code: 0 };
            let result = qs_eval(rt, source.as_ptr(), &mut error);
            assert!(!result.is_null());
            assert_eq!(qs_value_to_number(result), 7.0);
            qs_value_free(result);
            qs_runtime_free(rt);
        }
    }
}

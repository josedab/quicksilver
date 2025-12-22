//! String interning for property names
//!
//! This module provides O(1) string comparison for frequently used property names
//! by mapping strings to unique integer IDs.

use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;

/// A globally unique string ID
pub type StringId = u32;

/// Thread-local string interner
pub struct StringInterner {
    /// Map from string to ID
    string_to_id: HashMap<String, StringId>,
    /// Map from ID to string (for reverse lookup)
    id_to_string: Vec<String>,
}

impl StringInterner {
    /// Create a new string interner with pre-seeded common property names
    pub fn new() -> Self {
        let mut interner = Self {
            string_to_id: HashMap::default(),
            id_to_string: Vec::with_capacity(128),
        };

        // Pre-intern common property names for better cache locality
        let common_props = [
            // Object/prototype properties
            "prototype", "constructor", "__proto__", "length", "name",
            // Array methods
            "push", "pop", "shift", "unshift", "slice", "splice", "concat",
            "indexOf", "lastIndexOf", "includes", "find", "findIndex",
            "filter", "map", "reduce", "forEach", "some", "every", "join",
            "reverse", "sort", "fill", "flat", "flatMap", "at", "entries",
            "keys", "values", "toReversed", "toSorted", "toSpliced", "with",
            // String methods
            "charAt", "charCodeAt", "substring", "substr", "split", "trim",
            "toLowerCase", "toUpperCase", "startsWith", "endsWith", "repeat",
            "replace", "replaceAll", "padStart", "padEnd", "match", "search",
            // Object methods
            "hasOwnProperty", "toString", "valueOf", "toJSON",
            // Function properties
            "call", "apply", "bind", "arguments", "caller",
            // Common property names
            "value", "done", "next", "return", "throw",
            "get", "set", "writable", "enumerable", "configurable",
            "message", "stack", "cause",
            // Number properties
            "toFixed", "toPrecision", "toExponential",
            // Promise
            "then", "catch", "finally", "resolve", "reject",
            // Iterator
            "Symbol.iterator", "Symbol.toStringTag",
            // Math
            "PI", "E", "abs", "floor", "ceil", "round", "max", "min",
            "random", "sqrt", "pow", "sin", "cos", "tan", "log", "exp",
            // Console
            "log", "error", "warn", "info", "debug", "table", "time", "timeEnd",
            // JSON
            "parse", "stringify",
            // Array indices 0-9 (common)
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
        ];

        for prop in common_props {
            interner.intern(prop);
        }

        interner
    }

    /// Intern a string, returning its unique ID
    #[inline]
    pub fn intern(&mut self, s: &str) -> StringId {
        if let Some(&id) = self.string_to_id.get(s) {
            return id;
        }

        let id = self.id_to_string.len() as StringId;
        self.id_to_string.push(s.to_string());
        self.string_to_id.insert(s.to_string(), id);
        id
    }

    /// Get the ID for a string if it's already interned
    #[inline]
    pub fn get_id(&self, s: &str) -> Option<StringId> {
        self.string_to_id.get(s).copied()
    }

    /// Get the string for an ID
    #[inline]
    pub fn get_string(&self, id: StringId) -> Option<&str> {
        self.id_to_string.get(id as usize).map(|s| s.as_str())
    }

    /// Check if a string is interned
    #[inline]
    pub fn is_interned(&self, s: &str) -> bool {
        self.string_to_id.contains_key(s)
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// Global string interner instance
    static INTERNER: RefCell<StringInterner> = RefCell::new(StringInterner::new());
}

/// Intern a string using the global interner
#[inline]
pub fn intern(s: &str) -> StringId {
    INTERNER.with(|interner| interner.borrow_mut().intern(s))
}

/// Get the ID for a string if it's already interned
#[inline]
pub fn get_id(s: &str) -> Option<StringId> {
    INTERNER.with(|interner| interner.borrow().get_id(s))
}

/// Get the string for an ID from the global interner
#[inline]
pub fn get_string(id: StringId) -> Option<String> {
    INTERNER.with(|interner| {
        interner.borrow().get_string(id).map(|s| s.to_string())
    })
}

/// Check if two strings are equal using interned IDs when possible
/// This provides O(1) comparison for interned strings
#[inline]
pub fn strings_equal(a: &str, b: &str) -> bool {
    // Fast path: if both strings are interned, compare IDs
    if let (Some(id_a), Some(id_b)) = (get_id(a), get_id(b)) {
        return id_a == id_b;
    }
    // Fall back to string comparison
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_returns_same_id() {
        let id1 = intern("test");
        let id2 = intern("test");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_strings_different_ids() {
        let id1 = intern("foo");
        let id2 = intern("bar");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_get_string() {
        let id = intern("hello");
        assert_eq!(get_string(id), Some("hello".to_string()));
    }

    #[test]
    fn test_strings_equal() {
        intern("test1");
        intern("test2");
        assert!(strings_equal("test1", "test1"));
        assert!(!strings_equal("test1", "test2"));
    }

    #[test]
    fn test_common_properties_preinterned() {
        // Common properties should already be interned
        assert!(get_id("length").is_some());
        assert!(get_id("prototype").is_some());
        assert!(get_id("constructor").is_some());
    }
}

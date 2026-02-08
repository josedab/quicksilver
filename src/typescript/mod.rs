//! TypeScript Support for Quicksilver
//!
//! This module provides native TypeScript execution by implementing a fast
//! type-stripping transpiler. It converts TypeScript to JavaScript by removing
//! type annotations, interfaces, type aliases, and other TypeScript-specific
//! constructs while preserving the JavaScript semantics.
//!
//! # Features
//!
//! - Type annotation stripping (: Type, as Type)
//! - Interface removal
//! - Type alias removal
//! - Enum transpilation (to object literals)
//! - Generic type parameter removal
//! - Access modifier stripping (public, private, protected, readonly)
//! - Namespace/module handling
//!
//! # Example
//!
//! ```no_run
//! use quicksilver::typescript::TypeScriptTranspiler;
//!
//! let ts_code = r#"
//!     interface User {
//!         name: string;
//!         age: number;
//!     }
//!
//!     function greet(user: User): string {
//!         return `Hello, ${user.name}!`;
//!     }
//! "#;
//!
//! let transpiler = TypeScriptTranspiler::new();
//! let js_code = transpiler.transpile(ts_code).unwrap();
//! ```

//! **Status:** ⚠️ Partial — Type stripping transpiler — no type checking

mod parser;
mod transpiler;
mod types;

pub use parser::TypeScriptParser;
pub use transpiler::TypeScriptTranspiler;
pub use types::*;

use crate::error::Result;

/// Configuration options for TypeScript transpilation
#[derive(Debug, Clone)]
pub struct TranspileOptions {
    /// Target JavaScript version (affects feature output)
    pub target: JsTarget,
    /// Whether to preserve const enums or inline them
    pub preserve_const_enums: bool,
    /// Whether to emit decorator metadata
    pub emit_decorator_metadata: bool,
    /// Whether to use define for class fields
    pub use_define_for_class_fields: bool,
    /// Module system to use
    pub module: ModuleSystem,
    /// Source map options
    pub source_map: SourceMapOption,
    /// Whether to strip all comments
    pub remove_comments: bool,
}

impl Default for TranspileOptions {
    fn default() -> Self {
        Self {
            target: JsTarget::ES2020,
            preserve_const_enums: false,
            emit_decorator_metadata: false,
            use_define_for_class_fields: true,
            module: ModuleSystem::ESNext,
            source_map: SourceMapOption::None,
            remove_comments: false,
        }
    }
}

/// JavaScript target version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTarget {
    ES5,
    ES2015,
    ES2016,
    ES2017,
    ES2018,
    ES2019,
    ES2020,
    ES2021,
    ES2022,
    ESNext,
}

/// Module system for output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSystem {
    CommonJS,
    AMD,
    UMD,
    ESNext,
    None,
}

/// Source map generation options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMapOption {
    None,
    Inline,
    External,
}

/// Quick function to transpile TypeScript to JavaScript
pub fn transpile(source: &str) -> Result<String> {
    let transpiler = TypeScriptTranspiler::new();
    transpiler.transpile(source)
}

/// Quick function to transpile TypeScript to JavaScript with options
pub fn transpile_with_options(source: &str, options: TranspileOptions) -> Result<String> {
    let transpiler = TypeScriptTranspiler::with_options(options);
    transpiler.transpile(source)
}

/// Check if a file appears to be TypeScript based on its content
pub fn is_typescript(source: &str) -> bool {
    // Look for TypeScript-specific syntax
    source.contains(": ") && (
        source.contains("interface ") ||
        source.contains("type ") ||
        source.contains(": string") ||
        source.contains(": number") ||
        source.contains(": boolean") ||
        source.contains(": any") ||
        source.contains("as ") ||
        source.contains("<T>") ||
        source.contains(": Array<") ||
        source.contains("public ") ||
        source.contains("private ") ||
        source.contains("protected ") ||
        source.contains("readonly ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_transpile() {
        let ts = "let x: number = 42;";
        let js = transpile(ts).unwrap();
        assert!(js.contains("let x = 42;"));
        assert!(!js.contains(": number"));
    }

    #[test]
    fn test_interface_removal() {
        let ts = r#"
            interface User {
                name: string;
                age: number;
            }
            let user: User = { name: "Alice", age: 30 };
        "#;
        let js = transpile(ts).unwrap();
        assert!(!js.contains("interface"), "Should not contain 'interface', got: {}", js);
        assert!(js.contains("let user"), "Should contain 'let user', got: {}", js);
        assert!(js.contains("name: \"Alice\""), "Should contain 'name: \"Alice\"', got: {}", js);
    }

    #[test]
    fn test_function_types() {
        let ts = "function add(a: number, b: number): number { return a + b; }";
        let js = transpile(ts).unwrap();
        assert!(js.contains("function add(a, b)"), "Expected 'function add(a, b)' in: {}", js);
        assert!(js.contains("return a + b"), "Expected 'return a + b' in: {}", js);
    }

    #[test]
    fn test_is_typescript_detection() {
        assert!(is_typescript("let x: number = 5;"));
        assert!(is_typescript("interface Foo { bar: string }"));
        assert!(!is_typescript("let x = 5;"));
        assert!(!is_typescript("function foo() {}"));
    }
}

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
pub mod tsconfig;
pub mod declarations;

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

    #[test]
    fn test_namespace_simple() {
        let ns = RichNamespaceDeclaration {
            name: "MyNS".to_string(),
            members: vec![
                NamespaceMember::Variable {
                    name: "x".to_string(),
                    value: "42".to_string(),
                    is_exported: true,
                },
                NamespaceMember::Function {
                    name: "greet".to_string(),
                    params: vec!["name".to_string()],
                    body: "return \"Hello, \" + name;".to_string(),
                    is_exported: true,
                },
            ],
            is_exported: false,
        };
        let output = transpile_namespace(&ns);
        assert!(output.contains("var MyNS;"), "Got: {}", output);
        assert!(output.contains("(function (MyNS)"), "Got: {}", output);
        assert!(output.contains("MyNS.x = 42;"), "Got: {}", output);
        assert!(output.contains("function greet(name)"), "Got: {}", output);
        assert!(output.contains("MyNS.greet = greet;"), "Got: {}", output);
        assert!(output.contains("(MyNS || (MyNS = {}));"), "Got: {}", output);
    }

    #[test]
    fn test_namespace_nested() {
        let inner = RichNamespaceDeclaration {
            name: "Inner".to_string(),
            members: vec![NamespaceMember::Variable {
                name: "val".to_string(),
                value: "10".to_string(),
                is_exported: true,
            }],
            is_exported: true,
        };
        let outer = RichNamespaceDeclaration {
            name: "Outer".to_string(),
            members: vec![NamespaceMember::Namespace(inner)],
            is_exported: false,
        };
        let output = transpile_namespace(&outer);
        assert!(output.contains("var Outer;"), "Got: {}", output);
        assert!(output.contains("var Inner;"), "Got: {}", output);
        assert!(output.contains("Inner.val = 10;"), "Got: {}", output);
    }

    #[test]
    fn test_namespace_private_member() {
        let ns = RichNamespaceDeclaration {
            name: "NS".to_string(),
            members: vec![NamespaceMember::Variable {
                name: "secret".to_string(),
                value: "\"hidden\"".to_string(),
                is_exported: false,
            }],
            is_exported: false,
        };
        let output = transpile_namespace(&ns);
        assert!(output.contains("var secret = \"hidden\";"), "Got: {}", output);
        assert!(!output.contains("NS.secret"), "Got: {}", output);
    }

    #[test]
    fn test_namespace_type_only_members_stripped() {
        let ns = RichNamespaceDeclaration {
            name: "Types".to_string(),
            members: vec![
                NamespaceMember::TypeAlias { name: "ID".to_string() },
                NamespaceMember::Interface { name: "Foo".to_string() },
                NamespaceMember::Variable {
                    name: "x".to_string(),
                    value: "1".to_string(),
                    is_exported: true,
                },
            ],
            is_exported: false,
        };
        let output = transpile_namespace(&ns);
        assert!(!output.contains("ID"), "Type alias should be stripped, got: {}", output);
        assert!(!output.contains("Foo"), "Interface should be stripped, got: {}", output);
        assert!(output.contains("Types.x = 1;"), "Got: {}", output);
    }

    #[test]
    fn test_utility_type_names() {
        assert_eq!(UtilityType::Partial("T".to_string()).name(), "Partial");
        assert_eq!(UtilityType::Required("T".to_string()).name(), "Required");
        assert_eq!(UtilityType::Readonly("T".to_string()).name(), "Readonly");
        assert_eq!(UtilityType::Pick("T".to_string(), vec![]).name(), "Pick");
        assert_eq!(UtilityType::Omit("T".to_string(), vec![]).name(), "Omit");
        assert_eq!(UtilityType::Record("K".to_string(), "V".to_string()).name(), "Record");
        assert_eq!(UtilityType::Extract("T".to_string(), "U".to_string()).name(), "Extract");
        assert_eq!(UtilityType::Exclude("T".to_string(), "U".to_string()).name(), "Exclude");
        assert_eq!(UtilityType::ReturnType("F".to_string()).name(), "ReturnType");
        assert_eq!(UtilityType::Parameters("F".to_string()).name(), "Parameters");
        assert_eq!(UtilityType::NonNullable("T".to_string()).name(), "NonNullable");
    }

    #[test]
    fn test_utility_type_descriptions() {
        let partial = UtilityType::Partial("User".to_string());
        assert!(partial.description().contains("optional"));

        let required = UtilityType::Required("User".to_string());
        assert!(required.description().contains("required"));

        let readonly = UtilityType::Readonly("Config".to_string());
        assert!(readonly.description().contains("readonly"));

        let non_null = UtilityType::NonNullable("T".to_string());
        assert!(non_null.description().contains("null"));

        let return_type = UtilityType::ReturnType("F".to_string());
        assert!(return_type.description().contains("return type"));
    }
}

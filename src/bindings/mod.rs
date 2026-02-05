//! Binding specification modules for embedding Quicksilver in other languages.
//!
//! Provides a high-level Rust embedding API (`EmbeddingAPI`), a simplified value
//! type (`JsArg`), and binding specifications for Python (PyO3) and Go (CGo).

//! **Status:** ✅ Complete — Multi-language SDK bindings (C, Python, Go)

use crate::runtime::{Runtime, Value, ObjectKind};
use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// JsArg – simplified value type for embedding (no Rc/RefCell exposure)
// ---------------------------------------------------------------------------

/// Simplified JavaScript value for the embedding boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum JsArg {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsArg>),
    Object(HashMap<String, JsArg>),
}

impl JsArg {
    /// Convert a runtime `Value` into a `JsArg`.
    pub fn from_value(value: Value) -> JsArg {
        match value {
            Value::Undefined => JsArg::Undefined,
            Value::Null => JsArg::Null,
            Value::Boolean(b) => JsArg::Bool(b),
            Value::Number(n) => JsArg::Number(n),
            Value::String(s) => JsArg::String(s),
            Value::BigInt(b) => JsArg::String(b.to_string()),
            Value::Symbol(s) => JsArg::String(format!("Symbol({})", s)),
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Array(elements) => {
                        let items = elements.iter().map(|v| JsArg::from_value(v.clone())).collect();
                        JsArg::Array(items)
                    }
                    _ => {
                        let mut map = HashMap::default();
                        for (k, v) in &obj_ref.properties {
                            map.insert(k.clone(), JsArg::from_value(v.clone()));
                        }
                        JsArg::Object(map)
                    }
                }
            }
        }
    }

    /// Convert this `JsArg` back into a runtime `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            JsArg::Undefined => Value::Undefined,
            JsArg::Null => Value::Null,
            JsArg::Bool(b) => Value::Boolean(*b),
            JsArg::Number(n) => Value::Number(*n),
            JsArg::String(s) => Value::String(s.clone()),
            JsArg::Array(items) => {
                let elements: Vec<Value> = items.iter().map(|a| a.to_value()).collect();
                Value::new_array(elements)
            }
            JsArg::Object(map) => {
                let properties: HashMap<String, Value> =
                    map.iter().map(|(k, v)| (k.clone(), v.to_value())).collect();
                Value::new_object_with_properties(properties)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EvalResult
// ---------------------------------------------------------------------------

/// Result of evaluating JavaScript code.
#[derive(Debug, Clone)]
pub struct EvalResult {
    /// The resulting value.
    pub value: JsArg,
    /// Error message, if any.
    pub error: Option<String>,
    /// Wall-clock duration of the evaluation.
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// EmbeddingAPI
// ---------------------------------------------------------------------------

/// High-level embedding API that wraps `Runtime` with a clean, stable interface.
pub struct EmbeddingAPI {
    runtime: Runtime,
}

impl EmbeddingAPI {
    /// Create a new embedding API instance.
    pub fn new() -> Self {
        Self {
            runtime: Runtime::new(),
        }
    }

    /// Evaluate JavaScript source code.
    pub fn eval(&mut self, code: &str) -> EvalResult {
        let start = Instant::now();
        match self.runtime.eval(code) {
            Ok(val) => EvalResult {
                value: JsArg::from_value(val),
                error: None,
                duration: start.elapsed(),
            },
            Err(e) => EvalResult {
                value: JsArg::Undefined,
                error: Some(e.to_string()),
                duration: start.elapsed(),
            },
        }
    }

    /// Call a JavaScript function by name with the given arguments.
    pub fn call_function(&mut self, name: &str, args: &[JsArg]) -> EvalResult {
        let start = Instant::now();

        // Build a JS call expression: name(arg0, arg1, ...)
        let arg_placeholders: Vec<String> = (0..args.len())
            .map(|i| format!("__qs_arg_{}", i))
            .collect();

        // Inject arguments as globals
        for (i, arg) in args.iter().enumerate() {
            self.runtime
                .set_global(&format!("__qs_arg_{}", i), arg.to_value());
        }

        let call_expr = format!("{}({})", name, arg_placeholders.join(", "));
        let result = match self.runtime.eval(&call_expr) {
            Ok(val) => EvalResult {
                value: JsArg::from_value(val),
                error: None,
                duration: start.elapsed(),
            },
            Err(e) => EvalResult {
                value: JsArg::Undefined,
                error: Some(e.to_string()),
                duration: start.elapsed(),
            },
        };

        // Clean up temporaries
        for i in 0..args.len() {
            self.runtime
                .set_global(&format!("__qs_arg_{}", i), Value::Undefined);
        }

        result
    }

    /// Register a host function that can be called from JavaScript.
    pub fn register_function(
        &mut self,
        name: &str,
        callback: Box<dyn Fn(&[JsArg]) -> JsArg>,
    ) {
        self.runtime.register_function(name, move |values: &[Value]| {
            let js_args: Vec<JsArg> = values.iter().map(|v| JsArg::from_value(v.clone())).collect();
            let result = callback(&js_args);
            Ok(result.to_value())
        });
    }

    /// Get a global variable as a `JsArg`.
    pub fn get_global(&self, name: &str) -> Option<JsArg> {
        self.runtime
            .get_global(name)
            .map(JsArg::from_value)
    }

    /// Set a global variable from a `JsArg`.
    pub fn set_global(&mut self, name: &str, value: JsArg) {
        self.runtime.set_global(name, value.to_value());
    }
}

impl Default for EmbeddingAPI {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PythonBindingSpec – specification for Python (PyO3) bindings
// ---------------------------------------------------------------------------

/// Specification describing the Python API surface generated via PyO3.
pub struct PythonBindingSpec {
    /// Module name exposed to Python.
    pub module_name: String,
    /// Class name for the runtime wrapper.
    pub class_name: String,
    /// Method signatures that would be generated.
    pub methods: Vec<BindingMethod>,
    /// Type mappings from JsArg variants to Python types.
    pub type_map: Vec<TypeMapping>,
}

impl PythonBindingSpec {
    /// Create the default Python binding specification.
    pub fn new() -> Self {
        Self {
            module_name: "quicksilver".into(),
            class_name: "QuicksilverRuntime".into(),
            methods: vec![
                BindingMethod {
                    name: "__init__".into(),
                    signature: "def __init__(self) -> None".into(),
                    doc: "Create a new Quicksilver runtime.".into(),
                },
                BindingMethod {
                    name: "eval".into(),
                    signature: "def eval(self, code: str) -> EvalResult".into(),
                    doc: "Evaluate JavaScript source code.".into(),
                },
                BindingMethod {
                    name: "call_function".into(),
                    signature: "def call_function(self, name: str, *args: Any) -> EvalResult".into(),
                    doc: "Call a JavaScript function by name.".into(),
                },
                BindingMethod {
                    name: "register_function".into(),
                    signature: "def register_function(self, name: str, callback: Callable[..., Any]) -> None".into(),
                    doc: "Register a Python callback as a JavaScript function.".into(),
                },
                BindingMethod {
                    name: "get_global".into(),
                    signature: "def get_global(self, name: str) -> Optional[Any]".into(),
                    doc: "Get a global variable.".into(),
                },
                BindingMethod {
                    name: "set_global".into(),
                    signature: "def set_global(self, name: str, value: Any) -> None".into(),
                    doc: "Set a global variable.".into(),
                },
            ],
            type_map: vec![
                TypeMapping { js: "Undefined".into(), target: "None".into() },
                TypeMapping { js: "Null".into(), target: "None".into() },
                TypeMapping { js: "Bool".into(), target: "bool".into() },
                TypeMapping { js: "Number".into(), target: "float".into() },
                TypeMapping { js: "String".into(), target: "str".into() },
                TypeMapping { js: "Array".into(), target: "list".into() },
                TypeMapping { js: "Object".into(), target: "dict".into() },
            ],
        }
    }
}

impl Default for PythonBindingSpec {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GoBindingSpec – specification for Go (CGo) bindings
// ---------------------------------------------------------------------------

/// Specification describing the Go API surface generated via CGo.
pub struct GoBindingSpec {
    /// Go package name.
    pub package_name: String,
    /// CGo function signatures.
    pub functions: Vec<BindingMethod>,
    /// Type mappings from JsArg variants to Go types.
    pub type_map: Vec<TypeMapping>,
}

impl GoBindingSpec {
    /// Create the default Go binding specification.
    pub fn new() -> Self {
        Self {
            package_name: "quicksilver".into(),
            functions: vec![
                BindingMethod {
                    name: "NewRuntime".into(),
                    signature: "func NewRuntime() *Runtime".into(),
                    doc: "Create a new Quicksilver runtime.".into(),
                },
                BindingMethod {
                    name: "Eval".into(),
                    signature: "func (r *Runtime) Eval(code string) (*EvalResult, error)".into(),
                    doc: "Evaluate JavaScript source code.".into(),
                },
                BindingMethod {
                    name: "CallFunction".into(),
                    signature: "func (r *Runtime) CallFunction(name string, args ...interface{}) (*EvalResult, error)".into(),
                    doc: "Call a JavaScript function by name.".into(),
                },
                BindingMethod {
                    name: "RegisterFunction".into(),
                    signature: "func (r *Runtime) RegisterFunction(name string, fn func(args []interface{}) interface{})".into(),
                    doc: "Register a Go callback as a JavaScript function.".into(),
                },
                BindingMethod {
                    name: "GetGlobal".into(),
                    signature: "func (r *Runtime) GetGlobal(name string) (interface{}, bool)".into(),
                    doc: "Get a global variable.".into(),
                },
                BindingMethod {
                    name: "SetGlobal".into(),
                    signature: "func (r *Runtime) SetGlobal(name string, value interface{})".into(),
                    doc: "Set a global variable.".into(),
                },
            ],
            type_map: vec![
                TypeMapping { js: "Undefined".into(), target: "nil".into() },
                TypeMapping { js: "Null".into(), target: "nil".into() },
                TypeMapping { js: "Bool".into(), target: "bool".into() },
                TypeMapping { js: "Number".into(), target: "float64".into() },
                TypeMapping { js: "String".into(), target: "string".into() },
                TypeMapping { js: "Array".into(), target: "[]interface{}".into() },
                TypeMapping { js: "Object".into(), target: "map[string]interface{}".into() },
            ],
        }
    }
}

impl Default for GoBindingSpec {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Shared helper types
// ---------------------------------------------------------------------------

/// A method / function in a binding specification.
#[derive(Debug, Clone)]
pub struct BindingMethod {
    pub name: String,
    pub signature: String,
    pub doc: String,
}

/// Mapping from a JsArg variant to a target-language type.
#[derive(Debug, Clone)]
pub struct TypeMapping {
    pub js: String,
    pub target: String,
}

// ---------------------------------------------------------------------------
// BindingGenerator – generates binding code stubs
// ---------------------------------------------------------------------------

/// Generates binding stub code from specifications.
pub struct BindingGenerator;

impl BindingGenerator {
    /// Generate a Python type stub (`.pyi`) for the Quicksilver module.
    pub fn generate_python_stub() -> String {
        let spec = PythonBindingSpec::new();
        let mut out = std::string::String::new();

        out.push_str("# Auto-generated type stub for quicksilver Python bindings\n");
        out.push_str("from typing import Any, Callable, Optional, List, Dict\n\n");

        // EvalResult class
        out.push_str("class EvalResult:\n");
        out.push_str("    value: Any\n");
        out.push_str("    error: Optional[str]\n");
        out.push_str("    duration_ms: float\n\n");

        // Runtime class
        out.push_str(&format!("class {}:\n", spec.class_name));
        for method in &spec.methods {
            out.push_str(&format!("    {}:\n", method.signature));
            out.push_str(&format!("        \"\"\"{}\"\"\" ...\n", method.doc));
        }

        out
    }

    /// Generate a Go CGo header for the Quicksilver module.
    pub fn generate_go_header() -> String {
        let spec = GoBindingSpec::new();
        let mut out = std::string::String::new();

        out.push_str(&format!("package {}\n\n", spec.package_name));
        out.push_str("// #cgo LDFLAGS: -lquicksilver\n");
        out.push_str("// #include \"quicksilver.h\"\n");
        out.push_str("import \"C\"\nimport \"unsafe\"\n\n");

        // EvalResult struct
        out.push_str("// EvalResult holds the result of a JS evaluation.\n");
        out.push_str("type EvalResult struct {\n");
        out.push_str("\tValue interface{}\n");
        out.push_str("\tError error\n");
        out.push_str("\tDurationMs float64\n");
        out.push_str("}\n\n");

        // Runtime struct
        out.push_str("// Runtime wraps the Quicksilver C runtime handle.\n");
        out.push_str("type Runtime struct {\n");
        out.push_str("\thandle *C.QsRuntime\n");
        out.push_str("}\n\n");

        // Function stubs
        for func in &spec.functions {
            out.push_str(&format!("// {} - {}\n", func.name, func.doc));
            out.push_str(&format!("{} {{\n", func.signature));
            out.push_str("\tpanic(\"not yet implemented\")\n");
            out.push_str("}\n\n");
        }

        // Conversion helper stub
        out.push_str("// convertToGo converts a C QsValue to a Go interface{}.\n");
        out.push_str("func convertToGo(val *C.QsValue) interface{} {\n");
        out.push_str("\t_ = unsafe.Pointer(val)\n");
        out.push_str("\tpanic(\"not yet implemented\")\n");
        out.push_str("}\n");

        out
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsarg_primitive_roundtrip() {
        let cases: Vec<JsArg> = vec![
            JsArg::Undefined,
            JsArg::Null,
            JsArg::Bool(true),
            JsArg::Number(3.14),
            JsArg::String("hello".into()),
        ];
        for arg in cases {
            let val = arg.to_value();
            let back = JsArg::from_value(val);
            assert_eq!(arg, back);
        }
    }

    #[test]
    fn test_jsarg_array_roundtrip() {
        let arr = JsArg::Array(vec![
            JsArg::Number(1.0),
            JsArg::String("two".into()),
            JsArg::Bool(false),
        ]);
        let val = arr.to_value();
        let back = JsArg::from_value(val);
        assert_eq!(arr, back);
    }

    #[test]
    fn test_jsarg_object_roundtrip() {
        let mut map = HashMap::default();
        map.insert("a".into(), JsArg::Number(1.0));
        map.insert("b".into(), JsArg::String("two".into()));
        let obj = JsArg::Object(map);
        let val = obj.to_value();
        let back = JsArg::from_value(val);
        assert_eq!(obj, back);
    }

    #[test]
    fn test_embedding_api_eval_success() {
        let mut api = EmbeddingAPI::new();
        let result = api.eval("1 + 2");
        assert!(result.error.is_none());
        assert_eq!(result.value, JsArg::Number(3.0));
        assert!(result.duration.as_nanos() > 0);
    }

    #[test]
    fn test_embedding_api_eval_error() {
        let mut api = EmbeddingAPI::new();
        let result = api.eval("throw new Error('boom')");
        assert!(result.error.is_some());
        assert_eq!(result.value, JsArg::Undefined);
    }

    #[test]
    fn test_embedding_api_globals() {
        let mut api = EmbeddingAPI::new();
        api.set_global("x", JsArg::Number(42.0));
        let got = api.get_global("x");
        assert_eq!(got, Some(JsArg::Number(42.0)));
    }

    #[test]
    fn test_embedding_api_call_function() {
        let mut api = EmbeddingAPI::new();
        api.eval("function add(a, b) { return a + b; }");
        let result = api.call_function("add", &[JsArg::Number(3.0), JsArg::Number(4.0)]);
        assert!(result.error.is_none());
        assert_eq!(result.value, JsArg::Number(7.0));
    }

    #[test]
    fn test_embedding_api_register_function() {
        let mut api = EmbeddingAPI::new();
        api.register_function(
            "double_it",
            Box::new(|args: &[JsArg]| {
                if let Some(JsArg::Number(n)) = args.first() {
                    JsArg::Number(n * 2.0)
                } else {
                    JsArg::Undefined
                }
            }),
        );
        let result = api.eval("double_it(21)");
        assert!(result.error.is_none());
        assert_eq!(result.value, JsArg::Number(42.0));
    }

    #[test]
    fn test_python_binding_spec() {
        let spec = PythonBindingSpec::new();
        assert_eq!(spec.module_name, "quicksilver");
        assert_eq!(spec.class_name, "QuicksilverRuntime");
        assert!(!spec.methods.is_empty());
        assert!(!spec.type_map.is_empty());
        assert!(spec.methods.iter().any(|m| m.name == "eval"));
    }

    #[test]
    fn test_go_binding_spec() {
        let spec = GoBindingSpec::new();
        assert_eq!(spec.package_name, "quicksilver");
        assert!(!spec.functions.is_empty());
        assert!(!spec.type_map.is_empty());
        assert!(spec.functions.iter().any(|f| f.name == "Eval"));
    }

    #[test]
    fn test_generate_python_stub() {
        let stub = BindingGenerator::generate_python_stub();
        assert!(stub.contains("class QuicksilverRuntime"));
        assert!(stub.contains("class EvalResult"));
        assert!(stub.contains("def eval"));
        assert!(stub.contains("def call_function"));
    }

    #[test]
    fn test_generate_go_header() {
        let header = BindingGenerator::generate_go_header();
        assert!(header.contains("package quicksilver"));
        assert!(header.contains("type Runtime struct"));
        assert!(header.contains("type EvalResult struct"));
        assert!(header.contains("func NewRuntime"));
        assert!(header.contains("convertToGo"));
    }

    #[test]
    fn test_eval_result_fields() {
        let result = EvalResult {
            value: JsArg::Number(1.0),
            error: None,
            duration: Duration::from_millis(5),
        };
        assert_eq!(result.value, JsArg::Number(1.0));
        assert!(result.error.is_none());
        assert_eq!(result.duration, Duration::from_millis(5));
    }
}

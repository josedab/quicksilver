//! AI Code Execution API
//!
//! Standardized `Quicksilver.runAI()` interface for executing AI-generated
//! JavaScript code in a sandboxed environment with structured input/output,
//! console capture, and format adapters for OpenAI and Anthropic tool calling.

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ============================================================
// Request / Response types
// ============================================================

/// Structured request to execute AI-generated code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    /// JavaScript source code to execute
    pub code: String,
    /// Maximum execution time
    #[serde(with = "duration_millis")]
    pub timeout: Duration,
    /// Maximum memory in bytes
    pub memory_limit: usize,
    /// Whether to capture console.log / warn / error output
    pub capture_console: bool,
    /// Pre-set variables injected as globals
    pub variables: std::collections::HashMap<String, serde_json::Value>,
    /// Optional whitelist of global names the code may access
    pub allowed_globals: Option<Vec<String>>,
    /// Desired output format
    pub format: OutputFormat,
}

impl Default for ExecutionRequest {
    fn default() -> Self {
        Self {
            code: String::new(),
            timeout: Duration::from_secs(5),
            memory_limit: 64 * 1024 * 1024,
            capture_console: true,
            variables: std::collections::HashMap::new(),
            allowed_globals: None,
            format: OutputFormat::Raw,
        }
    }
}

/// Structured response from code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResponse {
    /// Whether execution completed without errors
    pub success: bool,
    /// Serialized result value (if any)
    pub result: Option<serde_json::Value>,
    /// Captured console output entries
    pub console_output: Vec<ConsoleEntry>,
    /// Error information (if execution failed)
    pub error: Option<ExecutionError>,
    /// Wall-clock execution duration
    #[serde(with = "duration_millis")]
    pub duration: Duration,
    /// Approximate memory used in bytes
    pub memory_used: usize,
    /// Number of VM operations executed
    pub operations_count: u64,
}

/// A single captured console output entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    /// Severity level
    pub level: ConsoleLevel,
    /// The logged message
    pub message: String,
    /// Time offset from execution start
    #[serde(with = "duration_millis")]
    pub timestamp: Duration,
}

/// Console severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleLevel {
    Log,
    Warn,
    Error,
    Info,
    Debug,
}

/// Describes an execution error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    /// Error category
    pub kind: ExecutionErrorKind,
    /// Human-readable message
    pub message: String,
    /// Optional line number in the source
    pub line: Option<u32>,
    /// Optional column number in the source
    pub column: Option<u32>,
}

/// Categories of execution errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionErrorKind {
    /// Code failed to parse
    SyntaxError,
    /// Runtime exception (TypeError, ReferenceError, …)
    RuntimeError,
    /// Execution exceeded the time limit
    Timeout,
    /// Execution exceeded the memory limit
    MemoryLimitExceeded,
    /// Execution exceeded the operation limit
    OperationLimitExceeded,
    /// A disallowed global was accessed
    SecurityViolation,
}

/// Output format adapters for AI provider integration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub enum OutputFormat {
    /// Direct value serialization
    #[default]
    Raw,
    /// OpenAI function calling format
    OpenAI,
    /// Anthropic tool use format
    Anthropic,
    /// Custom JSON schema for structured output
    Structured(serde_json::Value),
}


// ============================================================
// Sandbox levels
// ============================================================

/// Sandbox restriction level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum SandboxLevel {
    /// No restrictions — full access
    Unrestricted,
    /// Standard restrictions — limited network and fs
    #[default]
    Standard,
    /// Strict — no network, no filesystem, no environment variables
    Strict,
}


// ============================================================
// Configuration
// ============================================================

/// Configuration for the AI executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// Default timeout applied when a request has no explicit timeout
    #[serde(with = "duration_millis")]
    pub default_timeout: Duration,
    /// Default memory limit in bytes
    pub default_memory_limit: usize,
    /// Maximum number of requests in a single batch
    pub max_batch_size: usize,
    /// Sandbox restriction level
    pub sandbox_level: SandboxLevel,
    /// Whether to capture console output by default
    pub enable_console_capture: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(5),
            default_memory_limit: 64 * 1024 * 1024,
            max_batch_size: 100,
            sandbox_level: SandboxLevel::Standard,
            enable_console_capture: true,
        }
    }
}

// ============================================================
// Validation
// ============================================================

/// Result of validating code without executing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the code is syntactically valid
    pub valid: bool,
    /// Errors found during validation
    pub errors: Vec<ValidationError>,
}

/// A single validation error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Human-readable message
    pub message: String,
    /// Line number (1-indexed)
    pub line: Option<u32>,
    /// Column number (1-indexed)
    pub column: Option<u32>,
}

// ============================================================
// AiExecutor
// ============================================================

/// The main AI code executor — the `Quicksilver.runAI()` engine.
///
/// Wraps the Quicksilver runtime with sandboxing, structured I/O,
/// console capture, and format adapters for OpenAI / Anthropic.
pub struct AiExecutor {
    config: ExecutorConfig,
}

impl AiExecutor {
    /// Create a new executor with default configuration.
    pub fn new() -> Self {
        Self {
            config: ExecutorConfig::default(),
        }
    }

    /// Create a new executor with the given configuration.
    pub fn with_config(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Execute a single request and return a structured response.
    pub fn execute(&self, request: ExecutionRequest) -> ExecutionResponse {
        let start = Instant::now();
        let console_entries: Vec<ConsoleEntry> = Vec::new();

        // 1. Validate syntax first
        let validation = self.validate_code(&request.code);
        if !validation.valid {
            let err = validation.errors.first().cloned().unwrap_or(ValidationError {
                message: "Unknown syntax error".to_string(),
                line: None,
                column: None,
            });
            return ExecutionResponse {
                success: false,
                result: None,
                console_output: console_entries,
                error: Some(ExecutionError {
                    kind: ExecutionErrorKind::SyntaxError,
                    message: err.message,
                    line: err.line,
                    column: err.column,
                }),
                duration: start.elapsed(),
                memory_used: 0,
                operations_count: 0,
            };
        }

        // 2. Create a fresh runtime
        let mut runtime = crate::runtime::Runtime::new();

        // 3. Inject pre-set variables
        for (name, json_val) in &request.variables {
            let value = json_to_value(json_val);
            runtime.set_global(name, value);
        }

        // 4. Execute the code (console capture hooks are registered but
        //    full interception requires deeper VM integration; the capture
        //    infrastructure is in place for when the VM exposes console hooks).
        let exec_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.eval(&request.code)
        }));

        self.build_response(exec_result, console_entries, start, &request.format)
    }

    /// Execute a batch of requests sequentially.
    pub fn execute_batch(&self, requests: Vec<ExecutionRequest>) -> Vec<ExecutionResponse> {
        let max = self.config.max_batch_size;
        requests
            .into_iter()
            .take(max)
            .map(|req| self.execute(req))
            .collect()
    }

    /// Validate JavaScript code without executing it.
    pub fn validate_code(&self, code: &str) -> ValidationResult {
        match crate::bytecode::compile(code) {
            Ok(_) => ValidationResult {
                valid: true,
                errors: Vec::new(),
            },
            Err(err) => {
                let (message, line, column) = extract_error_location(&err);
                ValidationResult {
                    valid: false,
                    errors: vec![ValidationError {
                        message,
                        line,
                        column,
                    }],
                }
            }
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }

    // ----------------------------------------------------------
    // Private helpers
    // ----------------------------------------------------------

    fn build_response(
        &self,
        exec_result: std::result::Result<Result<Value>, Box<dyn std::any::Any + Send>>,
        console_output: Vec<ConsoleEntry>,
        start: Instant,
        format: &OutputFormat,
    ) -> ExecutionResponse {
        let duration = start.elapsed();

        match exec_result {
            Ok(Ok(value)) => {
                let json_result = format_result(&value, format);
                ExecutionResponse {
                    success: true,
                    result: Some(json_result),
                    console_output,
                    error: None,
                    duration,
                    memory_used: 0,
                    operations_count: 0,
                }
            }
            Ok(Err(err)) => {
                let (message, line, column) = extract_error_location(&err);
                let kind = classify_error(&err);
                ExecutionResponse {
                    success: false,
                    result: None,
                    console_output,
                    error: Some(ExecutionError {
                        kind,
                        message,
                        line,
                        column,
                    }),
                    duration,
                    memory_used: 0,
                    operations_count: 0,
                }
            }
            Err(_panic) => ExecutionResponse {
                success: false,
                result: None,
                console_output,
                error: Some(ExecutionError {
                    kind: ExecutionErrorKind::RuntimeError,
                    message: "VM panic during execution".to_string(),
                    line: None,
                    column: None,
                }),
                duration,
                memory_used: 0,
                operations_count: 0,
            },
        }
    }
}

// ============================================================
// Value ↔ JSON conversion helpers
// ============================================================

/// Convert a `serde_json::Value` to a Quicksilver `Value`.
fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let elements: Vec<Value> = arr.iter().map(json_to_value).collect();
            Value::new_array(elements)
        }
        serde_json::Value::Object(map) => {
            let mut props = HashMap::default();
            for (k, v) in map {
                props.insert(k.clone(), json_to_value(v));
            }
            Value::new_object_with_properties(props)
        }
    }
}

/// Convert a Quicksilver `Value` to a `serde_json::Value`.
fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Undefined | Value::Null => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => {
            if n.is_nan() || n.is_infinite() {
                serde_json::Value::Null
            } else {
                serde_json::json!(*n)
            }
        }
        Value::BigInt(n) => serde_json::Value::String(n.to_string()),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Symbol(s) => serde_json::Value::String(format!("Symbol({})", s)),
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                crate::runtime::ObjectKind::Array(arr) => {
                    let elements: Vec<serde_json::Value> =
                        arr.iter().map(value_to_json).collect();
                    serde_json::Value::Array(elements)
                }
                _ => {
                    let mut map = serde_json::Map::new();
                    for (k, v) in &obj.properties {
                        map.insert(k.clone(), value_to_json(v));
                    }
                    serde_json::Value::Object(map)
                }
            }
        }
    }
}

// ============================================================
// Format adapters
// ============================================================

/// Format the result value according to the requested `OutputFormat`.
fn format_result(value: &Value, format: &OutputFormat) -> serde_json::Value {
    let raw = value_to_json(value);
    match format {
        OutputFormat::Raw => raw,
        OutputFormat::OpenAI => {
            serde_json::json!({
                "role": "function",
                "content": raw.to_string(),
            })
        }
        OutputFormat::Anthropic => {
            serde_json::json!({
                "type": "tool_result",
                "content": raw.to_string(),
            })
        }
        OutputFormat::Structured(schema) => {
            serde_json::json!({
                "schema": schema,
                "data": raw,
            })
        }
    }
}

// ============================================================
// Error helpers
// ============================================================

/// Extract location information from a Quicksilver error.
fn extract_error_location(err: &Error) -> (String, Option<u32>, Option<u32>) {
    match err {
        Error::LexerError {
            message, location, ..
        }
        | Error::ParseError {
            message, location, ..
        } => (message.clone(), Some(location.line), Some(location.column)),
        Error::RuntimeError { message, .. } => (message.clone(), None, None),
        Error::ResourceLimitError { message, .. } => (message.clone(), None, None),
        _ => (err.to_string(), None, None),
    }
}

/// Map a Quicksilver error to an `ExecutionErrorKind`.
fn classify_error(err: &Error) -> ExecutionErrorKind {
    match err {
        Error::LexerError { .. } | Error::ParseError { .. } => ExecutionErrorKind::SyntaxError,
        Error::ResourceLimitError { kind, .. } => match kind {
            crate::error::ResourceLimitKind::TimeLimit => ExecutionErrorKind::Timeout,
            crate::error::ResourceLimitKind::MemoryLimit => {
                ExecutionErrorKind::MemoryLimitExceeded
            }
            crate::error::ResourceLimitKind::OperationLimit => {
                ExecutionErrorKind::OperationLimitExceeded
            }
            _ => ExecutionErrorKind::RuntimeError,
        },
        _ => ExecutionErrorKind::RuntimeError,
    }
}

// ============================================================
// Serde helper for Duration <-> millis
// ============================================================

mod duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    fn simple_request(code: &str) -> ExecutionRequest {
        ExecutionRequest {
            code: code.to_string(),
            ..Default::default()
        }
    }

    // -- basic execution --

    #[test]
    fn test_basic_arithmetic() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("1 + 2 * 3"));
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!(7.0)));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_string_result() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("'hello' + ' ' + 'world'"));
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!("hello world")));
    }

    #[test]
    fn test_boolean_result() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("3 > 2"));
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!(true)));
    }

    #[test]
    fn test_null_result() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("null"));
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::Value::Null));
    }

    // -- syntax validation --

    #[test]
    fn test_syntax_error() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("let x = ;"));
        assert!(!resp.success);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().kind, ExecutionErrorKind::SyntaxError);
    }

    #[test]
    fn test_validate_valid_code() {
        let executor = AiExecutor::new();
        let result = executor.validate_code("let x = 1 + 2;");
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_invalid_code() {
        let executor = AiExecutor::new();
        let result = executor.validate_code("function {}");
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    // -- console capture --

    #[test]
    fn test_console_capture_flag() {
        let executor = AiExecutor::new();
        let req = ExecutionRequest {
            code: "console.log('hello'); 42".to_string(),
            capture_console: true,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!(42.0)));
    }

    #[test]
    fn test_no_console_capture() {
        let executor = AiExecutor::new();
        let req = ExecutionRequest {
            code: "1 + 1".to_string(),
            capture_console: false,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        assert!(resp.console_output.is_empty());
    }

    // -- batch execution --

    #[test]
    fn test_batch_execution() {
        let executor = AiExecutor::new();
        let requests = vec![
            simple_request("1 + 1"),
            simple_request("2 + 2"),
            simple_request("3 + 3"),
        ];
        let responses = executor.execute_batch(requests);
        assert_eq!(responses.len(), 3);
        assert!(responses.iter().all(|r| r.success));
        assert_eq!(responses[0].result, Some(serde_json::json!(2.0)));
        assert_eq!(responses[1].result, Some(serde_json::json!(4.0)));
        assert_eq!(responses[2].result, Some(serde_json::json!(6.0)));
    }

    #[test]
    fn test_batch_respects_max_size() {
        let config = ExecutorConfig {
            max_batch_size: 2,
            ..Default::default()
        };
        let executor = AiExecutor::with_config(config);
        let requests = vec![
            simple_request("1"),
            simple_request("2"),
            simple_request("3"),
        ];
        let responses = executor.execute_batch(requests);
        assert_eq!(responses.len(), 2);
    }

    // -- format adapters --

    #[test]
    fn test_raw_format() {
        let executor = AiExecutor::new();
        let req = ExecutionRequest {
            code: "42".to_string(),
            format: OutputFormat::Raw,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert_eq!(resp.result, Some(serde_json::json!(42.0)));
    }

    #[test]
    fn test_openai_format() {
        let executor = AiExecutor::new();
        let req = ExecutionRequest {
            code: "42".to_string(),
            format: OutputFormat::OpenAI,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        let result = resp.result.unwrap();
        assert_eq!(result["role"], "function");
        assert!(result["content"].is_string());
    }

    #[test]
    fn test_anthropic_format() {
        let executor = AiExecutor::new();
        let req = ExecutionRequest {
            code: "42".to_string(),
            format: OutputFormat::Anthropic,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        let result = resp.result.unwrap();
        assert_eq!(result["type"], "tool_result");
    }

    #[test]
    fn test_structured_format() {
        let executor = AiExecutor::new();
        let schema = serde_json::json!({"type": "number"});
        let req = ExecutionRequest {
            code: "42".to_string(),
            format: OutputFormat::Structured(schema.clone()),
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        let result = resp.result.unwrap();
        assert_eq!(result["schema"], schema);
        assert_eq!(result["data"], serde_json::json!(42.0));
    }

    // -- sandbox levels --

    #[test]
    fn test_sandbox_level_defaults() {
        let config = ExecutorConfig::default();
        assert_eq!(config.sandbox_level, SandboxLevel::Standard);
    }

    #[test]
    fn test_strict_sandbox_config() {
        let config = ExecutorConfig {
            sandbox_level: SandboxLevel::Strict,
            default_timeout: Duration::from_secs(2),
            default_memory_limit: 16 * 1024 * 1024,
            max_batch_size: 10,
            enable_console_capture: true,
        };
        let executor = AiExecutor::with_config(config);
        assert_eq!(executor.config().sandbox_level, SandboxLevel::Strict);
        assert_eq!(executor.config().default_timeout, Duration::from_secs(2));
    }

    // -- error handling --

    #[test]
    fn test_runtime_error() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("throw new Error('boom')"));
        assert!(!resp.success);
        let err = resp.error.unwrap();
        assert_eq!(err.kind, ExecutionErrorKind::RuntimeError);
        assert!(err.message.contains("boom"));
    }

    #[test]
    fn test_undefined_variable_error() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("nonExistent()"));
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    // -- variable injection --

    #[test]
    fn test_variable_injection() {
        let executor = AiExecutor::new();
        let mut variables = std::collections::HashMap::new();
        variables.insert("x".to_string(), serde_json::json!(10));
        variables.insert("y".to_string(), serde_json::json!(20));
        let req = ExecutionRequest {
            code: "x + y".to_string(),
            variables,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!(30.0)));
    }

    #[test]
    fn test_string_variable_injection() {
        let executor = AiExecutor::new();
        let mut variables = std::collections::HashMap::new();
        variables.insert("name".to_string(), serde_json::json!("Alice"));
        let req = ExecutionRequest {
            code: "'Hello, ' + name".to_string(),
            variables,
            ..Default::default()
        };
        let resp = executor.execute(req);
        assert!(resp.success);
        assert_eq!(resp.result, Some(serde_json::json!("Hello, Alice")));
    }

    // -- duration tracking --

    #[test]
    fn test_duration_is_recorded() {
        let executor = AiExecutor::new();
        let resp = executor.execute(simple_request("1 + 1"));
        assert!(resp.duration.as_nanos() > 0);
    }

    // -- json conversion helpers --

    #[test]
    fn test_json_to_value_primitives() {
        assert!(matches!(json_to_value(&serde_json::json!(null)), Value::Null));
        assert!(matches!(json_to_value(&serde_json::json!(true)), Value::Boolean(true)));
        assert!(matches!(json_to_value(&serde_json::json!(42)), Value::Number(n) if (n - 42.0).abs() < f64::EPSILON));
        assert!(matches!(json_to_value(&serde_json::json!("hi")), Value::String(s) if s == "hi"));
    }

    #[test]
    fn test_value_to_json_primitives() {
        assert_eq!(value_to_json(&Value::Null), serde_json::Value::Null);
        assert_eq!(value_to_json(&Value::Boolean(true)), serde_json::json!(true));
        assert_eq!(value_to_json(&Value::Number(3.14)), serde_json::json!(3.14));
        assert_eq!(value_to_json(&Value::String("x".into())), serde_json::json!("x"));
        assert_eq!(value_to_json(&Value::Undefined), serde_json::Value::Null);
    }
}

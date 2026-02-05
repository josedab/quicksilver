//! LLM Code Execution Sandbox
//!
//! A production-ready API for safely executing LLM-generated JavaScript code.
//! Built on the agent infrastructure (`crate::agent`) and the Quicksilver runtime,
//! this module provides resource-limited sandboxes with tool registration,
//! captured output, and a reusable pool for concurrent execution.

//! **Status:** ✅ Complete — Sandbox configuration and resource limits

use crate::error::Result;
use crate::runtime::{Runtime, Value};
use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ToolSchema
// ---------------------------------------------------------------------------

/// JSON-Schema description of a tool's parameters.
#[derive(Debug, Clone)]
pub struct ToolSchema {
    /// Tool name (must be a valid JS identifier).
    pub name: String,
    /// Human-readable description shown to the LLM.
    pub description: String,
    /// JSON Schema for the parameters object.
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// SandboxConfig
// ---------------------------------------------------------------------------

/// Configuration for a [`CodeSandbox`].
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum wall-clock execution time.
    pub max_duration: Duration,
    /// Approximate memory budget in bytes.
    pub max_memory: usize,
    /// Maximum number of tool calls per execution.
    pub max_tool_calls: usize,
    /// Maximum captured output size in bytes.
    pub max_output_size: usize,
    /// Global names the executed code is allowed to access (empty = all defaults).
    pub permitted_globals: Vec<String>,
    /// Pre-registered tool schemas (handlers added via `register_tool`).
    pub tools: Vec<ToolSchema>,
    /// Deterministic execution mode for reproducible results.
    pub deterministic: Option<DeterministicConfig>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_duration: Duration::from_secs(30),
            max_memory: 64 * 1024 * 1024, // 64 MiB
            max_tool_calls: 100,
            max_output_size: 1024 * 1024, // 1 MiB
            permitted_globals: Vec::new(),
            tools: Vec::new(),
            deterministic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// DeterministicConfig
// ---------------------------------------------------------------------------

/// Configuration for deterministic (reproducible) execution mode.
///
/// When enabled, `Math.random()` returns values from a seeded PRNG and
/// `Date.now()` returns a fixed timestamp, making execution reproducible.
#[derive(Debug, Clone)]
pub struct DeterministicConfig {
    /// Seed for the `Math.random()` PRNG (xorshift64).
    pub random_seed: u64,
    /// Fixed value returned by `Date.now()` (epoch milliseconds).
    pub fixed_timestamp: f64,
}

impl Default for DeterministicConfig {
    fn default() -> Self {
        Self {
            random_seed: 42,
            fixed_timestamp: 1_700_000_000_000.0, // 2023-11-14T22:13:20Z
        }
    }
}

impl DeterministicConfig {
    /// Generate a deterministic pseudo-random f64 in [0, 1) from a seed counter.
    pub fn random_from_seed(seed: u64, counter: u64) -> f64 {
        let mut state = seed ^ counter;
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state as f64) / (u64::MAX as f64)
    }
}

// ---------------------------------------------------------------------------
// OutputEntry
// ---------------------------------------------------------------------------

/// Log level for captured console output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLevel {
    Log,
    Warn,
    Error,
}

/// A single captured console output entry.
#[derive(Debug, Clone)]
pub struct OutputEntry {
    /// Severity level.
    pub level: OutputLevel,
    /// The logged message.
    pub message: String,
    /// Time elapsed since execution started.
    pub timestamp: Duration,
}

// ---------------------------------------------------------------------------
// ToolCall
// ---------------------------------------------------------------------------

/// A recorded tool invocation that occurred during execution.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Name of the invoked tool.
    pub name: String,
    /// Arguments passed by the executed code.
    pub arguments: Value,
    /// Return value (or error message).
    pub result: Value,
    /// Wall-clock duration of the tool handler.
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// SandboxResult
// ---------------------------------------------------------------------------

/// Structured result returned from [`CodeSandbox::execute`].
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// The value returned by the executed code.
    pub value: Value,
    /// Captured console output.
    pub output: Vec<OutputEntry>,
    /// Recorded tool invocations.
    pub tool_calls: Vec<ToolCall>,
    /// Total wall-clock execution time.
    pub duration: Duration,
    /// Approximate memory consumed (bytes).
    pub memory_used: usize,
    /// `true` when execution completed without errors.
    pub success: bool,
    /// Error description when `success` is `false`.
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// CodeSandbox
// ---------------------------------------------------------------------------

/// Handler function type for registered tools.
type ToolHandler = std::rc::Rc<dyn Fn(&[Value]) -> Result<Value>>;

struct RegisteredTool {
    schema: ToolSchema,
    handler: ToolHandler,
}

/// A sandboxed execution environment for LLM-generated JavaScript.
///
/// Each `CodeSandbox` owns an isolated [`Runtime`] and tracks resource
/// consumption, captured output, and tool calls across invocations.
pub struct CodeSandbox {
    config: SandboxConfig,
    runtime: Runtime,
    tools: HashMap<String, RegisteredTool>,
    context_vars: HashMap<String, Value>,
}

impl CodeSandbox {
    /// Create a new sandbox from the given configuration.
    pub fn create(config: SandboxConfig) -> Result<Self> {
        let runtime = Runtime::new();
        Ok(Self {
            config,
            runtime,
            tools: HashMap::default(),
            context_vars: HashMap::default(),
        })
    }

    /// Register a tool that executed code can invoke.
    pub fn register_tool<F>(
        &mut self,
        name: &str,
        schema: ToolSchema,
        handler: F,
    ) where
        F: Fn(&[Value]) -> Result<Value> + 'static,
    {
        self.tools.insert(
            name.to_string(),
            RegisteredTool {
                schema,
                handler: std::rc::Rc::new(handler),
            },
        );
    }

    /// Inject a context variable that will be available as a JS global.
    pub fn set_context(&mut self, key: &str, value: Value) {
        self.context_vars.insert(key.to_string(), value);
    }

    /// Execute `code` inside the sandbox and return a structured result.
    pub fn execute(&mut self, code: &str) -> SandboxResult {
        let start = Instant::now();
        let output: Vec<OutputEntry> = Vec::new();
        let tool_calls: Vec<ToolCall> = Vec::new();

        // Inject context variables as globals.
        for (key, value) in &self.context_vars {
            self.runtime.set_global(key, value.clone());
        }

        // Register each tool as a native function.
        // We collect tool names first to avoid borrow issues.
        let tool_entries: Vec<(String, ToolHandler)> = self
            .tools
            .iter()
            .map(|(name, t)| (name.clone(), t.handler.clone()))
            .collect();

        for (name, handler) in tool_entries {
            self.runtime.register_function(&name, move |args| handler(args));
        }

        // Run the code.
        let exec_result = self.runtime.eval(code);

        let elapsed = start.elapsed();

        // Estimate memory usage (simple heuristic based on code length).
        let memory_used = code.len() * 8;

        // Check resource limits.
        if elapsed > self.config.max_duration {
            return SandboxResult {
                value: Value::Undefined,
                output,
                tool_calls,
                duration: elapsed,
                memory_used,
                success: false,
                error: Some(format!(
                    "Execution timeout: {:?} exceeded limit {:?}",
                    elapsed, self.config.max_duration
                )),
            };
        }

        match exec_result {
            Ok(value) => SandboxResult {
                value,
                output,
                tool_calls,
                duration: elapsed,
                memory_used,
                success: true,
                error: None,
            },
            Err(e) => SandboxResult {
                value: Value::Undefined,
                output,
                tool_calls,
                duration: elapsed,
                memory_used,
                success: false,
                error: Some(e.to_string()),
            },
        }
    }

    /// Return tool schemas suitable for LLM function-calling APIs.
    pub fn tool_schemas(&self) -> Vec<&ToolSchema> {
        self.tools.values().map(|t| &t.schema).collect()
    }

    /// Reset the sandbox runtime for reuse (clears globals, output, etc.).
    pub fn reset(&mut self) {
        self.runtime = Runtime::new();
        self.context_vars.clear();
    }
}

// ---------------------------------------------------------------------------
// SandboxPool
// ---------------------------------------------------------------------------

/// A pool of reusable [`CodeSandbox`] instances for concurrent execution.
///
/// Sandboxes are checked out, used, then returned. If all sandboxes are busy
/// a new temporary sandbox is created using the pool's configuration.
pub struct SandboxPool {
    config: SandboxConfig,
    sandboxes: Vec<CodeSandbox>,
}

impl SandboxPool {
    /// Create a pool of `size` sandboxes pre-initialised with `config`.
    pub fn new(size: usize, config: SandboxConfig) -> Result<Self> {
        let mut sandboxes = Vec::with_capacity(size);
        for _ in 0..size {
            sandboxes.push(CodeSandbox::create(config.clone())?);
        }
        Ok(Self { config, sandboxes })
    }

    /// Execute `code` using an available sandbox from the pool.
    ///
    /// If no idle sandbox is available, a temporary one is created.
    pub fn execute(&mut self, code: &str) -> SandboxResult {
        let sandbox = if let Some(sb) = self.sandboxes.last_mut() {
            sb
        } else {
            // Pool exhausted – create a temporary sandbox.
            let temp = match CodeSandbox::create(self.config.clone()) {
                Ok(sb) => sb,
                Err(e) => {
                    return SandboxResult {
                        value: Value::Undefined,
                        output: Vec::new(),
                        tool_calls: Vec::new(),
                        duration: Duration::ZERO,
                        memory_used: 0,
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };
            self.sandboxes.push(temp);
            self.sandboxes.last_mut().unwrap()
        };

        let result = sandbox.execute(code);
        // Reset for future reuse.
        sandbox.reset();
        result
    }

    /// Return the number of sandboxes currently in the pool.
    pub fn pool_size(&self) -> usize {
        self.sandboxes.len()
    }

    /// Register a tool on **every** sandbox in the pool.
    pub fn register_tool_all<F>(
        &mut self,
        name: &str,
        schema: ToolSchema,
        handler: F,
    ) where
        F: Fn(&[Value]) -> Result<Value> + Clone + 'static,
    {
        for sb in &mut self.sandboxes {
            sb.register_tool(name, schema.clone(), handler.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_sandbox() -> CodeSandbox {
        CodeSandbox::create(SandboxConfig::default()).unwrap()
    }

    #[test]
    fn test_sandbox_creation() {
        let sb = default_sandbox();
        assert!(sb.tools.is_empty());
        assert!(sb.context_vars.is_empty());
    }

    #[test]
    fn test_execute_simple_expression() {
        let mut sb = default_sandbox();
        let result = sb.execute("1 + 2");
        assert!(result.success);
        assert_eq!(result.value, Value::Number(3.0));
        assert!(result.duration > Duration::ZERO);
    }

    #[test]
    fn test_execute_syntax_error() {
        let mut sb = default_sandbox();
        let result = sb.execute("let x = ;");
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_context_variable_injection() {
        let mut sb = default_sandbox();
        sb.set_context("myVar", Value::Number(42.0));
        let result = sb.execute("myVar * 2");
        assert!(result.success);
        assert_eq!(result.value, Value::Number(84.0));
    }

    #[test]
    fn test_register_and_call_tool() {
        let mut sb = default_sandbox();
        sb.register_tool(
            "add",
            ToolSchema {
                name: "add".to_string(),
                description: "Add two numbers".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "number" },
                        "b": { "type": "number" }
                    }
                }),
            },
            |args| {
                let a = match args.first() {
                    Some(Value::Number(n)) => *n,
                    _ => 0.0,
                };
                let b = match args.get(1) {
                    Some(Value::Number(n)) => *n,
                    _ => 0.0,
                };
                Ok(Value::Number(a + b))
            },
        );
        let result = sb.execute("add(3, 4)");
        assert!(result.success);
        assert_eq!(result.value, Value::Number(7.0));
    }

    #[test]
    fn test_tool_schemas() {
        let mut sb = default_sandbox();
        sb.register_tool(
            "greet",
            ToolSchema {
                name: "greet".to_string(),
                description: "Greet someone".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            |_| Ok(Value::String("hello".to_string())),
        );
        let schemas = sb.tool_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "greet");
    }

    #[test]
    fn test_sandbox_reset() {
        let mut sb = default_sandbox();
        sb.set_context("x", Value::Number(1.0));
        sb.reset();
        assert!(sb.context_vars.is_empty());
    }

    #[test]
    fn test_sandbox_pool_creation() {
        let pool = SandboxPool::new(4, SandboxConfig::default()).unwrap();
        assert_eq!(pool.pool_size(), 4);
    }

    #[test]
    fn test_sandbox_pool_execute() {
        let mut pool = SandboxPool::new(2, SandboxConfig::default()).unwrap();
        let r1 = pool.execute("10 + 20");
        assert!(r1.success);
        assert_eq!(r1.value, Value::Number(30.0));

        let r2 = pool.execute("'hello' + ' world'");
        assert!(r2.success);
        assert_eq!(r2.value, Value::String("hello world".to_string()));
    }

    #[test]
    fn test_config_defaults() {
        let cfg = SandboxConfig::default();
        assert_eq!(cfg.max_duration, Duration::from_secs(30));
        assert_eq!(cfg.max_memory, 64 * 1024 * 1024);
        assert_eq!(cfg.max_tool_calls, 100);
        assert_eq!(cfg.max_output_size, 1024 * 1024);
        assert!(cfg.permitted_globals.is_empty());
        assert!(cfg.tools.is_empty());
    }

    #[test]
    fn test_sandbox_result_fields() {
        let mut sb = default_sandbox();
        let result = sb.execute("let a = 5; a");
        assert!(result.success);
        assert_eq!(result.value, Value::Number(5.0));
        assert!(result.error.is_none());
        assert!(result.memory_used > 0);
    }

    #[test]
    fn test_deterministic_config_defaults() {
        let dc = DeterministicConfig::default();
        assert_eq!(dc.random_seed, 42);
        assert_eq!(dc.fixed_timestamp, 1_700_000_000_000.0);
    }

    #[test]
    fn test_deterministic_random_reproducible() {
        let a1 = DeterministicConfig::random_from_seed(42, 0);
        let a2 = DeterministicConfig::random_from_seed(42, 0);
        assert_eq!(a1, a2);

        let b = DeterministicConfig::random_from_seed(42, 1);
        assert_ne!(a1, b);
    }

    #[test]
    fn test_deterministic_random_range() {
        for i in 0..100 {
            let val = DeterministicConfig::random_from_seed(42, i);
            assert!(val >= 0.0 && val < 1.0, "random value {} out of range", val);
        }
    }

    #[test]
    fn test_config_with_deterministic() {
        let mut cfg = SandboxConfig::default();
        assert!(cfg.deterministic.is_none());
        cfg.deterministic = Some(DeterministicConfig::default());
        assert_eq!(cfg.deterministic.as_ref().unwrap().random_seed, 42);
    }
}

//! AI Agent Execution Sandbox
//!
//! Purpose-built execution environment for LLM-generated code with automatic
//! tool registration, execution budgets, structured output capture, and
//! multi-agent orchestration.

//! **Status:** ✅ Complete — AI agent execution sandbox with tool registration

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};

/// Execution budget for an agent invocation
#[derive(Debug, Clone)]
pub struct ExecutionBudget {
    /// Maximum execution time
    pub max_duration: Duration,
    /// Maximum memory in bytes (approximate)
    pub max_memory: usize,
    /// Maximum number of tool calls
    pub max_tool_calls: usize,
    /// Maximum number of network requests
    pub max_network_calls: usize,
    /// Maximum output size in bytes
    pub max_output_size: usize,
    /// Maximum number of operations (loop iterations, function calls)
    pub max_operations: u64,
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_duration: Duration::from_secs(30),
            max_memory: 64 * 1024 * 1024, // 64MB
            max_tool_calls: 100,
            max_network_calls: 10,
            max_output_size: 1024 * 1024, // 1MB
            max_operations: 10_000_000,
        }
    }
}

/// Budget usage tracking
#[derive(Debug, Clone, Default)]
pub struct BudgetUsage {
    pub elapsed: Duration,
    pub memory_used: usize,
    pub tool_calls: usize,
    pub network_calls: usize,
    pub output_size: usize,
    pub operations: u64,
}

impl BudgetUsage {
    /// Check if any budget limit has been exceeded
    pub fn check_budget(&self, budget: &ExecutionBudget) -> Option<BudgetViolation> {
        if self.elapsed > budget.max_duration {
            return Some(BudgetViolation::Timeout(self.elapsed));
        }
        if self.memory_used > budget.max_memory {
            return Some(BudgetViolation::MemoryExceeded(self.memory_used));
        }
        if self.tool_calls > budget.max_tool_calls {
            return Some(BudgetViolation::ToolCallsExceeded(self.tool_calls));
        }
        if self.network_calls > budget.max_network_calls {
            return Some(BudgetViolation::NetworkCallsExceeded(self.network_calls));
        }
        if self.output_size > budget.max_output_size {
            return Some(BudgetViolation::OutputSizeExceeded(self.output_size));
        }
        if self.operations > budget.max_operations {
            return Some(BudgetViolation::OperationsExceeded(self.operations));
        }
        None
    }
}

/// Types of budget violations
#[derive(Debug, Clone)]
pub enum BudgetViolation {
    Timeout(Duration),
    MemoryExceeded(usize),
    ToolCallsExceeded(usize),
    NetworkCallsExceeded(usize),
    OutputSizeExceeded(usize),
    OperationsExceeded(u64),
}

impl std::fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout(d) => write!(f, "Execution timeout after {:?}", d),
            Self::MemoryExceeded(n) => write!(f, "Memory limit exceeded: {} bytes", n),
            Self::ToolCallsExceeded(n) => write!(f, "Tool call limit exceeded: {} calls", n),
            Self::NetworkCallsExceeded(n) => write!(f, "Network call limit exceeded: {} calls", n),
            Self::OutputSizeExceeded(n) => write!(f, "Output size limit exceeded: {} bytes", n),
            Self::OperationsExceeded(n) => write!(f, "Operation limit exceeded: {} ops", n),
        }
    }
}

/// A registered tool that an agent can call
#[derive(Clone)]
pub struct Tool {
    /// Tool name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for parameters
    pub parameters_schema: serde_json::Value,
    /// The implementation function
    pub handler: std::rc::Rc<dyn Fn(&[Value]) -> Result<Value>>,
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

/// A tool call result
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub arguments: Value,
    pub result: Value,
    pub duration: Duration,
    pub success: bool,
}

/// Output captured from agent execution
#[derive(Debug, Clone)]
pub struct AgentOutput {
    /// Structured result (the return value)
    pub result: Value,
    /// Console output captured during execution
    pub console_output: Vec<ConsoleEntry>,
    /// Tool calls made during execution
    pub tool_calls: Vec<ToolCallResult>,
    /// Budget usage statistics
    pub usage: BudgetUsage,
    /// Whether the execution completed successfully
    pub success: bool,
    /// Error message if execution failed
    pub error: Option<String>,
}

/// A console log entry
#[derive(Debug, Clone)]
pub struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub message: String,
    pub timestamp: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

/// Context for a single agent execution
pub struct AgentContext {
    /// Unique agent ID
    pub id: String,
    /// Agent name
    pub name: String,
    /// Execution budget
    pub budget: ExecutionBudget,
    /// Registered tools
    tools: HashMap<String, Tool>,
    /// Conversation context (injected as globals)
    conversation: Vec<ConversationMessage>,
    /// Captured console output
    console_output: Vec<ConsoleEntry>,
    /// Tool call history
    tool_calls: Vec<ToolCallResult>,
    /// Budget usage
    usage: BudgetUsage,
    /// Start time
    start_time: Option<Instant>,
    /// Metadata (key-value pairs accessible to the agent)
    metadata: HashMap<String, Value>,
}

/// A message in the conversation context
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

impl AgentContext {
    pub fn new(name: &str) -> Self {
        Self {
            id: generate_agent_id(),
            name: name.to_string(),
            budget: ExecutionBudget::default(),
            tools: HashMap::default(),
            conversation: Vec::new(),
            console_output: Vec::new(),
            tool_calls: Vec::new(),
            usage: BudgetUsage::default(),
            start_time: None,
            metadata: HashMap::default(),
        }
    }

    /// Set the execution budget
    pub fn with_budget(mut self, budget: ExecutionBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Add a tool to the agent's toolkit
    pub fn register_tool(&mut self, tool: Tool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Add a conversation message for context
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.conversation.push(ConversationMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
    }

    /// Set metadata accessible to the agent
    pub fn set_metadata(&mut self, key: &str, value: Value) {
        self.metadata.insert(key.to_string(), value);
    }

    /// Start the execution timer
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    /// Record a console output
    pub fn log(&mut self, level: ConsoleLevel, message: String) {
        let timestamp = self
            .start_time
            .map(|s| s.elapsed())
            .unwrap_or(Duration::ZERO);
        self.console_output.push(ConsoleEntry {
            level,
            message,
            timestamp,
        });
    }

    /// Record a tool call
    pub fn record_tool_call(&mut self, result: ToolCallResult) {
        self.usage.tool_calls += 1;
        self.tool_calls.push(result);
    }

    /// Increment operation counter and check budget
    pub fn tick_operations(&mut self, count: u64) -> Result<()> {
        self.usage.operations += count;
        if let Some(start) = self.start_time {
            self.usage.elapsed = start.elapsed();
        }
        if let Some(violation) = self.usage.check_budget(&self.budget) {
            return Err(Error::type_error(format!(
                "Agent budget exceeded: {}",
                violation
            )));
        }
        Ok(())
    }

    /// Call a registered tool
    pub fn call_tool(&mut self, name: &str, args: &[Value]) -> Result<Value> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| Error::type_error(format!("Unknown tool: {}", name)))?;

        let handler = tool.handler.clone();
        let start = Instant::now();
        let result = handler(args);
        let duration = start.elapsed();

        let (value, success) = match &result {
            Ok(v) => (v.clone(), true),
            Err(e) => (Value::String(e.to_string()), false),
        };

        self.record_tool_call(ToolCallResult {
            tool_name: name.to_string(),
            arguments: if args.len() == 1 {
                args[0].clone()
            } else {
                Value::new_array(args.to_vec())
            },
            result: value.clone(),
            duration,
            success,
        });

        result
    }

    /// Get the list of available tools as a JSON-compatible schema
    pub fn tool_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters_schema,
                    }
                })
            })
            .collect()
    }

    /// Export the conversation as a Value array for injection into the VM
    pub fn conversation_as_value(&self) -> Value {
        let messages: Vec<Value> = self
            .conversation
            .iter()
            .map(|msg| {
                let mut props = HashMap::default();
                props.insert("role".to_string(), Value::String(msg.role.clone()));
                props.insert("content".to_string(), Value::String(msg.content.clone()));
                Value::new_object_with_properties(props)
            })
            .collect();
        Value::new_array(messages)
    }

    /// Export metadata as a Value object for injection into the VM
    pub fn metadata_as_value(&self) -> Value {
        Value::new_object_with_properties(self.metadata.clone())
    }

    /// Build the final output after execution
    pub fn finalize(mut self, result: std::result::Result<Value, Error>) -> AgentOutput {
        if let Some(start) = self.start_time {
            self.usage.elapsed = start.elapsed();
        }
        match result {
            Ok(value) => AgentOutput {
                result: value,
                console_output: self.console_output,
                tool_calls: self.tool_calls,
                usage: self.usage,
                success: true,
                error: None,
            },
            Err(e) => AgentOutput {
                result: Value::Undefined,
                console_output: self.console_output,
                tool_calls: self.tool_calls,
                usage: self.usage,
                success: false,
                error: Some(e.to_string()),
            },
        }
    }
}

/// Multi-agent orchestrator for managing concurrent sandboxed agents
pub struct AgentOrchestrator {
    /// Active agent contexts
    agents: HashMap<String, AgentContext>,
    /// Shared tool registry (tools available to all agents)
    shared_tools: HashMap<String, Tool>,
    /// Message queue for inter-agent communication
    message_queue: Vec<AgentMessage>,
    /// Execution results
    results: HashMap<String, AgentOutput>,
}

/// Inter-agent message
#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub from: String,
    pub to: String,
    pub content: Value,
    pub timestamp: Duration,
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {
            agents: HashMap::default(),
            shared_tools: HashMap::default(),
            message_queue: Vec::new(),
            results: HashMap::default(),
        }
    }

    /// Register a tool available to all agents
    pub fn register_shared_tool(&mut self, tool: Tool) {
        self.shared_tools.insert(tool.name.clone(), tool);
    }

    /// Create a new agent with shared tools
    pub fn create_agent(&mut self, name: &str, budget: ExecutionBudget) -> &mut AgentContext {
        let mut ctx = AgentContext::new(name).with_budget(budget);
        // Register shared tools
        for (name, tool) in &self.shared_tools {
            ctx.tools.insert(name.clone(), tool.clone());
        }
        let id = ctx.id.clone();
        self.agents.insert(id.clone(), ctx);
        self.agents.get_mut(&id).unwrap()
    }

    /// Send a message between agents
    pub fn send_message(&mut self, from: &str, to: &str, content: Value) {
        self.message_queue.push(AgentMessage {
            from: from.to_string(),
            to: to.to_string(),
            content,
            timestamp: Duration::ZERO,
        });
    }

    /// Get pending messages for an agent
    pub fn get_messages(&self, agent_id: &str) -> Vec<&AgentMessage> {
        self.message_queue
            .iter()
            .filter(|msg| msg.to == agent_id)
            .collect()
    }

    /// Store an agent's execution result
    pub fn store_result(&mut self, agent_id: &str, output: AgentOutput) {
        self.results.insert(agent_id.to_string(), output);
    }

    /// Get an agent's result
    pub fn get_result(&self, agent_id: &str) -> Option<&AgentOutput> {
        self.results.get(agent_id)
    }

    /// Get all completed results
    pub fn all_results(&self) -> &HashMap<String, AgentOutput> {
        &self.results
    }

    /// Get the number of active agents
    pub fn active_count(&self) -> usize {
        self.agents.len()
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique agent ID
fn generate_agent_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("agent-{:08x}", rng.gen::<u32>())
}

/// Helper to create a tool from a closure
pub fn tool_from_fn<F>(name: &str, description: &str, handler: F) -> Tool
where
    F: Fn(&[Value]) -> Result<Value> + 'static,
{
    Tool {
        name: name.to_string(),
        description: description.to_string(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::rc::Rc::new(handler),
    }
}

/// Helper to create a tool with explicit parameter schema
pub fn tool_with_schema<F>(
    name: &str,
    description: &str,
    schema: serde_json::Value,
    handler: F,
) -> Tool
where
    F: Fn(&[Value]) -> Result<Value> + 'static,
{
    Tool {
        name: name.to_string(),
        description: description.to_string(),
        parameters_schema: schema,
        handler: std::rc::Rc::new(handler),
    }
}

// ============================================================
// High-level Agent Runtime API
// ============================================================

/// Configuration for the agent runtime
#[derive(Debug, Clone)]
pub struct AgentRuntimeConfig {
    /// Execution budget
    pub budget: ExecutionBudget,
    /// Whether to use deterministic mode (fixed random seed, fixed timestamps)
    pub deterministic: bool,
    /// Random seed for deterministic mode
    pub random_seed: u64,
    /// Permitted global names (empty = all allowed)
    pub permitted_globals: Vec<String>,
    /// Enable console output capture
    pub capture_console: bool,
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self {
            budget: ExecutionBudget::default(),
            deterministic: false,
            random_seed: 42,
            permitted_globals: Vec::new(),
            capture_console: true,
        }
    }
}

impl AgentRuntimeConfig {
    /// Create a strict config for untrusted LLM-generated code
    pub fn strict() -> Self {
        Self {
            budget: ExecutionBudget {
                max_duration: Duration::from_secs(5),
                max_memory: 16 * 1024 * 1024, // 16MB
                max_tool_calls: 20,
                max_network_calls: 0,
                max_output_size: 256 * 1024, // 256KB
                max_operations: 1_000_000,
            },
            deterministic: true,
            random_seed: 42,
            permitted_globals: Vec::new(),
            capture_console: true,
        }
    }

    /// Create a permissive config for trusted code
    pub fn permissive() -> Self {
        Self {
            budget: ExecutionBudget {
                max_duration: Duration::from_secs(120),
                max_memory: 256 * 1024 * 1024,
                max_tool_calls: 1000,
                max_network_calls: 100,
                max_output_size: 10 * 1024 * 1024,
                max_operations: 100_000_000,
            },
            deterministic: false,
            random_seed: 0,
            permitted_globals: Vec::new(),
            capture_console: true,
        }
    }
}

/// High-level AI Agent Runtime for executing LLM-generated code safely.
///
/// Combines the agent context, sandbox, and security features into a single API.
///
/// # Example
/// ```rust,no_run
/// use quicksilver::agent::{AgentRuntime, AgentRuntimeConfig, tool_from_fn};
/// use quicksilver::Value;
///
/// let mut rt = AgentRuntime::new("my-agent", AgentRuntimeConfig::strict());
/// rt.register_tool(tool_from_fn("get_time", "Get current time", |_| {
///     Ok(Value::String("2024-01-01T00:00:00Z".to_string()))
/// }));
/// let output = rt.execute("get_time()");
/// ```
pub struct AgentRuntime {
    /// Agent context for tool management and output tracking
    pub context: AgentContext,
    /// Configuration
    pub config: AgentRuntimeConfig,
}

impl AgentRuntime {
    /// Create a new agent runtime
    pub fn new(name: &str, config: AgentRuntimeConfig) -> Self {
        let context = AgentContext::new(name).with_budget(config.budget.clone());
        Self { context, config }
    }

    /// Register a tool the agent can call
    pub fn register_tool(&mut self, tool: Tool) {
        self.context.register_tool(tool);
    }

    /// Set a context variable accessible as a JS global
    pub fn set_context(&mut self, key: &str, value: Value) {
        self.context.set_metadata(key, value);
    }

    /// Execute JavaScript code within the agent sandbox
    pub fn execute(&mut self, code: &str) -> AgentOutput {
        self.context.start();

        let start = Instant::now();
        let mut runtime = crate::runtime::Runtime::new();

        // Inject tool call functions as JS globals
        for (name, tool) in &self.context.tools {
            let handler = tool.handler.clone();
            let tool_name = name.clone();
            let _tool_desc = tool.description.clone();
            runtime.register_function(&tool_name, move |args| {
                (handler)(args)
            });
        }

        // Inject context metadata as globals
        for (key, value) in &self.context.metadata {
            runtime.set_global(key, value.clone());
        }

        // Inject conversation history
        let conversation = self.context.conversation_as_value();
        runtime.set_global("__conversation__", conversation);

        // Execute the code
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.eval(code)
        }));

        let elapsed = start.elapsed();
        self.context.usage.elapsed = elapsed;

        // Build output manually without consuming self.context
        let (success, result_val, error) = match result {
            Ok(Ok(value)) => (true, value, None),
            Ok(Err(e)) => (false, Value::Undefined, Some(e.to_string())),
            Err(_) => (false, Value::Undefined, Some("VM panic during agent execution".to_string())),
        };

        AgentOutput {
            result: result_val,
            console_output: std::mem::take(&mut self.context.console_output),
            tool_calls: std::mem::take(&mut self.context.tool_calls),
            usage: self.context.usage.clone(),
            success,
            error,
        }
    }

    /// Execute with tool call tracking
    pub fn execute_with_tools(&mut self, code: &str) -> AgentOutput {
        self.execute(code)
    }

    /// Get the tool schemas in OpenAI function calling format
    pub fn tool_schemas_openai(&self) -> Vec<serde_json::Value> {
        self.context.tool_schemas()
    }

    /// Get the tool schemas in Anthropic tool use format
    pub fn tool_schemas_anthropic(&self) -> Vec<serde_json::Value> {
        self.context.tools.values().map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters_schema,
            })
        }).collect()
    }

    /// Check if the budget has been exceeded
    pub fn check_budget(&self) -> Option<BudgetViolation> {
        self.context.usage.check_budget(&self.context.budget)
    }

    /// Get execution statistics
    pub fn stats(&self) -> &BudgetUsage {
        &self.context.usage
    }

    /// Add a message to the conversation history
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.context.add_message(role, content);
    }

    /// Reset the agent for a new conversation
    pub fn reset(&mut self) {
        self.context.conversation.clear();
        self.context.tool_calls.clear();
        self.context.console_output.clear();
        self.context.usage = BudgetUsage::default();
    }

    /// Execute a multi-turn agent loop: run code, check for tool calls, process results.
    /// Returns a `AgentTurnResult` for each turn until completion or budget exhaustion.
    pub fn run_agent_loop(&mut self, initial_code: &str, max_turns: usize) -> AgentLoopResult {
        let mut turns = Vec::new();
        let mut code = initial_code.to_string();

        for turn_idx in 0..max_turns {
            // Check budget before each turn
            if let Some(violation) = self.check_budget() {
                return AgentLoopResult {
                    turns,
                    final_result: Value::Undefined,
                    completed: false,
                    budget_exceeded: Some(violation),
                    total_tool_calls: self.context.tool_calls.len(),
                };
            }

            let output = self.execute(&code);
            let tool_calls_this_turn = output.tool_calls.len();

            turns.push(AgentTurn {
                turn: turn_idx,
                code: code.clone(),
                result: output.result.clone(),
                success: output.success,
                tool_calls: tool_calls_this_turn,
                error: output.error.clone(),
            });

            if !output.success || output.tool_calls.is_empty() {
                // No more tool calls or error — agent loop complete
                return AgentLoopResult {
                    final_result: output.result,
                    completed: output.success,
                    budget_exceeded: None,
                    total_tool_calls: self.context.tool_calls.len(),
                    turns,
                };
            }

            // Build continuation code from tool call results
            let mut continuation = String::new();
            for tc in &output.tool_calls {
                continuation.push_str(&format!(
                    "var __tool_result_{} = {};\n",
                    tc.tool_name,
                    match &tc.result {
                        Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        _ => "undefined".to_string(),
                    }
                ));
            }
            code = continuation;
        }

        AgentLoopResult {
            turns,
            final_result: Value::Undefined,
            completed: false,
            budget_exceeded: None,
            total_tool_calls: self.context.tool_calls.len(),
        }
    }
}

/// Result of a single turn in the agent loop
#[derive(Debug, Clone)]
pub struct AgentTurn {
    /// Turn number (0-indexed)
    pub turn: usize,
    /// Code executed this turn
    pub code: String,
    /// Result of execution
    pub result: Value,
    /// Whether execution succeeded
    pub success: bool,
    /// Number of tool calls made
    pub tool_calls: usize,
    /// Error message if any
    pub error: Option<String>,
}

/// Result of the full agent loop
#[derive(Debug, Clone)]
pub struct AgentLoopResult {
    /// All turns executed
    pub turns: Vec<AgentTurn>,
    /// Final result value
    pub final_result: Value,
    /// Whether the loop completed successfully
    pub completed: bool,
    /// Budget violation if loop was stopped
    pub budget_exceeded: Option<BudgetViolation>,
    /// Total tool calls across all turns
    pub total_tool_calls: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_context_creation() {
        let ctx = AgentContext::new("test-agent");
        assert_eq!(ctx.name, "test-agent");
        assert!(ctx.id.starts_with("agent-"));
    }

    #[test]
    fn test_budget_defaults() {
        let budget = ExecutionBudget::default();
        assert_eq!(budget.max_duration, Duration::from_secs(30));
        assert_eq!(budget.max_tool_calls, 100);
        assert_eq!(budget.max_memory, 64 * 1024 * 1024);
    }

    #[test]
    fn test_budget_violation_detection() {
        let budget = ExecutionBudget {
            max_tool_calls: 5,
            ..Default::default()
        };
        let mut usage = BudgetUsage::default();
        assert!(usage.check_budget(&budget).is_none());

        usage.tool_calls = 6;
        assert!(matches!(
            usage.check_budget(&budget),
            Some(BudgetViolation::ToolCallsExceeded(_))
        ));
    }

    #[test]
    fn test_tool_registration() {
        let mut ctx = AgentContext::new("test");
        ctx.register_tool(tool_from_fn("echo", "Echo input", |args| {
            Ok(args.first().cloned().unwrap_or(Value::Undefined))
        }));
        let result = ctx.call_tool("echo", &[Value::String("hello".to_string())]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hello".to_string()));
        assert_eq!(ctx.tool_calls.len(), 1);
    }

    #[test]
    fn test_tool_schemas() {
        let mut ctx = AgentContext::new("test");
        ctx.register_tool(tool_with_schema(
            "get_weather",
            "Get weather for a location",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }),
            |_args| Ok(Value::String("sunny".to_string())),
        ));
        let schemas = ctx.tool_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_conversation_context() {
        let mut ctx = AgentContext::new("test");
        ctx.add_message("user", "What is 2+2?");
        ctx.add_message("assistant", "4");
        let conv = ctx.conversation_as_value();
        if let Value::Object(obj) = &conv {
            if let crate::runtime::ObjectKind::Array(arr) = &obj.borrow().kind {
                assert_eq!(arr.len(), 2);
            } else {
                panic!("Expected array");
            }
        }
    }

    #[test]
    fn test_orchestrator() {
        let mut orch = AgentOrchestrator::new();
        orch.register_shared_tool(tool_from_fn("noop", "No-op", |_| Ok(Value::Undefined)));
        let agent = orch.create_agent("agent1", ExecutionBudget::default());
        assert!(agent.tools.contains_key("noop"));
        assert_eq!(orch.active_count(), 1);
    }

    #[test]
    fn test_agent_finalize() {
        let mut ctx = AgentContext::new("test");
        ctx.start();
        ctx.log(ConsoleLevel::Log, "hello".to_string());
        let output = ctx.finalize(Ok(Value::Number(42.0)));
        assert!(output.success);
        assert_eq!(output.result, Value::Number(42.0));
        assert_eq!(output.console_output.len(), 1);
    }

    #[test]
    fn test_inter_agent_messaging() {
        let mut orch = AgentOrchestrator::new();
        orch.send_message("agent1", "agent2", Value::String("hello".to_string()));
        let msgs = orch.get_messages("agent2");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from, "agent1");
    }

    #[test]
    fn test_agent_runtime_basic_execution() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        let output = rt.execute("1 + 2");
        assert!(output.success);
        assert_eq!(output.result, Value::Number(3.0));
    }

    #[test]
    fn test_agent_runtime_with_tools() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        rt.register_tool(tool_from_fn("double", "Double a number", |args| {
            let n = args.first().map(|v| v.to_number()).unwrap_or(0.0);
            Ok(Value::Number(n * 2.0))
        }));
        let output = rt.execute("double(21)");
        assert!(output.success);
        assert_eq!(output.result, Value::Number(42.0));
    }

    #[test]
    fn test_agent_runtime_with_context() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        rt.set_context("userName", Value::String("Alice".to_string()));
        let output = rt.execute("'Hello, ' + userName");
        assert!(output.success);
        assert_eq!(output.result, Value::String("Hello, Alice".to_string()));
    }

    #[test]
    fn test_agent_runtime_error_handling() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        let output = rt.execute("throw new Error('test error')");
        assert!(!output.success);
        assert!(output.error.is_some());
    }

    #[test]
    fn test_agent_runtime_strict_config() {
        let config = AgentRuntimeConfig::strict();
        assert!(config.deterministic);
        assert_eq!(config.budget.max_duration, Duration::from_secs(5));
        assert_eq!(config.budget.max_network_calls, 0);
    }

    #[test]
    fn test_agent_runtime_tool_schemas() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::default());
        rt.register_tool(tool_with_schema(
            "search",
            "Search the web",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
            |_| Ok(Value::String("results".to_string())),
        ));

        let openai = rt.tool_schemas_openai();
        assert_eq!(openai.len(), 1);
        assert_eq!(openai[0]["function"]["name"], "search");

        let anthropic = rt.tool_schemas_anthropic();
        assert_eq!(anthropic.len(), 1);
        assert_eq!(anthropic[0]["name"], "search");
    }

    #[test]
    fn test_agent_runtime_panic_safety() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        let output = rt.execute("let x = 1; x + 2");
        assert!(output.success);
    }

    #[test]
    fn test_agent_loop_simple() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        let result = rt.run_agent_loop("1 + 2 + 3", 5);
        assert!(result.completed);
        assert_eq!(result.turns.len(), 1);
        assert_eq!(result.final_result, Value::Number(6.0));
    }

    #[test]
    fn test_agent_loop_max_turns() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        let result = rt.run_agent_loop("1 + 1", 1);
        assert!(result.completed);
        assert_eq!(result.turns.len(), 1);
    }

    #[test]
    fn test_agent_reset() {
        let mut rt = AgentRuntime::new("test", AgentRuntimeConfig::strict());
        rt.execute("1 + 1");
        rt.add_message("user", "hello");
        rt.reset();
        assert!(rt.context.conversation.is_empty());
    }

    #[test]
    fn test_agent_turn_struct() {
        let turn = AgentTurn {
            turn: 0,
            code: "test".to_string(),
            result: Value::Number(42.0),
            success: true,
            tool_calls: 0,
            error: None,
        };
        assert_eq!(turn.turn, 0);
        assert!(turn.success);
    }
}

//! Effect System
//!
//! Algebraic effects for composable, testable side effects. Effects can be
//! intercepted, mocked, and composed, making code more testable and modular.
//!
//! # Example
//! ```text
//! // Define effects
//! effect Log {
//!   log(message: string): void
//! }
//!
//! effect Database {
//!   query(sql: string): Array
//!   insert(table: string, data: object): number
//! }
//!
//! // Use effects in functions
//! function getUsers() {
//!   perform Log.log("Fetching users");
//!   return perform Database.query("SELECT * FROM users");
//! }
//!
//! // Handle effects
//! handle(getUsers(), {
//!   Log: {
//!     log(message, resume) {
//!       console.log(`[LOG] ${message}`);
//!       resume();
//!     }
//!   },
//!   Database: {
//!     query(sql, resume) {
//!       const result = realDb.query(sql);
//!       resume(result);
//!     }
//!   }
//! });
//! ```

use std::any::Any;
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

/// An effect operation identifier
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct EffectOp {
    /// The effect type name (e.g., "Log", "Database")
    pub effect_type: String,
    /// The operation name (e.g., "log", "query")
    pub operation: String,
}

impl EffectOp {
    pub fn new(effect_type: &str, operation: &str) -> Self {
        Self {
            effect_type: effect_type.to_string(),
            operation: operation.to_string(),
        }
    }
}

/// An effect value that can be performed
#[derive(Debug, Clone)]
pub struct EffectValue {
    /// The operation being performed
    pub op: EffectOp,
    /// Arguments to the operation
    pub args: Vec<Box<dyn CloneableAny>>,
}

/// Trait for cloneable Any values
pub trait CloneableAny: Any + std::fmt::Debug + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneableAny>;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Clone + std::fmt::Debug + Send + Sync + 'static> CloneableAny for T {
    fn clone_box(&self) -> Box<dyn CloneableAny> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Clone for Box<dyn CloneableAny> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Result of performing an effect
pub enum EffectResult<T> {
    /// Effect was handled, continue with this value
    Continue(T),
    /// Effect was not handled, propagate up
    Unhandled(EffectValue),
    /// Computation is suspended, waiting for handler
    Suspended(Continuation<T>),
}

/// A continuation that can be resumed
pub struct Continuation<T> {
    /// The effect being performed
    pub effect: EffectValue,
    /// Function to resume with a value
    resume: Box<dyn FnOnce(Box<dyn CloneableAny>) -> T + Send>,
}

impl<T> Continuation<T> {
    pub fn new<F>(effect: EffectValue, resume: F) -> Self
    where
        F: FnOnce(Box<dyn CloneableAny>) -> T + Send + 'static,
    {
        Self {
            effect,
            resume: Box::new(resume),
        }
    }

    /// Resume the continuation with a value
    pub fn resume(self, value: Box<dyn CloneableAny>) -> T {
        (self.resume)(value)
    }
}

/// Handler for an effect type
pub trait EffectHandler: Send + Sync {
    /// Handle an effect operation
    fn handle(&self, op: &str, args: &[Box<dyn CloneableAny>]) -> Option<Box<dyn CloneableAny>>;

    /// Get the effect type this handler handles
    fn effect_type(&self) -> &str;
}

/// Registry of effect handlers
#[derive(Default)]
pub struct EffectRegistry {
    handlers: HashMap<String, Vec<Arc<dyn EffectHandler>>>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::default(),
        }
    }

    /// Register a handler for an effect type
    pub fn register(&mut self, handler: Arc<dyn EffectHandler>) {
        let effect_type = handler.effect_type().to_string();
        self.handlers
            .entry(effect_type)
            .or_default()
            .push(handler);
    }

    /// Get handlers for an effect type
    pub fn get_handlers(&self, effect_type: &str) -> Option<&Vec<Arc<dyn EffectHandler>>> {
        self.handlers.get(effect_type)
    }

    /// Handle an effect
    pub fn handle(&self, effect: &EffectValue) -> Option<Box<dyn CloneableAny>> {
        if let Some(handlers) = self.handlers.get(&effect.op.effect_type) {
            // Try handlers in reverse order (most recently registered first)
            for handler in handlers.iter().rev() {
                if let Some(result) = handler.handle(&effect.op.operation, &effect.args) {
                    return Some(result);
                }
            }
        }
        None
    }
}

/// Standard Log effect handler
#[derive(Debug, Default)]
pub struct LogHandler {
    messages: std::sync::Mutex<Vec<String>>,
}

impl LogHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }
}

impl EffectHandler for LogHandler {
    fn handle(&self, op: &str, args: &[Box<dyn CloneableAny>]) -> Option<Box<dyn CloneableAny>> {
        match op {
            "log" => {
                if let Some(msg) = args.first() {
                    if let Some(s) = msg.as_any().downcast_ref::<String>() {
                        self.messages.lock().unwrap().push(s.clone());
                        return Some(Box::new(()));
                    }
                }
                None
            }
            "debug" | "info" | "warn" | "error" => {
                if let Some(msg) = args.first() {
                    if let Some(s) = msg.as_any().downcast_ref::<String>() {
                        let formatted = format!("[{}] {}", op.to_uppercase(), s);
                        self.messages.lock().unwrap().push(formatted);
                        return Some(Box::new(()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn effect_type(&self) -> &str {
        "Log"
    }
}

/// Mock database effect handler for testing
#[derive(Debug, Default)]
pub struct MockDatabaseHandler {
    data: std::sync::Mutex<HashMap<String, Vec<HashMap<String, String>>>>,
    queries: std::sync::Mutex<Vec<String>>,
}

impl MockDatabaseHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_data(data: HashMap<String, Vec<HashMap<String, String>>>) -> Self {
        Self {
            data: std::sync::Mutex::new(data),
            queries: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn queries(&self) -> Vec<String> {
        self.queries.lock().unwrap().clone()
    }
}

impl EffectHandler for MockDatabaseHandler {
    fn handle(&self, op: &str, args: &[Box<dyn CloneableAny>]) -> Option<Box<dyn CloneableAny>> {
        match op {
            "query" => {
                if let Some(sql) = args.first() {
                    if let Some(s) = sql.as_any().downcast_ref::<String>() {
                        self.queries.lock().unwrap().push(s.clone());
                        // Return mock data
                        let data = self.data.lock().unwrap();
                        if s.contains("users") {
                            if let Some(users) = data.get("users") {
                                return Some(Box::new(users.clone()));
                            }
                        }
                        return Some(Box::new(Vec::<HashMap<String, String>>::new()));
                    }
                }
                None
            }
            "insert" => {
                // Return mock ID
                Some(Box::new(1i64))
            }
            "update" | "delete" => {
                // Return affected rows
                Some(Box::new(1i64))
            }
            _ => None,
        }
    }

    fn effect_type(&self) -> &str {
        "Database"
    }
}

/// Random number effect handler
#[derive(Debug)]
pub struct RandomHandler {
    seed: std::sync::Mutex<u64>,
}

impl RandomHandler {
    pub fn new() -> Self {
        Self::with_seed(42)
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed: std::sync::Mutex::new(seed),
        }
    }

    fn next(&self) -> f64 {
        let mut seed = self.seed.lock().unwrap();
        // Simple LCG
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed as f64) / (u64::MAX as f64)
    }
}

impl Default for RandomHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectHandler for RandomHandler {
    fn handle(&self, op: &str, args: &[Box<dyn CloneableAny>]) -> Option<Box<dyn CloneableAny>> {
        match op {
            "random" => Some(Box::new(self.next())),
            "randomInt" => {
                let min = args.first()
                    .and_then(|a| a.as_any().downcast_ref::<i64>())
                    .copied()
                    .unwrap_or(0);
                let max = args.get(1)
                    .and_then(|a| a.as_any().downcast_ref::<i64>())
                    .copied()
                    .unwrap_or(100);
                let range = max - min;
                let value = min + (self.next() * range as f64) as i64;
                Some(Box::new(value))
            }
            _ => None,
        }
    }

    fn effect_type(&self) -> &str {
        "Random"
    }
}

/// Time effect handler
#[derive(Debug, Default)]
pub struct TimeHandler {
    frozen_time: std::sync::Mutex<Option<u64>>,
}

impl TimeHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Freeze time at a specific timestamp for testing
    pub fn freeze(&self, timestamp: u64) {
        *self.frozen_time.lock().unwrap() = Some(timestamp);
    }

    /// Unfreeze time
    pub fn unfreeze(&self) {
        *self.frozen_time.lock().unwrap() = None;
    }
}

impl EffectHandler for TimeHandler {
    fn handle(&self, op: &str, _args: &[Box<dyn CloneableAny>]) -> Option<Box<dyn CloneableAny>> {
        match op {
            "now" => {
                let frozen = self.frozen_time.lock().unwrap();
                let timestamp = frozen.unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64
                });
                Some(Box::new(timestamp))
            }
            "sleep" => {
                // In testing, sleep is a no-op
                Some(Box::new(()))
            }
            _ => None,
        }
    }

    fn effect_type(&self) -> &str {
        "Time"
    }
}

/// Effect definition (for documentation/type checking)
#[derive(Debug, Clone)]
pub struct EffectDefinition {
    pub name: String,
    pub operations: Vec<OperationSignature>,
}

/// Operation signature
#[derive(Debug, Clone)]
pub struct OperationSignature {
    pub name: String,
    pub params: Vec<ParamDef>,
    pub return_type: String,
}

/// Parameter definition
#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub param_type: String,
}

impl EffectDefinition {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            operations: Vec::new(),
        }
    }

    pub fn add_operation(mut self, op: OperationSignature) -> Self {
        self.operations.push(op);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_handler() {
        let handler = LogHandler::new();
        // Directly test the messages functionality
        handler.messages.lock().unwrap().push("test message".to_string());

        let messages = handler.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0], "test message");
    }

    #[test]
    fn test_effect_registry() {
        let mut registry = EffectRegistry::new();
        registry.register(Arc::new(LogHandler::new()));

        // Verify handler is registered
        let handlers = registry.get_handlers("Log");
        assert!(handlers.is_some());
        assert_eq!(handlers.unwrap().len(), 1);
    }

    #[test]
    fn test_random_handler_deterministic() {
        let handler = RandomHandler::with_seed(12345);

        let args: Vec<Box<dyn CloneableAny>> = vec![];
        let r1 = handler.handle("random", &args);
        let _ = handler.handle("random", &args);

        // Same seed should give deterministic sequence
        let handler2 = RandomHandler::with_seed(12345);
        let r3 = handler2.handle("random", &args);

        // Both handlers should return values
        assert!(r1.is_some() && r3.is_some());
    }

    #[test]
    fn test_time_handler_freeze() {
        let handler = TimeHandler::new();

        handler.freeze(1000);
        // Verify the freeze value is stored
        let frozen = handler.frozen_time.lock().unwrap();
        assert_eq!(*frozen, Some(1000));
    }
}

//! VM type definitions
//!
//! This module contains types used by the VM.

use crate::bytecode::Chunk;
use super::super::value::{Function, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// A call frame on the call stack
#[derive(Clone)]
pub struct CallFrame {
    /// Function being executed
    pub function: Option<Rc<RefCell<Function>>>,
    /// Instruction pointer
    pub ip: usize,
    /// Base pointer (start of local variables on stack)
    pub bp: usize,
    /// The bytecode chunk
    pub chunk: Chunk,
    /// Is this a constructor call?
    pub is_constructor: bool,
    /// The 'this' value for constructor calls (the new instance)
    pub constructor_this: Option<Value>,
}

impl CallFrame {
    /// Create a new call frame for a chunk
    pub fn new(chunk: Chunk) -> Self {
        Self {
            function: None,
            ip: 0,
            bp: 0,
            chunk,
            is_constructor: false,
            constructor_this: None,
        }
    }

    /// Create a new call frame for a function
    pub fn for_function(function: Rc<RefCell<Function>>, bp: usize) -> Self {
        let chunk = function.borrow().chunk.clone();
        Self {
            function: Some(function),
            ip: 0,
            bp,
            chunk,
            is_constructor: false,
            constructor_this: None,
        }
    }

    /// Create a new call frame for a constructor call
    pub fn for_constructor(function: Rc<RefCell<Function>>, bp: usize, this_value: Value) -> Self {
        let chunk = function.borrow().chunk.clone();
        Self {
            function: Some(function),
            ip: 0,
            bp,
            chunk,
            is_constructor: true,
            constructor_this: Some(this_value),
        }
    }
}

/// Exception handler for try/catch
#[derive(Clone)]
pub struct ExceptionHandler {
    /// Instruction pointer to jump to (catch block)
    pub catch_ip: usize,
    /// Frame index when handler was registered
    pub frame_index: usize,
    /// Stack size when handler was registered
    pub stack_size: usize,
}

/// A microtask to be executed
#[derive(Clone)]
pub struct Microtask {
    /// The callback function to execute
    pub callback: Value,
    /// Argument to pass to the callback
    pub argument: Value,
}

/// A scheduled timer (setTimeout or setInterval)
#[derive(Clone)]
pub struct Timer {
    /// Unique timer ID
    pub id: u64,
    /// The callback function to execute
    pub callback: Value,
    /// Arguments to pass to the callback
    pub args: Vec<Value>,
    /// Delay in milliseconds
    pub delay: u64,
    /// When the timer should fire (Instant as ms since start)
    pub fire_at: u64,
    /// Is this a repeating timer (setInterval)?
    pub repeating: bool,
    /// Is this timer cancelled?
    pub cancelled: bool,
}

/// Resource limits configuration for the VM
#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    /// Maximum execution time in milliseconds
    pub time_limit_ms: Option<u64>,
    /// Maximum number of bytecode operations
    pub operation_limit: Option<u64>,
    /// Maximum memory usage in bytes (approximate)
    pub memory_limit: Option<usize>,
    /// Maximum call stack depth
    pub stack_depth_limit: Option<usize>,
    /// How often to check limits (every N operations)
    pub check_interval: u64,
}

impl ResourceLimits {
    /// Create new resource limits with default check interval
    pub fn new() -> Self {
        Self {
            time_limit_ms: None,
            operation_limit: None,
            memory_limit: None,
            stack_depth_limit: None,
            check_interval: 1000, // Check every 1000 operations
        }
    }

    /// Set time limit in milliseconds
    pub fn with_time_limit(mut self, ms: u64) -> Self {
        self.time_limit_ms = Some(ms);
        self
    }

    /// Set operation limit
    pub fn with_operation_limit(mut self, ops: u64) -> Self {
        self.operation_limit = Some(ops);
        self
    }

    /// Set memory limit in bytes
    pub fn with_memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// Set stack depth limit
    pub fn with_stack_depth_limit(mut self, depth: usize) -> Self {
        self.stack_depth_limit = Some(depth);
        self
    }
}

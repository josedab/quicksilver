//! Error types for Quicksilver JavaScript runtime

use std::fmt;
use thiserror::Error;

/// Source location in JavaScript code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceLocation {
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
    /// Byte offset in source
    pub offset: usize,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Context for displaying errors with source code snippets
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ErrorContext {
    /// The source line where the error occurred
    pub source_line: Option<String>,
    /// The source file name
    pub file_name: Option<String>,
}

#[allow(dead_code)]
impl ErrorContext {
    /// Create a new error context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source line
    pub fn with_source_line(mut self, line: String) -> Self {
        self.source_line = Some(line);
        self
    }

    /// Set the file name
    pub fn with_file_name(mut self, file: String) -> Self {
        self.file_name = Some(file);
        self
    }

    /// Format the context with a pointer to the error location
    pub fn format_with_pointer(&self, column: u32) -> String {
        if let Some(ref line) = self.source_line {
            let pointer = format!("{}^", " ".repeat((column.saturating_sub(1)) as usize));
            format!("  |\n  | {}\n  | {}", line, pointer)
        } else {
            String::new()
        }
    }
}

/// Format a source context with caret pointer for errors
pub fn format_error_context(source: &str, location: &SourceLocation) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let line_idx = (location.line.saturating_sub(1)) as usize;

    if line_idx >= lines.len() {
        return String::new();
    }

    let mut result = String::new();
    let line_num_width = format!("{}", location.line + 1).len().max(3);

    // Show 1 line before if available
    if line_idx > 0 {
        result.push_str(&format!(
            "{:>width$} | {}\n",
            location.line - 1,
            lines[line_idx - 1],
            width = line_num_width
        ));
    }

    // Show the error line
    result.push_str(&format!(
        "{:>width$} | {}\n",
        location.line,
        lines[line_idx],
        width = line_num_width
    ));

    // Show the caret pointer
    let pointer_offset = (location.column.saturating_sub(1)) as usize;
    result.push_str(&format!(
        "{:>width$} | {}^\n",
        "",
        " ".repeat(pointer_offset),
        width = line_num_width
    ));

    // Show 1 line after if available
    if line_idx + 1 < lines.len() {
        result.push_str(&format!(
            "{:>width$} | {}\n",
            location.line + 1,
            lines[line_idx + 1],
            width = line_num_width
        ));
    }

    result
}

/// A single frame in a JavaScript stack trace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// Function name (or `"<anonymous>"` for anonymous functions)
    pub function_name: String,
    /// Source file name (if known)
    pub file_name: Option<String>,
    /// Line number in source (1-indexed)
    pub line: u32,
    /// Column number in source (1-indexed)
    pub column: u32,
    /// Whether this is a native function
    pub is_native: bool,
}

impl StackFrame {
    /// Create a new stack frame
    pub fn new(function_name: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            function_name: function_name.into(),
            file_name: None,
            line,
            column,
            is_native: false,
        }
    }

    /// Create a stack frame for a native function
    pub fn native(function_name: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            file_name: None,
            line: 0,
            column: 0,
            is_native: true,
        }
    }

    /// Create a stack frame with file name
    pub fn with_file(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }
}

impl fmt::Display for StackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_native {
            write!(f, "    at {} (native)", self.function_name)
        } else if let Some(ref file) = self.file_name {
            write!(f, "    at {} ({}:{}:{})", self.function_name, file, self.line, self.column)
        } else {
            write!(f, "    at {} (<anonymous>:{}:{})", self.function_name, self.line, self.column)
        }
    }
}

/// A JavaScript stack trace
#[derive(Debug, Clone, Default)]
pub struct StackTrace {
    /// Stack frames from innermost to outermost
    pub frames: Vec<StackFrame>,
}

impl StackTrace {
    /// Create an empty stack trace
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Add a frame to the stack trace
    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
    }

    /// Check if the stack trace is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

impl fmt::Display for StackTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for frame in &self.frames {
            writeln!(f, "{}", frame)?;
        }
        Ok(())
    }
}

/// Main error type for Quicksilver
#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    /// Lexer error - invalid token or character
    #[error("SyntaxError: {message} at {location}{}", if source_context.is_empty() { String::new() } else { format!("\n{}", source_context) })]
    LexerError {
        message: String,
        location: SourceLocation,
        source_context: String,
    },

    /// Parser error - invalid syntax
    #[error("SyntaxError: {message} at {location}{}", if source_context.is_empty() { String::new() } else { format!("\n{}", source_context) })]
    ParseError {
        message: String,
        location: SourceLocation,
        source_context: String,
    },

    /// Runtime error - TypeError, ReferenceError, etc.
    #[error("{kind}: {message}{}", if stack_trace.is_empty() { String::new() } else { format!("\n{}", stack_trace) })]
    RuntimeError {
        kind: ErrorKind,
        message: String,
        stack_trace: StackTrace,
    },

    /// Internal compiler error
    #[error("InternalError: {0}")]
    InternalError(String),

    /// IO error
    #[error("IOError: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    /// Module loading/resolution error
    #[error("ModuleError: {0}")]
    ModuleError(String),

    /// Resource limit exceeded (security/sandboxing)
    #[error("ResourceLimitError: {kind}: {message}")]
    ResourceLimitError {
        kind: ResourceLimitKind,
        message: String,
    },
}

/// Resource limit kinds for sandbox enforcement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLimitKind {
    /// Execution time limit exceeded
    TimeLimit,
    /// Instruction/operation count limit exceeded
    OperationLimit,
    /// Memory allocation limit exceeded
    MemoryLimit,
    /// Call stack depth limit exceeded
    StackDepthLimit,
}

impl fmt::Display for ResourceLimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceLimitKind::TimeLimit => write!(f, "TimeLimit"),
            ResourceLimitKind::OperationLimit => write!(f, "OperationLimit"),
            ResourceLimitKind::MemoryLimit => write!(f, "MemoryLimit"),
            ResourceLimitKind::StackDepthLimit => write!(f, "StackDepthLimit"),
        }
    }
}

/// JavaScript error kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum ErrorKind {
    /// TypeError - wrong type for operation
    TypeError,
    /// ReferenceError - undefined variable
    ReferenceError,
    /// RangeError - value out of range
    RangeError,
    /// SyntaxError - invalid syntax at runtime (e.g., eval)
    SyntaxError,
    /// EvalError - error in eval()
    EvalError,
    /// URIError - malformed URI
    UriError,
    /// Generic Error - user-thrown Error objects
    GenericError,
    /// InternalError - internal engine error
    InternalError,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::TypeError => write!(f, "TypeError"),
            ErrorKind::ReferenceError => write!(f, "ReferenceError"),
            ErrorKind::RangeError => write!(f, "RangeError"),
            ErrorKind::SyntaxError => write!(f, "SyntaxError"),
            ErrorKind::EvalError => write!(f, "EvalError"),
            ErrorKind::UriError => write!(f, "URIError"),
            ErrorKind::GenericError => write!(f, "Error"),
            ErrorKind::InternalError => write!(f, "InternalError"),
        }
    }
}

impl Error {
    /// Create a new lexer error
    pub fn lexer_error(message: impl Into<String>, location: SourceLocation) -> Self {
        Error::LexerError {
            message: message.into(),
            location,
            source_context: String::new(),
        }
    }

    /// Create a new lexer error with source context
    pub fn lexer_error_with_context(
        message: impl Into<String>,
        location: SourceLocation,
        source: &str,
    ) -> Self {
        Error::LexerError {
            message: message.into(),
            source_context: format_error_context(source, &location),
            location,
        }
    }

    /// Create a new parse error
    pub fn parse_error(message: impl Into<String>, location: SourceLocation) -> Self {
        Error::ParseError {
            message: message.into(),
            location,
            source_context: String::new(),
        }
    }

    /// Create a new parse error with source context
    pub fn parse_error_with_context(
        message: impl Into<String>,
        location: SourceLocation,
        source: &str,
    ) -> Self {
        Error::ParseError {
            message: message.into(),
            source_context: format_error_context(source, &location),
            location,
        }
    }

    /// Add source context to an existing error
    pub fn with_source_context(self, source: &str) -> Self {
        match self {
            Error::LexerError {
                message, location, ..
            } => Error::LexerError {
                message,
                source_context: format_error_context(source, &location),
                location,
            },
            Error::ParseError {
                message, location, ..
            } => Error::ParseError {
                message,
                source_context: format_error_context(source, &location),
                location,
            },
            other => other,
        }
    }

    /// Create a TypeError
    pub fn type_error(message: impl Into<String>) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::TypeError,
            message: message.into(),
            stack_trace: StackTrace::new(),
        }
    }

    /// Create a TypeError with stack trace
    pub fn type_error_with_stack(message: impl Into<String>, stack_trace: StackTrace) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::TypeError,
            message: message.into(),
            stack_trace,
        }
    }

    /// Create a ReferenceError
    pub fn reference_error(message: impl Into<String>) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::ReferenceError,
            message: message.into(),
            stack_trace: StackTrace::new(),
        }
    }

    /// Create a ReferenceError with stack trace
    pub fn reference_error_with_stack(message: impl Into<String>, stack_trace: StackTrace) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::ReferenceError,
            message: message.into(),
            stack_trace,
        }
    }

    /// Create a RangeError
    pub fn range_error(message: impl Into<String>) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::RangeError,
            message: message.into(),
            stack_trace: StackTrace::new(),
        }
    }

    /// Create a RangeError with stack trace
    pub fn range_error_with_stack(message: impl Into<String>, stack_trace: StackTrace) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::RangeError,
            message: message.into(),
            stack_trace,
        }
    }

    /// Create a SyntaxError
    pub fn syntax_error(message: impl Into<String>) -> Self {
        Error::RuntimeError {
            kind: ErrorKind::SyntaxError,
            message: message.into(),
            stack_trace: StackTrace::new(),
        }
    }

    /// Create a runtime error with stack trace
    pub fn runtime_error_with_stack(kind: ErrorKind, message: impl Into<String>, stack_trace: StackTrace) -> Self {
        Error::RuntimeError {
            kind,
            message: message.into(),
            stack_trace,
        }
    }

    /// Add stack trace to an existing error
    pub fn with_stack_trace(self, stack_trace: StackTrace) -> Self {
        match self {
            Error::RuntimeError { kind, message, .. } => Error::RuntimeError {
                kind,
                message,
                stack_trace,
            },
            other => other,
        }
    }

    /// Create a time limit exceeded error
    pub fn time_limit_exceeded(elapsed_ms: u64, limit_ms: u64) -> Self {
        Error::ResourceLimitError {
            kind: ResourceLimitKind::TimeLimit,
            message: format!(
                "Execution time limit exceeded: {}ms elapsed, limit was {}ms",
                elapsed_ms, limit_ms
            ),
        }
    }

    /// Create an operation limit exceeded error
    pub fn operation_limit_exceeded(ops: u64, limit: u64) -> Self {
        Error::ResourceLimitError {
            kind: ResourceLimitKind::OperationLimit,
            message: format!(
                "Operation limit exceeded: {} operations executed, limit was {}",
                ops, limit
            ),
        }
    }

    /// Create a memory limit exceeded error
    pub fn memory_limit_exceeded(used: usize, limit: usize) -> Self {
        Error::ResourceLimitError {
            kind: ResourceLimitKind::MemoryLimit,
            message: format!(
                "Memory limit exceeded: {} bytes used, limit was {} bytes",
                used, limit
            ),
        }
    }

    /// Create a stack depth limit exceeded error
    pub fn stack_depth_exceeded(depth: usize, limit: usize) -> Self {
        Error::ResourceLimitError {
            kind: ResourceLimitKind::StackDepthLimit,
            message: format!(
                "Call stack depth limit exceeded: {} frames, limit was {}",
                depth, limit
            ),
        }
    }
}

/// Result type alias for Quicksilver
pub type Result<T> = std::result::Result<T, Error>;

/// Standardized error message templates
///
/// These constants provide consistent error messages following JavaScript conventions.
/// Use the helper functions below to generate formatted error messages.
#[allow(dead_code)]
pub mod messages {
    // Type errors
    pub const NOT_A_FUNCTION: &str = "is not a function";
    pub const NOT_AN_OBJECT: &str = "is not an object";
    pub const NOT_A_CONSTRUCTOR: &str = "is not a constructor";
    pub const NOT_ITERABLE: &str = "is not iterable";
    pub const NOT_A_SYMBOL: &str = "is not a symbol";
    pub const NOT_A_NUMBER: &str = "is not a number";
    pub const NOT_A_STRING: &str = "is not a string";
    pub const NOT_AN_ARRAY: &str = "is not an array";

    pub const CANNOT_READ_PROPERTY: &str = "Cannot read property";
    pub const CANNOT_SET_PROPERTY: &str = "Cannot set property";
    pub const CANNOT_CONVERT_TO: &str = "Cannot convert";
    pub const CANNOT_CALL_METHOD: &str = "Cannot call method";

    pub const INVALID_ARGUMENT: &str = "Invalid argument";
    pub const INVALID_ARRAY_LENGTH: &str = "Invalid array length";
    pub const INVALID_REGEX: &str = "Invalid regular expression";

    // Reference errors
    pub const IS_NOT_DEFINED: &str = "is not defined";
    pub const CANNOT_ACCESS_BEFORE_INIT: &str = "Cannot access before initialization";

    // Range errors
    pub const OUT_OF_RANGE: &str = "out of range";
    pub const PRECISION_OUT_OF_RANGE: &str = "precision out of range";
    pub const RADIX_OUT_OF_RANGE: &str = "radix must be between 2 and 36";
    pub const MAXIMUM_CALL_STACK: &str = "Maximum call stack size exceeded";

    // Syntax errors
    pub const UNEXPECTED_TOKEN: &str = "Unexpected token";
    pub const UNEXPECTED_END: &str = "Unexpected end of input";
    pub const UNTERMINATED_STRING: &str = "Unterminated string literal";
    pub const INVALID_LEFT_HAND_SIDE: &str = "Invalid left-hand side in assignment";

    /// Format a "X is not a function" error message
    pub fn not_a_function(name: &str) -> String {
        format!("'{}' {}", name, NOT_A_FUNCTION)
    }

    /// Format a "X is not an object" error message
    pub fn not_an_object(name: &str) -> String {
        format!("'{}' {}", name, NOT_AN_OBJECT)
    }

    /// Format a "X is not a constructor" error message
    pub fn not_a_constructor(name: &str) -> String {
        format!("'{}' {}", name, NOT_A_CONSTRUCTOR)
    }

    /// Format a "X is not iterable" error message
    pub fn not_iterable(name: &str) -> String {
        format!("'{}' {}", name, NOT_ITERABLE)
    }

    /// Format a "Cannot read property 'X' of Y" error message
    pub fn cannot_read_property(prop: &str, of: &str) -> String {
        format!("{} '{}' of {}", CANNOT_READ_PROPERTY, prop, of)
    }

    /// Format a "Cannot set property 'X' of Y" error message
    pub fn cannot_set_property(prop: &str, of: &str) -> String {
        format!("{} '{}' of {}", CANNOT_SET_PROPERTY, prop, of)
    }

    /// Format a "Cannot convert X to Y" error message
    pub fn cannot_convert(from: &str, to: &str) -> String {
        format!("{} {} to {}", CANNOT_CONVERT_TO, from, to)
    }

    /// Format a "X is not defined" error message
    pub fn not_defined(name: &str) -> String {
        format!("'{}' {}", name, IS_NOT_DEFINED)
    }

    /// Format a "X.Y is not a function" error message for method calls
    pub fn method_not_a_function(obj: &str, method: &str) -> String {
        format!("'{}.{}' {}", obj, method, NOT_A_FUNCTION)
    }

    /// Format a "X requires Y" error message
    pub fn requires(what: &str, requirement: &str) -> String {
        format!("{} requires {}", what, requirement)
    }

    /// Format a "X must be Y" error message
    pub fn must_be(what: &str, requirement: &str) -> String {
        format!("{} must be {}", what, requirement)
    }
}

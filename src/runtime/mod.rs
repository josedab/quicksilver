//! Runtime environment for Quicksilver
//!
//! This module provides the runtime execution environment including
//! the interpreter, value types, and built-in functions.

mod builtins;
pub mod intern;
mod value;
mod vm;

pub use value::{Function as JsFunction, Object, ObjectKind, Value};
pub use vm::{CallFrame, VM};

use crate::error::Result;
use std::path::Path;

/// The Quicksilver runtime
pub struct Runtime {
    vm: VM,
}

impl Runtime {
    /// Create a new runtime
    pub fn new() -> Self {
        let mut vm = VM::new();
        builtins::register_globals(&mut vm);
        Self { vm }
    }

    /// Evaluate JavaScript source code
    pub fn eval(&mut self, source: &str) -> Result<Value> {
        let chunk = crate::bytecode::compile(source)?;
        let result = self.vm.run(&chunk)?;
        // Process any pending timers and run the event loop
        self.vm.process_pending_timers();
        self.vm.run_event_loop()?;
        Ok(result)
    }

    /// Evaluate JavaScript source code from a file
    /// This sets up proper module resolution relative to the file's directory
    pub fn eval_file(&mut self, path: &Path, source: &str) -> Result<Value> {
        // Set current file for module resolution
        let file_path = path.to_str().unwrap_or("<unknown>");
        self.vm.set_source(file_path, source);

        // Set module base directory to the file's parent directory
        if let Some(parent) = path.parent() {
            self.vm.set_module_base_dir(parent.to_path_buf());
        }

        // Compile with source file for proper source maps
        let chunk = crate::bytecode::compile_with_source_file(source, file_path)?;
        let result = self.vm.run(&chunk)?;
        // Process any pending timers and run the event loop
        self.vm.process_pending_timers();
        self.vm.run_event_loop()?;
        Ok(result)
    }

    /// Get a global value
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.vm.get_global(name)
    }

    /// Set a global value
    pub fn set_global(&mut self, name: &str, value: Value) {
        self.vm.set_global(name, value);
    }

    /// Register a native function
    pub fn register_function<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&[Value]) -> Result<Value> + 'static,
    {
        self.vm.register_native(name, func);
    }

    /// Set source code for debugging
    pub fn set_source(&mut self, filename: &str, source: &str) {
        self.vm.set_source(filename, source);
    }

    /// Attach a debugger to the VM
    pub fn attach_debugger(&mut self, debugger: crate::debugger::TimeTravelDebugger) {
        self.vm.attach_debugger(debugger);
    }

    /// Detach the debugger from the VM
    pub fn detach_debugger(&mut self) -> Option<crate::debugger::TimeTravelDebugger> {
        self.vm.detach_debugger()
    }

    /// Get a reference to the debugger if attached
    pub fn get_debugger(&self) -> Option<std::rc::Rc<std::cell::RefCell<crate::debugger::TimeTravelDebugger>>> {
        self.vm.get_debugger()
    }

    /// Run the debugger's interactive REPL
    pub fn run_debugger_interactive(&mut self) {
        self.vm.debug_interactive();
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_eval() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("1 + 2").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_runtime_variables() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("let x = 10; x * 2").unwrap();
        assert_eq!(result, Value::Number(20.0));
    }

    #[test]
    fn test_runtime_functions() {
        let mut runtime = Runtime::new();
        let result = runtime
            .eval("function add(a, b) { return a + b; } add(3, 4)")
            .unwrap();
        assert_eq!(result, Value::Number(7.0));
    }
}

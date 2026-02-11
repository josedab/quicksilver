//! Virtual machine (interpreter) for Quicksilver
//!
//! This module implements the bytecode interpreter that executes
//! compiled JavaScript code.

mod cache;
mod types;

// Re-export public types from submodules
pub use cache::IC_SIZE;
pub use types::{CallFrame, ExceptionHandler, Microtask, ResourceLimits, Timer};

use cache::{compute_shape_id_raw, hash_property_name, InlineCacheEntry};

use super::async_runtime::{AsyncExecutor, SuspendedAsyncFunction};
use super::value::{Function, GeneratorState, NativeFn, Object, ObjectKind, PromiseState, Value};
use crate::bytecode::{compile, Chunk, Opcode};
use crate::event_loop::EventLoop;
use crate::debugger::TimeTravelDebugger;
use crate::error::{Error, Result, StackFrame, StackTrace};
use crate::modules::{ModuleLoader, HmrModuleLoader, HmrUpdateResult};
use crate::effects::{EffectRegistry, EffectHandler, EffectOp, EffectValue, CloneableAny};
use crate::distributed::{DistributedRuntime, ClusterConfig, TaskId, ClusterError};
use crate::security::Sandbox;
use std::sync::Arc;
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Maximum call stack depth
const MAX_CALL_DEPTH: usize = 1024;

/// Maximum stack size
const MAX_STACK_SIZE: usize = 65536;

/// The Quicksilver virtual machine
pub struct VM {
    /// Value stack
    stack: Vec<Value>,
    /// Call stack
    frames: Vec<CallFrame>,
    /// Global variables
    globals: HashMap<String, Value>,
    /// Compiled functions (reserved for function caching)
    #[allow(dead_code)]
    functions: Vec<Rc<RefCell<Function>>>,
    /// Native functions
    natives: HashMap<String, NativeFn>,
    /// Current 'this' value
    this_value: Value,
    /// Current class context for super resolution
    current_class: Option<Value>,
    /// Exception handler stack
    exception_handlers: Vec<ExceptionHandler>,
    /// Microtask queue for Promise callbacks
    microtask_queue: Vec<Microtask>,
    /// Security sandbox for permission checks
    sandbox: Option<Rc<RefCell<Sandbox>>>,
    /// Time-travel debugger for recording and stepping through execution
    debugger: Option<Rc<RefCell<TimeTravelDebugger>>>,
    /// Source code for debugger display
    source_code: Option<String>,
    /// Current filename being executed
    current_file: Option<String>,
    /// Inline cache for property access (GetProperty optimization)
    inline_cache: Vec<InlineCacheEntry>,
    /// Target frame depth to return at (for nested function calls like super())
    return_at_depth: Option<usize>,
    /// Module loader for ES Modules support
    module_loader: ModuleLoader,
    /// HMR-enabled module loader (optional)
    hmr_loader: Option<HmrModuleLoader>,
    /// Whether HMR is enabled
    hmr_enabled: bool,
    /// Cache of evaluated module namespace objects by path
    module_cache: HashMap<String, Value>,
    /// Timer queue for setTimeout/setInterval
    timers: Vec<Timer>,
    /// Next timer ID
    next_timer_id: u64,
    /// Virtual time in milliseconds (for timer scheduling)
    virtual_time: u64,
    /// Resource limits configuration
    resource_limits: ResourceLimits,
    /// Execution start time
    execution_start: Option<Instant>,
    /// Number of operations executed
    operation_count: u64,
    /// Approximate memory usage tracking
    #[allow(dead_code)]
    memory_usage: usize,
    /// Unhandled promise rejections for tracking
    unhandled_rejections: Vec<Value>,
    /// Whether to warn on unhandled rejections
    warn_unhandled_rejections: bool,
    /// Async executor for managing suspended async functions
    async_executor: AsyncExecutor,
    /// Event loop for Promise/A+ compliance
    event_loop: EventLoop,
    /// Effect registry for algebraic effects
    effect_registry: EffectRegistry,
    /// Distributed runtime for cluster computing
    distributed_runtime: Option<DistributedRuntime>,
    /// Whether distributed computing is enabled
    distributed_enabled: bool,
}

impl VM {
    /// Create a new VM
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            globals: HashMap::default(),
            functions: Vec::with_capacity(16),
            natives: HashMap::default(),
            this_value: Value::Undefined,
            current_class: None,
            exception_handlers: Vec::with_capacity(8),
            microtask_queue: Vec::with_capacity(16),
            sandbox: None,
            debugger: None,
            source_code: None,
            current_file: None,
            inline_cache: vec![InlineCacheEntry::default(); IC_SIZE],
            return_at_depth: None,
            module_loader: ModuleLoader::new(),
            hmr_loader: None,
            hmr_enabled: false,
            module_cache: HashMap::default(),
            timers: Vec::with_capacity(8),
            next_timer_id: 1,
            virtual_time: 0,
            resource_limits: ResourceLimits::new(),
            execution_start: None,
            operation_count: 0,
            memory_usage: 0,
            unhandled_rejections: Vec::new(),
            warn_unhandled_rejections: true,
            async_executor: AsyncExecutor::new(),
            event_loop: EventLoop::new(),
            effect_registry: EffectRegistry::new(),
            distributed_runtime: None,
            distributed_enabled: false,
        }
    }

    /// Track an unhandled promise rejection
    pub fn track_unhandled_rejection(&mut self, reason: Value) {
        self.unhandled_rejections.push(reason.clone());
        if self.warn_unhandled_rejections {
            eprintln!(
                "UnhandledPromiseRejectionWarning: {}",
                reason.to_js_string()
            );
        }
    }

    /// Mark a rejection as handled (e.g., when .catch() is called)
    pub fn mark_rejection_handled(&mut self, _reason: &Value) {
        // In a full implementation, we'd track specific rejections
        // For now, we just clear the last one if it matches
        self.unhandled_rejections.pop();
    }

    /// Get unhandled rejections
    pub fn get_unhandled_rejections(&self) -> &[Value] {
        &self.unhandled_rejections
    }

    /// Set resource limits for the VM
    pub fn set_resource_limits(&mut self, limits: ResourceLimits) {
        self.resource_limits = limits;
    }

    /// Set resource limits from a sandbox configuration
    pub fn apply_sandbox_limits(&mut self) {
        if let Some(ref sandbox) = self.sandbox {
            let sandbox = sandbox.borrow();
            if let Some(time_limit) = sandbox.time_limit() {
                self.resource_limits.time_limit_ms = Some(time_limit);
            }
            if let Some(memory_limit) = sandbox.memory_limit() {
                self.resource_limits.memory_limit = Some(memory_limit);
            }
            if let Some(stack_limit) = sandbox.stack_limit() {
                self.resource_limits.stack_depth_limit = Some(stack_limit);
            }
        }
    }

    /// Reset resource tracking for a new execution
    fn reset_resource_tracking(&mut self) {
        self.execution_start = Some(Instant::now());
        self.operation_count = 0;
    }

    /// Check if any resource limits have been exceeded
    /// Returns an error if a limit is violated
    fn check_resource_limits(&mut self) -> Result<()> {
        self.operation_count += 1;

        // Stack depth is checked on every iteration (critical for recursion)
        self.check_stack_depth()?;

        // Other checks are done periodically to reduce overhead
        if !self.operation_count.is_multiple_of(self.resource_limits.check_interval) {
            return Ok(());
        }

        // Check time limit
        if let Some(time_limit) = self.resource_limits.time_limit_ms {
            if let Some(start) = self.execution_start {
                let elapsed = start.elapsed().as_millis() as u64;
                if elapsed > time_limit {
                    return Err(Error::time_limit_exceeded(elapsed, time_limit));
                }
            }
        }

        // Check operation limit
        if let Some(op_limit) = self.resource_limits.operation_limit {
            if self.operation_count > op_limit {
                return Err(Error::operation_limit_exceeded(self.operation_count, op_limit));
            }
        }

        // Check memory limit (approximate based on stack size)
        if let Some(mem_limit) = self.resource_limits.memory_limit {
            // Rough approximation: count stack entries * estimated value size
            let approx_memory = self.stack.len() * 64 + self.globals.len() * 128;
            if approx_memory > mem_limit {
                return Err(Error::memory_limit_exceeded(approx_memory, mem_limit));
            }
        }

        Ok(())
    }

    /// Check stack depth limit before pushing a new call frame
    fn check_stack_depth(&self) -> Result<()> {
        let limit = self.resource_limits.stack_depth_limit.unwrap_or(MAX_CALL_DEPTH);
        if self.frames.len() >= limit {
            return Err(Error::stack_depth_exceeded(self.frames.len(), limit));
        }
        Ok(())
    }

    /// Push a call frame with stack depth checking
    #[allow(dead_code)]
    fn push_frame(&mut self, frame: CallFrame) -> Result<()> {
        self.check_stack_depth()?;
        self.frames.push(frame);
        Ok(())
    }

    /// Set the base directory for module resolution
    pub fn set_module_base_dir(&mut self, base_dir: PathBuf) {
        self.module_loader = ModuleLoader::with_base_dir(base_dir.clone());
        // Also update HMR loader if enabled
        if self.hmr_enabled {
            self.hmr_loader = Some(HmrModuleLoader::with_config(
                base_dir,
                std::time::Duration::from_millis(500),
            ));
        }
    }

    /// Enable Hot Module Reloading
    ///
    /// When enabled, the VM will use an HMR-capable module loader that can
    /// detect file changes and reload modules without losing application state.
    ///
    /// # Example
    /// ```no_run
    /// use quicksilver::runtime::VM;
    /// let mut vm = VM::new();
    /// vm.enable_hmr();
    /// // Now modules can be hot-reloaded
    /// let updates = vm.check_hmr_updates();
    /// ```
    pub fn enable_hmr(&mut self) {
        if !self.hmr_enabled {
            self.hmr_enabled = true;
            let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            self.hmr_loader = Some(HmrModuleLoader::with_config(
                base_dir,
                Duration::from_millis(500),
            ));
        }
    }

    /// Enable HMR with custom configuration
    pub fn enable_hmr_with_config(&mut self, base_dir: PathBuf, poll_interval: Duration) {
        self.hmr_enabled = true;
        self.hmr_loader = Some(HmrModuleLoader::with_config(base_dir, poll_interval));
    }

    /// Disable Hot Module Reloading
    pub fn disable_hmr(&mut self) {
        self.hmr_enabled = false;
        self.hmr_loader = None;
    }

    /// Check if HMR is enabled
    pub fn is_hmr_enabled(&self) -> bool {
        self.hmr_enabled
    }

    /// Check for HMR updates and return any pending file changes
    ///
    /// This should be called periodically (e.g., in a dev server loop) to
    /// detect file changes and queue module updates.
    pub fn check_hmr_updates(&self) -> Vec<crate::hmr::FileChange> {
        if let Some(ref loader) = self.hmr_loader {
            loader.check_for_updates()
        } else {
            Vec::new()
        }
    }

    /// Apply pending HMR updates
    ///
    /// This reloads any modules that have changed since the last check.
    /// Returns the results of each update attempt.
    pub fn apply_hmr_updates(&mut self) -> Vec<HmrUpdateResult> {
        if let Some(ref loader) = self.hmr_loader {
            let results = loader.apply_pending_updates();

            // Invalidate cached module namespaces for updated modules
            for result in &results {
                if result.success {
                    self.module_cache.remove(&result.module_id);
                    // Also invalidate affected modules
                    for affected in &result.affected_modules {
                        self.module_cache.remove(affected);
                    }
                }
            }

            results
        } else {
            Vec::new()
        }
    }

    /// Accept HMR updates for a specific module
    ///
    /// This marks a module as able to accept hot updates without requiring
    /// a full page reload.
    pub fn accept_hmr(&self, module_path: &str) {
        if let Some(ref loader) = self.hmr_loader {
            loader.accept(&PathBuf::from(module_path));
        }
    }

    /// Register a callback to run when a module is hot-reloaded
    pub fn on_hmr_update<F>(&self, module_path: &str, callback: F)
    where
        F: Fn(&crate::modules::Module) + Send + Sync + 'static,
    {
        if let Some(ref loader) = self.hmr_loader {
            loader.on_update(module_path, callback);
        }
    }

    /// Invalidate a module and force a full reload on next access
    pub fn invalidate_module(&self, module_path: &str) {
        if let Some(ref loader) = self.hmr_loader {
            loader.invalidate(&PathBuf::from(module_path));
        }
    }

    // ==================== Algebraic Effects ====================

    /// Register an effect handler
    ///
    /// Effect handlers intercept `perform` operations and can return values
    /// to the caller. Multiple handlers can be registered for the same effect
    /// type - they are tried in reverse order (most recent first).
    ///
    /// # Example
    /// ```no_run
    /// use quicksilver::runtime::VM;
    /// use quicksilver::effects::LogHandler;
    /// use std::sync::Arc;
    ///
    /// let mut vm = VM::new();
    /// vm.register_effect_handler(Arc::new(LogHandler::new()));
    ///
    /// // Now `perform Log.log("message")` will be handled
    /// ```
    pub fn register_effect_handler(&mut self, handler: Arc<dyn EffectHandler>) {
        self.effect_registry.register(handler);
    }

    /// Handle an effect using the registered handlers
    ///
    /// This is called internally when a `perform` operation is executed.
    /// Returns the result value if a handler was found, None otherwise.
    fn handle_effect(&self, effect_type: &str, operation: &str, args: &[Value]) -> Option<Value> {
        // Convert Value args to CloneableAny args
        let cloneable_args: Vec<Box<dyn CloneableAny>> = args.iter().map(|v| {
            let boxed: Box<dyn CloneableAny> = match v {
                Value::Number(n) => Box::new(*n),
                Value::String(s) => Box::new(s.clone()),
                Value::Boolean(b) => Box::new(*b),
                Value::Null | Value::Undefined => Box::new(()),
                _ => Box::new(v.to_js_string()),
            };
            boxed
        }).collect();

        // Create effect value
        let effect = EffectValue {
            op: EffectOp::new(effect_type, operation),
            args: cloneable_args,
        };

        // Try to handle the effect
        if let Some(result) = self.effect_registry.handle(&effect) {
            // Convert the result back to a Value
            if let Some(n) = result.as_any().downcast_ref::<f64>() {
                return Some(Value::Number(*n));
            }
            if let Some(s) = result.as_any().downcast_ref::<String>() {
                return Some(Value::String(s.clone()));
            }
            if let Some(b) = result.as_any().downcast_ref::<bool>() {
                return Some(Value::Boolean(*b));
            }
            if let Some(i) = result.as_any().downcast_ref::<i64>() {
                return Some(Value::Number(*i as f64));
            }
            if let Some(u) = result.as_any().downcast_ref::<u64>() {
                return Some(Value::Number(*u as f64));
            }
            if result.as_any().downcast_ref::<()>().is_some() {
                return Some(Value::Undefined);
            }
            // Default: return undefined for handled effects with unknown result types
            return Some(Value::Undefined);
        }

        None
    }

    /// Get the effect registry for inspection or advanced usage
    pub fn effect_registry(&self) -> &EffectRegistry {
        &self.effect_registry
    }

    // ==================== Distributed Runtime Integration ====================

    /// Enable distributed computing with default configuration
    pub fn enable_distributed(&mut self) {
        self.distributed_runtime = Some(DistributedRuntime::new());
        self.distributed_enabled = true;
    }

    /// Enable distributed computing with custom configuration
    pub fn enable_distributed_with_config(&mut self, config: ClusterConfig) {
        self.distributed_runtime = Some(DistributedRuntime::with_config(config));
        self.distributed_enabled = true;
    }

    /// Connect to an existing cluster
    pub fn connect_to_cluster(&mut self, address: &str) -> std::result::Result<(), ClusterError> {
        let runtime = DistributedRuntime::connect(address)?;
        self.distributed_runtime = Some(runtime);
        self.distributed_enabled = true;
        Ok(())
    }

    /// Disable distributed computing
    pub fn disable_distributed(&mut self) {
        self.distributed_runtime = None;
        self.distributed_enabled = false;
    }

    /// Check if distributed computing is enabled
    pub fn is_distributed_enabled(&self) -> bool {
        self.distributed_enabled && self.distributed_runtime.is_some()
    }

    /// Submit a task to the cluster for distributed execution
    pub fn submit_distributed_task(&mut self, bytecode: Vec<u8>, args: Value) -> std::result::Result<TaskId, ClusterError> {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.submit_task(bytecode, args)
        } else {
            Err(ClusterError::ConnectionFailed("Distributed runtime not enabled".to_string()))
        }
    }

    /// Check if a distributed task has completed
    pub fn is_distributed_task_complete(&self, task_id: TaskId) -> bool {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.is_task_complete(task_id)
        } else {
            false
        }
    }

    /// Get the result of a distributed task
    pub fn get_distributed_task_result(&self, task_id: TaskId) -> std::result::Result<Option<Value>, ClusterError> {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.get_task_result(task_id)
        } else {
            Err(ClusterError::ConnectionFailed("Distributed runtime not enabled".to_string()))
        }
    }

    /// Cancel a distributed task
    pub fn cancel_distributed_task(&self, task_id: TaskId) -> bool {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.cancel_task(task_id)
        } else {
            false
        }
    }

    /// Spawn a new actor for message passing
    pub fn spawn_actor(&self) -> std::result::Result<u64, ClusterError> {
        if let Some(ref runtime) = self.distributed_runtime {
            Ok(runtime.spawn_actor())
        } else {
            Err(ClusterError::ConnectionFailed("Distributed runtime not enabled".to_string()))
        }
    }

    /// Send a message to an actor
    pub fn send_to_actor(&self, actor_id: u64, value: Value) -> std::result::Result<(), ClusterError> {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.send_to_actor(actor_id, value)
        } else {
            Err(ClusterError::ConnectionFailed("Distributed runtime not enabled".to_string()))
        }
    }

    /// Receive a message from an actor's mailbox
    pub fn receive_from_actor(&self, actor_id: u64) -> std::result::Result<Option<Value>, ClusterError> {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.receive_from_actor(actor_id)
        } else {
            Err(ClusterError::ConnectionFailed("Distributed runtime not enabled".to_string()))
        }
    }

    /// Get cluster information as a JavaScript value
    pub fn get_cluster_info(&self) -> Value {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.get_cluster_info()
        } else {
            Value::Undefined
        }
    }

    /// Tick the distributed runtime (check for timeouts, etc.)
    pub fn tick_distributed(&self) {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.tick();
        }
    }

    /// Get the number of pending distributed tasks
    pub fn pending_distributed_task_count(&self) -> usize {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.pending_task_count()
        } else {
            0
        }
    }

    /// Get the number of local actors
    pub fn actor_count(&self) -> usize {
        if let Some(ref runtime) = self.distributed_runtime {
            runtime.actor_count()
        } else {
            0
        }
    }

    /// Get a reference to the distributed runtime for advanced usage
    pub fn distributed_runtime(&self) -> Option<&DistributedRuntime> {
        self.distributed_runtime.as_ref()
    }

    /// Get the underlying cluster for advanced operations
    pub fn cluster(&self) -> Option<std::sync::Arc<crate::distributed::Cluster>> {
        self.distributed_runtime.as_ref().map(|r| r.cluster())
    }

    // ==================== End Distributed Runtime Integration ====================

    /// Get the current file path for module resolution
    fn get_current_path(&self) -> Option<PathBuf> {
        self.current_file.as_ref().map(PathBuf::from)
    }

    /// Load a module and return its namespace object
    fn load_module(&mut self, specifier: &str) -> Result<Value> {
        let referrer = self.get_current_path();

        // Resolve the module path
        let module_path = self
            .module_loader
            .resolve(specifier, referrer.as_deref())
            .map_err(|e| Error::ModuleError(e.to_string()))?;

        let module_id = module_path.to_string_lossy().to_string();

        // Check if module is already loaded and evaluated
        if let Some(cached_ns) = self.module_cache.get(&module_id) {
            return Ok(cached_ns.clone());
        }

        // Verify the module can be loaded (parse only, no execution yet)
        let _module = self
            .module_loader
            .load(specifier, referrer.as_deref())
            .map_err(|e| Error::ModuleError(e.to_string()))?;

        // Read the source code
        let source = std::fs::read_to_string(&module_path)
            .map_err(|e| Error::ModuleError(format!("Failed to read module: {}", e)))?;

        // Compile the module
        let chunk = compile(&source).map_err(|e| {
            Error::ModuleError(format!("Failed to compile module {}: {}", specifier, e))
        })?;

        // Create module namespace object
        let module_ns = Value::new_object();
        module_ns.set_property("__esModule", Value::Boolean(true));

        // Cache the module namespace before execution (for circular dependencies)
        self.module_cache
            .insert(module_id.clone(), module_ns.clone());

        // Save current state
        let saved_file = self.current_file.clone();
        let saved_exports_prefix = "__export__";

        // Clear any previous exports from globals
        let old_exports: Vec<String> = self
            .globals
            .keys()
            .filter(|k| k.starts_with(saved_exports_prefix))
            .cloned()
            .collect();

        // Set the current file for relative import resolution
        self.current_file = Some(module_id.clone());

        // Execute the module
        let result = self.run(&chunk);

        // Restore current file
        self.current_file = saved_file;

        // Handle execution errors
        if let Err(e) = result {
            // Remove from cache on error
            self.module_cache.remove(&module_id);
            return Err(Error::ModuleError(format!(
                "Failed to execute module {}: {}",
                specifier, e
            )));
        }

        // Collect exports from globals
        let export_keys: Vec<String> = self
            .globals
            .keys()
            .filter(|k| k.starts_with(saved_exports_prefix))
            .cloned()
            .collect();

        for key in export_keys {
            let export_name = key.strip_prefix(saved_exports_prefix).unwrap_or(&key);
            if let Some(value) = self.globals.get(&key) {
                module_ns.set_property(export_name, value.clone());
            }
        }

        // Update the cached namespace with exports
        self.module_cache
            .insert(module_id.clone(), module_ns.clone());

        // Clean up the old exports to avoid conflicts
        for key in old_exports {
            self.globals.remove(&key);
        }

        Ok(module_ns)
    }

    /// Create a promise for dynamic import
    /// Returns a Promise that resolves to the module namespace or rejects with an error
    fn create_dynamic_import_promise(&mut self, specifier: &str) -> Value {
        // Try to load the module synchronously
        // In a full async implementation, this would be done asynchronously
        match self.load_module(specifier) {
            Ok(module_ns) => {
                // Create a resolved promise with the module namespace
                Value::Object(Rc::new(RefCell::new(Object {
                    kind: ObjectKind::Promise {
                        state: PromiseState::Fulfilled,
                        value: Some(Box::new(module_ns)),
                        on_fulfilled: Vec::new(),
                        on_rejected: Vec::new(),
                    },
                    properties: HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None, cached_shape_id: None,
                })))
            }
            Err(e) => {
                // Create a rejected promise with the error
                let error_value = Value::new_error("ModuleError", &e.to_string());
                Value::Object(Rc::new(RefCell::new(Object {
                    kind: ObjectKind::Promise {
                        state: PromiseState::Rejected,
                        value: Some(Box::new(error_value)),
                        on_fulfilled: Vec::new(),
                        on_rejected: Vec::new(),
                    },
                    properties: HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None, cached_shape_id: None,
                })))
            }
        }
    }

    /// Set the security sandbox for this VM
    pub fn set_sandbox(&mut self, sandbox: Sandbox) {
        self.sandbox = Some(Rc::new(RefCell::new(sandbox)));
    }

    /// Get a reference to the sandbox if set
    pub fn get_sandbox(&self) -> Option<Rc<RefCell<Sandbox>>> {
        self.sandbox.clone()
    }

    /// Check if sandbox is enabled
    pub fn has_sandbox(&self) -> bool {
        self.sandbox.is_some()
    }

    /// Attach a debugger to this VM
    pub fn attach_debugger(&mut self, debugger: TimeTravelDebugger) {
        let debugger = Rc::new(RefCell::new(debugger));
        if let (Some(ref filename), Some(ref source)) = (&self.current_file, &self.source_code) {
            debugger.borrow_mut().load_source(filename, source);
        }
        self.debugger = Some(debugger);
    }

    /// Detach the debugger from this VM
    pub fn detach_debugger(&mut self) -> Option<TimeTravelDebugger> {
        self.debugger.take().map(|rc| {
            Rc::try_unwrap(rc)
                .map(|cell| cell.into_inner())
                .unwrap_or_else(|rc| rc.borrow().clone())
        })
    }

    /// Get a reference to the debugger if attached
    pub fn get_debugger(&self) -> Option<Rc<RefCell<TimeTravelDebugger>>> {
        self.debugger.clone()
    }

    /// Check if a debugger is attached
    pub fn has_debugger(&self) -> bool {
        self.debugger.is_some()
    }

    /// Set the source code and filename for debugging
    pub fn set_source(&mut self, filename: &str, source: &str) {
        self.current_file = Some(filename.to_string());
        self.source_code = Some(source.to_string());
        if let Some(ref debugger) = self.debugger {
            debugger.borrow_mut().load_source(filename, source);
        }
    }

    /// Record a debug step if debugger is attached
    fn debug_record_step(&mut self, opcode: Option<Opcode>, ip: usize, line: u32, description: &str) {
        if let Some(ref debugger) = self.debugger {
            // Collect locals from current frame
            let locals = self.collect_local_variables();
            debugger.borrow_mut().record_step(
                opcode,
                ip,
                line,
                &self.stack,
                &locals,
                description,
            );
        }
    }

    /// Check if debugger should break at this line
    fn debug_should_break(&mut self, line: u32) -> bool {
        if let Some(ref debugger) = self.debugger {
            debugger.borrow_mut().should_break(line)
        } else {
            false
        }
    }

    /// Run the debugger interactive REPL (pauses execution)
    pub fn debug_interactive(&mut self) {
        if let Some(ref debugger) = self.debugger {
            debugger.borrow_mut().run_interactive();
        }
    }

    /// Collect local variables for debugging
    fn collect_local_variables(&self) -> HashMap<String, Value> {
        let mut locals = HashMap::default();

        // Get variables from current frame
        if let Some(frame) = self.frames.last() {
            // Collect stack values as local slots
            let start = frame.bp;
            let end = self.stack.len().min(start + 20); // Limit to 20 locals

            for (i, idx) in (start..end).enumerate() {
                if let Some(value) = self.stack.get(idx) {
                    // Name as slot index since we don't have symbol names at runtime
                    locals.insert(format!("local_{}", i), value.clone());
                }
            }
        }

        // Also include globals that have been accessed
        for (name, value) in &self.globals {
            if !name.starts_with("__") && !name.contains('_') {
                // Only include user-visible globals (simple names)
                locals.insert(name.clone(), value.clone());
            }
        }

        locals
    }

    /// Get a global variable
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    /// Set a global variable
    pub fn set_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
    }

    /// Register a native function
    pub fn register_native<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&[Value]) -> Result<Value> + 'static,
    {
        self.natives.insert(name.to_string(), Rc::new(func));

        // Also add to globals
        let native_obj = Object {
            kind: ObjectKind::NativeFunction {
                name: name.to_string(),
                func: self.natives.get(name).unwrap().clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        };
        self.globals.insert(
            name.to_string(),
            Value::Object(Rc::new(RefCell::new(native_obj))),
        );
    }

    /// Schedule a microtask to be executed
    pub fn schedule_microtask(&mut self, callback: Value, argument: Value) {
        self.microtask_queue.push(Microtask { callback, argument });
    }

    /// Run all pending microtasks
    pub fn run_microtasks(&mut self) -> Result<()> {
        while !self.microtask_queue.is_empty() {
            let tasks: Vec<Microtask> = std::mem::take(&mut self.microtask_queue);
            for task in tasks {
                self.call_function(&task.callback, &[task.argument])?;
            }
        }
        Ok(())
    }

    /// Check if there are pending microtasks
    pub fn has_pending_microtasks(&self) -> bool {
        !self.microtask_queue.is_empty()
    }

    /// Schedule a timer (setTimeout or setInterval)
    pub fn schedule_timer(
        &mut self,
        callback: Value,
        delay: u64,
        args: Vec<Value>,
        repeating: bool,
    ) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;

        let fire_at = self.virtual_time + delay;
        self.timers.push(Timer {
            id,
            callback,
            args,
            delay,
            fire_at,
            repeating,
            cancelled: false,
        });

        id
    }

    /// Cancel a timer by ID
    pub fn cancel_timer(&mut self, id: u64) {
        for timer in &mut self.timers {
            if timer.id == id {
                timer.cancelled = true;
                break;
            }
        }
    }

    /// Check if there are pending timers
    pub fn has_pending_timers(&self) -> bool {
        self.timers.iter().any(|t| !t.cancelled)
    }

    /// Run all pending timers (event loop simulation)
    pub fn run_timers(&mut self) -> Result<()> {
        // Keep running until no more timers
        while self.has_pending_timers() {
            // Find the next timer to fire
            let next_fire_at = self
                .timers
                .iter()
                .filter(|t| !t.cancelled)
                .map(|t| t.fire_at)
                .min();

            if let Some(fire_at) = next_fire_at {
                // Advance virtual time
                self.virtual_time = fire_at;

                // Collect timers that should fire at this time
                let mut to_fire: Vec<(Value, Vec<Value>, bool, u64, u64)> = Vec::new();
                for timer in &self.timers {
                    if !timer.cancelled && timer.fire_at == fire_at {
                        to_fire.push((
                            timer.callback.clone(),
                            timer.args.clone(),
                            timer.repeating,
                            timer.delay,
                            timer.id,
                        ));
                    }
                }

                // Remove non-repeating timers, reschedule repeating ones
                self.timers.retain(|t| t.cancelled || t.fire_at != fire_at || t.repeating);

                for timer in &mut self.timers {
                    if timer.fire_at == fire_at && timer.repeating && !timer.cancelled {
                        timer.fire_at = self.virtual_time + timer.delay;
                    }
                }

                // Execute the callbacks
                for (callback, args, _repeating, _delay, _id) in to_fire {
                    self.call_function(&callback, &args)?;
                    // Run microtasks after each callback
                    self.run_microtasks()?;
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Run the event loop (timers + microtasks + async resumption)
    pub fn run_event_loop(&mut self) -> Result<()> {
        // First run any pending microtasks
        self.run_microtasks()?;

        // Resume any suspended async functions that are ready
        self.resume_ready_async_functions()?;

        // Then process timers
        self.run_timers()?;

        // Check for more ready async functions after timer processing
        self.resume_ready_async_functions()?;

        Ok(())
    }

    /// Resume suspended async functions whose awaited promises are ready
    fn resume_ready_async_functions(&mut self) -> Result<()> {
        while let Some(suspended) = self.async_executor.get_ready_function() {
            // Get the awaited value
            let awaited_value = match suspended.get_awaited_value() {
                Ok(v) => v,
                Err(e) => {
                    // Promise was rejected - reject the async function's result promise
                    let reason = Value::String(e.to_string());
                    self.event_loop.reject_promise(&suspended.result_promise, reason);
                    continue;
                }
            };

            // Restore execution state and continue
            match self.resume_async_function(suspended, awaited_value) {
                Ok(result) => {
                    // Async function completed - result promise already handled
                    // Run microtasks that might have been queued
                    self.run_microtasks()?;
                    // If result is a value, it means the function returned normally
                    // The promise resolution is handled in resume_async_function
                    let _ = result;
                }
                Err(e) => {
                    // Error during resumption - this shouldn't typically happen
                    // as errors are handled within resume_async_function
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Resume a suspended async function with the resolved value
    fn resume_async_function(
        &mut self,
        suspended: SuspendedAsyncFunction,
        resolved_value: Value,
    ) -> Result<Value> {
        // Restore the function's execution state
        let func = suspended.function.clone();
        let result_promise = suspended.result_promise.clone();

        // Restore stack state
        self.stack = suspended.stack;

        // Push the resolved value onto the stack (result of await expression)
        self.push(resolved_value)?;

        // Restore call frame
        let bp = suspended.bp;
        let mut frame = CallFrame::for_function(func.clone(), bp);
        frame.ip = suspended.ip;

        self.frames.push(frame);

        // Continue execution
        match self.execute() {
            Ok(result) => {
                // Async function completed successfully - fulfill the result promise
                self.event_loop.fulfill_promise(&result_promise, result.clone());
                Ok(result)
            }
            Err(e) => {
                // Async function threw an error - reject the result promise
                let reason = Value::String(e.to_string());
                self.event_loop.reject_promise(&result_promise, reason);
                Err(e)
            }
        }
    }

    /// Get a reference to the event loop
    pub fn event_loop(&mut self) -> &mut EventLoop {
        &mut self.event_loop
    }

    /// Get a reference to the async executor
    pub fn async_executor(&mut self) -> &mut AsyncExecutor {
        &mut self.async_executor
    }

    /// Check if there are any pending async operations
    pub fn has_pending_async(&self) -> bool {
        self.async_executor.has_suspended()
            || self.has_pending_microtasks()
            || self.has_pending_timers()
    }

    /// Process pending timer registrations and cancellations from native calls
    pub fn process_pending_timers(&mut self) {
        use super::builtins::{PENDING_CANCELS, PENDING_MICROTASKS, PENDING_TIMERS};

        // Process pending timer registrations
        PENDING_TIMERS.with(|timers| {
            let pending: Vec<_> = timers.borrow_mut().drain(..).collect();
            for timer in pending {
                // Use the pre-assigned ID but schedule with our method
                let fire_at = self.virtual_time + timer.delay;
                self.timers.push(Timer {
                    id: timer.id,
                    callback: timer.callback,
                    args: timer.args,
                    delay: timer.delay,
                    fire_at,
                    repeating: timer.repeating,
                    cancelled: false,
                });
            }
        });

        // Process pending cancellations
        PENDING_CANCELS.with(|cancels| {
            let pending: Vec<_> = cancels.borrow_mut().drain(..).collect();
            for id in pending {
                self.cancel_timer(id);
            }
        });

        // Process pending microtasks
        PENDING_MICROTASKS.with(|tasks| {
            let pending: Vec<_> = tasks.borrow_mut().drain(..).collect();
            for callback in pending {
                self.schedule_microtask(callback, Value::Undefined);
            }
        });
    }

    /// Capture the current stack trace
    pub fn capture_stack_trace(&self) -> StackTrace {
        let mut trace = StackTrace::new();

        for frame in self.frames.iter().rev() {
            let function_name = if let Some(ref func) = frame.function {
                let func_ref = func.borrow();
                func_ref.name.clone().unwrap_or_else(|| "<anonymous>".to_string())
            } else {
                "<main>".to_string()
            };

            // Get line and column numbers from chunk's source map info
            let offset = frame.ip.saturating_sub(1);
            let (line, column) = frame.chunk.get_location(offset);

            // Build stack frame with source file if available
            let mut stack_frame = StackFrame::new(function_name, line, column);
            if let Some(ref source_file) = frame.chunk.source_file {
                stack_frame = stack_frame.with_file(source_file.clone());
            } else if let Some(ref file) = self.current_file {
                stack_frame = stack_frame.with_file(file.clone());
            }
            trace.push(stack_frame);
        }

        trace
    }

    /// Add stack trace to an error
    fn error_with_stack(&self, error: Error) -> Error {
        error.with_stack_trace(self.capture_stack_trace())
    }

    /// Call a function with arguments (for callbacks and microtasks)
    pub fn call_function(&mut self, func: &Value, args: &[Value]) -> Result<Value> {
        match func {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::NativeFunction { func, .. } => {
                        let func = func.clone();
                        drop(obj_ref);
                        let result = func(args);
                        self.process_pending_timers();
                        result
                    }
                    ObjectKind::Function(f) => {
                        let func_rc = Rc::new(RefCell::new(f.clone()));
                        drop(obj_ref);

                        // Push function and arguments onto stack
                        self.push(func.clone())?;
                        for arg in args {
                            self.push(arg.clone())?;
                        }

                        // Set up call frame
                        let bp = self.stack.len() - args.len();
                        let frame = CallFrame::for_function(func_rc, bp);
                        self.frames.push(frame);

                        // Execute the function frame
                        let result = self.execute()?;
                        self.frames.pop();
                        Ok(result)
                    }
                    _ => Err(Error::type_error("Value is not a function")),
                }
            }
            _ => Err(Error::type_error("Value is not a function")),
        }
    }

    /// Call a function with a specific 'this' value (for super calls and method calls)
    pub fn call_function_with_this(
        &mut self,
        func: &Value,
        args: &[Value],
        this_value: Value,
    ) -> Result<Value> {
        let old_this = std::mem::replace(&mut self.this_value, this_value.clone());
        match func {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::NativeFunction { func, .. } => {
                        let func = func.clone();
                        drop(obj_ref);
                        let result = func(args);
                        self.this_value = old_this;
                        result
                    }
                    ObjectKind::Function(f) => {
                        let func_rc = Rc::new(RefCell::new(f.clone()));
                        drop(obj_ref);

                        // Save frame depth before the call
                        let original_frame_depth = self.frames.len();

                        // Push the callee as a placeholder (Return expects callee at bp-1)
                        self.push(func.clone())?;

                        // Push arguments onto stack (this is already set)
                        for arg in args {
                            self.push(arg.clone())?;
                        }

                        // Set up call frame with this value as constructor_this
                        // bp points to the first argument (after the callee)
                        let bp = self.stack.len() - args.len();
                        let mut frame = CallFrame::for_function(func_rc, bp);
                        frame.constructor_this = Some(this_value);
                        self.frames.push(frame);

                        // Execute until we return to original frame depth
                        let result = self.execute_until_depth(original_frame_depth);
                        self.this_value = old_this;
                        result
                    }
                    _ => {
                        self.this_value = old_this;
                        Err(Error::type_error("Value is not a function"))
                    }
                }
            }
            _ => {
                self.this_value = old_this;
                Err(Error::type_error("Value is not a function"))
            }
        }
    }

    /// ToPrimitive abstract operation (ECMAScript spec 7.1.1)
    /// Converts a value to a primitive, optionally with a preferred type hint
    pub fn to_primitive(&mut self, value: &Value, preferred_type: Option<&str>) -> Result<Value> {
        match value {
            // Primitives return themselves
            Value::Undefined | Value::Null | Value::Boolean(_) |
            Value::Number(_) | Value::BigInt(_) | Value::String(_) | Value::Symbol(_) => {
                Ok(value.clone())
            }
            Value::Object(obj) => {
                // Check for Symbol.toPrimitive method (ES6+)
                // For now, we use the OrdinaryToPrimitive algorithm

                let hint = preferred_type.unwrap_or("default");

                // Determine method order based on hint
                let method_names: &[&str] = match hint {
                    "string" => &["toString", "valueOf"],
                    _ => &["valueOf", "toString"], // "number" or "default"
                };

                for method_name in method_names {
                    // Try to get the method
                    let method = {
                        let obj_ref = obj.borrow();
                        obj_ref.get_property(method_name)
                    };

                    if let Some(method) = method {
                        // Check if it's callable
                        if let Value::Object(method_obj) = &method {
                            let is_callable = {
                                let method_ref = method_obj.borrow();
                                matches!(method_ref.kind,
                                    ObjectKind::Function(_) |
                                    ObjectKind::NativeFunction { .. } |
                                    ObjectKind::BoundFunction { .. })
                            };

                            if is_callable {
                                let result = self.call_function_with_this(&method, &[], value.clone())?;

                                // If result is primitive, return it
                                if !matches!(result, Value::Object(_)) {
                                    return Ok(result);
                                }
                            }
                        }
                    }
                }

                // If neither method returns a primitive, throw TypeError
                Err(Error::type_error("Cannot convert object to primitive value"))
            }
        }
    }

    /// ToNumber with proper object coercion (ECMAScript spec 7.1.4)
    pub fn to_number_coerced(&mut self, value: &Value) -> Result<f64> {
        match value {
            Value::Undefined => Ok(f64::NAN),
            Value::Null => Ok(0.0),
            Value::Boolean(true) => Ok(1.0),
            Value::Boolean(false) => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::BigInt(_) => Err(Error::type_error("Cannot convert BigInt to number")),
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Ok(0.0)
                } else {
                    Ok(trimmed.parse().unwrap_or(f64::NAN))
                }
            }
            Value::Symbol(_) => Err(Error::type_error("Cannot convert Symbol to number")),
            Value::Object(_) => {
                let primitive = self.to_primitive(value, Some("number"))?;
                self.to_number_coerced(&primitive)
            }
        }
    }

    /// ToString with proper object coercion (ECMAScript spec 7.1.17)
    pub fn to_string_coerced(&mut self, value: &Value) -> Result<String> {
        match value {
            Value::Undefined => Ok("undefined".to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Boolean(true) => Ok("true".to_string()),
            Value::Boolean(false) => Ok("false".to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    Ok("NaN".to_string())
                } else if n.is_infinite() {
                    Ok(if *n > 0.0 { "Infinity" } else { "-Infinity" }.to_string())
                } else if *n == 0.0 {
                    Ok("0".to_string())
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::BigInt(n) => Ok(n.to_string()),
            Value::String(s) => Ok(s.clone()),
            Value::Symbol(_) => Err(Error::type_error("Cannot convert Symbol to string")),
            Value::Object(_) => {
                let primitive = self.to_primitive(value, Some("string"))?;
                self.to_string_coerced(&primitive)
            }
        }
    }

    /// Run a bytecode chunk
    pub fn run(&mut self, chunk: &Chunk) -> Result<Value> {
        // Reset resource tracking at the start of execution
        self.reset_resource_tracking();

        // Apply sandbox limits if a sandbox is configured
        self.apply_sandbox_limits();

        let frame = CallFrame::new(chunk.clone());
        self.frames.push(frame);

        let result = self.execute();

        self.frames.pop();

        // Run any pending microtasks after main execution
        if result.is_ok() {
            self.run_microtasks()?;
        }

        result
    }

    /// Run a generator bytecode chunk from a specific instruction pointer
    /// Returns (value, new_ip, yielded) where:
    /// - value: the yielded or returned value
    /// - new_ip: the instruction pointer after yield (for resumption)
    /// - yielded: true if execution was suspended by yield, false if completed
    pub fn run_generator_from(&mut self, chunk: &Chunk, start_ip: usize) -> Result<(Value, usize, bool)> {
        let mut frame = CallFrame::new(chunk.clone());
        frame.ip = start_ip;
        self.frames.push(frame);

        // Run execution - the yield opcode will return early
        let result = self.execute();

        // Check if we yielded or completed
        let final_ip = self.frames.last().map(|f| f.ip).unwrap_or(chunk.code.len());
        let yielded = final_ip < chunk.code.len();

        self.frames.pop();

        match result {
            Ok(value) => Ok((value, final_ip, yielded)),
            Err(e) => Err(e),
        }
    }

    /// Execute until frames.len() returns to target_depth
    /// Used for nested function calls like super()
    fn execute_until_depth(&mut self, target_depth: usize) -> Result<Value> {
        let old_depth = self.return_at_depth;
        self.return_at_depth = Some(target_depth);
        let result = self.execute();
        self.return_at_depth = old_depth;
        result
    }

    /// Execute the current frame
    fn execute(&mut self) -> Result<Value> {
        loop {
            // Check resource limits (time, operations, memory)
            self.check_resource_limits()?;

            if self.frames.is_empty() {
                return Ok(Value::Undefined);
            }

            let frame = self.frames.last_mut().unwrap();
            if frame.ip >= frame.chunk.code.len() {
                return Ok(self.stack.pop().unwrap_or(Value::Undefined));
            }

            // Get line number for debugging
            let ip = frame.ip;
            let line = frame.chunk.get_line(ip);

            let opcode = Opcode::from_u8(frame.chunk.code[frame.ip]);
            frame.ip += 1;

            // Debug: record step and check breakpoints
            if self.debugger.is_some() {
                let opcode_desc = opcode.map(|o| format!("{:?}", o)).unwrap_or_else(|| "Unknown".to_string());
                self.debug_record_step(opcode, ip, line, &opcode_desc);

                // Check for breakpoints
                if self.debug_should_break(line) {
                    self.debug_interactive();
                    // Check if debugger was paused and user requested continue
                    if let Some(ref debugger) = self.debugger {
                        if debugger.borrow().is_paused() {
                            continue;
                        }
                    }
                }
            }

            match opcode {
                Some(Opcode::Nop) => {}

                Some(Opcode::Pop) => {
                    self.stack.pop();
                }

                Some(Opcode::Dup) => {
                    if let Some(value) = self.stack.last().cloned() {
                        self.push(value)?;
                    }
                }

                Some(Opcode::Swap) => {
                    let len = self.stack.len();
                    if len >= 2 {
                        self.stack.swap(len - 1, len - 2);
                    }
                }

                Some(Opcode::Constant) => {
                    let index = self.read_u16()?;
                    let value = self
                        .current_frame()
                        .chunk
                        .get_constant(index)
                        .cloned()
                        .unwrap_or(Value::Undefined);
                    self.push(value)?;
                }

                Some(Opcode::Undefined) => {
                    self.push(Value::Undefined)?;
                }

                Some(Opcode::Null) => {
                    self.push(Value::Null)?;
                }

                Some(Opcode::True) => {
                    self.push(Value::Boolean(true))?;
                }

                Some(Opcode::False) => {
                    self.push(Value::Boolean(false))?;
                }

                Some(Opcode::GetLocal) => {
                    let slot = self.read_u8()? as usize;
                    let bp = self.current_frame().bp;
                    let value = self
                        .stack
                        .get(bp + slot)
                        .cloned()
                        .unwrap_or(Value::Undefined);
                    self.push(value)?;
                }

                Some(Opcode::SetLocal) => {
                    let slot = self.read_u8()? as usize;
                    let bp = self.current_frame().bp;
                    let value = self.peek(0).clone();
                    if bp + slot < self.stack.len() {
                        self.stack[bp + slot] = value;
                    }
                }

                Some(Opcode::GetGlobal) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    match self.globals.get(&name).cloned() {
                        Some(value) => self.push(value)?,
                        None => {
                            let suggestion = self.suggest_similar_global(&name);
                            let msg = if let Some(similar) = suggestion {
                                format!("'{}' is not defined. Did you mean '{}'?", name, similar)
                            } else {
                                format!("'{}' is not defined", name)
                            };
                            return Err(Error::reference_error(msg));
                        }
                    }
                }

                Some(Opcode::SetGlobal) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.peek(0).clone();
                    self.globals.insert(name, value);
                }

                Some(Opcode::TryGetGlobal) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                    self.push(value)?;
                }

                Some(Opcode::DefineGlobal) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    self.globals.insert(name, value);
                }

                Some(Opcode::GetProperty) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);

                    // Check for Proxy first
                    if let Value::Object(obj_rc) = &obj {
                        let proxy_info = {
                            let obj_ref = obj_rc.borrow();
                            if let ObjectKind::Proxy { target, handler, revoked } = &obj_ref.kind {
                                Some((target.clone(), handler.clone(), *revoked))
                            } else {
                                None
                            }
                        };

                        if let Some((target, handler, revoked)) = proxy_info {
                            if revoked {
                                // Create TypeError for revoked proxy
                                let error_msg = "Cannot perform 'get' on a revoked proxy";
                                let error_value = Value::new_error("TypeError", error_msg);

                                // Check for exception handler
                                if let Some(handler) = self.exception_handlers.pop() {
                                    // Unwind stack to the handler's frame
                                    while self.frames.len() > handler.frame_index + 1 {
                                        self.frames.pop();
                                    }
                                    // Restore stack to handler's stack size
                                    self.stack.truncate(handler.stack_size);
                                    // Push the error value for the catch block
                                    self.push(error_value)?;
                                    // Jump to catch block
                                    if let Some(frame) = self.frames.last_mut() {
                                        frame.ip = handler.catch_ip;
                                    }
                                    continue;
                                } else {
                                    // No handler, propagate the error
                                    return Err(Error::type_error(error_msg));
                                }
                            }

                            // Check for get trap
                            if let Some(trap) = handler.get_property("get") {
                                if matches!(&trap, Value::Object(_)) {
                                    let property = Value::String(name.clone());
                                    let receiver = obj.clone();
                                    let target_val = (*target).clone();
                                    let result = self.call_function_with_this(
                                        &trap,
                                        &[target_val, property, receiver],
                                        *handler,
                                    )?;
                                    self.push(result)?;
                                    continue;
                                }
                            }

                            // No trap, forward to target
                            let value = (*target).get_property(&name).unwrap_or(Value::Undefined);
                            self.push(value)?;
                            continue;
                        }
                    }

                    // Check for getter first
                    if let Some(getter) = self.find_getter(&obj, &name) {
                        // Call the getter with obj as 'this'
                        let result = self.call_function_with_this(&getter, &[], obj)?;
                        self.push(result)?;
                    } else {
                        // Use fast path with inline cache
                        let value = self
                            .get_property_fast(&obj, &name)
                            .unwrap_or(Value::Undefined);
                        self.push(value)?;
                    }
                }

                Some(Opcode::SetProperty) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.peek(0);

                    // Check for Proxy first
                    if let Value::Object(obj_rc) = &obj {
                        let proxy_info = {
                            let obj_ref = obj_rc.borrow();
                            if let ObjectKind::Proxy { target, handler, revoked } = &obj_ref.kind {
                                Some((target.clone(), handler.clone(), *revoked))
                            } else {
                                None
                            }
                        };

                        if let Some((target, handler, revoked)) = proxy_info {
                            if revoked {
                                // Create TypeError for revoked proxy
                                let error_msg = "Cannot perform 'set' on a revoked proxy";
                                let error_value = Value::new_error("TypeError", error_msg);

                                // Check for exception handler
                                if let Some(handler) = self.exception_handlers.pop() {
                                    // Unwind stack to the handler's frame
                                    while self.frames.len() > handler.frame_index + 1 {
                                        self.frames.pop();
                                    }
                                    // Restore stack to handler's stack size
                                    self.stack.truncate(handler.stack_size);
                                    // Push the error value for the catch block
                                    self.push(error_value)?;
                                    // Jump to catch block
                                    if let Some(frame) = self.frames.last_mut() {
                                        frame.ip = handler.catch_ip;
                                    }
                                    continue;
                                } else {
                                    // No handler, propagate the error
                                    return Err(Error::type_error(error_msg));
                                }
                            }

                            // Try to get the 'set' trap from the handler
                            if let Some(trap) = handler.get_property("set") {
                                if let Value::Object(_) = &trap {
                                    // Call the trap: trap(target, property, value, receiver)
                                    let property = Value::String(name.clone());
                                    let receiver = obj.clone();
                                    self.call_function_with_this(
                                        &trap,
                                        &[*target, property, value.clone(), receiver],
                                        *handler,
                                    )?;
                                    self.stack.pop();
                                    self.push(value)?;
                                    continue;
                                }
                            }

                            // No trap, set on target
                            target.set_property(&name, value.clone());
                            self.stack.pop();
                            self.push(value)?;
                            continue;
                        }
                    }

                    // Check for setter first
                    if let Some(setter) = self.find_setter(obj, &name) {
                        // Call the setter with obj as 'this' and value as argument
                        let obj_clone = obj.clone();
                        self.stack.pop(); // Remove object from stack
                        self.call_function_with_this(&setter, std::slice::from_ref(&value), obj_clone)?;
                        self.push(value)?;
                    } else {
                        obj.set_property(&name, value.clone());
                        // Replace object with value as result
                        self.stack.pop();
                        self.push(value)?;
                    }
                }

                Some(Opcode::DefineProperty) => {
                    let _index = self.read_u16()?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    let key = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.peek(0);
                    let key_str = key.to_js_string();
                    obj.set_property(&key_str, value);
                }

                Some(Opcode::GetPrivateField) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);

                    let value = if let Value::Object(obj_ref) = &obj {
                        let obj_borrow = obj_ref.borrow();
                        obj_borrow
                            .private_fields
                            .get(&name)
                            .cloned()
                            .unwrap_or(Value::Undefined)
                    } else {
                        return Err(Error::type_error(
                            "Cannot read private field of non-object",
                        ));
                    };
                    self.push(value)?;
                }

                Some(Opcode::SetPrivateField) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.peek(0);

                    if let Value::Object(obj_ref) = &obj {
                        obj_ref.borrow_mut().private_fields.insert(name, value.clone());
                        // Replace object with value as result
                        self.stack.pop();
                        self.push(value)?;
                    } else {
                        return Err(Error::type_error(
                            "Cannot set private field of non-object",
                        ));
                    }
                }

                Some(Opcode::DefinePrivateField) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.peek(0);

                    if let Value::Object(obj_ref) = &obj {
                        obj_ref.borrow_mut().private_fields.insert(name, value);
                    } else {
                        return Err(Error::type_error(
                            "Cannot define private field on non-object",
                        ));
                    }
                }

                Some(Opcode::Spread) => {
                    // Spread source object properties into target object
                    let source = self.stack.pop().unwrap_or(Value::Undefined);
                    let target = self.peek(0);

                    // Copy all enumerable own properties from source to target
                    if let Value::Object(source_obj) = &source {
                        let source_ref = source_obj.borrow();
                        // Copy properties
                        for (key, value) in source_ref.properties.iter() {
                            target.set_property(key, value.clone());
                        }
                        // Handle array elements
                        if let ObjectKind::Array(arr) = &source_ref.kind {
                            for (i, val) in arr.iter().enumerate() {
                                target.set_property(&i.to_string(), val.clone());
                            }
                        }
                    }
                }

                Some(Opcode::GetElement) => {
                    let index = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let key = index.to_js_string();
                    let value = obj.get_property(&key).unwrap_or(Value::Undefined);
                    self.push(value)?;
                }

                Some(Opcode::SetElement) => {
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    let index = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let key = index.to_js_string();
                    obj.set_property(&key, value.clone());
                    self.push(value)?;
                }

                Some(Opcode::Add) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match (&a, &b) {
                        (Value::String(s1), Value::String(s2)) => {
                            Value::String(format!("{}{}", s1, s2))
                        }
                        (Value::String(s), _) => {
                            Value::String(format!("{}{}", s, b.to_js_string()))
                        }
                        (_, Value::String(s)) => {
                            Value::String(format!("{}{}", a.to_js_string(), s))
                        }
                        (Value::BigInt(n1), Value::BigInt(n2)) => Value::BigInt(n1 + n2),
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(Error::type_error(
                                "Cannot mix BigInt and other types in arithmetic operations",
                            ));
                        }
                        _ => Value::Number(a.to_number() + b.to_number()),
                    };
                    self.push(result)?;
                }

                Some(Opcode::Sub) => {
                    self.binary_op_with_bigint(|a, b| a - b, |a, b| a - b)?;
                }

                Some(Opcode::Mul) => {
                    self.binary_op_with_bigint(|a, b| a * b, |a, b| a * b)?;
                }

                Some(Opcode::Div) => {
                    self.binary_op_with_bigint(|a, b| a / b, |a, b| a / b)?;
                }

                Some(Opcode::Mod) => {
                    self.binary_op_with_bigint(|a, b| a % b, |a, b| a % b)?;
                }

                Some(Opcode::Pow) => {
                    // BigInt exponentiation requires non-negative exponent
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match (&a, &b) {
                        (Value::BigInt(base), Value::BigInt(exp)) => {
                            use num_traits::ToPrimitive;
                            if let Some(e) = exp.to_u32() {
                                Value::BigInt(base.pow(e))
                            } else {
                                return Err(Error::type_error("BigInt exponent must be non-negative"));
                            }
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(Error::type_error(
                                "Cannot mix BigInt and other types in arithmetic operations",
                            ));
                        }
                        _ => Value::Number(a.to_number().powf(b.to_number())),
                    };
                    self.push(result)?;
                }

                Some(Opcode::Neg) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match a {
                        Value::BigInt(n) => Value::BigInt(-n),
                        _ => Value::Number(-a.to_number()),
                    };
                    self.push(result)?;
                }

                Some(Opcode::Increment) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match a {
                        Value::BigInt(n) => Value::BigInt(n + 1),
                        _ => Value::Number(a.to_number() + 1.0),
                    };
                    self.push(result)?;
                }

                Some(Opcode::Decrement) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match a {
                        Value::BigInt(n) => Value::BigInt(n - 1),
                        _ => Value::Number(a.to_number() - 1.0),
                    };
                    self.push(result)?;
                }

                Some(Opcode::BitwiseNot) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match a {
                        Value::BigInt(n) => Value::BigInt(!n),
                        _ => {
                            let n = a.to_number() as i32;
                            Value::Number((!n) as f64)
                        }
                    };
                    self.push(result)?;
                }

                Some(Opcode::BitwiseAnd) => {
                    self.binary_bitwise_op(|a, b| a & b, |a, b| a & b)?;
                }

                Some(Opcode::BitwiseOr) => {
                    self.binary_bitwise_op(|a, b| a | b, |a, b| a | b)?;
                }

                Some(Opcode::BitwiseXor) => {
                    self.binary_bitwise_op(|a, b| a ^ b, |a, b| a ^ b)?;
                }

                Some(Opcode::Shl) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match (&a, &b) {
                        (Value::BigInt(n), Value::BigInt(shift)) => {
                            use num_traits::ToPrimitive;
                            let shift_amount = shift.to_u64().unwrap_or(0);
                            Value::BigInt(n.clone() << shift_amount)
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(Error::type_error(
                                "Cannot mix BigInt and other types in shift operations",
                            ));
                        }
                        _ => {
                            let shift = (b.to_number() as u32) & 0x1f;
                            let result = (a.to_number() as i32) << shift;
                            Value::Number(result as f64)
                        }
                    };
                    self.push(result)?;
                }

                Some(Opcode::Shr) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    let result = match (&a, &b) {
                        (Value::BigInt(n), Value::BigInt(shift)) => {
                            use num_traits::ToPrimitive;
                            let shift_amount = shift.to_u64().unwrap_or(0);
                            Value::BigInt(n.clone() >> shift_amount)
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(Error::type_error(
                                "Cannot mix BigInt and other types in shift operations",
                            ));
                        }
                        _ => {
                            let shift = (b.to_number() as u32) & 0x1f;
                            let result = (a.to_number() as i32) >> shift;
                            Value::Number(result as f64)
                        }
                    };
                    self.push(result)?;
                }

                Some(Opcode::UShr) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    // BigInt doesn't support unsigned right shift in JS
                    if matches!(a, Value::BigInt(_)) || matches!(b, Value::BigInt(_)) {
                        return Err(Error::type_error(
                            "BigInts have no unsigned right shift, use >> instead",
                        ));
                    }
                    let shift = (b.to_number() as u32) & 0x1f;
                    let result = (a.to_number() as u32) >> shift;
                    self.push(Value::Number(result as f64))?;
                }

                Some(Opcode::Eq) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::Boolean(a.equals(&b)))?;
                }

                Some(Opcode::Ne) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::Boolean(!a.equals(&b)))?;
                }

                Some(Opcode::StrictEq) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::Boolean(a.strict_equals(&b)))?;
                }

                Some(Opcode::StrictNe) => {
                    let b = self.stack.pop().unwrap_or(Value::Undefined);
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::Boolean(!a.strict_equals(&b)))?;
                }

                Some(Opcode::Lt) => {
                    self.binary_compare_op(|a, b| a < b)?;
                }

                Some(Opcode::Le) => {
                    self.binary_compare_op(|a, b| a <= b)?;
                }

                Some(Opcode::Gt) => {
                    self.binary_compare_op(|a, b| a > b)?;
                }

                Some(Opcode::Ge) => {
                    self.binary_compare_op(|a, b| a >= b)?;
                }

                Some(Opcode::Not) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::Boolean(!a.to_boolean()))?;
                }

                Some(Opcode::Typeof) => {
                    let a = self.stack.pop().unwrap_or(Value::Undefined);
                    self.push(Value::String(a.type_of().to_string()))?;
                }

                Some(Opcode::Void) => {
                    self.stack.pop();
                    self.push(Value::Undefined)?;
                }

                Some(Opcode::Delete) => {
                    // Simplified: just return true
                    // For computed property deletes
                    self.stack.pop();
                    self.push(Value::Boolean(true))?;
                }

                Some(Opcode::DeleteProperty) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);

                    // Check for Proxy first
                    if let Value::Object(obj_rc) = &obj {
                        let proxy_info = {
                            let obj_ref = obj_rc.borrow();
                            if let ObjectKind::Proxy { target, handler, revoked } = &obj_ref.kind {
                                Some((target.clone(), handler.clone(), *revoked))
                            } else {
                                None
                            }
                        };

                        if let Some((target, handler, revoked)) = proxy_info {
                            if revoked {
                                return Err(Error::type_error("Cannot perform 'deleteProperty' on a revoked proxy"));
                            }

                            // Check for deleteProperty trap
                            if let Some(trap) = handler.get_property("deleteProperty") {
                                if matches!(&trap, Value::Object(_)) {
                                    let property = Value::String(name.clone());
                                    let target_val = (*target).clone();
                                    let result = self.call_function_with_this(
                                        &trap,
                                        &[target_val, property],
                                        *handler,
                                    )?;
                                    self.push(Value::Boolean(result.to_boolean()))?;
                                    continue;
                                }
                            }

                            // No trap, delete from target
                            let deleted = target.delete_property(&name);
                            self.push(Value::Boolean(deleted))?;
                            continue;
                        }
                    }

                    // Non-proxy: delete property
                    let deleted = obj.delete_property(&name);
                    self.push(Value::Boolean(deleted))?;
                }

                Some(Opcode::In) => {
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let key = self.stack.pop().unwrap_or(Value::Undefined);
                    let key_str = key.to_js_string();

                    // Check for Proxy first
                    if let Value::Object(obj_rc) = &obj {
                        let proxy_info = {
                            let obj_ref = obj_rc.borrow();
                            if let ObjectKind::Proxy { target, handler, revoked } = &obj_ref.kind {
                                Some((target.clone(), handler.clone(), *revoked))
                            } else {
                                None
                            }
                        };

                        if let Some((target, handler, revoked)) = proxy_info {
                            if revoked {
                                return Err(Error::type_error("Cannot perform 'has' on a revoked proxy"));
                            }

                            // Check for has trap
                            if let Some(trap) = handler.get_property("has") {
                                if matches!(&trap, Value::Object(_)) {
                                    let property = Value::String(key_str.clone());
                                    let target_val = (*target).clone();
                                    let result = self.call_function_with_this(
                                        &trap,
                                        &[target_val, property],
                                        *handler,
                                    )?;
                                    self.push(Value::Boolean(result.to_boolean()))?;
                                    continue;
                                }
                            }

                            // No trap, check target
                            let result = (*target).get_property(&key_str).is_some();
                            self.push(Value::Boolean(result))?;
                            continue;
                        }
                    }

                    // Non-proxy: check property
                    let result = obj.get_property(&key_str).is_some();
                    self.push(Value::Boolean(result))?;
                }

                Some(Opcode::Instanceof) => {
                    let constructor = self.stack.pop().unwrap_or(Value::Undefined);
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);

                    let result = match (&obj, &constructor) {
                        (Value::Object(obj_ref), Value::Object(ctor_ref)) => {
                            // Check if obj's prototype chain includes constructor
                            let mut current_proto = obj_ref.borrow().prototype.clone();
                            let mut found = false;

                            // Walk the prototype chain
                            while let Some(proto) = current_proto {
                                if Rc::ptr_eq(&proto, ctor_ref) {
                                    found = true;
                                    break;
                                }

                                // For classes, also check the superclass chain
                                if let ObjectKind::Class { .. } = &proto.borrow().kind {
                                    let mut current_class = Some(proto.clone());
                                    while let Some(class) = current_class {
                                        if Rc::ptr_eq(&class, ctor_ref) {
                                            found = true;
                                            break;
                                        }
                                        // Get superclass
                                        let class_ref = class.borrow();
                                        if let ObjectKind::Class { super_class, .. } = &class_ref.kind {
                                            current_class = super_class.as_ref().and_then(|sc| {
                                                if let Value::Object(obj) = sc.as_ref() {
                                                    Some(obj.clone())
                                                } else {
                                                    None
                                                }
                                            });
                                        } else {
                                            current_class = None;
                                        }
                                    }
                                    if found {
                                        break;
                                    }
                                }

                                current_proto = proto.borrow().prototype.clone();
                            }
                            found
                        }
                        _ => false,
                    };

                    self.push(Value::Boolean(result))?;
                }

                Some(Opcode::Jump) => {
                    let offset = self.read_i16()?;
                    let frame = self.frames.last_mut().unwrap();
                    frame.ip = (frame.ip as isize + offset as isize) as usize;
                }

                Some(Opcode::JumpIfFalse) => {
                    let offset = self.read_i16()?;
                    let value = self.peek(0);
                    if !value.to_boolean() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }

                Some(Opcode::JumpIfTrue) => {
                    let offset = self.read_i16()?;
                    let value = self.peek(0);
                    if value.to_boolean() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }

                Some(Opcode::JumpIfNull) => {
                    let offset = self.read_i16()?;
                    let value = self.peek(0);
                    if value.is_nullish() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }

                Some(Opcode::JumpIfNotNull) => {
                    let offset = self.read_i16()?;
                    let value = self.peek(0);
                    if !value.is_nullish() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }

                Some(Opcode::Call) => {
                    let arg_count = self.read_u8()? as usize;
                    self.call_value(arg_count)?;
                }

                Some(Opcode::TailCall) => {
                    let arg_count = self.read_u8()? as usize;
                    self.tail_call_value(arg_count)?;
                }

                Some(Opcode::Return) => {
                    let result = self.stack.pop().unwrap_or(Value::Undefined);

                    // Pop frame
                    let frame = self.frames.pop().unwrap();
                    let is_async = frame.chunk.is_async;

                    // Clear locals and callee (bp - 1 is where the callee sits)
                    let callee_pos = if frame.bp > 0 { frame.bp - 1 } else { 0 };
                    self.stack.truncate(callee_pos);

                    // For constructor calls, return the 'this' object instead of return value
                    // (unless the return value is an object, per JS spec)
                    let final_result = if frame.is_constructor {
                        if let Some(this_val) = frame.constructor_this {
                            // If constructor explicitly returns an object, use that
                            // Otherwise use 'this'
                            match &result {
                                Value::Object(_) => result,
                                _ => this_val,
                            }
                        } else {
                            result
                        }
                    } else if is_async {
                        // Async function: wrap result in a resolved Promise
                        self.create_resolved_promise(result)
                    } else {
                        result
                    };

                    // Check if we've returned to target depth (for nested calls like super())
                    if let Some(target_depth) = self.return_at_depth {
                        if self.frames.len() == target_depth {
                            return Ok(final_result);
                        }
                    }

                    if self.frames.is_empty() {
                        return Ok(final_result);
                    }

                    self.push(final_result)?;
                }

                Some(Opcode::ReturnUndefined) => {
                    // Pop frame
                    let frame = self.frames.pop().unwrap();
                    let is_async = frame.chunk.is_async;

                    // Clear locals and callee (bp - 1 is where the callee sits)
                    let callee_pos = if frame.bp > 0 { frame.bp - 1 } else { 0 };
                    self.stack.truncate(callee_pos);

                    // For constructor calls, return the 'this' object
                    let result = if frame.is_constructor {
                        frame.constructor_this.unwrap_or(Value::Undefined)
                    } else if is_async {
                        // Async function: wrap undefined in a resolved Promise
                        self.create_resolved_promise(Value::Undefined)
                    } else {
                        Value::Undefined
                    };

                    // Check if we've returned to target depth (for nested calls like super())
                    if let Some(target_depth) = self.return_at_depth {
                        if self.frames.len() == target_depth {
                            return Ok(result);
                        }
                    }

                    if self.frames.is_empty() {
                        return Ok(result);
                    }

                    self.push(result)?;
                }

                Some(Opcode::New) => {
                    let arg_count = self.read_u8()? as usize;
                    self.new_instance(arg_count)?;
                }

                Some(Opcode::CallMethod) => {
                    let name_index = self.read_u16()?;
                    let arg_count = self.read_u8()? as usize;
                    let method_name = self.get_constant_string(name_index)?;
                    self.call_method(&method_name, arg_count)?;
                }

                Some(Opcode::CreateFunction) => {
                    let index = self.read_u16()?;
                    // Get the function from the constants pool
                    let func_value = self
                        .current_frame()
                        .chunk
                        .get_constant(index)
                        .cloned()
                        .unwrap_or(Value::Undefined);
                    self.push(func_value)?;
                }

                Some(Opcode::CreateClosure) => {
                    let _index = self.read_u16()?;
                    self.push(Value::new_function(Function::new(None, Chunk::default())))?;
                }

                Some(Opcode::CreateArray) => {
                    let count = self.read_u8()? as usize;
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.stack.pop().unwrap_or(Value::Undefined));
                    }
                    elements.reverse();
                    self.push(Value::new_array(elements))?;
                }

                Some(Opcode::CreateObject) => {
                    let _count = self.read_u8()?;
                    self.push(Value::new_object())?;
                }

                Some(Opcode::CreateClass) => {
                    let _index = self.read_u16()?;
                    // Simplified class creation
                    self.push(Value::new_object())?;
                }

                Some(Opcode::This) => {
                    self.push(self.this_value.clone())?;
                }

                Some(Opcode::Super) => {
                    // Push the superclass of the current class context
                    let super_value = self.get_super_class().unwrap_or(Value::Undefined);
                    self.push(super_value)?;
                }

                Some(Opcode::NewTarget) => {
                    self.push(Value::Undefined)?;
                }

                Some(Opcode::SetSuperClass) => {
                    // Stack: [class, super_class] -> [class]
                    let super_class = self.stack.pop().unwrap_or(Value::Undefined);
                    let class = self.stack.pop().unwrap_or(Value::Undefined);

                    if let Value::Object(class_obj) = &class {
                        let mut class_ref = class_obj.borrow_mut();
                        if let ObjectKind::Class {
                            super_class: ref mut sc,
                            prototype: ref mut proto,
                            getters: ref mut child_getters,
                            setters: ref mut child_setters,
                            ..
                        } = &mut class_ref.kind
                        {
                            // Set the superclass
                            *sc = Some(Box::new(super_class.clone()));

                            // Copy superclass prototype methods, getters, setters (only if not overridden)
                            if let Value::Object(super_obj) = &super_class {
                                let super_ref = super_obj.borrow();
                                if let ObjectKind::Class {
                                    prototype: super_proto,
                                    getters: super_getters,
                                    setters: super_setters,
                                    ..
                                } = &super_ref.kind
                                {
                                    // Copy methods
                                    for (k, v) in super_proto {
                                        proto.entry(k.clone()).or_insert(v.clone());
                                    }
                                    // Copy getters
                                    for (k, v) in super_getters {
                                        child_getters.entry(k.clone()).or_insert(v.clone());
                                    }
                                    // Copy setters
                                    for (k, v) in super_setters {
                                        child_setters.entry(k.clone()).or_insert(v.clone());
                                    }
                                }
                            }
                        }
                    }

                    self.push(class)?;
                }

                Some(Opcode::SuperCall) => {
                    // Call super constructor
                    let arg_count = self.read_u8()? as usize;

                    // Get arguments from stack
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().unwrap_or(Value::Undefined));
                    }
                    args.reverse();

                    // Get super constructor from current class
                    let mut result = Value::Undefined;
                    if let Some(super_class) = self.get_super_class() {
                        if let Value::Object(super_obj) = &super_class {
                            let super_ref = super_obj.borrow();
                            if let ObjectKind::Class { constructor, .. } = &super_ref.kind {
                                if let Some(ctor) = constructor {
                                    let ctor_value = Value::new_function(ctor.as_ref().clone());
                                    drop(super_ref);
                                    let this = self.this_value.clone();
                                    result = self.call_function_with_this(&ctor_value, &args, this)?;
                                }
                            }
                        }
                    }
                    // Push the result (or undefined if no super constructor found)
                    self.push(result)?;
                }

                Some(Opcode::SuperGet) => {
                    // Get property from super prototype
                    let name_index = self.read_u16()?;
                    let name = self.get_constant_string(name_index)?;

                    let result = if let Some(super_class) = self.get_super_class() {
                        if let Value::Object(super_obj) = &super_class {
                            let super_ref = super_obj.borrow();
                            if let ObjectKind::Class { prototype, .. } = &super_ref.kind {
                                prototype.get(&name).cloned().unwrap_or(Value::Undefined)
                            } else {
                                Value::Undefined
                            }
                        } else {
                            Value::Undefined
                        }
                    } else {
                        Value::Undefined
                    };
                    self.push(result)?;
                }

                Some(Opcode::GetIterator) => {
                    let obj = self.stack.pop().unwrap_or(Value::Undefined);
                    let iterator = match obj {
                        Value::Object(ref o) => {
                            let o_ref = o.borrow();
                            match &o_ref.kind {
                                ObjectKind::Array(arr) => {
                                    let arr = arr.clone();
                                    drop(o_ref);
                                    Value::Object(Rc::new(RefCell::new(Object {
                                        kind: ObjectKind::Iterator {
                                            values: arr,
                                            index: 0,
                                        },
                                        properties: HashMap::default(),
                                        private_fields: HashMap::default(),
                                        prototype: None, cached_shape_id: None,
                                    })))
                                }
                                ObjectKind::Iterator { .. } => {
                                    // Already an iterator, return it directly
                                    drop(o_ref);
                                    obj.clone()
                                }
                                ObjectKind::Generator { .. } => {
                                    // Generators are iterables - collect yields into an Iterator
                                    let yields = if let Some(Value::Object(arr)) = o_ref.properties.get("__yields__") {
                                        if let ObjectKind::Array(items) = &arr.borrow().kind {
                                            items.clone()
                                        } else { vec![] }
                                    } else { vec![] };
                                    drop(o_ref);
                                    Value::Object(Rc::new(RefCell::new(Object {
                                        kind: ObjectKind::Iterator {
                                            values: yields,
                                            index: 0,
                                        },
                                        properties: HashMap::default(),
                                        private_fields: HashMap::default(),
                                        prototype: None, cached_shape_id: None,
                                    })))
                                }
                                ObjectKind::Map(entries) => {
                                    // Map iterator yields [key, value] pairs
                                    let values: Vec<Value> = entries.iter().map(|(k, v)| {
                                        Value::new_array(vec![k.clone(), v.clone()])
                                    }).collect();
                                    drop(o_ref);
                                    Value::Object(Rc::new(RefCell::new(Object {
                                        kind: ObjectKind::Iterator { values, index: 0 },
                                        properties: HashMap::default(),
                                        private_fields: HashMap::default(),
                                        prototype: None, cached_shape_id: None,
                                    })))
                                }
                                ObjectKind::Set(items) => {
                                    // Set iterator yields values
                                    let values = items.clone();
                                    drop(o_ref);
                                    Value::Object(Rc::new(RefCell::new(Object {
                                        kind: ObjectKind::Iterator { values, index: 0 },
                                        properties: HashMap::default(),
                                        private_fields: HashMap::default(),
                                        prototype: None, cached_shape_id: None,
                                    })))
                                }
                                _ => {
                                    drop(o_ref);
                                    Value::Undefined
                                }
                            }
                        }
                        Value::String(s) => {
                            let values: Vec<Value> =
                                s.chars().map(|c| Value::String(c.to_string())).collect();
                            Value::Object(Rc::new(RefCell::new(Object {
                                kind: ObjectKind::Iterator { values, index: 0 },
                                properties: HashMap::default(),
                                private_fields: HashMap::default(),
                                prototype: None, cached_shape_id: None,
                            })))
                        }
                        _ => Value::Undefined,
                    };
                    self.push(iterator)?;
                }

                Some(Opcode::IteratorNext) => {
                    let iterator = self.peek(0).clone();
                    if let Value::Object(obj) = iterator {
                        let mut obj = obj.borrow_mut();
                        if let ObjectKind::Iterator { values, index } = &mut obj.kind {
                            if *index < values.len() {
                                let result = Value::new_object();
                                result.set_property("value", values[*index].clone());
                                result.set_property("done", Value::Boolean(false));
                                *index += 1;
                                self.push(result)?;
                            } else {
                                let result = Value::new_object();
                                result.set_property("value", Value::Undefined);
                                result.set_property("done", Value::Boolean(true));
                                self.push(result)?;
                            }
                        } else {
                            self.push(Value::Undefined)?;
                        }
                    } else {
                        self.push(Value::Undefined)?;
                    }
                }

                Some(Opcode::IteratorDone) => {
                    let result = self.peek(0).clone();
                    let done = result
                        .get_property("done")
                        .map(|v| v.to_boolean())
                        .unwrap_or(true);
                    self.push(Value::Boolean(done))?;
                }

                Some(Opcode::IteratorValue) => {
                    let result = self.stack.pop().unwrap_or(Value::Undefined);
                    let value = result.get_property("value").unwrap_or(Value::Undefined);
                    self.push(value)?;
                }

                Some(Opcode::EnterTry) => {
                    let catch_offset = self.read_i16()?;
                    let frame = self.current_frame();
                    // Calculate absolute catch IP using same logic as Jump
                    let catch_ip = (frame.ip as isize + catch_offset as isize) as usize;
                    self.exception_handlers.push(ExceptionHandler {
                        catch_ip,
                        frame_index: self.frames.len() - 1,
                        stack_size: self.stack.len(),
                    });
                }

                Some(Opcode::LeaveTry) => {
                    // Pop the exception handler
                    self.exception_handlers.pop();
                }

                Some(Opcode::Throw) => {
                    let error = self.stack.pop().unwrap_or(Value::Undefined);

                    // Look for an exception handler
                    if let Some(handler) = self.exception_handlers.pop() {
                        // Unwind stack to the handler's frame
                        while self.frames.len() > handler.frame_index + 1 {
                            self.frames.pop();
                        }

                        // Restore stack to handler's stack size
                        self.stack.truncate(handler.stack_size);

                        // Push the error value for the catch block
                        self.push(error)?;

                        // Jump to catch block
                        if let Some(frame) = self.frames.last_mut() {
                            frame.ip = handler.catch_ip;
                        }
                    } else {
                        // No handler, propagate the error with stack trace
                        let (kind, message) = if let Value::Object(ref obj) = error {
                            let obj = obj.borrow();
                            if let ObjectKind::Error { ref name, ref message } = obj.kind {
                                let k = match name.as_str() {
                                    "TypeError" => crate::error::ErrorKind::TypeError,
                                    "ReferenceError" => crate::error::ErrorKind::ReferenceError,
                                    "RangeError" => crate::error::ErrorKind::RangeError,
                                    "SyntaxError" => crate::error::ErrorKind::SyntaxError,
                                    "EvalError" => crate::error::ErrorKind::EvalError,
                                    "URIError" => crate::error::ErrorKind::UriError,
                                    "Error" => crate::error::ErrorKind::GenericError,
                                    _ => crate::error::ErrorKind::InternalError,
                                };
                                (k, message.clone())
                            } else {
                                (crate::error::ErrorKind::InternalError, error.to_string())
                            }
                        } else {
                            (crate::error::ErrorKind::InternalError, error.to_string())
                        };
                        return Err(self.error_with_stack(Error::RuntimeError {
                            kind,
                            message,
                            stack_trace: StackTrace::new(),
                        }));
                    }
                }

                Some(Opcode::EnterWith) => {
                    let _offset = self.read_u16()?;
                    // Simplified: just pop the object
                    self.stack.pop();
                }

                Some(Opcode::LeaveWith) => {
                    // Nothing to do
                }

                // Note: Opcode::Spread is handled earlier in the match

                Some(Opcode::RestParam) => {
                    // Simplified: just leave the value on stack
                }

                Some(Opcode::Yield) => {
                    // Simplified: just return the value
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    return Ok(value);
                }

                Some(Opcode::Await) => {
                    // Await a promise - extract its resolved value or suspend execution
                    let value = self.stack.pop().unwrap_or(Value::Undefined);

                    let result = if let Value::Object(obj) = &value {
                        let obj_ref = obj.borrow();
                        if let ObjectKind::Promise { state, value: promise_value, .. } = &obj_ref.kind {
                            match state {
                                PromiseState::Fulfilled => {
                                    // Extract the fulfilled value
                                    promise_value.as_ref()
                                        .map(|v| *v.clone())
                                        .unwrap_or(Value::Undefined)
                                }
                                PromiseState::Rejected => {
                                    // Throw the rejection reason
                                    let reason = promise_value.as_ref()
                                        .map(|v| *v.clone())
                                        .unwrap_or(Value::Undefined);
                                    drop(obj_ref);
                                    return Err(Error::InternalError(format!("Uncaught (in promise): {}", reason.to_js_string())));
                                }
                                PromiseState::Pending => {
                                    // Promise is pending - check if we're in an async function
                                    // and suspend execution if so
                                    drop(obj_ref);

                                    // Check if current function is async
                                    let is_async = self.frames.last()
                                        .and_then(|f| f.function.as_ref())
                                        .map(|f| f.borrow().is_async)
                                        .unwrap_or(false);

                                    if is_async {
                                        // Create a suspended async function state
                                        if let Some(frame) = self.frames.last() {
                                            if let Some(ref func) = frame.function {
                                                // Create internal promise for the async function result
                                                let result_promise = self.event_loop.create_promise();

                                                // Create suspended state
                                                let mut suspended = SuspendedAsyncFunction::new(
                                                    func.clone(),
                                                    result_promise.clone(),
                                                );

                                                // Save execution state
                                                let ip = frame.ip;
                                                let bp = frame.bp;
                                                let locals: Vec<Value> = self.stack[bp..].to_vec();
                                                let stack_snapshot = self.stack.clone();

                                                suspended.save_state(ip, locals, stack_snapshot, bp);
                                                suspended.awaited_promise = Some(value.clone());

                                                // Add to async executor for later resumption
                                                self.async_executor.suspend(suspended, value);

                                                // Return the promise wrapping the async function result
                                                let promise_obj = super::promise::create_promise_from_internal(result_promise);
                                                return Ok(promise_obj);
                                            }
                                        }
                                    }

                                    // Not in async function or can't suspend - return undefined
                                    Value::Undefined
                                }
                            }
                        } else {
                            // Not a promise, just use the value directly
                            drop(obj_ref);
                            value
                        }
                    } else {
                        // Not an object/promise, use value directly (await non-promise is valid JS)
                        value
                    };

                    self.push(result)?;
                }

                Some(Opcode::LoadReg) | Some(Opcode::StoreReg) => {
                    let _reg = self.read_u8()?;
                    // Register opcodes reserved for future register-based VM optimization.
                    // Currently, the compiler uses stack-based operations only.
                }

                Some(Opcode::GetUpvalue)
                | Some(Opcode::SetUpvalue)
                | Some(Opcode::CloseUpvalue) => {
                    let _index = self.read_u16()?;
                    // Upvalues not fully implemented
                    self.push(Value::Undefined)?;
                }

                Some(Opcode::LoadModule) => {
                    let index = self.read_u16()?;
                    let specifier = self.get_constant_string(index)?;

                    // Try to load and execute the module
                    let module_ns = self.load_module(&specifier)?;
                    self.push(module_ns)?;
                }

                Some(Opcode::ExportValue) => {
                    let index = self.read_u16()?;
                    let name = self.get_constant_string(index)?;
                    let value = self.stack.pop().unwrap_or(Value::Undefined);
                    // Store the export in the module's export namespace
                    // For now, store as a global with __export__ prefix
                    self.globals.insert(format!("__export__{}", name), value);
                }

                Some(Opcode::ExportAll) => {
                    // Re-export all from the module on the stack
                    let module = self.stack.pop().unwrap_or(Value::Undefined);

                    // Iterate over the module's exports and re-export them
                    if let Value::Object(obj) = module {
                        let borrowed = obj.borrow();
                        for (key, value) in &borrowed.properties {
                            // Skip internal properties like __esModule
                            if !key.starts_with("__") {
                                self.globals.insert(format!("__export__{}", key), value.clone());
                            }
                        }
                    }
                }

                Some(Opcode::DynamicImport) => {
                    // Dynamic import: import(source) returns Promise<Module>
                    let source = self.stack.pop().unwrap_or(Value::Undefined);
                    let specifier = source.to_js_string();

                    // Create a promise that resolves to the module namespace
                    let promise = self.create_dynamic_import_promise(&specifier);
                    self.push(promise)?;
                }

                Some(Opcode::Perform) => {
                    // Perform an algebraic effect operation
                    let effect_type_index = self.read_u16()?;
                    let operation_index = self.read_u16()?;
                    let arg_count = self.read_u8()? as usize;

                    let effect_type = self.get_constant_string(effect_type_index)?;
                    let operation = self.get_constant_string(operation_index)?;

                    // Collect arguments from stack
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().unwrap_or(Value::Undefined));
                    }
                    args.reverse();

                    // First, try the effect registry for a registered handler
                    if let Some(result) = self.handle_effect(&effect_type, &operation, &args) {
                        self.push(result)?;
                    } else {
                        // Fallback: check for a global JS handler function
                        let handler_name = format!("__effect_handler_{}_{}", effect_type, operation);
                        if let Some(handler) = self.get_global(&handler_name) {
                            // Call the handler with the arguments
                            let result = self.call_function_with_this(&handler, &args, Value::Undefined)?;
                            self.push(result)?;
                        } else {
                            // No handler found - create an Effect object for potential handling upstream
                            let effect_obj = Object {
                                kind: ObjectKind::Effect {
                                    effect_type: effect_type.clone(),
                                    operation: operation.clone(),
                                    args: args.clone(),
                                },
                                properties: HashMap::default(),
                                private_fields: HashMap::default(),
                                prototype: None,
                                cached_shape_id: None,
                            };
                            let effect_value = Value::Object(Rc::new(RefCell::new(effect_obj)));
                            self.push(effect_value)?;
                        }
                    }
                }

                None => {
                    return Err(Error::InternalError(format!(
                        "Unknown opcode at position {}",
                        self.current_frame().ip - 1
                    )));
                }
            }
        }
    }

    fn call_value(&mut self, arg_count: usize) -> Result<()> {
        let callee_pos = self.stack.len() - arg_count - 1;

        // Expand spread markers in arguments
        let mut expanded_args: Vec<Value> = Vec::with_capacity(arg_count);
        for i in 0..arg_count {
            let arg = self.stack[callee_pos + 1 + i].clone();
            if let Value::Object(obj) = &arg {
                let obj_ref = obj.borrow();
                if let ObjectKind::SpreadMarker(elements) = &obj_ref.kind {
                    expanded_args.extend(elements.clone());
                    continue;
                }
            }
            expanded_args.push(arg);
        }

        // Remove original arguments from stack
        self.stack.truncate(callee_pos + 1);

        // Push expanded arguments
        for arg in &expanded_args {
            self.push(arg.clone())?;
        }

        let actual_arg_count = expanded_args.len();
        let callee = self.stack[callee_pos].clone();
        let callee_desc = callee.to_js_string();

        match callee {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Function(func) => {
                        // Check stack depth limit before pushing new frame
                        self.check_stack_depth()?;

                        // Check if this is a generator function
                        if func.is_generator {
                            // For generators, return a Generator object instead of executing
                            let func_clone = func.clone();
                            drop(obj_ref);

                            // Collect arguments as initial locals
                            let locals: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();
                            self.stack.pop(); // Remove callee

                            // Create generator object
                            let generator = self.create_generator(func_clone, locals);
                            self.push(generator)?;
                            return Ok(());
                        }

                        let param_count = func.chunk.param_count as usize;
                        let has_rest_param = func.chunk.has_rest_param;
                        let func = Rc::new(RefCell::new(func.clone()));
                        drop(obj_ref);

                        if has_rest_param {
                            // Collect extra arguments into rest array
                            let rest_args: Vec<Value> = if actual_arg_count > param_count {
                                // Pop the extra arguments
                                self.stack
                                    .drain(callee_pos + 1 + param_count..)
                                    .collect()
                            } else {
                                Vec::new()
                            };

                            // Pad regular params with undefined if needed
                            let current_args =
                                self.stack.len().saturating_sub(callee_pos + 1);
                            for _ in current_args..param_count {
                                self.push(Value::Undefined)?;
                            }

                            // Push the rest array
                            self.push(Value::new_array(rest_args))?;

                            // bp includes regular params + rest param
                            let bp = self.stack.len() - param_count - 1;

                            // Create new frame
                            let frame = CallFrame::for_function(func, bp);
                            self.frames.push(frame);
                        } else {
                            // Pad with undefined if fewer arguments than parameters
                            for _ in actual_arg_count..param_count {
                                self.push(Value::Undefined)?;
                            }

                            // Set up arguments
                            let effective_arg_count = actual_arg_count.max(param_count);
                            let bp = self.stack.len() - effective_arg_count;

                            // Create new frame
                            let frame = CallFrame::for_function(func, bp);
                            self.frames.push(frame);
                        }
                    }
                    ObjectKind::NativeFunction { func, .. } => {
                        let func = func.clone();
                        drop(obj_ref);

                        // Collect arguments
                        let args: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();

                        // Remove callee
                        self.stack.pop();

                        // Call native function
                        let result = func(&args)?;
                        self.push(result)?;

                        // Process any pending timer operations
                        self.process_pending_timers();
                    }
                    ObjectKind::BoundArrayMethod { receiver, method } => {
                        let receiver = Value::Object(receiver.clone());
                        let method = method.clone();
                        drop(obj_ref);

                        // Collect arguments
                        let args: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();

                        // Remove callee
                        self.stack.pop();

                        // Call the array method with the bound receiver
                        let result = self.call_array_method(receiver, &method, &args)?;
                        self.push(result)?;
                    }
                    ObjectKind::BoundStringMethod { receiver, method } => {
                        let receiver = receiver.clone();
                        let method = method.clone();
                        drop(obj_ref);

                        // Collect arguments
                        let args: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();

                        // Remove callee
                        self.stack.pop();

                        // Call the string method with the bound receiver
                        let result = self.call_string_method(&receiver, &method, &args)?;
                        self.push(result)?;
                    }
                    ObjectKind::BoundFunction {
                        target,
                        bound_this,
                        bound_args,
                    } => {
                        let target = *target.clone();
                        let bound_this = *bound_this.clone();
                        let bound_args = bound_args.clone();
                        drop(obj_ref);

                        // Collect passed arguments
                        let passed_args: Vec<Value> =
                            self.stack.drain(callee_pos + 1..).collect();

                        // Remove callee
                        self.stack.pop();

                        // Combine bound args with passed args
                        let mut all_args = bound_args;
                        all_args.extend(passed_args);

                        // Call the target function with bound this
                        let result =
                            self.call_function_with_this(&target, &all_args, bound_this)?;
                        self.push(result)?;
                    }
                    _ => {
                        return Err(Error::type_error(format!("{} is not a function", callee_desc)));
                    }
                }
            }
            _ => {
                return Err(Error::type_error(format!("{} is not a function", callee_desc)));
            }
        }

        Ok(())
    }

    /// Tail call a function value - reuses the current call frame for optimization
    fn tail_call_value(&mut self, arg_count: usize) -> Result<()> {
        let callee_pos = self.stack.len() - arg_count - 1;

        // Expand spread markers in arguments and collect them
        let mut expanded_args: Vec<Value> = Vec::with_capacity(arg_count);
        for i in 0..arg_count {
            let arg = self.stack[callee_pos + 1 + i].clone();
            if let Value::Object(obj) = &arg {
                let obj_ref = obj.borrow();
                if let ObjectKind::SpreadMarker(elements) = &obj_ref.kind {
                    expanded_args.extend(elements.clone());
                    continue;
                }
            }
            expanded_args.push(arg);
        }

        // Get the callee before we modify the stack
        let callee = self.stack[callee_pos].clone();
        let callee_desc = callee.to_js_string();

        match &callee {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Function(func) => {
                        // For generators, fall back to regular call (no TCO)
                        if func.is_generator {
                            drop(obj_ref);
                            return self.call_value(arg_count);
                        }

                        let param_count = func.chunk.param_count as usize;
                        let has_rest_param = func.chunk.has_rest_param;
                        let func_rc = Rc::new(RefCell::new(func.clone()));
                        drop(obj_ref);

                        // Get current frame's base pointer - this is where the current locals start
                        let current_frame = self.frames.last().ok_or_else(|| {
                            Error::InternalError("No frame for tail call".to_string())
                        })?;
                        // The callee for the current frame is at bp - 1 (if it exists)
                        let caller_stack_pos = if current_frame.bp > 0 {
                            current_frame.bp - 1
                        } else {
                            0
                        };

                        // Pop the current frame
                        self.frames.pop();

                        // Truncate the stack to remove old callee and locals
                        // This is the KEY step for true TCO - reuse the same stack space
                        self.stack.truncate(caller_stack_pos);

                        // Push new callee
                        self.push(callee.clone())?;

                        // Push expanded arguments
                        let actual_arg_count = expanded_args.len();
                        for arg in expanded_args {
                            self.push(arg)?;
                        }

                        // Handle arguments
                        if has_rest_param {
                            // Collect extra arguments into rest array
                            let new_callee_pos = caller_stack_pos;
                            let rest_args: Vec<Value> = if actual_arg_count > param_count {
                                self.stack.drain(new_callee_pos + 1 + param_count..).collect()
                            } else {
                                Vec::new()
                            };

                            // Pad regular params with undefined if needed
                            let current_args = self.stack.len() - new_callee_pos - 1;
                            for _ in current_args..param_count {
                                self.push(Value::Undefined)?;
                            }

                            // Push the rest array
                            self.push(Value::new_array(rest_args))?;

                            // Set bp to start of arguments (after callee)
                            let bp = new_callee_pos + 1;

                            // Create new frame
                            let frame = CallFrame::for_function(func_rc, bp);
                            self.frames.push(frame);
                        } else {
                            // Pad with undefined if fewer arguments than parameters
                            for _ in actual_arg_count..param_count {
                                self.push(Value::Undefined)?;
                            }

                            // Set bp to start of arguments (after callee)
                            let bp = caller_stack_pos + 1;

                            // Create new frame
                            let frame = CallFrame::for_function(func_rc, bp);
                            self.frames.push(frame);
                        }
                    }
                    ObjectKind::NativeFunction { func, .. } => {
                        // Native functions can't be tail-call optimized in the stack sense,
                        // but we don't need a new JS frame for them
                        let func = func.clone();
                        drop(obj_ref);

                        // Pop current frame first
                        let current_frame = self.frames.last().ok_or_else(|| {
                            Error::InternalError("No frame for tail call".to_string())
                        })?;
                        let caller_stack_pos = if current_frame.bp > 0 {
                            current_frame.bp - 1
                        } else {
                            0
                        };
                        self.frames.pop();

                        // Call native function with the expanded arguments
                        let result = func(&expanded_args)?;

                        // Truncate stack and push result
                        self.stack.truncate(caller_stack_pos);
                        self.push(result)?;

                        // Process any pending timer operations
                        self.process_pending_timers();
                    }
                    _ => {
                        return Err(Error::type_error(format!("{} is not a function", callee_desc)));
                    }
                }
            }
            _ => {
                return Err(Error::type_error(format!("{} is not a function", callee_desc)));
            }
        }

        Ok(())
    }

    fn call_method(&mut self, method_name: &str, arg_count: usize) -> Result<()> {
        // Stack layout: [receiver, arg0, arg1, ...argN]
        let receiver_pos = self.stack.len() - arg_count - 1;
        let receiver = self.stack[receiver_pos].clone();

        // Set this for the method call
        let old_this = std::mem::replace(&mut self.this_value, receiver.clone());

        // Collect arguments
        let args: Vec<Value> = self.stack.drain(receiver_pos + 1..).collect();

        // Try to call built-in method
        let result = self.call_builtin_method(&receiver, method_name, &args)?;

        // Restore stack and this
        self.stack.pop(); // Remove receiver
        self.this_value = old_this;
        self.push(result)?;

        Ok(())
    }

    fn call_builtin_method(
        &mut self,
        receiver: &Value,
        method_name: &str,
        args: &[Value],
    ) -> Result<Value> {
        match receiver {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Array(_) => {
                        drop(obj_ref);
                        self.call_array_method(receiver.clone(), method_name, args)
                    }
                    ObjectKind::URLSearchParams { .. } => {
                        drop(obj_ref);
                        self.call_urlsearchparams_method(receiver.clone(), method_name, args)
                    }
                    ObjectKind::NativeFunction { func, .. } => {
                        // Handle Function.prototype methods
                        match method_name {
                            "call" => {
                                // func.call(thisArg, arg1, arg2, ...)
                                // Native functions don't use 'this' from context
                                let _this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                                let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                                let native_func = func.clone();
                                drop(obj_ref);
                                native_func(&call_args)
                            }
                            "apply" => {
                                // func.apply(thisArg, argsArray)
                                let _this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                                let args_array = args.get(1).cloned().unwrap_or(Value::Undefined);
                                let native_func = func.clone();
                                drop(obj_ref);

                                // Extract arguments from array
                                let call_args = match &args_array {
                                    Value::Object(arr_obj) => {
                                        let arr_ref = arr_obj.borrow();
                                        if let ObjectKind::Array(elements) = &arr_ref.kind {
                                            elements.clone()
                                        } else {
                                            vec![]
                                        }
                                    }
                                    Value::Null | Value::Undefined => vec![],
                                    _ => {
                                        return Err(Error::type_error(
                                            "CreateListFromArrayLike called on non-object",
                                        ))
                                    }
                                };

                                native_func(&call_args)
                            }
                            "bind" => {
                                // func.bind(thisArg, arg1, arg2, ...)
                                let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
                                let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                                drop(obj_ref);

                                // Create bound function
                                let bound_func = Object {
                                    kind: ObjectKind::BoundFunction {
                                        target: Box::new(receiver.clone()),
                                        bound_this: Box::new(bound_this),
                                        bound_args,
                                    },
                                    properties: HashMap::default(),
                                    private_fields: HashMap::default(),
                                    prototype: None, cached_shape_id: None,
                                };
                                Ok(Value::Object(Rc::new(RefCell::new(bound_func))))
                            }
                            _ => {
                                // First check if the method exists as a property on the function object
                                let method_opt = obj_ref.get_property(method_name);
                                let native_func = func.clone();
                                drop(obj_ref);

                                if let Some(method) = method_opt {
                                    // Method found on function object, call it
                                    if let Value::Object(method_obj) = &method {
                                        let method_ref = method_obj.borrow();
                                        match &method_ref.kind {
                                            ObjectKind::NativeFunction {
                                                func: method_func,
                                                ..
                                            } => {
                                                let method_func = method_func.clone();
                                                drop(method_ref);
                                                return method_func(args);
                                            }
                                            ObjectKind::Function(_) => {
                                                drop(method_ref);
                                                return self.call_function_with_this(
                                                    &method,
                                                    args,
                                                    receiver.clone(),
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                // No method found, this is a direct call on the native function
                                native_func(args)
                            }
                        }
                    }
                    ObjectKind::Function(func_val) => {
                        // Handle Function.prototype methods
                        match method_name {
                            "call" => {
                                // func.call(thisArg, arg1, arg2, ...)
                                let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                                let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                                let func_clone = func_val.clone();
                                drop(obj_ref);

                                // Create a function value and call it with the specified this
                                let func_value = Value::new_function(func_clone);
                                self.call_function_with_this(&func_value, &call_args, this_arg)
                            }
                            "apply" => {
                                // func.apply(thisArg, argsArray)
                                let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
                                let args_array = args.get(1).cloned().unwrap_or(Value::Undefined);
                                let func_clone = func_val.clone();
                                drop(obj_ref);

                                // Extract arguments from array
                                let call_args = match &args_array {
                                    Value::Object(arr_obj) => {
                                        let arr_ref = arr_obj.borrow();
                                        if let ObjectKind::Array(elements) = &arr_ref.kind {
                                            elements.clone()
                                        } else {
                                            vec![]
                                        }
                                    }
                                    Value::Null | Value::Undefined => vec![],
                                    _ => {
                                        return Err(Error::type_error(
                                            "CreateListFromArrayLike called on non-object",
                                        ))
                                    }
                                };

                                // Create a function value and call it with the specified this
                                let func_value = Value::new_function(func_clone);
                                self.call_function_with_this(&func_value, &call_args, this_arg)
                            }
                            "bind" => {
                                // func.bind(thisArg, arg1, arg2, ...)
                                let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
                                let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                                drop(obj_ref);

                                // Create bound function
                                let bound_func = Object {
                                    kind: ObjectKind::BoundFunction {
                                        target: Box::new(receiver.clone()),
                                        bound_this: Box::new(bound_this),
                                        bound_args,
                                    },
                                    properties: HashMap::default(),
                                    private_fields: HashMap::default(),
                                    prototype: None, cached_shape_id: None,
                                };
                                Ok(Value::Object(Rc::new(RefCell::new(bound_func))))
                            }
                            _ => {
                                drop(obj_ref);
                                Err(Error::type_error(format!(
                                    "Method '{}' not supported on function",
                                    method_name
                                )))
                            }
                        }
                    }
                    ObjectKind::BoundFunction {
                        target,
                        bound_this,
                        bound_args,
                    } => {
                        // Handle Function.prototype methods on bound functions
                        match method_name {
                            "call" => {
                                // Bound functions ignore the passed thisArg
                                let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                                let target = *target.clone();
                                let bound_this = *bound_this.clone();
                                let mut all_args = bound_args.clone();
                                all_args.extend(call_args);
                                drop(obj_ref);
                                self.call_function_with_this(&target, &all_args, bound_this)
                            }
                            "apply" => {
                                // Bound functions ignore the passed thisArg
                                let args_array = args.get(1).cloned().unwrap_or(Value::Undefined);
                                let target = *target.clone();
                                let bound_this = *bound_this.clone();
                                let mut all_args = bound_args.clone();
                                drop(obj_ref);

                                // Extract arguments from array
                                let apply_args = match &args_array {
                                    Value::Object(arr_obj) => {
                                        let arr_ref = arr_obj.borrow();
                                        if let ObjectKind::Array(elements) = &arr_ref.kind {
                                            elements.clone()
                                        } else {
                                            vec![]
                                        }
                                    }
                                    Value::Null | Value::Undefined => vec![],
                                    _ => {
                                        return Err(Error::type_error(
                                            "CreateListFromArrayLike called on non-object",
                                        ))
                                    }
                                };
                                all_args.extend(apply_args);
                                self.call_function_with_this(&target, &all_args, bound_this)
                            }
                            "bind" => {
                                // Re-binding: create new bound function with additional args
                                // Note: the thisArg is still ignored (uses original bound_this)
                                let additional_args: Vec<Value> =
                                    args.iter().skip(1).cloned().collect();
                                let target = target.clone();
                                let bound_this = bound_this.clone();
                                let mut new_bound_args = bound_args.clone();
                                new_bound_args.extend(additional_args);
                                drop(obj_ref);

                                let bound_func = Object {
                                    kind: ObjectKind::BoundFunction {
                                        target,
                                        bound_this,
                                        bound_args: new_bound_args,
                                    },
                                    properties: HashMap::default(),
                                    private_fields: HashMap::default(),
                                    prototype: None, cached_shape_id: None,
                                };
                                Ok(Value::Object(Rc::new(RefCell::new(bound_func))))
                            }
                            _ => {
                                drop(obj_ref);
                                Err(Error::type_error(format!(
                                    "Method '{}' not supported on bound function",
                                    method_name
                                )))
                            }
                        }
                    }
                    ObjectKind::Ordinary => {
                        // Check if this is the Reflect object
                        let is_reflect = if let Some(reflect) = self.get_global("Reflect") {
                            if let Value::Object(reflect_ref) = reflect {
                                Rc::ptr_eq(&reflect_ref, obj)
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if is_reflect {
                            drop(obj_ref);
                            // Handle Reflect.apply and Reflect.construct specially
                            match method_name {
                                "apply" => {
                                    // Reflect.apply(target, thisArg, argumentsList)
                                    let target = args.first().cloned().unwrap_or(Value::Undefined);
                                    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                                    let args_list = args.get(2).cloned().unwrap_or(Value::Undefined);

                                    // Extract arguments from array
                                    let call_args = match &args_list {
                                        Value::Object(arr_obj) => {
                                            let arr_ref = arr_obj.borrow();
                                            if let ObjectKind::Array(elements) = &arr_ref.kind {
                                                elements.clone()
                                            } else {
                                                vec![]
                                            }
                                        }
                                        Value::Null | Value::Undefined => vec![],
                                        _ => {
                                            return Err(Error::type_error(
                                                "CreateListFromArrayLike called on non-object",
                                            ))
                                        }
                                    };

                                    // Call the target function with thisArg
                                    return self.call_function_with_this(&target, &call_args, this_arg);
                                }
                                "construct" => {
                                    // Reflect.construct(target, argumentsList, newTarget?)
                                    let target = args.first().cloned().unwrap_or(Value::Undefined);
                                    let args_list = args.get(1).cloned().unwrap_or(Value::Undefined);

                                    // Extract arguments from array
                                    let call_args: Vec<Value> = match &args_list {
                                        Value::Object(arr_obj) => {
                                            let arr_ref = arr_obj.borrow();
                                            if let ObjectKind::Array(elements) = &arr_ref.kind {
                                                elements.clone()
                                            } else {
                                                vec![]
                                            }
                                        }
                                        Value::Null | Value::Undefined => vec![],
                                        _ => {
                                            return Err(Error::type_error(
                                                "CreateListFromArrayLike called on non-object",
                                            ))
                                        }
                                    };

                                    // Create a new instance object
                                    let instance = if let Value::Object(constructor_obj) = &target {
                                        Value::Object(Rc::new(RefCell::new(Object {
                                            kind: ObjectKind::Ordinary,
                                            properties: HashMap::default(),
                                            private_fields: HashMap::default(),
                                            prototype: Some(constructor_obj.clone()),
                                            cached_shape_id: None,
                                        })))
                                    } else {
                                        return Err(Error::type_error("Reflect.construct requires a constructor"));
                                    };

                                    // Call the constructor with the new instance as 'this'
                                    let result = self.call_function_with_this(&target, &call_args, instance.clone())?;

                                    // If constructor returned an object, use that; otherwise use the instance
                                    if let Value::Object(_) = result {
                                        return Ok(result);
                                    }
                                    return Ok(instance);
                                }
                                _ => {
                                    // Fall through to property lookup
                                }
                            }
                        }

                        // Try to get method from properties
                        if let Some(method) = receiver.get_property(method_name) {
                            // Method found, call it
                            if let Value::Object(method_obj) = &method {
                                let method_ref = method_obj.borrow();
                                match &method_ref.kind {
                                    ObjectKind::NativeFunction { func, .. } => {
                                        let func = func.clone();
                                        drop(method_ref);
                                        return func(args);
                                    }
                                    ObjectKind::Function(_) => {
                                        drop(method_ref);
                                        // Call user-defined function with this
                                        let this = receiver.clone();
                                        return self.call_function_with_this(&method, args, this);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(Error::type_error(format!(
                            "'{}' is not a function",
                            method_name
                        )))
                    }
                    _ => {
                        // Try to get method from properties
                        if let Some(method) = obj_ref.get_property(method_name) {
                            drop(obj_ref);
                            // Method found, call it
                            if let Value::Object(method_obj) = &method {
                                let method_ref = method_obj.borrow();
                                match &method_ref.kind {
                                    ObjectKind::NativeFunction { func, .. } => {
                                        let func = func.clone();
                                        drop(method_ref);
                                        return func(args);
                                    }
                                    ObjectKind::Function(_) => {
                                        drop(method_ref);
                                        // Call user-defined function with this
                                        let this = receiver.clone();
                                        return self.call_function_with_this(&method, args, this);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(Error::type_error(format!(
                            "'{}' is not a function",
                            method_name
                        )))
                    }
                }
            }
            Value::String(s) => self.call_string_method(s, method_name, args),
            Value::Number(n) => self.call_number_method(*n, method_name, args),
            _ => Err(Error::type_error(format!(
                "Cannot call method '{}' on {:?}",
                method_name, receiver
            ))),
        }
    }

    fn call_number_method(&self, n: f64, method_name: &str, args: &[Value]) -> Result<Value> {
        match method_name {
            "toString" => {
                let radix = args.first().map(|v| v.to_number() as u32).unwrap_or(10);
                if !(2..=36).contains(&radix) {
                    return Err(Error::range_error("radix must be between 2 and 36"));
                }
                if radix == 10 {
                    Ok(Value::String(n.to_string()))
                } else {
                    // Convert to different radix
                    let int_val = n as i64;
                    let s = match radix {
                        2 => format!("{:b}", int_val),
                        8 => format!("{:o}", int_val),
                        16 => format!("{:x}", int_val),
                        _ => {
                            // General radix conversion
                            if int_val == 0 {
                                "0".to_string()
                            } else {
                                let mut result = String::new();
                                let mut num = int_val.abs();
                                let digits = "0123456789abcdefghijklmnopqrstuvwxyz";
                                while num > 0 {
                                    let digit = (num % radix as i64) as usize;
                                    result.insert(0, digits.chars().nth(digit).unwrap());
                                    num /= radix as i64;
                                }
                                if int_val < 0 {
                                    result.insert(0, '-');
                                }
                                result
                            }
                        }
                    };
                    Ok(Value::String(s))
                }
            }
            "toFixed" => {
                let digits = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                if digits > 100 {
                    return Err(Error::range_error("toFixed() digits argument must be between 0 and 100"));
                }
                Ok(Value::String(format!("{:.1$}", n, digits)))
            }
            "toExponential" => {
                let digits = args.first().map(|v| v.to_number() as usize);
                if let Some(d) = digits {
                    if d > 100 {
                        return Err(Error::range_error("toExponential() digits argument must be between 0 and 100"));
                    }
                    Ok(Value::String(format!("{:.1$e}", n, d)))
                } else {
                    Ok(Value::String(format!("{:e}", n)))
                }
            }
            "toPrecision" => {
                let precision = args.first().map(|v| v.to_number() as usize).unwrap_or(6);
                if !(1..=100).contains(&precision) {
                    return Err(Error::range_error("toPrecision() argument must be between 1 and 100"));
                }
                Ok(Value::String(format!("{:.1$}", n, precision - 1)))
            }
            "valueOf" => Ok(Value::Number(n)),
            _ => Err(Error::type_error(format!(
                "Number method '{}' not implemented",
                method_name
            ))),
        }
    }

    fn call_array_method(
        &mut self,
        receiver: Value,
        method_name: &str,
        args: &[Value],
    ) -> Result<Value> {
        // Get the array elements
        let arr = if let Value::Object(obj) = &receiver {
            let obj_ref = obj.borrow();
            if let ObjectKind::Array(arr) = &obj_ref.kind {
                arr.clone()
            } else {
                return Err(Error::type_error("Not an array"));
            }
        } else {
            return Err(Error::type_error("Not an array"));
        };

        match method_name {
            "push" => {
                // Mutate array: add elements
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        for arg in args {
                            arr.push(arg.clone());
                        }
                        return Ok(Value::Number(arr.len() as f64));
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "pop" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        return Ok(arr.pop().unwrap_or(Value::Undefined));
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "shift" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        if arr.is_empty() {
                            return Ok(Value::Undefined);
                        }
                        return Ok(arr.remove(0));
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "unshift" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        for (i, arg) in args.iter().enumerate() {
                            arr.insert(i, arg.clone());
                        }
                        return Ok(Value::Number(arr.len() as f64));
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "length" => Ok(Value::Number(arr.len() as f64)),
            "indexOf" => {
                let search = args.first().cloned().unwrap_or(Value::Undefined);
                for (i, elem) in arr.iter().enumerate() {
                    if elem.strict_equals(&search) {
                        return Ok(Value::Number(i as f64));
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "includes" => {
                let search = args.first().cloned().unwrap_or(Value::Undefined);
                for elem in &arr {
                    if elem.strict_equals(&search) {
                        return Ok(Value::Boolean(true));
                    }
                }
                Ok(Value::Boolean(false))
            }
            "join" => {
                let sep = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_else(|| ",".to_string());
                let joined: Vec<String> = arr.iter().map(|v| v.to_js_string()).collect();
                Ok(Value::String(joined.join(&sep)))
            }
            "reverse" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        arr.reverse();
                        drop(obj_ref);
                        return Ok(receiver.clone());
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "slice" => {
                let start = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let end = args
                    .get(1)
                    .map(|v| v.to_number() as i64)
                    .unwrap_or(arr.len() as i64);

                let len = arr.len() as i64;
                let start = if start < 0 {
                    (len + start).max(0) as usize
                } else {
                    start.min(len) as usize
                };
                let end = if end < 0 {
                    (len + end).max(0) as usize
                } else {
                    end.min(len) as usize
                };

                let sliced: Vec<Value> = arr[start..end].to_vec();
                Ok(Value::new_array(sliced))
            }
            "concat" => {
                let mut result = arr.clone();
                for arg in args {
                    if let Value::Object(obj) = arg {
                        let obj_ref = obj.borrow();
                        if let ObjectKind::Array(other) = &obj_ref.kind {
                            result.extend(other.clone());
                        } else {
                            result.push(arg.clone());
                        }
                    } else {
                        result.push(arg.clone());
                    }
                }
                Ok(Value::new_array(result))
            }
            "map" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                let mut results = Vec::with_capacity(arr.len());
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    results.push(result);
                }
                Ok(Value::new_array(results))
            }
            "filter" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                let mut results = Vec::new();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    if result.to_boolean() {
                        results.push(elem.clone());
                    }
                }
                Ok(Value::new_array(results))
            }
            "forEach" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                for (i, elem) in arr.iter().enumerate() {
                    self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                }
                Ok(Value::Undefined)
            }
            "reduce" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let initial = args.get(1).cloned();
                let recv = receiver.clone();

                let mut iter = arr.iter().enumerate();
                let mut acc = if let Some(init) = initial {
                    init
                } else {
                    iter.next()
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Undefined)
                };

                for (i, elem) in iter {
                    acc = self.invoke_callback(&callback, &[
                        acc,
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                }
                Ok(acc)
            }
            "find" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    if result.to_boolean() {
                        return Ok(elem.clone());
                    }
                }
                Ok(Value::Undefined)
            }
            "findIndex" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    if result.to_boolean() {
                        return Ok(Value::Number(i as f64));
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "some" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    if result.to_boolean() {
                        return Ok(Value::Boolean(true));
                    }
                }
                Ok(Value::Boolean(false))
            }
            "every" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let recv = receiver.clone();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        recv.clone(),
                    ])?;
                    if !result.to_boolean() {
                        return Ok(Value::Boolean(false));
                    }
                }
                Ok(Value::Boolean(true))
            }
            "flat" => {
                let depth = args
                    .first()
                    .map(|v| v.to_number() as i32)
                    .unwrap_or(1);
                let flattened = self.flatten_array(&arr, depth);
                Ok(Value::new_array(flattened))
            }
            "sort" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        let compare_fn = args.first().cloned();

                        if let Some(callback) = compare_fn {
                            // Custom comparator - need to sort with callback
                            // We'll collect, sort outside borrow, then replace
                            let mut to_sort = arr.clone();
                            drop(obj_ref);

                            // Simple bubble sort with callback (not efficient but works)
                            let len = to_sort.len();
                            for i in 0..len {
                                for j in 0..len - 1 - i {
                                    let result = self.invoke_callback(&callback, &[
                                        to_sort[j].clone(),
                                        to_sort[j + 1].clone(),
                                    ])?;
                                    if result.to_number() > 0.0 {
                                        to_sort.swap(j, j + 1);
                                    }
                                }
                            }

                            // Put sorted array back
                            let mut obj_ref = obj.borrow_mut();
                            if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                                *arr = to_sort;
                            }
                            drop(obj_ref);
                        } else {
                            // Default string comparison sort
                            arr.sort_by(|a, b| {
                                a.to_js_string().cmp(&b.to_js_string())
                            });
                            drop(obj_ref);
                        }
                        return Ok(receiver.clone());
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "splice" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        let len = arr.len() as i64;

                        // Get start index
                        let start = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                        let start = if start < 0 {
                            (len + start).max(0) as usize
                        } else {
                            start.min(len) as usize
                        };

                        // Get delete count
                        let delete_count = args
                            .get(1)
                            .map(|v| v.to_number() as i64)
                            .unwrap_or(len - start as i64);
                        let delete_count = delete_count.max(0).min(len - start as i64) as usize;

                        // Get items to insert
                        let items: Vec<Value> = args.iter().skip(2).cloned().collect();

                        // Remove elements and collect deleted
                        let deleted: Vec<Value> = arr.drain(start..start + delete_count).collect();

                        // Insert new elements
                        for (i, item) in items.into_iter().enumerate() {
                            arr.insert(start + i, item);
                        }

                        return Ok(Value::new_array(deleted));
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "fill" => {
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        let value = args.first().cloned().unwrap_or(Value::Undefined);
                        let len = arr.len() as i64;

                        let start = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
                        let start = if start < 0 {
                            (len + start).max(0) as usize
                        } else {
                            start.min(len) as usize
                        };

                        let end = args.get(2).map(|v| v.to_number() as i64).unwrap_or(len);
                        let end = if end < 0 {
                            (len + end).max(0) as usize
                        } else {
                            end.min(len) as usize
                        };

                        for i in start..end {
                            arr[i] = value.clone();
                        }
                        drop(obj_ref);
                        return Ok(receiver.clone());
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "at" => {
                let index = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let len = arr.len() as i64;
                let actual_index = if index < 0 {
                    (len + index) as usize
                } else {
                    index as usize
                };
                Ok(arr.get(actual_index).cloned().unwrap_or(Value::Undefined))
            }
            "flatMap" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let mut results = Vec::new();
                for (i, elem) in arr.iter().enumerate() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        receiver.clone(),
                    ])?;
                    // Flatten one level
                    if let Value::Object(obj) = &result {
                        let obj_ref = obj.borrow();
                        if let ObjectKind::Array(inner) = &obj_ref.kind {
                            results.extend(inner.clone());
                        } else {
                            results.push(result.clone());
                        }
                    } else {
                        results.push(result);
                    }
                }
                Ok(Value::new_array(results))
            }
            // ES2023 non-mutating methods
            "toReversed" => {
                // Returns a new array with elements in reversed order (does not mutate original)
                let mut reversed = arr.clone();
                reversed.reverse();
                Ok(Value::new_array(reversed))
            }
            "toSorted" => {
                // Returns a new sorted array (does not mutate original)
                let mut sorted = arr.clone();
                let compare_fn = args.first().cloned();

                if let Some(callback) = compare_fn {
                    // Custom comparator - bubble sort with callback
                    let len = sorted.len();
                    for i in 0..len {
                        for j in 0..len.saturating_sub(1).saturating_sub(i) {
                            let result = self.invoke_callback(&callback, &[
                                sorted[j].clone(),
                                sorted[j + 1].clone(),
                            ])?;
                            if result.to_number() > 0.0 {
                                sorted.swap(j, j + 1);
                            }
                        }
                    }
                } else {
                    // Default string comparison sort
                    sorted.sort_by_key(|a| a.to_js_string());
                }
                Ok(Value::new_array(sorted))
            }
            "toSpliced" => {
                // Returns a new array with elements added/removed (does not mutate original)
                let len = arr.len() as i64;

                // Get start index
                let start = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let start = if start < 0 {
                    (len + start).max(0) as usize
                } else {
                    start.min(len) as usize
                };

                // Get delete count
                let delete_count = args
                    .get(1)
                    .map(|v| v.to_number() as i64)
                    .unwrap_or(len - start as i64);
                let delete_count = delete_count.max(0).min(len - start as i64) as usize;

                // Get items to insert
                let items: Vec<Value> = args.iter().skip(2).cloned().collect();

                // Build new array
                let mut result = Vec::with_capacity(arr.len() - delete_count + items.len());
                result.extend(arr[..start].iter().cloned());
                result.extend(items);
                result.extend(arr[start + delete_count..].iter().cloned());

                Ok(Value::new_array(result))
            }
            "with" => {
                // Returns a new array with element at index replaced (does not mutate original)
                let index = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let value = args.get(1).cloned().unwrap_or(Value::Undefined);
                let len = arr.len() as i64;

                let actual_index = if index < 0 {
                    len + index
                } else {
                    index
                };

                if actual_index < 0 || actual_index >= len {
                    return Err(Error::range_error(format!(
                        "Invalid index {} for array of length {}",
                        index, len
                    )));
                }

                let mut result = arr.clone();
                result[actual_index as usize] = value;
                Ok(Value::new_array(result))
            }
            "findLast" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                for (i, elem) in arr.iter().enumerate().rev() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        receiver.clone(),
                    ])?;
                    if result.to_boolean() {
                        return Ok(elem.clone());
                    }
                }
                Ok(Value::Undefined)
            }
            "findLastIndex" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                for (i, elem) in arr.iter().enumerate().rev() {
                    let result = self.invoke_callback(&callback, &[
                        elem.clone(),
                        Value::Number(i as f64),
                        receiver.clone(),
                    ])?;
                    if result.to_boolean() {
                        return Ok(Value::Number(i as f64));
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "reduceRight" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let initial = args.get(1).cloned();

                let mut iter = arr.iter().enumerate().rev().peekable();
                let mut acc = if let Some(init) = initial {
                    init
                } else {
                    iter.next()
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Undefined)
                };

                for (i, elem) in iter {
                    acc = self.invoke_callback(&callback, &[
                        acc,
                        elem.clone(),
                        Value::Number(i as f64),
                        receiver.clone(),
                    ])?;
                }
                Ok(acc)
            }
            "lastIndexOf" => {
                let search = args.first().cloned().unwrap_or(Value::Undefined);
                let from_index = args
                    .get(1)
                    .map(|v| v.to_number() as i64)
                    .unwrap_or(arr.len() as i64 - 1);

                let len = arr.len() as i64;
                let start = if from_index < 0 {
                    (len + from_index).max(-1)
                } else {
                    from_index.min(len - 1)
                };

                if start < 0 {
                    return Ok(Value::Number(-1.0));
                }

                for i in (0..=start as usize).rev() {
                    if arr[i].strict_equals(&search) {
                        return Ok(Value::Number(i as f64));
                    }
                }
                Ok(Value::Number(-1.0))
            }
            "copyWithin" => {
                // array.copyWithin(target, start, end) - mutating method
                if let Value::Object(obj) = &receiver {
                    let mut obj_ref = obj.borrow_mut();
                    if let ObjectKind::Array(arr) = &mut obj_ref.kind {
                        let len = arr.len() as i64;

                        // Get target index
                        let target = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                        let target = if target < 0 {
                            (len + target).max(0) as usize
                        } else {
                            target.min(len) as usize
                        };

                        // Get start index
                        let start = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
                        let start = if start < 0 {
                            (len + start).max(0) as usize
                        } else {
                            start.min(len) as usize
                        };

                        // Get end index
                        let end = args.get(2).map(|v| v.to_number() as i64).unwrap_or(len);
                        let end = if end < 0 {
                            (len + end).max(0) as usize
                        } else {
                            end.min(len) as usize
                        };

                        // Calculate count
                        let count = (end - start).min(len as usize - target);

                        // Copy elements (handle overlapping by copying to temp first)
                        let temp: Vec<Value> = arr[start..start + count].to_vec();
                        for (i, val) in temp.into_iter().enumerate() {
                            arr[target + i] = val;
                        }

                        drop(obj_ref);
                        return Ok(receiver.clone());
                    }
                }
                Err(Error::type_error("Not an array"))
            }
            "entries" => {
                // Returns an iterator of [index, value] pairs
                let pairs: Vec<Value> = arr
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Value::new_array(vec![Value::Number(i as f64), v.clone()]))
                    .collect();

                let iterator = Object {
                    kind: ObjectKind::Iterator {
                        values: pairs,
                        index: 0,
                    },
                    properties: HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None, cached_shape_id: None,
                };
                Ok(Value::Object(Rc::new(RefCell::new(iterator))))
            }
            "keys" => {
                // Returns an iterator of indices
                let keys: Vec<Value> = (0..arr.len())
                    .map(|i| Value::Number(i as f64))
                    .collect();

                let iterator = Object {
                    kind: ObjectKind::Iterator {
                        values: keys,
                        index: 0,
                    },
                    properties: HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None, cached_shape_id: None,
                };
                Ok(Value::Object(Rc::new(RefCell::new(iterator))))
            }
            "values" => {
                // Returns an iterator of values
                let iterator = Object {
                    kind: ObjectKind::Iterator {
                        values: arr.clone(),
                        index: 0,
                    },
                    properties: HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None, cached_shape_id: None,
                };
                Ok(Value::Object(Rc::new(RefCell::new(iterator))))
            }
            _ => Err(Error::type_error(format!(
                "Array method '{}' not implemented",
                method_name
            ))),
        }
    }

    fn flatten_array(&self, arr: &[Value], depth: i32) -> Vec<Value> {
        if depth <= 0 {
            return arr.to_vec();
        }
        let mut result = Vec::new();
        for elem in arr {
            if let Value::Object(obj) = elem {
                let obj_ref = obj.borrow();
                if let ObjectKind::Array(inner) = &obj_ref.kind {
                    let inner = inner.clone();
                    drop(obj_ref);
                    result.extend(self.flatten_array(&inner, depth - 1));
                    continue;
                }
            }
            result.push(elem.clone());
        }
        result
    }

    fn call_urlsearchparams_method(
        &mut self,
        receiver: Value,
        method_name: &str,
        args: &[Value],
    ) -> Result<Value> {
        use crate::runtime::value::url_encode;

        // Get the params from the receiver
        let obj_rc = if let Value::Object(obj) = &receiver {
            obj.clone()
        } else {
            return Err(Error::type_error("Expected URLSearchParams object"));
        };

        match method_name {
            "get" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    for (k, v) in params {
                        if k == &name {
                            return Ok(Value::String(v.clone()));
                        }
                    }
                }
                Ok(Value::Null)
            }
            "getAll" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let values: Vec<Value> = params
                        .iter()
                        .filter(|(k, _)| k == &name)
                        .map(|(_, v)| Value::String(v.clone()))
                        .collect();
                    return Ok(Value::new_array(values));
                }
                Ok(Value::new_array(vec![]))
            }
            "has" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    for (k, _) in params {
                        if k == &name {
                            return Ok(Value::Boolean(true));
                        }
                    }
                }
                Ok(Value::Boolean(false))
            }
            "set" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let value = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                let mut obj = obj_rc.borrow_mut();
                if let ObjectKind::URLSearchParams { params } = &mut obj.kind {
                    // Remove all existing entries with this name
                    params.retain(|(k, _)| k != &name);
                    // Add the new entry
                    params.push((name, value));
                }
                Ok(Value::Undefined)
            }
            "append" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let value = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                let mut obj = obj_rc.borrow_mut();
                if let ObjectKind::URLSearchParams { params } = &mut obj.kind {
                    params.push((name, value));
                }
                Ok(Value::Undefined)
            }
            "delete" => {
                let name = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let mut obj = obj_rc.borrow_mut();
                if let ObjectKind::URLSearchParams { params } = &mut obj.kind {
                    params.retain(|(k, _)| k != &name);
                }
                Ok(Value::Undefined)
            }
            "toString" => {
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let result = params
                        .iter()
                        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
                        .collect::<Vec<_>>()
                        .join("&");
                    return Ok(Value::String(result));
                }
                Ok(Value::String(String::new()))
            }
            "entries" => {
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let entries: Vec<Value> = params
                        .iter()
                        .map(|(k, v)| {
                            Value::new_array(vec![
                                Value::String(k.clone()),
                                Value::String(v.clone()),
                            ])
                        })
                        .collect();
                    return Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                        crate::runtime::value::Object {
                            kind: ObjectKind::Iterator {
                                values: entries,
                                index: 0,
                            },
                            properties: std::collections::HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None, cached_shape_id: None,
                        },
                    ))));
                }
                Ok(Value::Undefined)
            }
            "keys" => {
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let keys: Vec<Value> = params
                        .iter()
                        .map(|(k, _)| Value::String(k.clone()))
                        .collect();
                    return Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                        crate::runtime::value::Object {
                            kind: ObjectKind::Iterator {
                                values: keys,
                                index: 0,
                            },
                            properties: std::collections::HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None, cached_shape_id: None,
                        },
                    ))));
                }
                Ok(Value::Undefined)
            }
            "values" => {
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let values: Vec<Value> = params
                        .iter()
                        .map(|(_, v)| Value::String(v.clone()))
                        .collect();
                    return Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                        crate::runtime::value::Object {
                            kind: ObjectKind::Iterator {
                                values,
                                index: 0,
                            },
                            properties: std::collections::HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None, cached_shape_id: None,
                        },
                    ))));
                }
                Ok(Value::Undefined)
            }
            "forEach" => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let obj = obj_rc.borrow();
                if let ObjectKind::URLSearchParams { params } = &obj.kind {
                    let params_copy: Vec<(String, String)> = params.clone();
                    drop(obj);
                    for (key, value) in params_copy {
                        let callback_args = vec![
                            Value::String(value),
                            Value::String(key),
                            receiver.clone(),
                        ];
                        self.invoke_callback(&callback, &callback_args)?;
                    }
                }
                Ok(Value::Undefined)
            }
            "sort" => {
                let mut obj = obj_rc.borrow_mut();
                if let ObjectKind::URLSearchParams { params } = &mut obj.kind {
                    params.sort_by(|a, b| a.0.cmp(&b.0));
                }
                Ok(Value::Undefined)
            }
            _ => Err(Error::type_error(format!(
                "URLSearchParams.{} is not a function",
                method_name
            ))),
        }
    }

    fn call_string_method(&self, s: &str, method_name: &str, args: &[Value]) -> Result<Value> {
        match method_name {
            "toUpperCase" => Ok(Value::String(s.to_uppercase())),
            "toLowerCase" => Ok(Value::String(s.to_lowercase())),
            "trim" => Ok(Value::String(s.trim().to_string())),
            "trimStart" | "trimLeft" => Ok(Value::String(s.trim_start().to_string())),
            "trimEnd" | "trimRight" => Ok(Value::String(s.trim_end().to_string())),
            "charAt" => {
                let idx = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                Ok(Value::String(
                    s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            "charCodeAt" => {
                let idx = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                Ok(Value::Number(
                    s.chars().nth(idx).map(|c| c as u32 as f64).unwrap_or(f64::NAN),
                ))
            }
            "indexOf" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let start = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
                let result = s[start..].find(&search).map(|i| (i + start) as f64).unwrap_or(-1.0);
                Ok(Value::Number(result))
            }
            "lastIndexOf" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let result = s.rfind(&search).map(|i| i as f64).unwrap_or(-1.0);
                Ok(Value::Number(result))
            }
            "includes" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                Ok(Value::Boolean(s.contains(&search)))
            }
            "startsWith" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                Ok(Value::Boolean(s.starts_with(&search)))
            }
            "endsWith" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                Ok(Value::Boolean(s.ends_with(&search)))
            }
            "slice" => {
                let len = s.chars().count() as i64;
                let start = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let end = args.get(1).map(|v| v.to_number() as i64).unwrap_or(len);

                let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                let end = if end < 0 { (len + end).max(0) } else { end.min(len) } as usize;

                let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
                Ok(Value::String(result))
            }
            "substring" => {
                let len = s.chars().count();
                let start = args
                    .first()
                    .map(|v| (v.to_number() as usize).min(len))
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .map(|v| (v.to_number() as usize).min(len))
                    .unwrap_or(len);

                let (start, end) = if start > end { (end, start) } else { (start, end) };
                let result: String = s.chars().skip(start).take(end - start).collect();
                Ok(Value::String(result))
            }
            "substr" => {
                let len = s.chars().count() as i64;
                let start = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let length = args
                    .get(1)
                    .map(|v| v.to_number() as usize)
                    .unwrap_or(len as usize);

                let start = if start < 0 { (len + start).max(0) as usize } else { start as usize };
                let result: String = s.chars().skip(start).take(length).collect();
                Ok(Value::String(result))
            }
            "split" => {
                let sep = args.first().map(|v| v.to_js_string());
                let parts: Vec<Value> = if let Some(sep) = sep {
                    if sep.is_empty() {
                        s.chars().map(|c| Value::String(c.to_string())).collect()
                    } else {
                        s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                    }
                } else {
                    vec![Value::String(s.to_string())]
                };
                Ok(Value::new_array(parts))
            }
            "replace" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let replacement = args
                    .get(1)
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                Ok(Value::String(s.replacen(&search, &replacement, 1)))
            }
            "replaceAll" => {
                let search = args
                    .first()
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let replacement = args
                    .get(1)
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                Ok(Value::String(s.replace(&search, &replacement)))
            }
            "repeat" => {
                let count = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                Ok(Value::String(s.repeat(count)))
            }
            "padStart" => {
                let target_len = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                let pad = args.get(1).map(|v| v.to_js_string()).unwrap_or_else(|| " ".to_string());
                let current_len = s.chars().count();
                if current_len >= target_len || pad.is_empty() {
                    Ok(Value::String(s.to_string()))
                } else {
                    let pad_len = target_len - current_len;
                    let padding: String = pad.chars().cycle().take(pad_len).collect();
                    Ok(Value::String(format!("{}{}", padding, s)))
                }
            }
            "padEnd" => {
                let target_len = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                let pad = args.get(1).map(|v| v.to_js_string()).unwrap_or_else(|| " ".to_string());
                let current_len = s.chars().count();
                if current_len >= target_len || pad.is_empty() {
                    Ok(Value::String(s.to_string()))
                } else {
                    let pad_len = target_len - current_len;
                    let padding: String = pad.chars().cycle().take(pad_len).collect();
                    Ok(Value::String(format!("{}{}", s, padding)))
                }
            }
            "concat" => {
                let mut result = s.to_string();
                for arg in args {
                    result.push_str(&arg.to_js_string());
                }
                Ok(Value::String(result))
            }
            "at" => {
                let len = s.chars().count() as i64;
                let idx = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
                let actual_idx = if idx < 0 { len + idx } else { idx };
                if actual_idx < 0 || actual_idx >= len {
                    Ok(Value::Undefined)
                } else {
                    Ok(Value::String(
                        s.chars().nth(actual_idx as usize).map(|c| c.to_string()).unwrap_or_default(),
                    ))
                }
            }
            "codePointAt" => {
                let idx = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
                Ok(s.chars().nth(idx)
                    .map(|c| Value::Number(c as u32 as f64))
                    .unwrap_or(Value::Undefined))
            }
            "toString" | "valueOf" => Ok(Value::String(s.to_string())),
            "normalize" => {
                // Basic normalization - just return the string for now
                // Full Unicode normalization would require a library
                Ok(Value::String(s.to_string()))
            }
            "localeCompare" => {
                let other = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                Ok(Value::Number(s.cmp(&other) as i32 as f64))
            }
            "toLocaleLowerCase" => Ok(Value::String(s.to_lowercase())),
            "toLocaleUpperCase" => Ok(Value::String(s.to_uppercase())),
            "match" => {
                // Basic string matching (without RegExp)
                let search = args.first();
                match search {
                    Some(Value::String(pattern)) => {
                        // Simple string matching - returns array with match or null
                        if let Some(index) = s.find(pattern.as_str()) {
                            let result = Value::new_array(vec![Value::String(pattern.clone())]);
                            if let Value::Object(obj) = &result {
                                let mut obj_ref = obj.borrow_mut();
                                obj_ref.set_property("index", Value::Number(index as f64));
                                obj_ref.set_property("input", Value::String(s.to_string()));
                            }
                            Ok(result)
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    _ => Ok(Value::Null), // RegExp would need proper integration
                }
            }
            "matchAll" => {
                // Basic string matchAll (without RegExp)
                let search = args.first();
                match search {
                    Some(Value::String(pattern)) => {
                        // Find all occurrences
                        let matches: Vec<Value> = s
                            .match_indices(pattern.as_str())
                            .map(|(index, matched)| {
                                let match_arr = Value::new_array(vec![Value::String(matched.to_string())]);
                                if let Value::Object(obj) = &match_arr {
                                    let mut obj_ref = obj.borrow_mut();
                                    obj_ref.set_property("index", Value::Number(index as f64));
                                    obj_ref.set_property("input", Value::String(s.to_string()));
                                }
                                match_arr
                            })
                            .collect();

                        // Return an iterator
                        let iterator = Object {
                            kind: ObjectKind::Iterator {
                                values: matches,
                                index: 0,
                            },
                            properties: HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None, cached_shape_id: None,
                        };
                        Ok(Value::Object(Rc::new(RefCell::new(iterator))))
                    }
                    _ => {
                        // Return empty iterator for non-string/non-regex
                        let iterator = Object {
                            kind: ObjectKind::Iterator {
                                values: vec![],
                                index: 0,
                            },
                            properties: HashMap::default(),
                            private_fields: HashMap::default(),
                            prototype: None, cached_shape_id: None,
                        };
                        Ok(Value::Object(Rc::new(RefCell::new(iterator))))
                    }
                }
            }
            "search" => {
                // Basic string search (without RegExp)
                let search = args.first();
                match search {
                    Some(Value::String(pattern)) => {
                        Ok(Value::Number(
                            s.find(pattern.as_str()).map(|i| i as f64).unwrap_or(-1.0),
                        ))
                    }
                    _ => Ok(Value::Number(-1.0)),
                }
            }
            "isWellFormed" => {
                // Check if string is well-formed Unicode (ES2024)
                // In Rust, strings are always valid UTF-8, so this is always true
                Ok(Value::Boolean(true))
            }
            "toWellFormed" => {
                // Return well-formed version of string (ES2024)
                // In Rust, strings are always valid UTF-8
                Ok(Value::String(s.to_string()))
            }
            _ => Err(Error::type_error(format!(
                "String method '{}' not implemented",
                method_name
            ))),
        }
    }

    fn invoke_callback(&mut self, callback: &Value, args: &[Value]) -> Result<Value> {
        match callback {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::NativeFunction { func, .. } => {
                        let func = func.clone();
                        drop(obj_ref);
                        func(args)
                    }
                    ObjectKind::Function(func) => {
                        // Call user function
                        let func = Rc::new(RefCell::new(func.clone()));
                        drop(obj_ref);

                        // Remember current frame depth so we know when to return
                        let target_depth = self.frames.len();

                        // Push function and arguments onto stack
                        self.push(callback.clone())?;
                        for arg in args {
                            self.push(arg.clone())?;
                        }

                        // Set up call frame
                        let bp = self.stack.len() - args.len();
                        let frame = CallFrame::for_function(func, bp);
                        self.frames.push(frame);

                        // Execute until we return to the target depth
                        let result = self.execute_until_depth(target_depth)?;
                        Ok(result)
                    }
                    _ => Err(Error::type_error("Callback is not a function")),
                }
            }
            _ => Err(Error::type_error("Callback is not a function")),
        }
    }

    /// Try to dispatch to a built-in constructor.
    /// Returns Some(result) if callee matches the named constructor, None otherwise.
    fn try_builtin_constructor(
        &self,
        name: &str,
        callee: &Value,
        args: &[Value],
    ) -> Option<Result<Value>> {
        let global = self.get_global(name)?;
        if let (Value::Object(global_ref), Value::Object(callee_ref)) = (&global, callee) {
            if Rc::ptr_eq(global_ref, callee_ref) {
                let ctor_name = format!("__{}_constructor", name);
                if let Some(ctor) = self.get_global(&ctor_name) {
                    if let Value::Object(ctor_obj) = ctor {
                        if let ObjectKind::NativeFunction { func, .. } = &ctor_obj.borrow().kind {
                            return Some(func(args));
                        }
                    }
                }
            }
        }
        None
    }

    /// Get the super class from the current class context.
    /// Returns None if there is no current class or no super class.
    fn get_super_class(&self) -> Option<Value> {
        let class = self.current_class.as_ref()?;
        if let Value::Object(obj) = class {
            let obj_ref = obj.borrow();
            if let ObjectKind::Class { super_class, .. } = &obj_ref.kind {
                return super_class.as_ref().map(|sc| sc.as_ref().clone());
            }
        }
        None
    }

    fn new_instance(&mut self, arg_count: usize) -> Result<()> {
        let callee_pos = self.stack.len() - arg_count - 1;
        let callee = self.stack[callee_pos].clone();

        // Extract instance fields from class if available
        let instance_fields = if let Value::Object(ref obj) = callee {
            let obj_ref = obj.borrow();
            if let ObjectKind::Class { instance_fields, .. } = &obj_ref.kind {
                instance_fields.clone()
            } else {
                HashMap::default()
            }
        } else {
            HashMap::default()
        };

        // Create new instance with prototype reference to constructor
        // Initialize with instance fields (private fields start with #)
        let instance = if let Value::Object(ref constructor_obj) = callee {
            let mut properties = HashMap::default();
            let mut private_fields = HashMap::default();

            // Initialize instance fields
            for (name, value) in instance_fields {
                if let Some(field_name) = name.strip_prefix('#') {
                    private_fields.insert(field_name.to_string(), value);
                } else {
                    // Public field
                    properties.insert(name, value);
                }
            }

            Value::Object(Rc::new(RefCell::new(Object {
                kind: ObjectKind::Ordinary,
                properties,
                private_fields,
                prototype: Some(constructor_obj.clone()),
                cached_shape_id: None,
            })))
        } else {
            Value::new_object()
        };

        match &callee {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Class { constructor, .. } => {
                        if let Some(ctor) = constructor {
                            let ctor_func = ctor.clone();
                            drop(obj_ref);

                            if self.frames.len() >= MAX_CALL_DEPTH {
                                return Err(Error::InternalError(
                                    "Maximum call stack size exceeded".to_string(),
                                ));
                            }

                            // Set 'this' to the new instance
                            self.this_value = instance.clone();
                            // Set current class for super() calls
                            self.current_class = Some(callee.clone());

                            // Set up arguments - bp is right after the callee
                            let bp = callee_pos + 1;

                            // Create constructor frame with this tracking
                            let func = Rc::new(RefCell::new(*ctor_func));
                            let frame = CallFrame::for_constructor(func, bp, instance);
                            self.frames.push(frame);
                        } else {
                            // No constructor, just return empty instance
                            self.stack.truncate(callee_pos);
                            self.push(instance)?;
                        }
                    }
                    ObjectKind::Function(func) => {
                        // Constructor function (ES5 style)
                        let func_clone = func.clone();
                        drop(obj_ref);

                        if self.frames.len() >= MAX_CALL_DEPTH {
                            return Err(Error::InternalError(
                                "Maximum call stack size exceeded".to_string(),
                            ));
                        }

                        // Set 'this' to the new instance
                        self.this_value = instance.clone();

                        // Set up arguments - bp is right after the callee
                        let bp = callee_pos + 1;

                        // Create constructor frame with this tracking
                        let func = Rc::new(RefCell::new(func_clone));
                        let frame = CallFrame::for_constructor(func, bp, instance);
                        self.frames.push(frame);
                    }
                    ObjectKind::NativeFunction { func, .. } => {
                        // Native constructor function
                        let args: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();
                        self.stack.truncate(callee_pos);
                        let result = func(&args)?;
                        self.push(result)?;
                    }
                    ObjectKind::Ordinary => {
                        // Check if this is a built-in constructor (Date, Map, Set)
                        drop(obj_ref);
                        let args: Vec<Value> = self.stack.drain(callee_pos + 1..).collect();
                        self.stack.truncate(callee_pos);

                        // Check if callee is a known built-in constructor
                        let builtin_constructors = [
                            "Date", "Map", "Set", "WeakMap", "WeakSet",
                            "ArrayBuffer", "DataView", "Proxy",
                            "Int8Array", "Uint8Array", "Uint8ClampedArray",
                            "Int16Array", "Uint16Array",
                            "Int32Array", "Uint32Array",
                            "Float32Array", "Float64Array",
                        ];
                        for name in builtin_constructors {
                            if let Some(result) = self.try_builtin_constructor(name, &callee, &args) {
                                self.push(result?)?;
                                return Ok(());
                            }
                        }

                        // Check for Error constructors
                        let error_types = ["Error", "TypeError", "ReferenceError", "RangeError", "SyntaxError"];
                        for error_type in error_types {
                            if let Some(err_global) = self.get_global(error_type) {
                                if let (Value::Object(err_ref), Value::Object(callee_ref)) = (&err_global, &callee) {
                                    if Rc::ptr_eq(err_ref, callee_ref) {
                                        let ctor_name = format!("__{}_constructor", error_type);
                                        if let Some(ctor) = self.get_global(&ctor_name) {
                                            if let Value::Object(ctor_obj) = ctor {
                                                if let ObjectKind::NativeFunction { func, .. } = &ctor_obj.borrow().kind {
                                                    let result = func(&args)?;
                                                    // Add stack trace from current VM state
                                                    let stack_trace = self.capture_stack_trace();
                                                    let stack_str = format!("{}: {}\n{}",
                                                        error_type,
                                                        args.first().map(|v| v.to_js_string()).unwrap_or_default(),
                                                        stack_trace);
                                                    result.set_property("stack", Value::String(stack_str));
                                                    self.push(result)?;
                                                    return Ok(());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Default: return empty instance
                        self.push(instance)?;
                    }
                    _ => {
                        // Not a constructor
                        self.stack.truncate(callee_pos);
                        self.push(instance)?;
                    }
                }
            }
            _ => {
                // Not an object, just create empty instance
                self.stack.truncate(callee_pos);
                self.push(instance)?;
            }
        }

        Ok(())
    }

    #[inline]
    fn push(&mut self, value: Value) -> Result<()> {
        if self.stack.len() >= MAX_STACK_SIZE {
            return Err(Error::InternalError("Stack overflow".to_string()));
        }
        self.stack.push(value);
        Ok(())
    }

    #[inline]
    fn peek(&self, offset: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - offset]
    }

    /// Pop two values efficiently for binary operations (b, a order)
    #[inline]
    #[allow(dead_code)]
    fn pop_binary(&mut self) -> (Value, Value) {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        (a, b)
    }

    /// Pop one value and get numbers for binary numeric operations
    #[inline]
    fn pop_binary_numbers(&mut self) -> (f64, f64) {
        let b = self.stack.pop().unwrap_or(Value::Undefined).to_number();
        let a = self.stack.pop().unwrap_or(Value::Undefined).to_number();
        (a, b)
    }

    /// Pop and push numeric result in one operation (reduces intermediate allocations)
    #[inline]
    #[allow(dead_code)]
    fn binary_number_op<F>(&mut self, op: F) -> Result<()>
    where
        F: FnOnce(f64, f64) -> f64,
    {
        let (a, b) = self.pop_binary_numbers();
        self.push(Value::Number(op(a, b)))
    }

    /// Helper for binary operations that support both Number and BigInt
    #[inline]
    fn binary_op_with_bigint<F, G>(&mut self, num_op: F, bigint_op: G) -> Result<()>
    where
        F: FnOnce(f64, f64) -> f64,
        G: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
    {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        let result = match (&a, &b) {
            (Value::BigInt(n1), Value::BigInt(n2)) => Value::BigInt(bigint_op(n1.clone(), n2.clone())),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(Error::type_error(
                    "Cannot mix BigInt and other types in arithmetic operations",
                ));
            }
            _ => Value::Number(num_op(a.to_number(), b.to_number())),
        };
        self.push(result)
    }

    /// Pop and push boolean result for comparison operations
    #[inline]
    fn binary_compare_op<F>(&mut self, op: F) -> Result<()>
    where
        F: FnOnce(f64, f64) -> bool,
    {
        let (a, b) = self.pop_binary_numbers();
        self.push(Value::Boolean(op(a, b)))
    }

    /// Pop and push result for bitwise operations
    #[inline]
    fn binary_bitwise_op<F, G>(&mut self, num_op: F, bigint_op: G) -> Result<()>
    where
        F: FnOnce(i32, i32) -> i32,
        G: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
    {
        let b = self.stack.pop().unwrap_or(Value::Undefined);
        let a = self.stack.pop().unwrap_or(Value::Undefined);
        let result = match (&a, &b) {
            (Value::BigInt(n1), Value::BigInt(n2)) => {
                Value::BigInt(bigint_op(n1.clone(), n2.clone()))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(Error::type_error(
                    "Cannot mix BigInt and other types in bitwise operations",
                ));
            }
            _ => {
                let a_num = a.to_number();
                let b_num = b.to_number();
                Value::Number(num_op(a_num as i32, b_num as i32) as f64)
            }
        };
        self.push(result)
    }

    fn current_frame(&self) -> &CallFrame {
        self.frames.last().unwrap()
    }

    fn read_u8(&mut self) -> Result<u8> {
        let frame = self.frames.last_mut().unwrap();
        if frame.ip >= frame.chunk.code.len() {
            return Err(Error::InternalError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let byte = frame.chunk.code[frame.ip];
        frame.ip += 1;
        Ok(byte)
    }

    fn read_u16(&mut self) -> Result<u16> {
        let b1 = self.read_u8()?;
        let b2 = self.read_u8()?;
        Ok(u16::from_le_bytes([b1, b2]))
    }

    fn read_i16(&mut self) -> Result<i16> {
        let b1 = self.read_u8()?;
        let b2 = self.read_u8()?;
        Ok(i16::from_le_bytes([b1, b2]))
    }

    fn get_constant_string(&self, index: u16) -> Result<String> {
        let frame = self.frames.last().unwrap();
        match frame.chunk.get_constant(index) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(v) => Ok(v.to_js_string()),
            None => Err(Error::InternalError("Invalid constant index".to_string())),
        }
    }

    /// Look up property using polymorphic inline cache
    #[inline]
    fn pic_lookup(&self, name: &str, shape_id: u64) -> Option<usize> {
        let hash = hash_property_name(name);
        let slot = (hash as usize) % IC_SIZE;
        let entry = &self.inline_cache[slot];

        if entry.name_hash == hash {
            entry.lookup(shape_id)
        } else {
            None
        }
    }

    /// Update polymorphic inline cache with property access
    #[inline]
    fn pic_update(&mut self, name: &str, shape_id: u64, offset: usize) {
        let hash = hash_property_name(name);
        let slot = (hash as usize) % IC_SIZE;
        let entry = &mut self.inline_cache[slot];

        // If this is a new property name, reset the entry
        if entry.name_hash != hash {
            *entry = InlineCacheEntry::default();
            entry.name_hash = hash;
        }

        entry.update(shape_id, offset);
    }

    /// Check if a cache slot is megamorphic (too many shapes)
    #[inline]
    fn is_megamorphic(&self, name: &str) -> bool {
        let hash = hash_property_name(name);
        let slot = (hash as usize) % IC_SIZE;
        let entry = &self.inline_cache[slot];
        entry.name_hash == hash && entry.is_megamorphic
    }

    /// Fast path for getting a property from an object using polymorphic IC
    #[inline]
    fn get_property_fast(&mut self, obj: &Value, name: &str) -> Option<Value> {
        if let Value::Object(obj_rc) = obj {
            let obj_ref = obj_rc.borrow();

            // Check if this is a Proxy object
            if let ObjectKind::Proxy { target, handler, revoked } = &obj_ref.kind {
                if *revoked {
                    // Revoked proxies throw TypeError on access
                    return None;
                }

                let target = target.clone();
                let handler = handler.clone();
                drop(obj_ref);

                // Try to get the 'get' trap from the handler
                if let Some(trap) = handler.get_property("get") {
                    if let Value::Object(_) = &trap {
                        // Call the trap: trap(target, property, receiver)
                        let property = Value::String(name.to_string());
                        let receiver = obj.clone();
                        let target_clone = (*target).clone();
                        if let Ok(result) = self.call_function_with_this(
                            &trap,
                            &[target_clone, property, receiver],
                            *handler,
                        ) {
                            return Some(result);
                        }
                    }
                }

                // No trap, fall through to target
                return (*target).get_property(name);
            }

            // Skip IC for megamorphic sites
            if !self.is_megamorphic(name) {
                // Get cached shape ID or compute if not available
                // Note: We still compute without caching here to avoid borrow_mut overhead
                // The cache is populated when properties are modified
                let shape_id = obj_ref.cached_shape_id
                    .unwrap_or_else(|| compute_shape_id_raw(&obj_ref.properties));

                // Try polymorphic inline cache first
                if let Some(_cached_offset) = self.pic_lookup(name, shape_id) {
                    // Cache hit - directly access property
                    if let Some(v) = obj_ref.properties.get(name) {
                        return Some(v.clone());
                    }
                }

                // Cache miss - do normal lookup and update cache
                if let Some(v) = obj_ref.properties.get(name) {
                    let result = v.clone();
                    drop(obj_ref);
                    // Update polymorphic cache with this shape
                    self.pic_update(name, shape_id, 0);
                    return Some(result);
                }
            } else {
                // Megamorphic - just do direct lookup without cache
                if let Some(v) = obj_ref.properties.get(name) {
                    return Some(v.clone());
                }
            }
            drop(obj_ref);
        }

        // Use Value::get_property which handles bound methods for arrays and strings
        obj.get_property(name)
    }

    /// Find a getter for a property in the prototype chain
    fn find_getter(&self, obj: &Value, name: &str) -> Option<Value> {
        if let Value::Object(obj_rc) = obj {
            let obj_ref = obj_rc.borrow();

            // If the object itself is a class, check static getters
            if let ObjectKind::Class { static_getters, .. } = &obj_ref.kind {
                if let Some(getter) = static_getters.get(name) {
                    return Some(getter.clone());
                }
            }

            // Check if the prototype is a class with getters (for instances)
            if let Some(proto) = &obj_ref.prototype {
                let proto_ref = proto.borrow();
                if let ObjectKind::Class { getters, .. } = &proto_ref.kind {
                    if let Some(getter) = getters.get(name) {
                        return Some(getter.clone());
                    }
                }
            }
        }
        None
    }

    /// Find a setter for a property in the prototype chain
    fn find_setter(&self, obj: &Value, name: &str) -> Option<Value> {
        if let Value::Object(obj_rc) = obj {
            let obj_ref = obj_rc.borrow();

            // If the object itself is a class, check static setters
            if let ObjectKind::Class { static_setters, .. } = &obj_ref.kind {
                if let Some(setter) = static_setters.get(name) {
                    return Some(setter.clone());
                }
            }

            // Check if the prototype is a class with setters (for instances)
            if let Some(proto) = &obj_ref.prototype {
                let proto_ref = proto.borrow();
                if let ObjectKind::Class { setters, .. } = &proto_ref.kind {
                    if let Some(setter) = setters.get(name) {
                        return Some(setter.clone());
                    }
                }
            }
        }
        None
    }

    /// Fast path for setting a property on an object (avoids intermediate cloning)
    #[inline]
    #[allow(dead_code)]
    fn set_property_fast(&mut self, obj: &Value, name: &str, value: Value) {
        if let Value::Object(obj_rc) = obj {
            obj_rc.borrow_mut().set_property(name, value);
        }
    }

    /// Create a resolved Promise with the given value
    fn create_resolved_promise(&self, value: Value) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Promise {
                state: PromiseState::Fulfilled,
                value: Some(Box::new(value)),
                on_fulfilled: Vec::new(),
                on_rejected: Vec::new(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        })))
    }

    /// Create a rejected Promise with the given reason
    #[allow(dead_code)]
    fn create_rejected_promise(&self, reason: Value) -> Value {
        Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Promise {
                state: PromiseState::Rejected,
                value: Some(Box::new(reason)),
                on_fulfilled: Vec::new(),
                on_rejected: Vec::new(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        })))
    }

    /// Create a generator object from a generator function
    fn create_generator(&self, function: Function, initial_locals: Vec<Value>) -> Value {
        // For a simpler implementation, we collect all yielded values upfront
        // by running the generator function and collecting yields
        let yielded_values = self.collect_generator_yields(&function, &initial_locals);

        // Create generator with collected yields as an array to iterate through
        let generator_obj = Rc::new(RefCell::new(Object {
            kind: ObjectKind::Generator {
                function: Box::new(function),
                ip: 0, // Used as index into yielded values
                locals: initial_locals,
                state: GeneratorState::Suspended,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        }));

        // Store yielded values as a property
        generator_obj.borrow_mut().set_property("__yields__", Value::new_array(yielded_values));

        // Add .next() method
        let gen_clone = Rc::clone(&generator_obj);
        let next_fn: NativeFn = Rc::new(move |_args| {
            let mut obj_ref = gen_clone.borrow_mut();

            // Get the current index and yielded values
            let current_idx = if let ObjectKind::Generator { ip, state, .. } = &obj_ref.kind {
                if matches!(state, GeneratorState::Completed) {
                    let result = Value::new_object();
                    result.set_property("value", Value::Undefined);
                    result.set_property("done", Value::Boolean(true));
                    return Ok(result);
                }
                *ip
            } else {
                return Err(Error::type_error("Not a generator"));
            };

            // Get yielded values array
            let yields = if let Some(Value::Object(arr)) = obj_ref.properties.get("__yields__") {
                if let ObjectKind::Array(items) = &arr.borrow().kind {
                    items.clone()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            // Check if we have more values
            if current_idx < yields.len() {
                let value = yields[current_idx].clone();

                // Update index
                if let ObjectKind::Generator { ip, .. } = &mut obj_ref.kind {
                    *ip = current_idx + 1;
                }

                let result = Value::new_object();
                result.set_property("value", value);
                result.set_property("done", Value::Boolean(false));
                Ok(result)
            } else {
                // No more values - mark as completed
                if let ObjectKind::Generator { state, .. } = &mut obj_ref.kind {
                    *state = GeneratorState::Completed;
                }

                let result = Value::new_object();
                result.set_property("value", Value::Undefined);
                result.set_property("done", Value::Boolean(true));
                Ok(result)
            }
        });

        // Add .return() method
        let gen_clone2 = Rc::clone(&generator_obj);
        let return_fn: NativeFn = Rc::new(move |args| {
            let return_value = args.first().cloned().unwrap_or(Value::Undefined);
            let mut obj_ref = gen_clone2.borrow_mut();

            if let ObjectKind::Generator { state, .. } = &mut obj_ref.kind {
                *state = GeneratorState::Completed;

                let result = Value::new_object();
                if let Value::Object(r) = &result {
                    r.borrow_mut().set_property("value", return_value);
                    r.borrow_mut().set_property("done", Value::Boolean(true));
                }
                Ok(result)
            } else {
                Err(Error::type_error("Not a generator"))
            }
        });

        // Add .throw() method
        let gen_clone3 = Rc::clone(&generator_obj);
        let throw_fn: NativeFn = Rc::new(move |args| {
            let error_value = args.first().cloned().unwrap_or(Value::Undefined);
            let mut obj_ref = gen_clone3.borrow_mut();

            if let ObjectKind::Generator { state, .. } = &mut obj_ref.kind {
                *state = GeneratorState::Completed;
                drop(obj_ref);

                // Throw the error
                Err(Error::type_error(error_value.to_js_string()))
            } else {
                Err(Error::type_error("Not a generator"))
            }
        });

        // Attach methods to the generator object
        {
            let mut obj_ref = generator_obj.borrow_mut();
            obj_ref.set_property("next", Value::Object(Rc::new(RefCell::new(Object {
                kind: ObjectKind::NativeFunction {
                    name: "next".to_string(),
                    func: next_fn,
                },
                properties: HashMap::default(),
                private_fields: HashMap::default(),
                prototype: None, cached_shape_id: None,
            }))));
            obj_ref.set_property("return", Value::Object(Rc::new(RefCell::new(Object {
                kind: ObjectKind::NativeFunction {
                    name: "return".to_string(),
                    func: return_fn,
                },
                properties: HashMap::default(),
                private_fields: HashMap::default(),
                prototype: None, cached_shape_id: None,
            }))));
            obj_ref.set_property("throw", Value::Object(Rc::new(RefCell::new(Object {
                kind: ObjectKind::NativeFunction {
                    name: "throw".to_string(),
                    func: throw_fn,
                },
                properties: HashMap::default(),
                private_fields: HashMap::default(),
                prototype: None, cached_shape_id: None,
            }))));
        }

        Value::Object(generator_obj)
    }

    /// Collect all yielded values from a generator function by executing it
    /// This is a simplified approach that doesn't support lazy evaluation
    fn collect_generator_yields(&self, function: &Function, initial_locals: &[Value]) -> Vec<Value> {
        let mut yields = Vec::new();
        let chunk = &function.chunk;

        // Create a collector VM with a single frame
        let mut collector_vm = VM::new();

        // Copy globals from current VM for access
        for (k, v) in &self.globals {
            collector_vm.globals.insert(k.clone(), v.clone());
        }

        // Push initial locals (function arguments) onto the stack
        // This is how normal function calls work - args are on the stack
        for local in initial_locals {
            collector_vm.stack.push(local.clone());
        }

        // Create frame with bp pointing to start of locals on stack
        let mut frame = CallFrame::new(chunk.clone());
        frame.bp = 0; // Locals start at stack position 0
        collector_vm.frames.push(frame);

        let mut iter_count = 0;
        // Execute until completion, collecting yields
        loop {
            if collector_vm.frames.is_empty() {
                break;
            }

            let frame = collector_vm.frames.last_mut().unwrap();
            if frame.ip >= chunk.code.len() {
                break;
            }

            let opcode = Opcode::from_u8(chunk.code[frame.ip]);
            frame.ip += 1;

            match opcode {
                Some(Opcode::Yield) => {
                    // Collect the yielded value - use peek, not pop
                    // The compiler emits a Pop after Yield to clean up the value
                    let value = collector_vm.stack.last().cloned().unwrap_or(Value::Undefined);
                    yields.push(value);
                }
                Some(Opcode::Return) | Some(Opcode::ReturnUndefined) => {
                    break;
                }
                _ => {
                    if collector_vm.execute_single_gen_instruction(opcode).is_err() {
                        break;
                    }
                }
            }
            iter_count += 1;
            if iter_count > 10000 {
                break; // Safety limit
            }
        }

        collector_vm.frames.pop();
        yields
    }

    /// Execute a single instruction for generator collection (simplified)
    fn execute_single_gen_instruction(&mut self, opcode: Option<Opcode>) -> Result<()> {
        match opcode {
            Some(Opcode::Constant) => {
                let index = self.read_u16()?;
                let frame = self.frames.last().unwrap();
                let value = frame.chunk.constants.get(index as usize).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::Pop) => {
                self.stack.pop();
            }
            Some(Opcode::Dup) => {
                if let Some(top) = self.stack.last().cloned() {
                    self.push(top)?;
                }
            }
            Some(Opcode::Add) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                let result = match (&a, &b) {
                    (Value::String(s1), _) => Value::String(format!("{}{}", s1, b.to_js_string())),
                    (_, Value::String(s2)) => Value::String(format!("{}{}", a.to_js_string(), s2)),
                    (Value::Number(n1), Value::Number(n2)) => Value::Number(n1 + n2),
                    _ => Value::Number(a.to_number() + b.to_number()),
                };
                self.push(result)?;
            }
            Some(Opcode::Sub) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() - b.to_number()))?;
            }
            Some(Opcode::Mul) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() * b.to_number()))?;
            }
            Some(Opcode::Div) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() / b.to_number()))?;
            }
            Some(Opcode::Mod) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() % b.to_number()))?;
            }
            Some(Opcode::Neg) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(-value.to_number()))?;
            }
            Some(Opcode::Not) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!value.to_boolean()))?;
            }
            Some(Opcode::GetLocal) => {
                let slot = self.read_u8()? as usize;
                let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                let value = self.stack.get(bp + slot).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::SetLocal) => {
                let slot = self.read_u8()? as usize;
                let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                // Ensure stack is large enough
                let idx = bp + slot;
                while self.stack.len() <= idx {
                    self.stack.push(Value::Undefined);
                }
                self.stack[idx] = value;
            }
            Some(Opcode::GetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::SetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                self.globals.insert(name, value);
            }
            Some(Opcode::TryGetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::DefineGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.globals.insert(name, value);
            }
            Some(Opcode::Jump) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                if let Some(frame) = self.frames.last_mut() {
                    frame.ip = (frame.ip as isize + offset as isize) as usize;
                }
            }
            Some(Opcode::JumpIfFalse) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                // Use peek (not pop) - the compiler emits a Pop opcode to clean up
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                if !value.to_boolean() {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }
            }
            Some(Opcode::JumpIfTrue) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                // Use peek (not pop) - the compiler emits a Pop opcode to clean up
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                if value.to_boolean() {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }
            }
            Some(Opcode::Lt) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() < b.to_number()))?;
            }
            Some(Opcode::Le) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() <= b.to_number()))?;
            }
            Some(Opcode::Gt) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() > b.to_number()))?;
            }
            Some(Opcode::Ge) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() >= b.to_number()))?;
            }
            Some(Opcode::Eq) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.equals(&b)))?;
            }
            Some(Opcode::Ne) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!a.equals(&b)))?;
            }
            Some(Opcode::StrictEq) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.strict_equals(&b)))?;
            }
            Some(Opcode::StrictNe) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!a.strict_equals(&b)))?;
            }
            Some(Opcode::True) => {
                self.push(Value::Boolean(true))?;
            }
            Some(Opcode::False) => {
                self.push(Value::Boolean(false))?;
            }
            Some(Opcode::Null) => {
                self.push(Value::Null)?;
            }
            Some(Opcode::Undefined) => {
                self.push(Value::Undefined)?;
            }
            Some(Opcode::Increment) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(value.to_number() + 1.0))?;
            }
            Some(Opcode::Decrement) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(value.to_number() - 1.0))?;
            }
            Some(Opcode::Nop) => {
                // No operation
            }
            _ => {
                // For unhandled opcodes, we may need to skip operands
                // This is a simplified implementation
            }
        }
        Ok(())
    }

    /// Execute a single instruction and return
    /// Returns Some(value) if execution should stop (return/yield), None to continue
    #[allow(dead_code)]
    fn execute_one_instruction(&mut self) -> Result<Option<Value>> {
        if self.frames.is_empty() {
            return Ok(Some(Value::Undefined));
        }

        let frame = self.frames.last_mut().unwrap();
        if frame.ip >= frame.chunk.code.len() {
            return Ok(Some(self.stack.pop().unwrap_or(Value::Undefined)));
        }

        let opcode = Opcode::from_u8(frame.chunk.code[frame.ip]);
        frame.ip += 1;

        // Handle just the essential opcodes for generator collection
        match opcode {
            Some(Opcode::Return) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                return Ok(Some(value));
            }
            Some(Opcode::Yield) => {
                // Yield returns the value - handled specially in collect_generator_yields
                return Ok(Some(self.stack.pop().unwrap_or(Value::Undefined)));
            }
            Some(Opcode::Constant) => {
                let index = self.read_u16()?;
                let frame = self.frames.last().unwrap();
                let value = frame.chunk.constants.get(index as usize).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::Pop) => {
                self.stack.pop();
            }
            Some(Opcode::Dup) => {
                if let Some(top) = self.stack.last().cloned() {
                    self.push(top)?;
                }
            }
            Some(Opcode::Add) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                let result = match (&a, &b) {
                    (Value::String(s1), _) => Value::String(format!("{}{}", s1, b.to_js_string())),
                    (_, Value::String(s2)) => Value::String(format!("{}{}", a.to_js_string(), s2)),
                    (Value::Number(n1), Value::Number(n2)) => Value::Number(n1 + n2),
                    _ => Value::Number(a.to_number() + b.to_number()),
                };
                self.push(result)?;
            }
            Some(Opcode::Sub) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() - b.to_number()))?;
            }
            Some(Opcode::Mul) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() * b.to_number()))?;
            }
            Some(Opcode::Div) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() / b.to_number()))?;
            }
            Some(Opcode::Mod) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(a.to_number() % b.to_number()))?;
            }
            Some(Opcode::Neg) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(-value.to_number()))?;
            }
            Some(Opcode::Not) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!value.to_boolean()))?;
            }
            Some(Opcode::GetLocal) => {
                let slot = self.read_u8()? as usize;
                let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                let value = self.stack.get(bp + slot).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::SetLocal) => {
                let slot = self.read_u8()? as usize;
                let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                // Ensure stack is large enough
                let idx = bp + slot;
                while self.stack.len() <= idx {
                    self.stack.push(Value::Undefined);
                }
                self.stack[idx] = value;
            }
            Some(Opcode::GetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::SetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                self.globals.insert(name, value);
            }
            Some(Opcode::DefineGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.globals.insert(name, value);
            }
            Some(Opcode::Jump) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                if let Some(frame) = self.frames.last_mut() {
                    frame.ip = (frame.ip as isize + offset as isize) as usize;
                }
            }
            Some(Opcode::TryGetGlobal) => {
                let index = self.read_u16()?;
                let name = self.get_constant_string(index)?;
                let value = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                self.push(value)?;
            }
            Some(Opcode::JumpIfFalse) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                // Use peek (not pop) - the compiler emits a Pop opcode to clean up
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                if !value.to_boolean() {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }
            }
            Some(Opcode::JumpIfTrue) => {
                // Use signed relative offset like main VM
                let offset = self.read_i16()?;
                // Use peek (not pop) - the compiler emits a Pop opcode to clean up
                let value = self.stack.last().cloned().unwrap_or(Value::Undefined);
                if value.to_boolean() {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = (frame.ip as isize + offset as isize) as usize;
                    }
                }
            }
            Some(Opcode::Lt) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() < b.to_number()))?;
            }
            Some(Opcode::Le) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() <= b.to_number()))?;
            }
            Some(Opcode::Gt) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() > b.to_number()))?;
            }
            Some(Opcode::Ge) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.to_number() >= b.to_number()))?;
            }
            Some(Opcode::Eq) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.equals(&b)))?;
            }
            Some(Opcode::Ne) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!a.equals(&b)))?;
            }
            Some(Opcode::StrictEq) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(a.strict_equals(&b)))?;
            }
            Some(Opcode::StrictNe) => {
                let b = self.stack.pop().unwrap_or(Value::Undefined);
                let a = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Boolean(!a.strict_equals(&b)))?;
            }
            Some(Opcode::True) => {
                self.push(Value::Boolean(true))?;
            }
            Some(Opcode::False) => {
                self.push(Value::Boolean(false))?;
            }
            Some(Opcode::Null) => {
                self.push(Value::Null)?;
            }
            Some(Opcode::Undefined) => {
                self.push(Value::Undefined)?;
            }
            Some(Opcode::Increment) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(value.to_number() + 1.0))?;
            }
            Some(Opcode::Decrement) => {
                let value = self.stack.pop().unwrap_or(Value::Undefined);
                self.push(Value::Number(value.to_number() - 1.0))?;
            }
            _ => {
                // For other opcodes, skip them (simplified implementation)
                // This may cause issues with complex generators
            }
        }
        Ok(None)
    }

    /// Find a similar global name for "did you mean?" suggestions
    fn suggest_similar_global(&self, name: &str) -> Option<String> {
        let name_lower = name.to_lowercase();
        let mut best_match: Option<(&str, usize)> = None;

        for key in self.globals.keys() {
            let dist = levenshtein_distance_small(&name_lower, &key.to_lowercase());
            if dist <= 2 && dist > 0 {
                if best_match.is_none() || dist < best_match.unwrap().1 {
                    best_match = Some((key, dist));
                }
            }
        }

        best_match.map(|(s, _)| s.to_string())
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

/// Levenshtein distance for short strings (used for typo detection)
fn levenshtein_distance_small(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }
    // Skip if lengths differ by more than the threshold
    if a_len.abs_diff(b_len) > 2 { return 3; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost)
                .min(curr[j] + 1)
                .min(prev[j + 1] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::compile;

    #[test]
    fn test_simple_arithmetic() {
        let mut vm = VM::new();
        let chunk = compile("1 + 2").unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_comparison() {
        let mut vm = VM::new();
        let chunk = compile("5 > 3").unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_string_concat() {
        let mut vm = VM::new();
        let chunk = compile("'hello' + ' ' + 'world'").unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn test_boolean_logic() {
        let mut vm = VM::new();
        let chunk = compile("true && false").unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Boolean(false));

        let chunk = compile("true || false").unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    // ========== Resource Limit Tests ==========

    #[test]
    fn test_operation_limit_exceeded() {
        let mut vm = VM::new();
        vm.set_resource_limits(
            ResourceLimits::new()
                .with_operation_limit(100)
        );
        // Infinite loop that should be stopped
        let chunk = compile("let i = 0; while (true) { i++; }").unwrap();
        let result = vm.run(&chunk);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("OperationLimit"));
    }

    #[test]
    fn test_stack_depth_limit_exceeded() {
        let mut vm = VM::new();
        vm.set_resource_limits(
            ResourceLimits::new()
                .with_stack_depth_limit(10)
        );
        // Recursive function that should exceed stack depth
        // Note: Adding 1 to the result prevents tail call optimization (TCO)
        // because the addition must happen after the recursive call returns
        let chunk = compile(r#"
            function recurse(n) {
                return 1 + recurse(n + 1);
            }
            recurse(0);
        "#).unwrap();
        let result = vm.run(&chunk);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("StackDepthLimit"), "Expected StackDepthLimit error, got: {}", err);
    }

    #[test]
    fn test_normal_execution_within_limits() {
        let mut vm = VM::new();
        vm.set_resource_limits(
            ResourceLimits::new()
                .with_operation_limit(100000)
                .with_stack_depth_limit(100)
        );
        // Simple computation that should succeed
        let chunk = compile(r#"
            let sum = 0;
            for (let i = 0; i < 10; i++) {
                sum += i;
            }
            sum;
        "#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Number(45.0));
    }

    #[test]
    fn test_resource_limits_builder() {
        let limits = ResourceLimits::new()
            .with_time_limit(5000)
            .with_operation_limit(1000000)
            .with_memory_limit(1024 * 1024)
            .with_stack_depth_limit(500);

        assert_eq!(limits.time_limit_ms, Some(5000));
        assert_eq!(limits.operation_limit, Some(1000000));
        assert_eq!(limits.memory_limit, Some(1024 * 1024));
        assert_eq!(limits.stack_depth_limit, Some(500));
    }

    #[test]
    fn test_function_call_method() {
        let mut vm = VM::new();
        // Test Function.prototype.call with user-defined function
        let chunk = compile(
            r#"
            function greet(greeting) {
                return greeting + ", " + this.name;
            }
            let obj = { name: "World" };
            greet.call(obj, "Hello");
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("Hello, World".to_string()));
    }

    #[test]
    fn test_function_apply_method() {
        let mut vm = VM::new();
        // Test Function.prototype.apply with array of arguments
        let chunk = compile(
            r#"
            function sum(a, b, c) {
                return a + b + c;
            }
            sum.apply(null, [1, 2, 3]);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_function_bind_method() {
        let mut vm = VM::new();
        // Test Function.prototype.bind
        let chunk = compile(
            r#"
            function greet(greeting) {
                return greeting + ", " + this.name;
            }
            let obj = { name: "World" };
            let boundGreet = greet.bind(obj);
            boundGreet("Hello");
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("Hello, World".to_string()));
    }

    #[test]
    fn test_function_bind_with_args() {
        let mut vm = VM::new();
        // Test Function.prototype.bind with pre-filled arguments
        let chunk = compile(
            r#"
            function add(a, b, c) {
                return a + b + c;
            }
            let add5 = add.bind(null, 5);
            add5(10, 15);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Number(30.0));
    }

    #[test]
    fn test_bound_function_ignores_this() {
        let mut vm = VM::new();
        // Test that bound function ignores the thisArg passed to call
        let chunk = compile(
            r#"
            function getName() {
                return this.name;
            }
            let obj1 = { name: "First" };
            let obj2 = { name: "Second" };
            let boundGetName = getName.bind(obj1);
            // Trying to override this with call should not work
            boundGetName.call(obj2);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should return "First" because bind's this takes precedence
        assert_eq!(result, Value::String("First".to_string()));
    }

    #[test]
    fn test_promise_any_first_fulfilled() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();
        // Promise.any should resolve with first fulfilled value
        let result = runtime
            .eval(
                r#"
            let p1 = Promise.reject("error1");
            let p2 = Promise.resolve("success");
            let p3 = Promise.reject("error2");
            Promise.any([p1, p2, p3]);
        "#,
            )
            .unwrap();
        // Should be a fulfilled promise
        if let Value::Object(obj) = &result {
            let obj = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj.kind {
                assert!(
                    matches!(state, crate::runtime::value::PromiseState::Fulfilled),
                    "Expected Fulfilled state"
                );
                assert_eq!(
                    value.as_ref().map(|v| *v.clone()),
                    Some(Value::String("success".to_string()))
                );
            } else {
                panic!("Expected Promise object");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_promise_any_all_rejected() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();
        // Promise.any should reject when all promises reject
        let result = runtime
            .eval(
                r#"
            let p1 = Promise.reject("error1");
            let p2 = Promise.reject("error2");
            Promise.any([p1, p2]);
        "#,
            )
            .unwrap();
        // Should be a rejected promise with AggregateError
        if let Value::Object(obj) = &result {
            let obj = obj.borrow();
            if let ObjectKind::Promise { state, value, .. } = &obj.kind {
                assert!(
                    matches!(state, crate::runtime::value::PromiseState::Rejected),
                    "Expected Rejected state"
                );
                // Check that value contains an AggregateError
                if let Some(err) = value {
                    if let Value::Object(err_obj) = &**err {
                        let err_ref = err_obj.borrow();
                        let name = err_ref.get_property("name");
                        assert_eq!(name, Some(Value::String("AggregateError".to_string())));
                    }
                }
            } else {
                panic!("Expected Promise object");
            }
        } else {
            panic!("Expected Object");
        }
    }

    // ========== ES2023 Array Method Tests ==========

    #[test]
    fn test_array_to_reversed() {
        let mut vm = VM::new();
        // toReversed should return new array without mutating original
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 4, 5];
            let reversed = arr.toReversed();
            arr[0] + "-" + reversed[0];
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // arr[0] should still be 1, reversed[0] should be 5
        assert_eq!(result, Value::String("1-5".to_string()));
    }

    #[test]
    fn test_array_to_sorted() {
        let mut vm = VM::new();
        // toSorted should return new sorted array without mutating original
        let chunk = compile(
            r#"
            let arr = [3, 1, 4, 1, 5];
            let sorted = arr.toSorted((a, b) => a - b);
            arr[0] + "-" + sorted[0];
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // arr[0] should still be 3, sorted[0] should be 1
        assert_eq!(result, Value::String("3-1".to_string()));
    }

    #[test]
    fn test_array_to_spliced() {
        let mut vm = VM::new();
        // toSpliced should return new array with modifications
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 4, 5];
            let spliced = arr.toSpliced(1, 2, 10, 20);
            arr.join(",") + " | " + spliced.join(",");
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Original unchanged, new array has elements replaced
        assert_eq!(result, Value::String("1,2,3,4,5 | 1,10,20,4,5".to_string()));
    }

    #[test]
    fn test_array_with() {
        let mut vm = VM::new();
        // with() should return new array with element replaced
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 4, 5];
            let updated = arr.with(2, 100);
            arr[2] + "-" + updated[2];
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // arr[2] should still be 3, updated[2] should be 100
        assert_eq!(result, Value::String("3-100".to_string()));
    }

    #[test]
    fn test_array_find_last() {
        let mut vm = VM::new();
        // findLast should find last element matching predicate
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 4, 5];
            arr.findLast(x => x < 4);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should find 3 (last element < 4)
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_array_find_last_index() {
        let mut vm = VM::new();
        // findLastIndex should find index of last element matching predicate
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 2, 1];
            arr.findLastIndex(x => x === 2);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should find index 3 (last occurrence of 2)
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_array_reduce_right() {
        let mut vm = VM::new();
        // reduceRight should reduce from right to left
        let chunk = compile(
            r#"
            let arr = ["a", "b", "c"];
            arr.reduceRight((acc, val) => acc + val, "");
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should produce "cba"
        assert_eq!(result, Value::String("cba".to_string()));
    }

    #[test]
    fn test_array_last_index_of() {
        let mut vm = VM::new();
        // lastIndexOf should find last occurrence
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 2, 1];
            arr.lastIndexOf(2);
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should find index 3
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_array_copy_within() {
        let mut vm = VM::new();
        // copyWithin should copy elements within the array
        let chunk = compile(
            r#"
            let arr = [1, 2, 3, 4, 5];
            arr.copyWithin(0, 3);
            arr.join(",");
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        // Should copy [4, 5] to beginning: [4, 5, 3, 4, 5]
        assert_eq!(result, Value::String("4,5,3,4,5".to_string()));
    }

    // ========== String Method Tests ==========

    #[test]
    fn test_string_pad_start() {
        let mut vm = VM::new();
        let chunk = compile(r#""5".padStart(3, "0")"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("005".to_string()));
    }

    #[test]
    fn test_string_pad_end() {
        let mut vm = VM::new();
        let chunk = compile(r#""5".padEnd(3, "0")"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("500".to_string()));
    }

    #[test]
    fn test_string_repeat() {
        let mut vm = VM::new();
        let chunk = compile(r#""ab".repeat(3)"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("ababab".to_string()));
    }

    #[test]
    fn test_string_at() {
        let mut vm = VM::new();
        // Positive index
        let chunk = compile(r#""hello".at(1)"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("e".to_string()));

        // Negative index
        let chunk = compile(r#""hello".at(-1)"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("o".to_string()));
    }

    #[test]
    fn test_string_match() {
        let mut vm = VM::new();
        // Basic string match
        let chunk = compile(
            r#"
            let result = "hello world".match("world");
            result[0];
        "#,
        )
        .unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::String("world".to_string()));
    }

    #[test]
    fn test_string_search() {
        let mut vm = VM::new();
        let chunk = compile(r#""hello world".search("world")"#).unwrap();
        let result = vm.run(&chunk).unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    // ========== Number Static Method Tests ==========

    #[test]
    fn test_number_is_nan() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();

        // Number.isNaN doesn't coerce, unlike global isNaN
        let result = runtime.eval("Number.isNaN(NaN)").unwrap();
        assert_eq!(result, Value::Boolean(true));

        let result = runtime.eval("Number.isNaN(123)").unwrap();
        assert_eq!(result, Value::Boolean(false));

        let result = runtime.eval(r#"Number.isNaN("NaN")"#).unwrap();
        assert_eq!(result, Value::Boolean(false)); // String not coerced
    }

    #[test]
    fn test_number_is_finite() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();

        let result = runtime.eval("Number.isFinite(123)").unwrap();
        assert_eq!(result, Value::Boolean(true));

        let result = runtime.eval("Number.isFinite(Infinity)").unwrap();
        assert_eq!(result, Value::Boolean(false));

        let result = runtime.eval("Number.isFinite(NaN)").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_number_is_integer() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();

        let result = runtime.eval("Number.isInteger(5)").unwrap();
        assert_eq!(result, Value::Boolean(true));

        let result = runtime.eval("Number.isInteger(5.5)").unwrap();
        assert_eq!(result, Value::Boolean(false));

        let result = runtime.eval("Number.isInteger(Infinity)").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_number_is_safe_integer() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();

        let result = runtime.eval("Number.isSafeInteger(100)").unwrap();
        assert_eq!(result, Value::Boolean(true));

        let result = runtime.eval("Number.isSafeInteger(Number.MAX_SAFE_INTEGER)").unwrap();
        assert_eq!(result, Value::Boolean(true));

        let result = runtime.eval("Number.isSafeInteger(Number.MAX_SAFE_INTEGER + 1)").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_number_constants() {
        use crate::runtime::Runtime;
        let mut runtime = Runtime::new();

        let result = runtime.eval("Number.MAX_SAFE_INTEGER").unwrap();
        assert_eq!(result, Value::Number(9007199254740991.0));

        let result = runtime.eval("Number.MIN_SAFE_INTEGER").unwrap();
        assert_eq!(result, Value::Number(-9007199254740991.0));

        let result = runtime.eval("Number.POSITIVE_INFINITY === Infinity").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_did_you_mean_suggestion() {
        let mut runtime = crate::Runtime::new();
        let result = runtime.eval("consol.log('hi')");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("Did you mean 'console'?"), "Expected suggestion, got: {}", err_msg);
    }

    #[test]
    fn test_no_suggestion_for_unknown() {
        let mut runtime = crate::Runtime::new();
        let result = runtime.eval("totallyUnknownVariable");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(!err_msg.contains("Did you mean"), "Should not suggest for unknown: {}", err_msg);
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance_small("console", "consol"), 1);
        assert_eq!(levenshtein_distance_small("Math", "Maht"), 2);
        assert_eq!(levenshtein_distance_small("abc", "abc"), 0);
        assert_eq!(levenshtein_distance_small("", "abc"), 3);
    }
}

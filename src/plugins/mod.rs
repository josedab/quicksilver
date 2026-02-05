//! Plugin System with Effect Handlers
//!
//! Extensible plugin architecture that leverages algebraic effects for composable,
//! interceptable middleware. Plugins can hook into the runtime lifecycle, register
//! custom effects, and compose together without conflicts.
//!
//! # Example
//! ```text
//! // Define a logging plugin
//! let mut registry = PluginRegistry::new();
//! let plugin = Plugin::new("logger", "1.0.0")
//!     .with_hook(LifecycleHook::BeforeEval, |ctx| {
//!         println!("Evaluating: {}", ctx.source_preview());
//!         Ok(HookResult::Continue)
//!     })
//!     .with_effect_handler("Log", "info", |args| {
//!         println!("[INFO] {}", args[0]);
//!         Ok(Value::Undefined)
//!     });
//! registry.register(plugin)?;
//! ```

//! **Status:** ⚠️ Partial — Plugin loading and lifecycle management

use rustc_hash::FxHashMap as HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::fmt;

use crate::error::{Error, Result};
use crate::runtime::Value;

/// Plugin metadata and version info
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Unique plugin identifier
    pub name: String,
    /// Semantic version string
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Author name
    pub author: String,
    /// Plugin dependencies (name -> version requirement)
    pub dependencies: Vec<PluginDependency>,
    /// Tags for categorization
    pub tags: Vec<String>,
}

/// A plugin dependency specification
#[derive(Debug, Clone)]
pub struct PluginDependency {
    /// Name of the required plugin
    pub name: String,
    /// Minimum version required (semver)
    pub min_version: String,
}

/// Lifecycle hooks that plugins can intercept
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleHook {
    /// Before source code is evaluated
    BeforeEval,
    /// After evaluation completes
    AfterEval,
    /// Before a function is called
    BeforeCall,
    /// After a function returns
    AfterCall,
    /// When a value is created
    OnValueCreate,
    /// When an error occurs
    OnError,
    /// When the runtime is initialized
    OnInit,
    /// When the runtime is shutting down
    OnShutdown,
    /// Before module resolution
    BeforeModuleResolve,
    /// After module is loaded
    AfterModuleLoad,
}

impl fmt::Display for LifecycleHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LifecycleHook::BeforeEval => write!(f, "before_eval"),
            LifecycleHook::AfterEval => write!(f, "after_eval"),
            LifecycleHook::BeforeCall => write!(f, "before_call"),
            LifecycleHook::AfterCall => write!(f, "after_call"),
            LifecycleHook::OnValueCreate => write!(f, "on_value_create"),
            LifecycleHook::OnError => write!(f, "on_error"),
            LifecycleHook::OnInit => write!(f, "on_init"),
            LifecycleHook::OnShutdown => write!(f, "on_shutdown"),
            LifecycleHook::BeforeModuleResolve => write!(f, "before_module_resolve"),
            LifecycleHook::AfterModuleLoad => write!(f, "after_module_load"),
        }
    }
}

/// Result from a hook execution
#[derive(Debug, Clone)]
pub enum HookResult {
    /// Continue with normal execution
    Continue,
    /// Skip the operation (e.g., skip eval, skip call)
    Skip,
    /// Replace the result with a different value
    Replace(Value),
    /// Abort with an error
    Abort(String),
}

/// Context passed to hook handlers
#[derive(Debug, Clone)]
pub struct HookContext {
    /// The hook type being invoked
    pub hook: LifecycleHook,
    /// Source code (for eval hooks)
    pub source: Option<String>,
    /// Function name (for call hooks)
    pub function_name: Option<String>,
    /// Arguments (for call hooks)
    pub args: Vec<Value>,
    /// Result value (for after hooks)
    pub result: Option<Value>,
    /// Error message (for error hooks)
    pub error: Option<String>,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl HookContext {
    /// Create an empty context for a hook
    pub fn new(hook: LifecycleHook) -> Self {
        Self {
            hook,
            source: None,
            function_name: None,
            args: Vec::new(),
            result: None,
            error: None,
            metadata: HashMap::default(),
        }
    }

    /// Create a context for eval hooks
    pub fn for_eval(hook: LifecycleHook, source: &str) -> Self {
        Self {
            hook,
            source: Some(source.to_string()),
            function_name: None,
            args: Vec::new(),
            result: None,
            error: None,
            metadata: HashMap::default(),
        }
    }

    /// Create a context for call hooks
    pub fn for_call(hook: LifecycleHook, name: &str, args: Vec<Value>) -> Self {
        Self {
            hook,
            source: None,
            function_name: Some(name.to_string()),
            args,
            result: None,
            error: None,
            metadata: HashMap::default(),
        }
    }

    /// Get a preview of the source (first 80 chars)
    pub fn source_preview(&self) -> String {
        match &self.source {
            Some(s) if s.len() > 80 => format!("{}...", &s[..80]),
            Some(s) => s.clone(),
            None => String::from("<no source>"),
        }
    }
}

/// Type for hook handler functions
type HookHandler = Rc<dyn Fn(&HookContext) -> Result<HookResult>>;

/// Type for effect handler functions
type EffectHandler = Rc<dyn Fn(&[Value]) -> Result<Value>>;

/// An effect registration for a plugin
#[derive(Clone)]
struct EffectRegistration {
    /// Effect type name
    effect_type: String,
    /// Operation name
    operation: String,
    /// Handler function
    handler: EffectHandler,
}

impl fmt::Debug for EffectRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectRegistration")
            .field("effect_type", &self.effect_type)
            .field("operation", &self.operation)
            .finish()
    }
}

/// A registered hook with its handler
#[derive(Clone)]
struct HookRegistration {
    /// The hook point
    hook: LifecycleHook,
    /// Priority (lower = earlier execution)
    priority: i32,
    /// Handler function
    handler: HookHandler,
}

impl fmt::Debug for HookRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookRegistration")
            .field("hook", &self.hook)
            .field("priority", &self.priority)
            .finish()
    }
}

/// A plugin that extends the runtime
pub struct Plugin {
    /// Plugin metadata
    pub metadata: PluginMetadata,
    /// Registered hooks
    hooks: Vec<HookRegistration>,
    /// Registered effect handlers
    effects: Vec<EffectRegistration>,
    /// Plugin-specific configuration
    config: HashMap<String, Value>,
    /// Whether the plugin is enabled
    enabled: bool,
}

impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin")
            .field("metadata", &self.metadata)
            .field("hooks", &self.hooks.len())
            .field("effects", &self.effects.len())
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Plugin {
    /// Create a new plugin with name and version
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            metadata: PluginMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: String::new(),
                author: String::new(),
                dependencies: Vec::new(),
                tags: Vec::new(),
            },
            hooks: Vec::new(),
            effects: Vec::new(),
            config: HashMap::default(),
            enabled: true,
        }
    }

    /// Set the plugin description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.metadata.description = desc.to_string();
        self
    }

    /// Set the plugin author
    pub fn with_author(mut self, author: &str) -> Self {
        self.metadata.author = author.to_string();
        self
    }

    /// Add a dependency
    pub fn with_dependency(mut self, name: &str, min_version: &str) -> Self {
        self.metadata.dependencies.push(PluginDependency {
            name: name.to_string(),
            min_version: min_version.to_string(),
        });
        self
    }

    /// Register a lifecycle hook handler
    pub fn with_hook<F>(mut self, hook: LifecycleHook, handler: F) -> Self
    where
        F: Fn(&HookContext) -> Result<HookResult> + 'static,
    {
        self.hooks.push(HookRegistration {
            hook,
            priority: 0,
            handler: Rc::new(handler),
        });
        self
    }

    /// Register a lifecycle hook handler with priority
    pub fn with_hook_priority<F>(mut self, hook: LifecycleHook, priority: i32, handler: F) -> Self
    where
        F: Fn(&HookContext) -> Result<HookResult> + 'static,
    {
        self.hooks.push(HookRegistration {
            hook,
            priority,
            handler: Rc::new(handler),
        });
        self
    }

    /// Register an effect handler
    pub fn with_effect_handler<F>(mut self, effect_type: &str, operation: &str, handler: F) -> Self
    where
        F: Fn(&[Value]) -> Result<Value> + 'static,
    {
        self.effects.push(EffectRegistration {
            effect_type: effect_type.to_string(),
            operation: operation.to_string(),
            handler: Rc::new(handler),
        });
        self
    }

    /// Set a configuration value
    pub fn with_config(mut self, key: &str, value: Value) -> Self {
        self.config.insert(key.to_string(), value);
        self
    }

    /// Get a configuration value
    pub fn get_config(&self, key: &str) -> Option<&Value> {
        self.config.get(key)
    }

    /// Enable the plugin
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the plugin
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if the plugin is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Middleware that wraps around effect execution
pub struct Middleware {
    /// Middleware name
    pub name: String,
    /// Priority (lower = outer layer, executed first)
    pub priority: i32,
    /// The middleware function: takes args and a "next" function
    handler: Rc<dyn Fn(&[Value], &dyn Fn(&[Value]) -> Result<Value>) -> Result<Value>>,
}

impl fmt::Debug for Middleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Middleware")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .finish()
    }
}

impl Middleware {
    /// Create a new middleware
    pub fn new<F>(name: &str, priority: i32, handler: F) -> Self
    where
        F: Fn(&[Value], &dyn Fn(&[Value]) -> Result<Value>) -> Result<Value> + 'static,
    {
        Self {
            name: name.to_string(),
            priority,
            handler: Rc::new(handler),
        }
    }

    /// Execute this middleware
    pub fn execute(&self, args: &[Value], next: &dyn Fn(&[Value]) -> Result<Value>) -> Result<Value> {
        (self.handler)(args, next)
    }
}

/// Central plugin registry that manages all plugins and their interactions
pub struct PluginRegistry {
    /// Registered plugins in load order
    plugins: Vec<Rc<RefCell<Plugin>>>,
    /// Plugin name -> index mapping
    name_index: HashMap<String, usize>,
    /// Compiled hook chains: hook -> sorted list of (priority, plugin_idx, handler)
    hook_chains: HashMap<LifecycleHook, Vec<(i32, usize, HookHandler)>>,
    /// Effect handler registry: "type.operation" -> (plugin_idx, handler)
    effect_handlers: HashMap<String, (usize, EffectHandler)>,
    /// Global middleware stack
    middleware: Vec<Middleware>,
    /// Whether hook chains need recompilation
    dirty: bool,
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            name_index: HashMap::default(),
            hook_chains: HashMap::default(),
            effect_handlers: HashMap::default(),
            middleware: Vec::new(),
            dirty: false,
        }
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: Plugin) -> Result<()> {
        let name = plugin.metadata.name.clone();

        // Check for duplicate registration
        if self.name_index.contains_key(&name) {
            return Err(Error::type_error(format!(
                "Plugin '{}' is already registered",
                name
            )));
        }

        // Check dependencies
        for dep in &plugin.metadata.dependencies {
            if !self.name_index.contains_key(&dep.name) {
                return Err(Error::type_error(format!(
                    "Plugin '{}' requires plugin '{}' (>= {}), which is not registered",
                    name, dep.name, dep.min_version
                )));
            }
        }

        let idx = self.plugins.len();

        // Register effect handlers
        for effect in &plugin.effects {
            let key = format!("{}.{}", effect.effect_type, effect.operation);
            if self.effect_handlers.contains_key(&key) {
                return Err(Error::type_error(format!(
                    "Effect handler for '{}' is already registered by another plugin",
                    key
                )));
            }
            self.effect_handlers.insert(key, (idx, effect.handler.clone()));
        }

        self.name_index.insert(name, idx);
        self.plugins.push(Rc::new(RefCell::new(plugin)));
        self.dirty = true;

        Ok(())
    }

    /// Unregister a plugin by name
    pub fn unregister(&mut self, name: &str) -> Result<()> {
        let idx = self.name_index.get(name)
            .ok_or_else(|| Error::type_error(format!("Plugin '{}' is not registered", name)))?;
        let idx = *idx;

        // Check if any other plugin depends on this one
        for (i, plugin_rc) in self.plugins.iter().enumerate() {
            if i == idx { continue; }
            let plugin = plugin_rc.borrow();
            for dep in &plugin.metadata.dependencies {
                if dep.name == name {
                    return Err(Error::type_error(format!(
                        "Cannot unregister '{}': plugin '{}' depends on it",
                        name, plugin.metadata.name
                    )));
                }
            }
        }

        // Remove effect handlers for this plugin
        self.effect_handlers.retain(|_, (pidx, _)| *pidx != idx);

        // Mark plugin as disabled (we don't remove to preserve indices)
        self.plugins[idx].borrow_mut().disable();
        self.name_index.remove(name);
        self.dirty = true;

        Ok(())
    }

    /// Recompile hook chains (call after registration changes)
    fn recompile_hooks(&mut self) {
        self.hook_chains.clear();

        for (idx, plugin_rc) in self.plugins.iter().enumerate() {
            let plugin = plugin_rc.borrow();
            if !plugin.enabled { continue; }

            for hook_reg in &plugin.hooks {
                let chain = self.hook_chains
                    .entry(hook_reg.hook)
                    .or_default();
                chain.push((hook_reg.priority, idx, hook_reg.handler.clone()));
            }
        }

        // Sort each chain by priority (lower first)
        for chain in self.hook_chains.values_mut() {
            chain.sort_by_key(|(priority, idx, _)| (*priority, *idx));
        }

        self.dirty = false;
    }

    /// Execute a lifecycle hook chain
    pub fn execute_hook(&mut self, context: &HookContext) -> Result<HookResult> {
        if self.dirty {
            self.recompile_hooks();
        }

        let chain = match self.hook_chains.get(&context.hook) {
            Some(chain) => chain.clone(),
            None => return Ok(HookResult::Continue),
        };

        for (_priority, _idx, handler) in &chain {
            match handler(context)? {
                HookResult::Continue => continue,
                result => return Ok(result),
            }
        }

        Ok(HookResult::Continue)
    }

    /// Execute an effect through registered handlers
    pub fn execute_effect(&self, effect_type: &str, operation: &str, args: &[Value]) -> Result<Value> {
        let key = format!("{}.{}", effect_type, operation);

        let (_idx, handler) = self.effect_handlers.get(&key)
            .ok_or_else(|| Error::type_error(format!(
                "No handler registered for effect '{}'",
                key
            )))?;

        // Execute through middleware chain
        if self.middleware.is_empty() {
            return handler(args);
        }

        let handler = handler.clone();
        let mut sorted_middleware: Vec<&Middleware> = self.middleware.iter().collect();
        sorted_middleware.sort_by_key(|m| m.priority);

        // Build middleware chain from inside out
        let base: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |a: &[Value]| handler(a));

        let chain = sorted_middleware.iter().rev().fold(base, |next, mw| {
            let mw_handler = mw.handler.clone();
            Rc::new(move |a: &[Value]| {
                let next_ref = next.clone();
                mw_handler(a, &move |a2: &[Value]| next_ref(a2))
            })
        });

        chain(args)
    }

    /// Add global middleware
    pub fn add_middleware(&mut self, middleware: Middleware) {
        self.middleware.push(middleware);
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<Rc<RefCell<Plugin>>> {
        self.name_index.get(name).map(|&idx| self.plugins[idx].clone())
    }

    /// List all registered plugin names
    pub fn list_plugins(&self) -> Vec<String> {
        self.name_index.keys().cloned().collect()
    }

    /// Get the number of registered plugins
    pub fn plugin_count(&self) -> usize {
        self.name_index.len()
    }

    /// Check if a plugin is registered
    pub fn has_plugin(&self, name: &str) -> bool {
        self.name_index.contains_key(name)
    }

    /// Get all registered effect handler keys
    pub fn list_effects(&self) -> Vec<String> {
        self.effect_handlers.keys().cloned().collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a common logging plugin
pub fn create_logging_plugin() -> Plugin {
    Plugin::new("logger", "1.0.0")
        .with_description("Built-in logging plugin for runtime events")
        .with_hook(LifecycleHook::BeforeEval, |ctx| {
            if let Some(ref source) = ctx.source {
                let preview = if source.len() > 60 {
                    format!("{}...", &source[..60])
                } else {
                    source.clone()
                };
                eprintln!("[quicksilver:logger] eval: {}", preview);
            }
            Ok(HookResult::Continue)
        })
        .with_hook(LifecycleHook::OnError, |ctx| {
            if let Some(ref err) = ctx.error {
                eprintln!("[quicksilver:logger] error: {}", err);
            }
            Ok(HookResult::Continue)
        })
        .with_effect_handler("Log", "info", |args| {
            if let Some(msg) = args.first() {
                eprintln!("[INFO] {}", msg);
            }
            Ok(Value::Undefined)
        })
        .with_effect_handler("Log", "warn", |args| {
            if let Some(msg) = args.first() {
                eprintln!("[WARN] {}", msg);
            }
            Ok(Value::Undefined)
        })
        .with_effect_handler("Log", "error", |args| {
            if let Some(msg) = args.first() {
                eprintln!("[ERROR] {}", msg);
            }
            Ok(Value::Undefined)
        })
}

/// Create a timing middleware that measures effect execution time
pub fn create_timing_middleware() -> Middleware {
    Middleware::new("timing", 0, |args, next| {
        let start = std::time::Instant::now();
        let result = next(args);
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 10 {
            eprintln!("[quicksilver:timing] slow effect: {:?}", elapsed);
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = Plugin::new("test", "1.0.0")
            .with_description("A test plugin")
            .with_author("test-author");

        assert_eq!(plugin.metadata.name, "test");
        assert_eq!(plugin.metadata.version, "1.0.0");
        assert_eq!(plugin.metadata.description, "A test plugin");
        assert!(plugin.is_enabled());
    }

    #[test]
    fn test_plugin_registry_register() {
        let mut registry = PluginRegistry::new();
        let plugin = Plugin::new("test", "1.0.0");
        assert!(registry.register(plugin).is_ok());
        assert!(registry.has_plugin("test"));
        assert_eq!(registry.plugin_count(), 1);
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = PluginRegistry::new();
        let p1 = Plugin::new("test", "1.0.0");
        let p2 = Plugin::new("test", "2.0.0");
        assert!(registry.register(p1).is_ok());
        assert!(registry.register(p2).is_err());
    }

    #[test]
    fn test_missing_dependency() {
        let mut registry = PluginRegistry::new();
        let plugin = Plugin::new("child", "1.0.0")
            .with_dependency("parent", "1.0.0");
        assert!(registry.register(plugin).is_err());
    }

    #[test]
    fn test_dependency_satisfied() {
        let mut registry = PluginRegistry::new();
        let parent = Plugin::new("parent", "1.0.0");
        let child = Plugin::new("child", "1.0.0")
            .with_dependency("parent", "1.0.0");
        assert!(registry.register(parent).is_ok());
        assert!(registry.register(child).is_ok());
    }

    #[test]
    fn test_hook_execution() {
        let mut registry = PluginRegistry::new();
        let called = Rc::new(RefCell::new(false));
        let called_clone = called.clone();

        let plugin = Plugin::new("hook-test", "1.0.0")
            .with_hook(LifecycleHook::BeforeEval, move |_ctx| {
                *called_clone.borrow_mut() = true;
                Ok(HookResult::Continue)
            });

        registry.register(plugin).unwrap();

        let ctx = HookContext::for_eval(LifecycleHook::BeforeEval, "1 + 2");
        let result = registry.execute_hook(&ctx).unwrap();
        assert!(matches!(result, HookResult::Continue));
        assert!(*called.borrow());
    }

    #[test]
    fn test_hook_skip() {
        let mut registry = PluginRegistry::new();

        let plugin = Plugin::new("skip-test", "1.0.0")
            .with_hook(LifecycleHook::BeforeEval, |_ctx| {
                Ok(HookResult::Skip)
            });

        registry.register(plugin).unwrap();

        let ctx = HookContext::for_eval(LifecycleHook::BeforeEval, "dangerous()");
        let result = registry.execute_hook(&ctx).unwrap();
        assert!(matches!(result, HookResult::Skip));
    }

    #[test]
    fn test_hook_priority_ordering() {
        let mut registry = PluginRegistry::new();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order1 = order.clone();
        let p1 = Plugin::new("first", "1.0.0")
            .with_hook_priority(LifecycleHook::BeforeEval, 10, move |_| {
                order1.borrow_mut().push(1);
                Ok(HookResult::Continue)
            });

        let order2 = order.clone();
        let p2 = Plugin::new("second", "1.0.0")
            .with_hook_priority(LifecycleHook::BeforeEval, 5, move |_| {
                order2.borrow_mut().push(2);
                Ok(HookResult::Continue)
            });

        registry.register(p1).unwrap();
        registry.register(p2).unwrap();

        let ctx = HookContext::for_eval(LifecycleHook::BeforeEval, "test");
        registry.execute_hook(&ctx).unwrap();

        // Lower priority (5) should execute first
        assert_eq!(*order.borrow(), vec![2, 1]);
    }

    #[test]
    fn test_effect_handler() {
        let mut registry = PluginRegistry::new();

        let plugin = Plugin::new("math", "1.0.0")
            .with_effect_handler("Math", "double", |args| {
                match args.first() {
                    Some(Value::Number(n)) => Ok(Value::Number(n * 2.0)),
                    _ => Ok(Value::Undefined),
                }
            });

        registry.register(plugin).unwrap();

        let result = registry.execute_effect("Math", "double", &[Value::Number(21.0)]).unwrap();
        assert!(matches!(result, Value::Number(n) if n == 42.0));
    }

    #[test]
    fn test_effect_not_found() {
        let registry = PluginRegistry::new();
        assert!(registry.execute_effect("Missing", "op", &[]).is_err());
    }

    #[test]
    fn test_middleware_wrapping() {
        let mut registry = PluginRegistry::new();

        let plugin = Plugin::new("base", "1.0.0")
            .with_effect_handler("Test", "value", |_args| {
                Ok(Value::Number(10.0))
            });

        registry.register(plugin).unwrap();

        // Add middleware that doubles the result
        registry.add_middleware(Middleware::new("doubler", 0, |args, next| {
            let result = next(args)?;
            match result {
                Value::Number(n) => Ok(Value::Number(n * 2.0)),
                other => Ok(other),
            }
        }));

        let result = registry.execute_effect("Test", "value", &[]).unwrap();
        assert!(matches!(result, Value::Number(n) if n == 20.0));
    }

    #[test]
    fn test_unregister_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = Plugin::new("temp", "1.0.0");
        registry.register(plugin).unwrap();
        assert!(registry.has_plugin("temp"));

        registry.unregister("temp").unwrap();
        assert!(!registry.has_plugin("temp"));
    }

    #[test]
    fn test_unregister_with_dependents() {
        let mut registry = PluginRegistry::new();
        let parent = Plugin::new("parent", "1.0.0");
        let child = Plugin::new("child", "1.0.0")
            .with_dependency("parent", "1.0.0");

        registry.register(parent).unwrap();
        registry.register(child).unwrap();

        // Should fail because child depends on parent
        assert!(registry.unregister("parent").is_err());
    }

    #[test]
    fn test_list_plugins() {
        let mut registry = PluginRegistry::new();
        registry.register(Plugin::new("a", "1.0.0")).unwrap();
        registry.register(Plugin::new("b", "1.0.0")).unwrap();
        registry.register(Plugin::new("c", "1.0.0")).unwrap();

        let names = registry.list_plugins();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert!(names.contains(&"c".to_string()));
    }

    #[test]
    fn test_plugin_config() {
        let plugin = Plugin::new("cfg-test", "1.0.0")
            .with_config("timeout", Value::Number(5000.0))
            .with_config("debug", Value::Boolean(true));

        assert!(matches!(plugin.get_config("timeout"), Some(Value::Number(n)) if *n == 5000.0));
        assert!(matches!(plugin.get_config("debug"), Some(Value::Boolean(true))));
        assert!(plugin.get_config("missing").is_none());
    }

    #[test]
    fn test_hook_context_source_preview() {
        let ctx = HookContext::for_eval(LifecycleHook::BeforeEval, "short");
        assert_eq!(ctx.source_preview(), "short");

        let long_source = "a".repeat(200);
        let ctx2 = HookContext::for_eval(LifecycleHook::BeforeEval, &long_source);
        assert!(ctx2.source_preview().len() < 90);
        assert!(ctx2.source_preview().ends_with("..."));
    }

    #[test]
    fn test_logging_plugin_creation() {
        let plugin = create_logging_plugin();
        assert_eq!(plugin.metadata.name, "logger");
        assert!(!plugin.effects.is_empty());
    }
}

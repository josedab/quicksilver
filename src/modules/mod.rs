//! ES Modules System
//!
//! Implements ES2020 module loading, resolution, and execution.
//!
//! # Features
//! - Module resolution (relative, absolute, node_modules)
//! - Module caching (each module evaluated once)
//! - Circular dependency handling
//! - Named and default exports
//! - Namespace imports
//!
//! # Example
//! ```text
//! // math.js
//! export const PI = 3.14159;
//! export function square(x) { return x * x; }
//! export default function add(a, b) { return a + b; }
//!
//! // main.js
//! import add, { PI, square } from './math.js';
//! console.log(add(PI, square(2)));
//! ```

use crate::ast::{ExportKind, ExportSpecifier, ImportDeclaration, ImportSpecifier, Program, Statement};
use crate::npm::{PackageExports, PackageJson};
use crate::parser::parse;
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Extensions to try when resolving a path without an extension
const RESOLVE_EXTENSIONS: &[&str] = &["js", "mjs", "ts", "json"];

/// Index filenames to try when resolving a directory
const INDEX_FILES: &[&str] = &["index.js", "index.mjs", "index.ts"];

/// Module loading error
#[derive(Debug, Clone)]
pub enum ModuleError {
    /// Module file not found
    NotFound(String),
    /// Parse error in module
    ParseError(String),
    /// Circular dependency detected
    CircularDependency(String),
    /// Export not found
    ExportNotFound { module: String, export: String },
    /// File system error
    IoError(String),
    /// Module resolution failed
    ResolutionFailed(String),
}

impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "module not found: {}", path),
            Self::ParseError(msg) => write!(f, "parse error: {}", msg),
            Self::CircularDependency(path) => write!(f, "circular dependency: {}", path),
            Self::ExportNotFound { module, export } => {
                write!(f, "export '{}' not found in module '{}'", export, module)
            }
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::ResolutionFailed(msg) => write!(f, "resolution failed: {}", msg),
        }
    }
}

impl std::error::Error for ModuleError {}

pub type ModuleResult<T> = Result<T, ModuleError>;

/// Module status during loading
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    /// Module is being fetched
    Fetching,
    /// Module is being linked (dependencies resolved)
    Linking,
    /// Module is being evaluated
    Evaluating,
    /// Module has been fully evaluated
    Evaluated,
    /// Module evaluation failed
    Error,
}

/// A loaded module
#[derive(Debug, Clone)]
pub struct Module {
    /// Unique module identifier (resolved path)
    pub id: String,
    /// Module source path
    pub path: PathBuf,
    /// Parsed program
    pub program: Program,
    /// Module status
    pub status: ModuleStatus,
    /// Exported values
    pub exports: HashMap<String, Value>,
    /// Default export
    pub default_export: Option<Value>,
    /// Re-exports from other modules
    pub re_exports: Vec<ReExport>,
}

/// Re-export specification
#[derive(Debug, Clone)]
pub struct ReExport {
    /// Source module specifier
    pub source: String,
    /// Export mapping (None = export all)
    pub names: Option<Vec<(String, String)>>, // (local, exported)
}

impl Module {
    /// Create a new module
    pub fn new(id: String, path: PathBuf, program: Program) -> Self {
        Self {
            id,
            path,
            program,
            status: ModuleStatus::Fetching,
            exports: HashMap::default(),
            default_export: None,
            re_exports: Vec::new(),
        }
    }

    /// Get an export by name
    pub fn get_export(&self, name: &str) -> Option<&Value> {
        if name == "default" {
            self.default_export.as_ref()
        } else {
            self.exports.get(name)
        }
    }

    /// Set an export
    pub fn set_export(&mut self, name: String, value: Value) {
        if name == "default" {
            self.default_export = Some(value);
        } else {
            self.exports.insert(name, value);
        }
    }

    /// Get all export names
    pub fn export_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.exports.keys().cloned().collect();
        if self.default_export.is_some() {
            names.push("default".to_string());
        }
        names
    }

    /// Create namespace object containing all exports
    pub fn namespace_object(&self) -> Value {
        let mut props = self.exports.clone();
        if let Some(ref default) = self.default_export {
            props.insert("default".to_string(), default.clone());
        }
        Value::new_object_with_properties(props)
    }
}

/// Import map for bare specifier remapping (WICG Import Maps spec)
#[derive(Debug, Clone, Default)]
pub struct ImportMap {
    /// Direct specifier → URL/path mappings
    pub imports: HashMap<String, String>,
    /// Scoped mappings: scope prefix → { specifier → URL }
    pub scopes: HashMap<String, HashMap<String, String>>,
}

impl ImportMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse an import map from a JSON string
    pub fn from_json(json: &str) -> ModuleResult<Self> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| ModuleError::ResolutionFailed(format!("Invalid import map JSON: {}", e)))?;

        let mut map = Self::new();

        if let Some(imports) = value.get("imports").and_then(|v| v.as_object()) {
            for (key, val) in imports {
                if let Some(target) = val.as_str() {
                    map.imports.insert(key.clone(), target.to_string());
                }
            }
        }

        if let Some(scopes) = value.get("scopes").and_then(|v| v.as_object()) {
            for (scope, mappings) in scopes {
                if let Some(obj) = mappings.as_object() {
                    let mut scope_map = HashMap::default();
                    for (key, val) in obj {
                        if let Some(target) = val.as_str() {
                            scope_map.insert(key.clone(), target.to_string());
                        }
                    }
                    map.scopes.insert(scope.clone(), scope_map);
                }
            }
        }

        Ok(map)
    }

    /// Resolve a specifier using the import map
    pub fn resolve(&self, specifier: &str, referrer: Option<&Path>) -> Option<String> {
        // Check scoped mappings first (most specific wins)
        if let Some(referrer) = referrer {
            let referrer_str = referrer.to_string_lossy();
            for (scope, mappings) in &self.scopes {
                if referrer_str.starts_with(scope.as_str()) {
                    // Exact match
                    if let Some(target) = mappings.get(specifier) {
                        return Some(target.clone());
                    }
                    // Prefix match (for path-like mappings ending with /)
                    for (prefix, target) in mappings {
                        if prefix.ends_with('/') && specifier.starts_with(prefix.as_str()) {
                            let suffix = &specifier[prefix.len()..];
                            return Some(format!("{}{}", target, suffix));
                        }
                    }
                }
            }
        }

        // Check top-level imports
        if let Some(target) = self.imports.get(specifier) {
            return Some(target.clone());
        }

        // Prefix match for top-level imports
        for (prefix, target) in &self.imports {
            if prefix.ends_with('/') && specifier.starts_with(prefix.as_str()) {
                let suffix = &specifier[prefix.len()..];
                return Some(format!("{}{}", target, suffix));
            }
        }

        None
    }
}

/// Result of a dynamic import() call
#[derive(Debug, Clone)]
pub struct DynamicImportResult {
    /// The namespace object containing all exports
    pub namespace: Value,
    /// Module ID
    pub module_id: String,
}

/// import.meta object for a module
#[derive(Debug, Clone)]
pub struct ImportMeta {
    /// The URL/path of the current module
    pub url: String,
    /// The directory of the current module
    pub dirname: String,
    /// The filename of the current module
    pub filename: String,
    /// Whether this is the main (entry) module
    pub main: bool,
    /// Custom resolve function placeholder
    pub resolve: Option<String>,
}

impl ImportMeta {
    /// Create import.meta for a module path
    pub fn from_path(path: &Path, is_main: bool) -> Self {
        let url = format!("file://{}", path.to_string_lossy());
        let dirname = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let filename = path.to_string_lossy().to_string();

        Self {
            url,
            dirname,
            filename,
            main: is_main,
            resolve: None,
        }
    }

    /// Convert to a JavaScript Value object
    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        props.insert("url".to_string(), Value::String(self.url.clone()));
        props.insert("dirname".to_string(), Value::String(self.dirname.clone()));
        props.insert("filename".to_string(), Value::String(self.filename.clone()));
        props.insert("main".to_string(), Value::Boolean(self.main));
        Value::new_object_with_properties(props)
    }
}

/// Tracks which module is the entry point for import.meta.main
#[derive(Debug, Clone, Default)]
pub struct ModuleRegistry {
    /// The entry module path
    pub entry_module: Option<PathBuf>,
    /// All loaded module paths in order
    pub load_order: Vec<PathBuf>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the entry module (the first module loaded via CLI)
    pub fn set_entry(&mut self, path: PathBuf) {
        self.entry_module = Some(path);
    }

    /// Check if a module is the entry module
    pub fn is_entry(&self, path: &Path) -> bool {
        self.entry_module.as_deref() == Some(path)
    }

    /// Record a module load
    pub fn record_load(&mut self, path: PathBuf) {
        if !self.load_order.contains(&path) {
            self.load_order.push(path);
        }
    }

    /// Get import.meta for a module
    pub fn import_meta_for(&self, path: &Path) -> ImportMeta {
        ImportMeta::from_path(path, self.is_entry(path))
    }
}

/// Module loader and cache
#[derive(Debug)]
pub struct ModuleLoader {
    /// Loaded modules by resolved path
    modules: Arc<RwLock<HashMap<String, Module>>>,
    /// Modules currently being loaded (for cycle detection)
    loading: Arc<RwLock<Vec<String>>>,
    /// Base directory for resolution
    base_dir: PathBuf,
    /// Import map for bare specifier remapping
    import_map: Option<ImportMap>,
}

impl ModuleLoader {
    /// Create a new module loader
    pub fn new() -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::default())),
            loading: Arc::new(RwLock::new(Vec::new())),
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            import_map: None,
        }
    }

    /// Create a module loader with a specific base directory
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::default())),
            loading: Arc::new(RwLock::new(Vec::new())),
            base_dir,
            import_map: None,
        }
    }

    /// Set the import map for bare specifier remapping
    pub fn set_import_map(&mut self, import_map: ImportMap) {
        self.import_map = Some(import_map);
    }

    /// Load an import map from a JSON file path
    pub fn load_import_map(&mut self, path: &Path) -> ModuleResult<()> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ModuleError::IoError(e.to_string()))?;
        let import_map = ImportMap::from_json(&contents)?;
        self.import_map = Some(import_map);
        Ok(())
    }

    /// Resolve a module specifier to an absolute path
    pub fn resolve(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<PathBuf> {
        // Check import map first for remapping
        let resolved_specifier = if let Some(ref import_map) = self.import_map {
            if let Some(mapped) = import_map.resolve(specifier, referrer) {
                mapped
            } else {
                specifier.to_string()
            }
        } else {
            specifier.to_string()
        };
        let specifier = &resolved_specifier;

        // Determine base directory for resolution
        let base = referrer
            .and_then(|p| p.parent())
            .unwrap_or(&self.base_dir);

        if specifier.starts_with("./") || specifier.starts_with("../") {
            // Relative import
            let path = base.join(specifier);
            self.try_resolve_path(&path)
                .ok_or_else(|| ModuleError::NotFound(specifier.to_string()))
        } else if specifier.starts_with('/') {
            // Absolute import
            let path = PathBuf::from(specifier);
            self.try_resolve_path(&path)
                .ok_or_else(|| ModuleError::NotFound(specifier.to_string()))
        } else {
            // Bare specifier (node_modules traversal)
            self.resolve_bare_specifier(specifier, base)
                .ok_or_else(|| ModuleError::ResolutionFailed(format!(
                    "Cannot find module '{}' in node_modules",
                    specifier
                )))
        }
    }

    /// Try to resolve a file path, attempting extensions and index files
    fn try_resolve_path(&self, path: &Path) -> Option<PathBuf> {
        // 1. Try exact path
        if path.is_file() {
            return path.canonicalize().ok();
        }

        // 2. Try with extensions (only if path has no extension)
        if path.extension().is_none() {
            for ext in RESOLVE_EXTENSIONS {
                let with_ext = path.with_extension(ext);
                if with_ext.is_file() {
                    return with_ext.canonicalize().ok();
                }
            }
        }

        // 3. Try as directory with index files
        if path.is_dir() {
            return self.try_index_files(path);
        }

        None
    }

    /// Try to resolve index files in a directory
    fn try_index_files(&self, dir: &Path) -> Option<PathBuf> {
        for name in INDEX_FILES {
            let index = dir.join(name);
            if index.is_file() {
                return index.canonicalize().ok();
            }
        }
        None
    }

    /// Resolve a bare specifier by walking up node_modules directories
    fn resolve_bare_specifier(&self, specifier: &str, base: &Path) -> Option<PathBuf> {
        let mut dir = Some(base.to_path_buf());
        while let Some(current) = dir {
            let pkg_dir = current.join("node_modules").join(specifier);

            // 1. Check package.json for entry point
            let pkg_json_path = pkg_dir.join("package.json");
            if pkg_json_path.is_file() {
                if let Some(resolved) = self.resolve_via_package_json(&pkg_dir, &pkg_json_path) {
                    return Some(resolved);
                }
            }

            // 2. Try index files in the package directory
            if let Some(resolved) = self.try_index_files(&pkg_dir) {
                return Some(resolved);
            }

            // 3. Try as a file with extensions (e.g. node_modules/specifier.js)
            let file_path = current.join("node_modules").join(specifier);
            if file_path.is_file() {
                return file_path.canonicalize().ok();
            }
            if file_path.extension().is_none() {
                for ext in RESOLVE_EXTENSIONS {
                    let with_ext = file_path.with_extension(ext);
                    if with_ext.is_file() {
                        return with_ext.canonicalize().ok();
                    }
                }
            }

            dir = current.parent().map(Path::to_path_buf);
        }
        None
    }

    /// Resolve a module entry point via its package.json
    fn resolve_via_package_json(&self, pkg_dir: &Path, pkg_json_path: &Path) -> Option<PathBuf> {
        let contents = std::fs::read_to_string(pkg_json_path).ok()?;
        let pkg = PackageJson::parse(&contents).ok()?;

        // Check "exports" field first
        if let Some(ref exports) = pkg.exports {
            if let Some(entry) = Self::resolve_exports(exports) {
                let entry_path = pkg_dir.join(&entry);
                if let Some(resolved) = self.try_resolve_path(&entry_path) {
                    return Some(resolved);
                }
            }
        }

        // Then "module" field (ESM preference)
        if let Some(ref module) = pkg.module {
            let entry_path = pkg_dir.join(module);
            if let Some(resolved) = self.try_resolve_path(&entry_path) {
                return Some(resolved);
            }
        }

        // Then "main" field
        if let Some(ref main) = pkg.main {
            let entry_path = pkg_dir.join(main);
            if let Some(resolved) = self.try_resolve_path(&entry_path) {
                return Some(resolved);
            }
        }

        None
    }

    /// Resolve the "exports" field of package.json
    fn resolve_exports(exports: &PackageExports) -> Option<String> {
        match exports {
            PackageExports::Path(s) => Some(s.clone()),
            PackageExports::Conditional(map) => {
                map.get("import")
                    .or_else(|| map.get("default"))
                    .cloned()
            }
            PackageExports::Subpaths(map) => {
                map.get(".").and_then(Self::resolve_exports)
            }
        }
    }

    /// Load a module from a specifier
    pub fn load(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<Module> {
        let path = self.resolve(specifier, referrer)?;
        let id = path.to_string_lossy().to_string();

        // Check if already loaded
        {
            let modules = self.modules.read().unwrap();
            if let Some(module) = modules.get(&id) {
                return Ok(module.clone());
            }
        }

        // Check for circular dependency
        {
            let loading = self.loading.read().unwrap();
            if loading.contains(&id) {
                return Err(ModuleError::CircularDependency(id));
            }
        }

        // Mark as loading
        {
            let mut loading = self.loading.write().unwrap();
            loading.push(id.clone());
        }

        // Read and parse the module
        let source = std::fs::read_to_string(&path)
            .map_err(|e| ModuleError::IoError(e.to_string()))?;

        let program = parse(&source)
            .map_err(|e| ModuleError::ParseError(e.to_string()))?;

        let module = Module::new(id.clone(), path, program);

        // Cache the module
        {
            let mut modules = self.modules.write().unwrap();
            modules.insert(id.clone(), module.clone());
        }

        // Remove from loading
        {
            let mut loading = self.loading.write().unwrap();
            loading.retain(|x| x != &id);
        }

        Ok(module)
    }

    /// Dynamic import() — loads a module at runtime and returns its namespace
    pub fn dynamic_import(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<DynamicImportResult> {
        let module = self.load(specifier, referrer)?;
        Ok(DynamicImportResult {
            namespace: module.namespace_object(),
            module_id: module.id.clone(),
        })
    }

    /// Get a cached module
    pub fn get(&self, id: &str) -> Option<Module> {
        let modules = self.modules.read().unwrap();
        modules.get(id).cloned()
    }

    /// Update a module's exports
    pub fn update_exports(&self, id: &str, exports: HashMap<String, Value>, default: Option<Value>) {
        let mut modules = self.modules.write().unwrap();
        if let Some(module) = modules.get_mut(id) {
            module.exports = exports;
            module.default_export = default;
            module.status = ModuleStatus::Evaluated;
        }
    }

    /// Get import bindings for a module
    pub fn get_import_bindings(
        &self,
        import: &ImportDeclaration,
        referrer: Option<&Path>,
    ) -> ModuleResult<HashMap<String, Value>> {
        let module = self.load(&import.source, referrer)?;
        let mut bindings = HashMap::default();

        for spec in &import.specifiers {
            match spec {
                ImportSpecifier::Default { local, .. } => {
                    let value = module.default_export.clone().unwrap_or(Value::Undefined);
                    bindings.insert(local.name.clone(), value);
                }
                ImportSpecifier::Named { local, imported, .. } => {
                    let value = module
                        .get_export(&imported.name)
                        .cloned()
                        .unwrap_or(Value::Undefined);
                    bindings.insert(local.name.clone(), value);
                }
                ImportSpecifier::Namespace { local, .. } => {
                    bindings.insert(local.name.clone(), module.namespace_object());
                }
            }
        }

        Ok(bindings)
    }

    /// Extract exports from module statements
    pub fn analyze_exports(&self, program: &Program) -> Vec<ExportInfo> {
        let mut exports = Vec::new();

        for stmt in &program.body {
            if let Statement::Export(export) = stmt {
                match &export.kind {
                    ExportKind::Named { specifiers, source } => {
                        for spec in specifiers {
                            if let ExportSpecifier::Named { local, exported, .. } = spec {
                                exports.push(ExportInfo::Named {
                                    local: local.name.clone(),
                                    exported: exported.name.clone(),
                                    source: source.clone(),
                                });
                            }
                        }
                    }
                    ExportKind::Default(_) | ExportKind::DefaultDeclaration(_) => {
                        exports.push(ExportInfo::Default);
                    }
                    ExportKind::Declaration(decl) => {
                        // Extract names from declaration
                        if let Statement::VariableDeclaration(var) = decl.as_ref() {
                            for declarator in &var.declarations {
                                if let crate::ast::Pattern::Identifier(id) = &declarator.id {
                                    exports.push(ExportInfo::Named {
                                        local: id.name.clone(),
                                        exported: id.name.clone(),
                                        source: None,
                                    });
                                }
                            }
                        } else if let Statement::FunctionDeclaration(func) = decl.as_ref() {
                            if let Some(id) = &func.id {
                                exports.push(ExportInfo::Named {
                                    local: id.name.clone(),
                                    exported: id.name.clone(),
                                    source: None,
                                });
                            }
                        } else if let Statement::ClassDeclaration(class) = decl.as_ref() {
                            if let Some(id) = &class.id {
                                exports.push(ExportInfo::Named {
                                    local: id.name.clone(),
                                    exported: id.name.clone(),
                                    source: None,
                                });
                            }
                        }
                    }
                    ExportKind::All { source } => {
                        exports.push(ExportInfo::All {
                            source: source.clone(),
                        });
                    }
                    ExportKind::AllAs { exported, source } => {
                        exports.push(ExportInfo::AllAs {
                            name: exported.name.clone(),
                            source: source.clone(),
                        });
                    }
                }
            }
        }

        exports
    }

    /// List all loaded modules with their status and export counts
    pub fn list_modules(&self) -> Vec<ModuleSummary> {
        let modules = self.modules.read().unwrap();
        modules.values().map(|m| ModuleSummary {
            id: m.id.clone(),
            path: m.path.clone(),
            status: m.status.clone(),
            export_count: m.exports.len(),
            has_default: m.default_export.is_some(),
            re_export_count: m.re_exports.len(),
        }).collect()
    }

    /// Build a dependency graph from loaded modules by analyzing import statements
    pub fn dependency_graph(&self) -> HashMap<String, Vec<String>> {
        let modules = self.modules.read().unwrap();
        let mut graph = HashMap::default();
        for module in modules.values() {
            let mut deps = Vec::new();
            for stmt in &module.program.body {
                if let Statement::Import(import) = stmt {
                    deps.push(import.source.clone());
                }
            }
            graph.insert(module.id.clone(), deps);
        }
        graph
    }

    /// Check if a module has been loaded
    pub fn is_loaded(&self, specifier: &str, referrer: Option<&Path>) -> bool {
        if let Ok(path) = self.resolve(specifier, referrer) {
            let id = path.to_string_lossy().to_string();
            let modules = self.modules.read().unwrap();
            modules.contains_key(&id)
        } else {
            false
        }
    }

    /// Invalidate (unload) a module from the cache
    pub fn invalidate(&self, id: &str) -> bool {
        let mut modules = self.modules.write().unwrap();
        modules.remove(id).is_some()
    }
}

/// Summary of a loaded module
#[derive(Debug, Clone)]
pub struct ModuleSummary {
    /// Module identifier
    pub id: String,
    /// File path
    pub path: PathBuf,
    /// Current loading status
    pub status: ModuleStatus,
    /// Number of named exports
    pub export_count: usize,
    /// Whether a default export exists
    pub has_default: bool,
    /// Number of re-exports
    pub re_export_count: usize,
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Export information
#[derive(Debug, Clone)]
pub enum ExportInfo {
    /// Named export
    Named {
        local: String,
        exported: String,
        source: Option<String>,
    },
    /// Default export
    Default,
    /// Re-export all
    All { source: String },
    /// Re-export all as namespace
    AllAs { name: String, source: String },
}

// ==================== HMR Integration ====================

use crate::hmr::{FileChange, HmrRuntime, HotContext, HotUpdate, ModuleGraph, ModuleId};
use std::sync::Mutex;
use std::time::Duration;

/// Module loader with Hot Module Reloading support
pub struct HmrModuleLoader {
    /// Base module loader
    loader: ModuleLoader,
    /// HMR runtime
    hmr: HmrRuntime,
    /// Update callbacks
    update_callbacks: Mutex<HashMap<String, Box<dyn Fn(&Module) + Send + Sync>>>,
}

impl std::fmt::Debug for HmrModuleLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HmrModuleLoader")
            .field("loader", &self.loader)
            .field("hmr", &self.hmr)
            .field(
                "update_callbacks",
                &format!("<{} callbacks>", self.update_callbacks.lock().unwrap().len()),
            )
            .finish()
    }
}

impl HmrModuleLoader {
    /// Create a new HMR-enabled module loader
    pub fn new() -> Self {
        Self {
            loader: ModuleLoader::new(),
            hmr: HmrRuntime::new(),
            update_callbacks: Mutex::new(HashMap::default()),
        }
    }

    /// Create with custom base directory and poll interval
    pub fn with_config(base_dir: PathBuf, poll_interval: Duration) -> Self {
        Self {
            loader: ModuleLoader::with_base_dir(base_dir),
            hmr: HmrRuntime::with_poll_interval(poll_interval),
            update_callbacks: Mutex::new(HashMap::default()),
        }
    }

    /// Load a module and register it for HMR
    pub fn load(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<Module> {
        let module = self.loader.load(specifier, referrer)?;

        // Register with HMR runtime
        let hmr_id = self.hmr.register_module(&module.path);

        // If referrer exists, add dependency relationship
        if let Some(ref_path) = referrer {
            let ref_id = ModuleId::from_path(ref_path);
            self.hmr.add_dependency(&ref_id, &hmr_id);
        }

        Ok(module)
    }

    /// Resolve a module specifier
    pub fn resolve(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<PathBuf> {
        self.loader.resolve(specifier, referrer)
    }

    /// Get a cached module
    pub fn get(&self, id: &str) -> Option<Module> {
        self.loader.get(id)
    }

    /// Get the hot context for a module
    pub fn get_hot_context(&self, path: &Path) -> Option<HotContext> {
        let id = ModuleId::from_path(path);
        self.hmr.get_hot_context(&id)
    }

    /// Check for file changes and return pending updates
    pub fn check_for_updates(&self) -> Vec<FileChange> {
        self.hmr.check_for_updates()
    }

    /// Apply all pending updates
    pub fn apply_pending_updates(&self) -> Vec<HmrUpdateResult> {
        let updates = self.hmr.pending_updates();
        let mut results = Vec::new();

        for update in updates {
            let result = self.apply_update(&update);
            results.push(result);
        }

        results
    }

    /// Apply a single update
    fn apply_update(&self, update: &HotUpdate) -> HmrUpdateResult {
        // Reload the module
        let path = PathBuf::from(&update.module_id.0);
        let module_id = path.to_string_lossy().to_string();

        // Try to reload the module
        match self.reload_module(&path, &update.new_source) {
            Ok(module) => {
                // Call update callbacks
                let callbacks = self.update_callbacks.lock().unwrap();
                if let Some(callback) = callbacks.get(&module_id) {
                    callback(&module);
                }

                // Apply HMR update
                let hmr_result = self.hmr.apply_update(update);

                HmrUpdateResult {
                    module_id,
                    success: hmr_result.success,
                    reloaded: true,
                    affected_modules: hmr_result
                        .affected_modules
                        .iter()
                        .map(|id| id.0.clone())
                        .collect(),
                    error: hmr_result.error,
                }
            }
            Err(e) => HmrUpdateResult {
                module_id,
                success: false,
                reloaded: false,
                affected_modules: vec![],
                error: Some(e.to_string()),
            },
        }
    }

    /// Reload a module from new source
    fn reload_module(&self, path: &Path, source: &str) -> ModuleResult<Module> {
        let program = parse(source).map_err(|e| ModuleError::ParseError(e.to_string()))?;

        let id = path.to_string_lossy().to_string();
        let module = Module::new(id.clone(), path.to_path_buf(), program);

        // Update the cache
        {
            let mut modules = self.loader.modules.write().unwrap();
            modules.insert(id, module.clone());
        }

        Ok(module)
    }

    /// Register an update callback for a module
    pub fn on_update<F>(&self, module_path: &str, callback: F)
    where
        F: Fn(&Module) + Send + Sync + 'static,
    {
        let mut callbacks = self.update_callbacks.lock().unwrap();
        callbacks.insert(module_path.to_string(), Box::new(callback));
    }

    /// Accept hot updates for a module
    pub fn accept(&self, path: &Path) {
        let id = ModuleId::from_path(path);
        let graph = self.hmr.graph();
        let mut graph = graph.lock().unwrap();
        if let Some(module) = graph.get_mut(&id) {
            module.hot.accept();
        }
    }

    /// Get the module graph
    pub fn graph(&self) -> Arc<Mutex<ModuleGraph>> {
        self.hmr.graph()
    }

    /// Get the underlying module loader
    pub fn loader(&self) -> &ModuleLoader {
        &self.loader
    }

    /// Update exports for a module
    pub fn update_exports(
        &self,
        id: &str,
        exports: HashMap<String, Value>,
        default: Option<Value>,
    ) {
        self.loader.update_exports(id, exports, default);
    }

    /// Get import bindings
    pub fn get_import_bindings(
        &self,
        import: &ImportDeclaration,
        referrer: Option<&Path>,
    ) -> ModuleResult<HashMap<String, Value>> {
        self.loader.get_import_bindings(import, referrer)
    }

    /// Analyze exports
    pub fn analyze_exports(&self, program: &Program) -> Vec<ExportInfo> {
        self.loader.analyze_exports(program)
    }

    /// Invalidate a module (force full reload)
    pub fn invalidate(&self, path: &Path) {
        let id = ModuleId::from_path(path);
        self.hmr.invalidate(&id);
    }
}

impl Default for HmrModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of applying an HMR update
#[derive(Debug)]
pub struct HmrUpdateResult {
    /// Module that was updated
    pub module_id: String,
    /// Whether the update was successful
    pub success: bool,
    /// Whether the module was reloaded
    pub reloaded: bool,
    /// Other modules affected by this update
    pub affected_modules: Vec<String>,
    /// Error message if update failed
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_relative() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.js");
        fs::write(&file_path, "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./test.js", None).unwrap();
        assert!(resolved.exists());
    }

    #[test]
    fn test_resolve_with_extension() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.js");
        fs::write(&file_path, "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        // Should add .js extension automatically
        let resolved = loader.resolve("./test", None).unwrap();
        assert!(resolved.exists());
    }

    #[test]
    fn test_load_module() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("math.js");
        fs::write(&file_path, "export const PI = 3.14159;\nexport function square(x) { return x * x; }").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let module = loader.load("./math.js", None).unwrap();

        assert_eq!(module.status, ModuleStatus::Fetching);
        assert!(!module.program.body.is_empty());
    }

    #[test]
    fn test_module_caching() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("cached.js");
        fs::write(&file_path, "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());

        // Load twice
        let module1 = loader.load("./cached.js", None).unwrap();
        let module2 = loader.load("./cached.js", None).unwrap();

        // Should be the same module ID
        assert_eq!(module1.id, module2.id);
    }

    #[test]
    fn test_not_found() {
        let loader = ModuleLoader::new();
        let result = loader.resolve("./nonexistent.js", None);
        assert!(matches!(result, Err(ModuleError::NotFound(_))));
    }

    #[test]
    fn test_analyze_exports() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("exports.js");
        fs::write(
            &file_path,
            r#"
            export const PI = 3.14;
            export function add(a, b) { return a + b; }
            export default class Calculator {}
            "#,
        )
        .unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let module = loader.load("./exports.js", None).unwrap();
        let exports = loader.analyze_exports(&module.program);

        assert!(exports.iter().any(|e| matches!(e, ExportInfo::Named { exported, .. } if exported == "PI")));
        assert!(exports.iter().any(|e| matches!(e, ExportInfo::Named { exported, .. } if exported == "add")));
        assert!(exports.iter().any(|e| matches!(e, ExportInfo::Default)));
    }

    #[test]
    fn test_hmr_module_loader() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("counter.js");
        fs::write(&file_path, "export let count = 0;").unwrap();

        let loader = HmrModuleLoader::with_config(
            dir.path().to_path_buf(),
            Duration::from_millis(100),
        );

        // Load the module
        let module = loader.load("./counter.js", None).unwrap();
        assert!(!module.program.body.is_empty());

        // Accept updates for this module
        loader.accept(&file_path);

        // Module should be registered in HMR graph
        let graph = loader.graph();
        let graph = graph.lock().unwrap();
        assert!(!graph.is_empty());
    }

    #[test]
    fn test_hmr_update_callback() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("app.js");
        fs::write(&file_path, "export const version = 1;").unwrap();

        let loader = HmrModuleLoader::with_config(
            dir.path().to_path_buf(),
            Duration::from_millis(50),
        );

        // Load the module
        let _module = loader.load("./app.js", None).unwrap();
        let resolved_path = loader.resolve("./app.js", None).unwrap();

        // Register callback
        use std::sync::atomic::{AtomicBool, Ordering};
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        loader.on_update(&resolved_path.to_string_lossy(), move |_module| {
            called_clone.store(true, Ordering::SeqCst);
        });

        // Accept updates
        loader.accept(&resolved_path);

        // Initially should not be called
        assert!(!called.load(Ordering::SeqCst));
    }

    // ==================== Extended Resolution Tests ====================

    #[test]
    fn test_resolve_mjs_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.mjs"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./lib", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("lib.mjs"));
    }

    #[test]
    fn test_resolve_ts_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.ts"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./lib", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("lib.ts"));
    }

    #[test]
    fn test_resolve_json_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./config", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("config.json"));
    }

    #[test]
    fn test_resolve_js_preferred_over_mjs() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.js"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("lib.mjs"), "export const x = 2;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./lib", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("lib.js"));
    }

    #[test]
    fn test_resolve_index_mjs() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("mylib");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("index.mjs"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./mylib", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("index.mjs"));
    }

    #[test]
    fn test_resolve_index_ts() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("mylib");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("index.ts"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("./mylib", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("index.ts"));
    }

    #[test]
    fn test_resolve_bare_specifier_with_main() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("my-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("package.json"), r#"{"name":"my-pkg","main":"lib/entry.js"}"#).unwrap();
        let lib = nm.join("lib");
        fs::create_dir(&lib).unwrap();
        fs::write(lib.join("entry.js"), "module.exports = {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("my-pkg", None).unwrap();
        assert!(resolved.to_string_lossy().contains("entry.js"));
    }

    #[test]
    fn test_resolve_bare_specifier_with_module_field() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("esm-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("package.json"), r#"{"name":"esm-pkg","main":"index.cjs","module":"index.mjs"}"#).unwrap();
        fs::write(nm.join("index.cjs"), "module.exports = {};").unwrap();
        fs::write(nm.join("index.mjs"), "export default {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("esm-pkg", None).unwrap();
        // Should prefer "module" over "main"
        assert!(resolved.to_string_lossy().ends_with("index.mjs"));
    }

    #[test]
    fn test_resolve_bare_specifier_index_js() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("simple-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "module.exports = {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("simple-pkg", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("index.js"));
    }

    #[test]
    fn test_resolve_bare_specifier_file_with_ext() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("single-file.js"), "module.exports = {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("single-file", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("single-file.js"));
    }

    #[test]
    fn test_resolve_bare_specifier_walks_up() {
        let dir = tempdir().unwrap();
        // Create node_modules at root
        let nm = dir.path().join("node_modules").join("root-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "module.exports = {};").unwrap();

        // Create a nested directory as the referrer
        let nested = dir.path().join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let referrer = nested.join("app.js");
        fs::write(&referrer, "").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("root-pkg", Some(&referrer)).unwrap();
        assert!(resolved.to_string_lossy().contains("root-pkg"));
    }

    #[test]
    fn test_resolve_bare_specifier_exports_path() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("exports-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("package.json"), r#"{"name":"exports-pkg","exports":"./dist/index.js"}"#).unwrap();
        let dist = nm.join("dist");
        fs::create_dir(&dist).unwrap();
        fs::write(dist.join("index.js"), "export default {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("exports-pkg", None).unwrap();
        assert!(resolved.to_string_lossy().contains("dist"));
    }

    #[test]
    fn test_resolve_bare_specifier_exports_conditional() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("cond-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("package.json"), r#"{"name":"cond-pkg","exports":{"import":"./esm/index.js","default":"./cjs/index.js"}}"#).unwrap();
        let esm = nm.join("esm");
        fs::create_dir(&esm).unwrap();
        fs::write(esm.join("index.js"), "export default {};").unwrap();
        let cjs = nm.join("cjs");
        fs::create_dir(&cjs).unwrap();
        fs::write(cjs.join("index.js"), "module.exports = {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("cond-pkg", None).unwrap();
        // Should prefer "import" condition
        assert!(resolved.to_string_lossy().contains("esm"));
    }

    #[test]
    fn test_resolve_bare_specifier_exports_default_fallback() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("def-pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("package.json"), r#"{"name":"def-pkg","exports":{"default":"./lib.js"}}"#).unwrap();
        fs::write(nm.join("lib.js"), "export default {};").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let resolved = loader.resolve("def-pkg", None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("lib.js"));
    }

    #[test]
    fn test_resolve_bare_specifier_not_found() {
        let dir = tempdir().unwrap();
        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let result = loader.resolve("nonexistent-pkg", None);
        assert!(matches!(result, Err(ModuleError::ResolutionFailed(_))));
    }

    #[test]
    fn test_resolve_absolute_with_extension_resolution() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("abs.ts"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::new();
        let full_path = dir.path().join("abs");
        let resolved = loader.resolve(&full_path.to_string_lossy(), None).unwrap();
        assert!(resolved.to_string_lossy().ends_with("abs.ts"));
    }

    // ==================== Import Map Tests ====================

    #[test]
    fn test_import_map_basic() {
        let map = ImportMap::from_json(r#"{
            "imports": {
                "lodash": "./node_modules/lodash-es/lodash.js",
                "react": "./vendor/react.js"
            }
        }"#).unwrap();

        assert_eq!(map.resolve("lodash", None), Some("./node_modules/lodash-es/lodash.js".to_string()));
        assert_eq!(map.resolve("react", None), Some("./vendor/react.js".to_string()));
        assert_eq!(map.resolve("unknown", None), None);
    }

    #[test]
    fn test_import_map_prefix() {
        let map = ImportMap::from_json(r#"{
            "imports": {
                "lodash/": "./node_modules/lodash-es/"
            }
        }"#).unwrap();

        assert_eq!(
            map.resolve("lodash/clone", None),
            Some("./node_modules/lodash-es/clone".to_string())
        );
    }

    #[test]
    fn test_import_map_scoped() {
        let map = ImportMap::from_json(r#"{
            "imports": { "lodash": "./vendor/lodash.js" },
            "scopes": {
                "/src/special/": { "lodash": "./vendor/lodash-custom.js" }
            }
        }"#).unwrap();

        // Without scope, uses top-level
        assert_eq!(map.resolve("lodash", None), Some("./vendor/lodash.js".to_string()));

        // With matching scope, uses scoped mapping
        let referrer = Path::new("/src/special/app.js");
        assert_eq!(
            map.resolve("lodash", Some(referrer)),
            Some("./vendor/lodash-custom.js".to_string())
        );
    }

    #[test]
    fn test_import_map_integration_with_loader() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("vendor-lib.js"), "export const x = 42;").unwrap();

        let mut loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let mut imports = HashMap::default();
        imports.insert("my-lib".to_string(), "./vendor-lib.js".to_string());
        loader.set_import_map(ImportMap { imports, scopes: HashMap::default() });

        // Should resolve bare specifier via import map
        let resolved = loader.resolve("my-lib", None).unwrap();
        assert!(resolved.to_string_lossy().contains("vendor-lib.js"));
    }

    // ==================== Dynamic Import Tests ====================

    #[test]
    fn test_dynamic_import() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("dynamic.js"), "export const val = 99;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let result = loader.dynamic_import("./dynamic.js", None).unwrap();
        assert!(!result.module_id.is_empty());
    }

    #[test]
    fn test_dynamic_import_not_found() {
        let dir = tempdir().unwrap();
        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let result = loader.dynamic_import("./missing.js", None);
        assert!(result.is_err());
    }

    // ==================== import.meta Tests ====================

    #[test]
    fn test_import_meta_from_path() {
        let path = Path::new("/src/app.js");
        let meta = ImportMeta::from_path(path, true);
        assert_eq!(meta.url, "file:///src/app.js");
        assert_eq!(meta.dirname, "/src");
        assert_eq!(meta.filename, "/src/app.js");
        assert!(meta.main);
    }

    #[test]
    fn test_import_meta_non_main() {
        let path = Path::new("/src/utils.js");
        let meta = ImportMeta::from_path(path, false);
        assert!(!meta.main);
    }

    #[test]
    fn test_import_meta_to_js_value() {
        let path = Path::new("/src/app.js");
        let meta = ImportMeta::from_path(path, true);
        let value = meta.to_js_value();
        assert!(matches!(value, Value::Object(_)));
    }

    #[test]
    fn test_module_registry() {
        let mut registry = ModuleRegistry::new();
        let entry = PathBuf::from("/src/main.js");
        let dep = PathBuf::from("/src/utils.js");

        registry.set_entry(entry.clone());
        registry.record_load(entry.clone());
        registry.record_load(dep.clone());

        assert!(registry.is_entry(&entry));
        assert!(!registry.is_entry(&dep));
        assert_eq!(registry.load_order.len(), 2);

        let meta = registry.import_meta_for(&entry);
        assert!(meta.main);
        let meta = registry.import_meta_for(&dep);
        assert!(!meta.main);
    }

    #[test]
    fn test_list_modules_empty() {
        let loader = ModuleLoader::new();
        let modules = loader.list_modules();
        assert!(modules.is_empty());
    }

    #[test]
    fn test_list_modules_after_load() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.js"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("b.js"), "export const y = 2;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        loader.load("./a.js", None).unwrap();
        loader.load("./b.js", None).unwrap();

        let modules = loader.list_modules();
        assert_eq!(modules.len(), 2);
    }

    #[test]
    fn test_is_loaded() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("check.js"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        assert!(!loader.is_loaded("./check.js", None));

        loader.load("./check.js", None).unwrap();
        assert!(loader.is_loaded("./check.js", None));
    }

    #[test]
    fn test_invalidate_module() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("inv.js"), "export const x = 1;").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        let module = loader.load("./inv.js", None).unwrap();

        assert!(loader.invalidate(&module.id));
        assert!(!loader.is_loaded("./inv.js", None));
    }

    #[test]
    fn test_dependency_graph() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("utils.js"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("main.js"), "import { x } from './utils.js';\nconsole.log(x);").unwrap();

        let loader = ModuleLoader::with_base_dir(dir.path().to_path_buf());
        loader.load("./main.js", None).unwrap();

        let graph = loader.dependency_graph();
        assert!(!graph.is_empty());
    }
}

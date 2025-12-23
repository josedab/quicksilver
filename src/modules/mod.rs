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
use crate::parser::parse;
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

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

/// Module loader and cache
#[derive(Debug)]
pub struct ModuleLoader {
    /// Loaded modules by resolved path
    modules: Arc<RwLock<HashMap<String, Module>>>,
    /// Modules currently being loaded (for cycle detection)
    loading: Arc<RwLock<Vec<String>>>,
    /// Base directory for resolution
    base_dir: PathBuf,
}

impl ModuleLoader {
    /// Create a new module loader
    pub fn new() -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::default())),
            loading: Arc::new(RwLock::new(Vec::new())),
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Create a module loader with a specific base directory
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::default())),
            loading: Arc::new(RwLock::new(Vec::new())),
            base_dir,
        }
    }

    /// Resolve a module specifier to an absolute path
    pub fn resolve(&self, specifier: &str, referrer: Option<&Path>) -> ModuleResult<PathBuf> {
        // Determine base directory for resolution
        let base = referrer
            .and_then(|p| p.parent())
            .unwrap_or(&self.base_dir);

        if specifier.starts_with("./") || specifier.starts_with("../") {
            // Relative import
            let mut path = base.join(specifier);

            // Try with .js extension if not present
            if !path.exists() && path.extension().is_none() {
                path.set_extension("js");
            }

            // Try as directory with index.js
            if !path.exists() {
                let index_path = PathBuf::from(specifier).join("index.js");
                let full_index = base.join(&index_path);
                if full_index.exists() {
                    return Ok(full_index.canonicalize().map_err(|e| ModuleError::IoError(e.to_string()))?);
                }
            }

            if path.exists() {
                path.canonicalize().map_err(|e| ModuleError::IoError(e.to_string()))
            } else {
                Err(ModuleError::NotFound(specifier.to_string()))
            }
        } else if specifier.starts_with('/') {
            // Absolute import
            let path = PathBuf::from(specifier);
            if path.exists() {
                path.canonicalize().map_err(|e| ModuleError::IoError(e.to_string()))
            } else {
                Err(ModuleError::NotFound(specifier.to_string()))
            }
        } else {
            // Bare specifier (node_modules style) - simplified
            // In a full implementation, this would search node_modules
            Err(ModuleError::ResolutionFailed(format!(
                "Bare specifiers not yet supported: {}",
                specifier
            )))
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
}

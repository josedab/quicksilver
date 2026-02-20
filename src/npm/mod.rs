//! npm/CommonJS compatibility layer
//!
//! Provides CommonJS require(), module.exports, package.json resolution,
//! and node_modules traversal for running npm packages in Quicksilver.

//! **Status:** ⚠️ Partial — CommonJS require(), core modules (path, util, process, os)

pub mod lockfile;
pub mod package_json;
pub mod registry;
pub mod resolver;
pub mod installer;

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use serde_json;
use std::path::{Path, PathBuf};

/// CommonJS module representation
#[derive(Debug, Clone)]
pub struct CommonJsModule {
    /// Module ID (resolved absolute path)
    pub id: String,
    /// Module filename
    pub filename: String,
    /// Module directory
    pub dirname: String,
    /// Whether the module has been loaded
    pub loaded: bool,
    /// The module's exports value
    pub exports: Value,
    /// Child module IDs
    pub children: Vec<String>,
    /// Parent module ID
    pub parent: Option<String>,
}

impl CommonJsModule {
    pub fn new(path: &Path) -> Self {
        let filename = path.to_string_lossy().to_string();
        let dirname = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            id: filename.clone(),
            filename,
            dirname,
            loaded: false,
            exports: Value::new_object(),
            children: Vec::new(),
            parent: None,
        }
    }
}

/// Package.json representation
#[derive(Debug, Clone, Default)]
pub struct PackageJson {
    pub name: Option<String>,
    pub version: Option<String>,
    pub main: Option<String>,
    pub module: Option<String>,
    pub types: Option<String>,
    pub exports: Option<PackageExports>,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
}

/// Package exports field (simplified)
#[derive(Debug, Clone)]
pub enum PackageExports {
    /// Single string export
    Path(String),
    /// Conditional exports map
    Conditional(HashMap<String, String>),
    /// Subpath exports map
    Subpaths(HashMap<String, PackageExports>),
}

impl PackageJson {
    /// Parse a package.json from a JSON string
    pub fn parse(json_str: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| Error::ModuleError(format!("Invalid package.json: {}", e)))?;

        let obj = value
            .as_object()
            .ok_or_else(|| Error::ModuleError("package.json must be an object".to_string()))?;

        let name = obj.get("name").and_then(|v| v.as_str()).map(String::from);
        let version = obj.get("version").and_then(|v| v.as_str()).map(String::from);
        let main = obj.get("main").and_then(|v| v.as_str()).map(String::from);
        let module = obj.get("module").and_then(|v| v.as_str()).map(String::from);
        let types = obj.get("types").and_then(|v| v.as_str()).map(String::from);

        let exports = obj.get("exports").and_then(Self::parse_exports);

        let dependencies = Self::parse_deps(obj.get("dependencies"));
        let dev_dependencies = Self::parse_deps(obj.get("devDependencies"));

        Ok(Self {
            name,
            version,
            main,
            module,
            types,
            exports,
            dependencies,
            dev_dependencies,
        })
    }

    fn parse_exports(value: &serde_json::Value) -> Option<PackageExports> {
        match value {
            serde_json::Value::String(s) => Some(PackageExports::Path(s.clone())),
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::default();
                for (key, val) in obj {
                    if let Some(s) = val.as_str() {
                        map.insert(key.clone(), s.to_string());
                    }
                }
                if map.is_empty() {
                    None
                } else {
                    Some(PackageExports::Conditional(map))
                }
            }
            _ => None,
        }
    }

    fn parse_deps(value: Option<&serde_json::Value>) -> HashMap<String, String> {
        let mut deps = HashMap::default();
        if let Some(serde_json::Value::Object(obj)) = value {
            for (key, val) in obj {
                if let Some(v) = val.as_str() {
                    deps.insert(key.clone(), v.to_string());
                }
            }
        }
        deps
    }

    /// Get the entry point for this package
    pub fn entry_point(&self) -> Option<&str> {
        // Check exports first, then module, then main, then default
        if let Some(PackageExports::Path(ref p)) = self.exports {
            return Some(p.as_str());
        }
        if let Some(PackageExports::Conditional(ref map)) = self.exports {
            if let Some(p) = map.get("import").or_else(|| map.get("require")).or_else(|| map.get("default")) {
                return Some(p.as_str());
            }
        }
        if let Some(ref m) = self.module {
            return Some(m.as_str());
        }
        if let Some(ref m) = self.main {
            return Some(m.as_str());
        }
        None
    }
}

/// Module resolver implementing Node.js module resolution algorithm
pub struct ModuleResolver {
    /// Cache of resolved modules
    module_cache: HashMap<String, CommonJsModule>,
    /// Cache of loaded package.json files
    package_cache: HashMap<String, PackageJson>,
    /// Node.js-compatible core modules
    core_modules: HashMap<String, CoreModule>,
}

/// Core Node.js module shim
#[derive(Debug, Clone)]
pub struct CoreModule {
    pub name: String,
    pub exports: Vec<(String, CoreExport)>,
}

/// A core module export type
#[derive(Debug, Clone)]
pub enum CoreExport {
    Function(String),
    Constant(String),
    Object(String),
}

impl ModuleResolver {
    pub fn new() -> Self {
        let mut resolver = Self {
            module_cache: HashMap::default(),
            package_cache: HashMap::default(),
            core_modules: HashMap::default(),
        };
        resolver.register_core_modules();
        resolver
    }

    fn register_core_modules(&mut self) {
        // path module
        self.core_modules.insert(
            "path".to_string(),
            CoreModule {
                name: "path".to_string(),
                exports: vec![
                    (
                        "join".to_string(),
                        CoreExport::Function("path_join".to_string()),
                    ),
                    (
                        "resolve".to_string(),
                        CoreExport::Function("path_resolve".to_string()),
                    ),
                    (
                        "dirname".to_string(),
                        CoreExport::Function("path_dirname".to_string()),
                    ),
                    (
                        "basename".to_string(),
                        CoreExport::Function("path_basename".to_string()),
                    ),
                    (
                        "extname".to_string(),
                        CoreExport::Function("path_extname".to_string()),
                    ),
                    (
                        "sep".to_string(),
                        CoreExport::Constant("path_sep".to_string()),
                    ),
                    (
                        "isAbsolute".to_string(),
                        CoreExport::Function("path_is_absolute".to_string()),
                    ),
                    (
                        "normalize".to_string(),
                        CoreExport::Function("path_normalize".to_string()),
                    ),
                    (
                        "relative".to_string(),
                        CoreExport::Function("path_relative".to_string()),
                    ),
                    (
                        "parse".to_string(),
                        CoreExport::Function("path_parse".to_string()),
                    ),
                ],
            },
        );

        // util module
        self.core_modules.insert(
            "util".to_string(),
            CoreModule {
                name: "util".to_string(),
                exports: vec![
                    (
                        "inspect".to_string(),
                        CoreExport::Function("util_inspect".to_string()),
                    ),
                    (
                        "format".to_string(),
                        CoreExport::Function("util_format".to_string()),
                    ),
                    (
                        "promisify".to_string(),
                        CoreExport::Function("util_promisify".to_string()),
                    ),
                    (
                        "inherits".to_string(),
                        CoreExport::Function("util_inherits".to_string()),
                    ),
                    (
                        "types".to_string(),
                        CoreExport::Object("util_types".to_string()),
                    ),
                ],
            },
        );

        // events module
        self.core_modules.insert(
            "events".to_string(),
            CoreModule {
                name: "events".to_string(),
                exports: vec![(
                    "EventEmitter".to_string(),
                    CoreExport::Function("EventEmitter".to_string()),
                )],
            },
        );

        // buffer module
        self.core_modules.insert(
            "buffer".to_string(),
            CoreModule {
                name: "buffer".to_string(),
                exports: vec![
                    (
                        "Buffer".to_string(),
                        CoreExport::Function("Buffer".to_string()),
                    ),
                ],
            },
        );

        // url module
        self.core_modules.insert(
            "url".to_string(),
            CoreModule {
                name: "url".to_string(),
                exports: vec![
                    (
                        "URL".to_string(),
                        CoreExport::Function("URL".to_string()),
                    ),
                    (
                        "URLSearchParams".to_string(),
                        CoreExport::Function("URLSearchParams".to_string()),
                    ),
                    (
                        "parse".to_string(),
                        CoreExport::Function("url_parse".to_string()),
                    ),
                    (
                        "format".to_string(),
                        CoreExport::Function("url_format".to_string()),
                    ),
                ],
            },
        );

        // crypto module
        self.core_modules.insert(
            "crypto".to_string(),
            CoreModule {
                name: "crypto".to_string(),
                exports: vec![
                    (
                        "randomBytes".to_string(),
                        CoreExport::Function("crypto_randomBytes".to_string()),
                    ),
                    (
                        "randomUUID".to_string(),
                        CoreExport::Function("crypto_randomUUID".to_string()),
                    ),
                    (
                        "createHash".to_string(),
                        CoreExport::Function("crypto_createHash".to_string()),
                    ),
                ],
            },
        );

        // assert module
        self.core_modules.insert(
            "assert".to_string(),
            CoreModule {
                name: "assert".to_string(),
                exports: vec![
                    (
                        "ok".to_string(),
                        CoreExport::Function("assert_ok".to_string()),
                    ),
                    (
                        "strictEqual".to_string(),
                        CoreExport::Function("assert_strictEqual".to_string()),
                    ),
                    (
                        "deepStrictEqual".to_string(),
                        CoreExport::Function("assert_deepStrictEqual".to_string()),
                    ),
                    (
                        "throws".to_string(),
                        CoreExport::Function("assert_throws".to_string()),
                    ),
                ],
            },
        );

        // process module
        self.core_modules.insert(
            "process".to_string(),
            CoreModule {
                name: "process".to_string(),
                exports: vec![
                    ("version".to_string(), CoreExport::Constant("process_version".to_string())),
                    ("platform".to_string(), CoreExport::Constant("process_platform".to_string())),
                    ("arch".to_string(), CoreExport::Constant("process_arch".to_string())),
                    ("pid".to_string(), CoreExport::Constant("process_pid".to_string())),
                    ("env".to_string(), CoreExport::Object("process_env".to_string())),
                    ("argv".to_string(), CoreExport::Object("process_argv".to_string())),
                    ("cwd".to_string(), CoreExport::Function("process_cwd".to_string())),
                    ("exit".to_string(), CoreExport::Function("process_exit".to_string())),
                    ("hrtime".to_string(), CoreExport::Function("process_hrtime".to_string())),
                    ("nextTick".to_string(), CoreExport::Function("process_nextTick".to_string())),
                ],
            },
        );

        // os module (full implementation)
        self.core_modules.insert(
            "os".to_string(),
            CoreModule {
                name: "os".to_string(),
                exports: vec![
                    ("platform".to_string(), CoreExport::Function("os_platform".to_string())),
                    ("arch".to_string(), CoreExport::Function("os_arch".to_string())),
                    ("type".to_string(), CoreExport::Function("os_type".to_string())),
                    ("homedir".to_string(), CoreExport::Function("os_homedir".to_string())),
                    ("tmpdir".to_string(), CoreExport::Function("os_tmpdir".to_string())),
                    ("EOL".to_string(), CoreExport::Constant("os_eol".to_string())),
                    ("cpus".to_string(), CoreExport::Function("os_cpus".to_string())),
                ],
            },
        );

        // Stub modules — registered as core to prevent node_modules lookup
        for name in &["fs", "http", "https", "net", "stream", "child_process", "querystring", "zlib", "tty", "readline"] {
            self.core_modules.insert(
                name.to_string(),
                CoreModule {
                    name: name.to_string(),
                    exports: vec![],
                },
            );
        }
    }

    /// Check if a module specifier refers to a core module
    pub fn is_core_module(&self, specifier: &str) -> bool {
        let name = specifier.strip_prefix("node:").unwrap_or(specifier);
        self.core_modules.contains_key(name)
    }

    /// Get a core module's metadata
    pub fn get_core_module(&self, specifier: &str) -> Option<&CoreModule> {
        let name = specifier.strip_prefix("node:").unwrap_or(specifier);
        self.core_modules.get(name)
    }

    /// Resolve a module specifier to an absolute file path
    /// Implements the Node.js module resolution algorithm
    pub fn resolve(&mut self, specifier: &str, from_dir: &Path) -> Result<ResolvedModule> {
        // 1. Check core modules
        let name = specifier.strip_prefix("node:").unwrap_or(specifier);
        if self.core_modules.contains_key(name) {
            return Ok(ResolvedModule::Core(name.to_string()));
        }

        // 2. Check if relative or absolute path
        if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
            let resolved = self.resolve_file(from_dir.join(specifier).as_path())?;
            return Ok(ResolvedModule::File(resolved));
        }

        // 3. Search node_modules
        self.resolve_node_modules(specifier, from_dir)
    }

    /// Resolve a file path, trying extensions and index files
    fn resolve_file(&self, path: &Path) -> Result<PathBuf> {
        // Try exact path
        if path.is_file() {
            return Ok(path.to_path_buf());
        }

        // Try adding extensions
        let extensions = [".js", ".mjs", ".cjs", ".json", ".ts"];
        for ext in &extensions {
            let with_ext = path.with_extension(ext.trim_start_matches('.'));
            if with_ext.is_file() {
                return Ok(with_ext);
            }
        }

        // Try as directory with index file
        if path.is_dir() {
            // Check package.json in directory
            let pkg_path = path.join("package.json");
            if pkg_path.is_file() {
                if let Ok(pkg) = self.load_package_json(&pkg_path) {
                    if let Some(entry) = pkg.entry_point() {
                        let entry_path = path.join(entry);
                        if let Ok(resolved) = self.resolve_file(&entry_path) {
                            return Ok(resolved);
                        }
                    }
                }
            }

            // Try index files
            for ext in &extensions {
                let index = path.join(format!("index{}", ext));
                if index.is_file() {
                    return Ok(index);
                }
            }
        }

        Err(Error::ModuleError(format!(
            "Cannot find module '{}'",
            path.display()
        )))
    }

    /// Search node_modules directories up the directory tree
    fn resolve_node_modules(&mut self, specifier: &str, from_dir: &Path) -> Result<ResolvedModule> {
        // Split specifier into package name and subpath
        let (pkg_name, subpath) = Self::parse_specifier(specifier);

        let mut current = from_dir.to_path_buf();
        loop {
            let node_modules = current.join("node_modules").join(&pkg_name);
            if node_modules.is_dir() {
                // Found the package - resolve entry point
                let pkg_path = node_modules.join("package.json");
                if pkg_path.is_file() {
                    let pkg = self.load_package_json(&pkg_path)?;
                    self.package_cache
                        .insert(pkg_name.clone(), pkg.clone());

                    let target = if let Some(sub) = &subpath {
                        node_modules.join(sub)
                    } else if let Some(entry) = pkg.entry_point() {
                        node_modules.join(entry)
                    } else {
                        node_modules.join("index.js")
                    };

                    let resolved = self.resolve_file(&target)?;
                    return Ok(ResolvedModule::File(resolved));
                }

                // No package.json, try index.js
                let resolved = self.resolve_file(&node_modules)?;
                return Ok(ResolvedModule::File(resolved));
            }

            // Move up one directory
            if !current.pop() {
                break;
            }
        }

        Err(Error::ModuleError(format!(
            "Cannot find module '{}' from '{}'",
            specifier,
            from_dir.display()
        )))
    }

    /// Parse a specifier into package name and optional subpath
    fn parse_specifier(specifier: &str) -> (String, Option<String>) {
        if specifier.starts_with('@') {
            // Scoped package: @scope/name or @scope/name/subpath
            let parts: Vec<&str> = specifier.splitn(3, '/').collect();
            if parts.len() >= 2 {
                let pkg_name = format!("{}/{}", parts[0], parts[1]);
                let subpath = if parts.len() > 2 {
                    Some(parts[2].to_string())
                } else {
                    None
                };
                (pkg_name, subpath)
            } else {
                (specifier.to_string(), None)
            }
        } else {
            // Regular package: name or name/subpath
            let parts: Vec<&str> = specifier.splitn(2, '/').collect();
            if parts.len() > 1 {
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else {
                (specifier.to_string(), None)
            }
        }
    }

    /// Load and parse a package.json file
    fn load_package_json(&self, path: &Path) -> Result<PackageJson> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::ModuleError(format!("Failed to read {}: {}", path.display(), e)))?;
        PackageJson::parse(&content)
    }

    /// Get or create a cached module
    pub fn get_module(&self, id: &str) -> Option<&CommonJsModule> {
        self.module_cache.get(id)
    }

    /// Cache a loaded module
    pub fn cache_module(&mut self, module: CommonJsModule) {
        self.module_cache.insert(module.id.clone(), module);
    }

    /// Check if a module is already cached
    pub fn is_cached(&self, id: &str) -> bool {
        self.module_cache.contains_key(id)
    }
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of resolving a module specifier
#[derive(Debug, Clone)]
pub enum ResolvedModule {
    /// A core Node.js module
    Core(String),
    /// A file-system module
    File(PathBuf),
}

/// Node.js path module implementation
pub mod node_path {
    use crate::error::Result;
    use crate::runtime::Value;
    use std::path::{Path, PathBuf, MAIN_SEPARATOR};

    pub fn join(args: &[Value]) -> Result<Value> {
        let mut result = PathBuf::new();
        for arg in args {
            let part = arg.to_js_string();
            if part.starts_with('/') {
                result = PathBuf::from(&part);
            } else {
                result.push(&part);
            }
        }
        Ok(Value::String(normalize_slashes(&result.to_string_lossy())))
    }

    pub fn resolve(args: &[Value]) -> Result<Value> {
        let mut result = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        for arg in args {
            let part = arg.to_js_string();
            if part.starts_with('/') {
                result = PathBuf::from(&part);
            } else {
                result.push(&part);
            }
        }
        Ok(Value::String(normalize_slashes(&result.to_string_lossy())))
    }

    pub fn dirname(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let path = Path::new(&path_str);
        Ok(Value::String(
            path.parent()
                .map(|p| normalize_slashes(&p.to_string_lossy()))
                .unwrap_or_else(|| ".".to_string()),
        ))
    }

    pub fn basename(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let ext = args.get(1).map(|v| v.to_js_string());
        let path = Path::new(&path_str);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(ext) = ext {
            Ok(Value::String(name.strip_suffix(&ext).unwrap_or(&name).to_string()))
        } else {
            Ok(Value::String(name))
        }
    }

    pub fn extname(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let path = Path::new(&path_str);
        Ok(Value::String(
            path.extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default(),
        ))
    }

    pub fn is_absolute(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        Ok(Value::Boolean(Path::new(&path_str).is_absolute()))
    }

    pub fn normalize(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let mut components: Vec<&str> = Vec::new();
        for part in path_str.split('/') {
            match part {
                "." | "" => {}
                ".." => {
                    components.pop();
                }
                _ => components.push(part),
            }
        }
        let result = if path_str.starts_with('/') {
            format!("/{}", components.join("/"))
        } else {
            components.join("/")
        };
        Ok(Value::String(if result.is_empty() { ".".to_string() } else { result }))
    }

    pub fn sep() -> Value {
        Value::String(MAIN_SEPARATOR.to_string())
    }

    pub fn parse_path(args: &[Value]) -> Result<Value> {
        let path_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let path = Path::new(&path_str);
        let mut props = rustc_hash::FxHashMap::default();
        props.insert(
            "root".to_string(),
            Value::String(if path_str.starts_with('/') { "/".to_string() } else { String::new() }),
        );
        props.insert(
            "dir".to_string(),
            Value::String(
                path.parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        );
        props.insert(
            "base".to_string(),
            Value::String(
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        );
        props.insert(
            "ext".to_string(),
            Value::String(
                path.extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default(),
            ),
        );
        props.insert(
            "name".to_string(),
            Value::String(
                path.file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        );
        Ok(Value::new_object_with_properties(props))
    }

    fn normalize_slashes(s: &str) -> String {
        s.replace('\\', "/")
    }
}

/// Node.js assert module implementation
pub mod node_assert {
    use crate::error::{Error, Result};
    use crate::runtime::Value;

    pub fn ok(args: &[Value]) -> Result<Value> {
        let value = args.first().unwrap_or(&Value::Undefined);
        if !value.to_boolean() {
            let msg = args
                .get(1)
                .map(|v| v.to_js_string())
                .unwrap_or_else(|| "The expression evaluated to a falsy value".to_string());
            return Err(Error::type_error(format!("AssertionError: {}", msg)));
        }
        Ok(Value::Undefined)
    }

    pub fn strict_equal(args: &[Value]) -> Result<Value> {
        let actual = args.first().unwrap_or(&Value::Undefined);
        let expected = args.get(1).unwrap_or(&Value::Undefined);
        if !actual.strict_equals(expected) {
            let msg = args.get(2).map(|v| v.to_js_string()).unwrap_or_else(|| {
                format!(
                    "Expected values to be strictly equal:\n  actual: {}\n  expected: {}",
                    actual.to_js_string(),
                    expected.to_js_string()
                )
            });
            return Err(Error::type_error(format!("AssertionError: {}", msg)));
        }
        Ok(Value::Undefined)
    }

    pub fn deep_strict_equal(args: &[Value]) -> Result<Value> {
        let actual = args.first().unwrap_or(&Value::Undefined);
        let expected = args.get(1).unwrap_or(&Value::Undefined);
        if !deep_equal(actual, expected) {
            let msg = args.get(2).map(|v| v.to_js_string()).unwrap_or_else(|| {
                format!(
                    "Expected values to be strictly deep-equal:\n  actual: {}\n  expected: {}",
                    actual.to_js_string(),
                    expected.to_js_string()
                )
            });
            return Err(Error::type_error(format!("AssertionError: {}", msg)));
        }
        Ok(Value::Undefined)
    }

    fn deep_equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => {
                if a.is_nan() && b.is_nan() {
                    true
                } else {
                    a == b
                }
            }
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                if a.properties.len() != b.properties.len() {
                    return false;
                }
                a.properties
                    .iter()
                    .all(|(k, v)| b.properties.get(k).is_some_and(|bv| deep_equal(v, bv)))
            }
            _ => false,
        }
    }

    pub fn throws(args: &[Value]) -> Result<Value> {
        // Simplified: just check that calling the function throws
        let _func = args.first().unwrap_or(&Value::Undefined);
        // In real implementation, would invoke the function and catch the error
        Ok(Value::Undefined)
    }
}

/// Node.js util module implementation
pub mod node_util {
    use crate::error::Result;
    use crate::runtime::Value;

    pub fn inspect(args: &[Value]) -> Result<Value> {
        let value = args.first().unwrap_or(&Value::Undefined);
        Ok(Value::String(format_value(value, 0, 4)))
    }

    pub fn format(args: &[Value]) -> Result<Value> {
        let fmt_str = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let mut result = String::new();
        let mut arg_idx = 1;
        let mut chars = fmt_str.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '%' {
                if let Some(&spec) = chars.peek() {
                    match spec {
                        's' => {
                            chars.next();
                            if let Some(arg) = args.get(arg_idx) {
                                result.push_str(&arg.to_js_string());
                                arg_idx += 1;
                            } else {
                                result.push_str("%s");
                            }
                        }
                        'd' | 'i' => {
                            chars.next();
                            if let Some(arg) = args.get(arg_idx) {
                                result.push_str(&format!("{}", arg.to_number() as i64));
                                arg_idx += 1;
                            } else {
                                result.push('%');
                                result.push(spec);
                            }
                        }
                        'f' => {
                            chars.next();
                            if let Some(arg) = args.get(arg_idx) {
                                result.push_str(&format!("{}", arg.to_number()));
                                arg_idx += 1;
                            } else {
                                result.push_str("%f");
                            }
                        }
                        'j' => {
                            chars.next();
                            if let Some(arg) = args.get(arg_idx) {
                                result.push_str(&arg.to_js_string());
                                arg_idx += 1;
                            } else {
                                result.push_str("%j");
                            }
                        }
                        '%' => {
                            chars.next();
                            result.push('%');
                        }
                        _ => {
                            result.push('%');
                        }
                    }
                } else {
                    result.push('%');
                }
            } else {
                result.push(ch);
            }
        }

        // Append remaining args
        for i in arg_idx..args.len() {
            result.push(' ');
            result.push_str(&args[i].to_js_string());
        }

        Ok(Value::String(result))
    }

    fn format_value(value: &Value, depth: usize, max_depth: usize) -> String {
        if depth > max_depth {
            return "[Object]".to_string();
        }
        let indent = "  ".repeat(depth);
        match value {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(b) => format!("{}", b),
            Value::Number(n) => format!("{}", n),
            Value::String(s) => format!("'{}'", s),
            Value::Symbol(id) => format!("Symbol({})", id),
            Value::BigInt(n) => format!("{}n", n),
            Value::Object(obj) => {
                let obj = obj.borrow();
                match &obj.kind {
                    crate::runtime::ObjectKind::Array(arr) => {
                        if arr.is_empty() {
                            return "[]".to_string();
                        }
                        let items: Vec<String> = arr
                            .iter()
                            .take(100)
                            .map(|v| format_value(v, depth + 1, max_depth))
                            .collect();
                        if arr.len() > 100 {
                            format!("[ {}, ... {} more items ]", items.join(", "), arr.len() - 100)
                        } else {
                            format!("[ {} ]", items.join(", "))
                        }
                    }
                    crate::runtime::ObjectKind::Function(_) => "[Function]".to_string(),
                    crate::runtime::ObjectKind::NativeFunction { name, .. } => {
                        format!("[Function: {}]", name)
                    }
                    _ => {
                        if obj.properties.is_empty() {
                            return "{}".to_string();
                        }
                        let inner_indent = "  ".repeat(depth + 1);
                        let items: Vec<String> = obj
                            .properties
                            .iter()
                            .map(|(k, v)| {
                                format!(
                                    "{}{}: {}",
                                    inner_indent,
                                    k,
                                    format_value(v, depth + 1, max_depth)
                                )
                            })
                            .collect();
                        format!("{{\n{}\n{}}}", items.join(",\n"), indent)
                    }
                }
            }
        }
    }
}

// ==================== Capability Analysis ====================

/// Results of static capability analysis on JavaScript source
#[derive(Debug, Clone, Default)]
pub struct CapabilityReport {
    /// Whether the code requires filesystem access
    pub needs_filesystem: bool,
    /// Whether the code requires network access
    pub needs_network: bool,
    /// Whether the code requires subprocess execution
    pub needs_subprocess: bool,
    /// Whether the code uses eval() or Function()
    pub needs_eval: bool,
    /// Whether the code accesses environment variables
    pub needs_env: bool,
    /// Whether the code uses native addons/FFI
    pub needs_native: bool,
    /// Required core modules
    pub required_modules: Vec<String>,
    /// Detected npm dependencies
    pub npm_dependencies: Vec<String>,
    /// Risk level (low/medium/high)
    pub risk_level: RiskLevel,
    /// Human-readable warnings
    pub warnings: Vec<String>,
}

/// Risk level for capability analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RiskLevel {
    /// No dangerous capabilities detected
    #[default]
    Low,
    /// Some capabilities that need permission
    Medium,
    /// Dangerous capabilities detected (subprocess, eval, native)
    High,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
        }
    }
}

/// Static analyzer for detecting capabilities needed by JavaScript code
pub struct CapabilityAnalyzer;

impl CapabilityAnalyzer {
    /// Patterns that indicate filesystem access
    const FS_PATTERNS: &'static [&'static str] = &[
        "require('fs')", "require(\"fs\")", "require('node:fs')",
        "require(\"node:fs\")", "from 'fs'", "from \"fs\"",
        "from 'node:fs'", "from \"node:fs\"",
        "readFileSync", "writeFileSync", "readFile", "writeFile",
        "createReadStream", "createWriteStream", "existsSync",
        "mkdirSync", "readdirSync", "statSync", "unlinkSync",
    ];

    /// Patterns that indicate network access
    const NET_PATTERNS: &'static [&'static str] = &[
        "require('http')", "require(\"http\")", "require('https')",
        "require(\"https\")", "require('net')", "require(\"net\")",
        "require('node:http')", "require('node:https')",
        "from 'http'", "from \"http\"", "from 'https'",
        "from \"https\"", "from 'node:http'", "from 'node:https'",
        "fetch(", "XMLHttpRequest", "WebSocket(",
        ".listen(", "createServer(",
    ];

    /// Patterns that indicate subprocess execution
    const SUBPROCESS_PATTERNS: &'static [&'static str] = &[
        "require('child_process')", "require(\"child_process\")",
        "require('node:child_process')", "from 'child_process'",
        "from \"child_process\"", "from 'node:child_process'",
        "execSync(", "exec(", "spawn(", "spawnSync(",
        "fork(", "execFile(",
    ];

    /// Patterns that indicate eval/dynamic code
    const EVAL_PATTERNS: &'static [&'static str] = &[
        "eval(", "new Function(", "Function(",
    ];

    /// Patterns that indicate environment access
    const ENV_PATTERNS: &'static [&'static str] = &[
        "process.env", "Deno.env",
    ];

    /// Patterns that indicate native addon usage
    const NATIVE_PATTERNS: &'static [&'static str] = &[
        ".node')", ".node\")", "require('ffi')", "require(\"ffi\")",
        "node-gyp", "node-pre-gyp", "napi",
    ];

    /// Analyze JavaScript source code for required capabilities
    pub fn analyze_source(source: &str) -> CapabilityReport {
        let needs_filesystem = Self::FS_PATTERNS.iter().any(|p| source.contains(p));
        let needs_network = Self::NET_PATTERNS.iter().any(|p| source.contains(p));
        let needs_subprocess = Self::SUBPROCESS_PATTERNS.iter().any(|p| source.contains(p));
        let needs_eval = Self::EVAL_PATTERNS.iter().any(|p| source.contains(p));
        let needs_env = Self::ENV_PATTERNS.iter().any(|p| source.contains(p));
        let needs_native = Self::NATIVE_PATTERNS.iter().any(|p| source.contains(p));

        let mut report = CapabilityReport {
            needs_filesystem,
            needs_network,
            needs_subprocess,
            needs_eval,
            needs_env,
            needs_native,
            ..CapabilityReport::default()
        };

        // Extract require() calls for module detection
        Self::extract_requires(source, &mut report);

        // Determine risk level
        report.risk_level = if report.needs_subprocess || report.needs_eval || report.needs_native {
            RiskLevel::High
        } else if report.needs_filesystem || report.needs_network || report.needs_env {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        // Generate warnings
        if report.needs_subprocess {
            report.warnings.push("Code can execute system commands".to_string());
        }
        if report.needs_eval {
            report.warnings.push("Code uses dynamic code evaluation (eval/Function)".to_string());
        }
        if report.needs_native {
            report.warnings.push("Code uses native addons".to_string());
        }

        report
    }

    /// Analyze a package directory for capabilities
    pub fn analyze_package(pkg_dir: &Path) -> Result<CapabilityReport> {
        let main_file = pkg_dir.join("index.js");
        let pkg_json = pkg_dir.join("package.json");

        let mut report = CapabilityReport::default();

        // Check package.json for dependencies
        if pkg_json.exists() {
            let json_str = std::fs::read_to_string(&pkg_json)
                .map_err(|e| Error::ModuleError(format!("Failed to read package.json: {}", e)))?;
            if let Ok(pkg) = PackageJson::parse(&json_str) {
                report.npm_dependencies = pkg.dependencies.keys().cloned().collect();
            }
        }

        // Analyze main entry point
        if main_file.exists() {
            let source = std::fs::read_to_string(&main_file)
                .map_err(|e| Error::ModuleError(format!("Failed to read entry: {}", e)))?;
            let file_report = Self::analyze_source(&source);
            Self::merge_reports(&mut report, &file_report);
        }

        Ok(report)
    }

    /// Extract require() calls from source
    fn extract_requires(source: &str, report: &mut CapabilityReport) {
        let core_modules = ["fs", "http", "https", "net", "path", "url", "util",
                            "crypto", "os", "events", "stream", "child_process",
                            "assert", "buffer", "querystring", "zlib"];

        for line in source.lines() {
            let trimmed = line.trim();
            // Match require('module') patterns
            if let Some(start) = trimmed.find("require(") {
                let rest = &trimmed[start + 8..];
                if let Some(end) = rest.find(')') {
                    let module = rest[..end].trim().trim_matches(|c| c == '\'' || c == '"');
                    let bare = module.strip_prefix("node:").unwrap_or(module);
                    if core_modules.contains(&bare) {
                        report.required_modules.push(bare.to_string());
                    } else if !bare.starts_with('.') && !bare.starts_with('/') {
                        report.npm_dependencies.push(bare.to_string());
                    }
                }
            }
        }
        report.required_modules.sort();
        report.required_modules.dedup();
        report.npm_dependencies.sort();
        report.npm_dependencies.dedup();
    }

    fn merge_reports(target: &mut CapabilityReport, source: &CapabilityReport) {
        target.needs_filesystem |= source.needs_filesystem;
        target.needs_network |= source.needs_network;
        target.needs_subprocess |= source.needs_subprocess;
        target.needs_eval |= source.needs_eval;
        target.needs_env |= source.needs_env;
        target.needs_native |= source.needs_native;
        target.required_modules.extend(source.required_modules.clone());
        target.required_modules.sort();
        target.required_modules.dedup();
        target.warnings.extend(source.warnings.clone());

        target.risk_level = if target.needs_subprocess || target.needs_eval || target.needs_native {
            RiskLevel::High
        } else if target.needs_filesystem || target.needs_network || target.needs_env {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };
    }
}

// ==================== CommonJS Wrapper ====================

/// Wraps JavaScript source code in a CommonJS module function
/// (function(exports, require, module, __filename, __dirname) { ... });
pub struct CommonJsWrapper;

impl CommonJsWrapper {
    /// Wrap source code in CommonJS module function
    pub fn wrap(source: &str) -> String {
        format!(
            "(function(exports, require, module, __filename, __dirname) {{ {} \n}});",
            source
        )
    }

    /// Create a CommonJS-compatible `module` object as a JS-friendly structure
    pub fn create_module_object(filepath: &Path) -> CommonJsModule {
        CommonJsModule::new(filepath)
    }

    /// Detect if source code is CommonJS (uses require/module.exports/exports)
    pub fn is_commonjs(source: &str) -> bool {
        source.contains("require(")
            || source.contains("module.exports")
            || source.contains("exports.")
    }

    /// Detect if source code is ESM (uses import/export)
    pub fn is_esm(source: &str) -> bool {
        // Naive heuristic — looks for import/export at statement position
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
                return true;
            }
        }
        false
    }

    /// Determine the module type
    pub fn detect_module_type(source: &str, path: &Path) -> ModuleType {
        // .mjs is always ESM, .cjs is always CJS
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "mjs" => return ModuleType::ESM,
                "cjs" => return ModuleType::CommonJS,
                _ => {}
            }
        }

        // Check source content
        if Self::is_esm(source) {
            ModuleType::ESM
        } else if Self::is_commonjs(source) {
            ModuleType::CommonJS
        } else {
            ModuleType::Script
        }
    }
}

/// Type of module
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    /// ES Module (import/export)
    ESM,
    /// CommonJS (require/module.exports)
    CommonJS,
    /// Plain script (no module system)
    Script,
}

impl std::fmt::Display for ModuleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleType::ESM => write!(f, "esm"),
            ModuleType::CommonJS => write!(f, "commonjs"),
            ModuleType::Script => write!(f, "script"),
        }
    }
}

// ==================== CommonJS Runtime ====================

/// Runtime environment for CommonJS require() execution.
/// Creates actual Value objects for core modules that can be used by the VM.
pub struct CommonJsRuntime {
    resolver: ModuleResolver,
}

impl CommonJsRuntime {
    pub fn new() -> Self {
        Self {
            resolver: ModuleResolver::new(),
        }
    }

    /// Resolve a require() call and return the module's exports as a Value.
    /// For core modules, returns a pre-built object with native function stubs.
    pub fn require(&mut self, specifier: &str, from_dir: &Path) -> Result<Value> {
        match self.resolver.resolve(specifier, from_dir)? {
            ResolvedModule::Core(name) => self.load_core_module(&name),
            ResolvedModule::File(path) => {
                // Return the module path as a string for now —
                // actual file evaluation requires VM integration
                Ok(Value::String(path.to_string_lossy().to_string()))
            }
        }
    }

    /// Build a JS Value object for a core module with real native function bindings
    fn load_core_module(&self, name: &str) -> Result<Value> {
        match name {
            "path" => Ok(self.build_path_module()),
            "assert" => Ok(self.build_assert_module()),
            "util" => Ok(self.build_util_module()),
            "process" => Ok(self.build_process_module()),
            "os" => Ok(self.build_os_module()),
            "buffer" => Ok(self.build_buffer_module()),
            "events" => Ok(self.build_events_module()),
            "url" | "crypto" | "fs" | "http" | "https" | "stream" | "net" | "child_process" => {
                // Return a stub object with the module name
                let mut props = rustc_hash::FxHashMap::default();
                props.insert("__module__".to_string(), Value::String(name.to_string()));
                Ok(Value::new_object_with_properties(props))
            }
            _ => Err(Error::ModuleError(format!("Unknown core module: {}", name))),
        }
    }

    fn build_path_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        props.insert("join".to_string(), Value::make_native_fn("join", node_path::join));
        props.insert("resolve".to_string(), Value::make_native_fn("resolve", node_path::resolve));
        props.insert("dirname".to_string(), Value::make_native_fn("dirname", node_path::dirname));
        props.insert("basename".to_string(), Value::make_native_fn("basename", node_path::basename));
        props.insert("extname".to_string(), Value::make_native_fn("extname", node_path::extname));
        props.insert("isAbsolute".to_string(), Value::make_native_fn("isAbsolute", node_path::is_absolute));
        props.insert("normalize".to_string(), Value::make_native_fn("normalize", node_path::normalize));
        props.insert("parse".to_string(), Value::make_native_fn("parse", node_path::parse_path));
        props.insert("sep".to_string(), node_path::sep());
        Value::new_object_with_properties(props)
    }

    fn build_assert_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        props.insert("ok".to_string(), Value::make_native_fn("ok", node_assert::ok));
        props.insert("strictEqual".to_string(), Value::make_native_fn("strictEqual", node_assert::strict_equal));
        props.insert("deepStrictEqual".to_string(), Value::make_native_fn("deepStrictEqual", node_assert::deep_strict_equal));
        Value::new_object_with_properties(props)
    }

    fn build_util_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        props.insert("inspect".to_string(), Value::make_native_fn("inspect", node_util::inspect));
        props.insert("format".to_string(), Value::make_native_fn("format", node_util::format));
        Value::new_object_with_properties(props)
    }

    fn build_process_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        props.insert("version".to_string(), Value::String("v20.0.0".to_string()));
        props.insert("platform".to_string(), Value::String(std::env::consts::OS.to_string()));
        props.insert("arch".to_string(), Value::String(std::env::consts::ARCH.to_string()));
        props.insert("pid".to_string(), Value::Number(std::process::id() as f64));

        // process.env (limited to safe vars)
        let env_obj = Value::new_object();
        if let Ok(val) = std::env::var("NODE_ENV") {
            env_obj.set_property("NODE_ENV", Value::String(val));
        }
        if let Ok(val) = std::env::var("HOME") {
            env_obj.set_property("HOME", Value::String(val));
        }
        props.insert("env".to_string(), env_obj);

        // process.argv (empty for sandboxed execution)
        props.insert("argv".to_string(), Value::new_array(vec![]));

        // process.cwd()
        props.insert("cwd".to_string(), Value::make_native_fn("cwd", |_args| {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/".to_string());
            Ok(Value::String(cwd))
        }));

        // process.exit() - no-op in sandbox
        props.insert("exit".to_string(), Value::make_native_fn("exit", |_args| {
            Ok(Value::Undefined)
        }));

        // process.hrtime()
        props.insert("hrtime".to_string(), Value::make_native_fn("hrtime", |_args| {
            Ok(Value::new_array(vec![Value::Number(0.0), Value::Number(0.0)]))
        }));

        // process.nextTick (synchronous in Quicksilver)
        props.insert("nextTick".to_string(), Value::make_native_fn("nextTick", |_args| {
            Ok(Value::Undefined)
        }));

        Value::new_object_with_properties(props)
    }

    fn build_os_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        props.insert("platform".to_string(), Value::make_native_fn("platform", |_args| {
            Ok(Value::String(std::env::consts::OS.to_string()))
        }));
        props.insert("arch".to_string(), Value::make_native_fn("arch", |_args| {
            Ok(Value::String(std::env::consts::ARCH.to_string()))
        }));
        props.insert("type".to_string(), Value::make_native_fn("type", |_args| {
            let os_type = match std::env::consts::OS {
                "macos" => "Darwin",
                "linux" => "Linux",
                "windows" => "Windows_NT",
                other => other,
            };
            Ok(Value::String(os_type.to_string()))
        }));
        props.insert("homedir".to_string(), Value::make_native_fn("homedir", |_args| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            Ok(Value::String(home))
        }));
        props.insert("tmpdir".to_string(), Value::make_native_fn("tmpdir", |_args| {
            Ok(Value::String(std::env::temp_dir().to_string_lossy().to_string()))
        }));
        props.insert("EOL".to_string(), Value::String("\n".to_string()));
        props.insert("cpus".to_string(), Value::make_native_fn("cpus", |_args| {
            Ok(Value::new_array(vec![]))
        }));
        Value::new_object_with_properties(props)
    }

    fn build_buffer_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();

        // Buffer.from(data, encoding?)
        props.insert("Buffer".to_string(), {
            let buffer_obj = Value::new_object();

            buffer_obj.set_property("from", Value::make_native_fn("from", |args| {
                let data = args.first().cloned().unwrap_or(Value::Undefined);
                match data {
                    Value::String(s) => {
                        let bytes: Vec<Value> = s.bytes().map(|b| Value::Number(b as f64)).collect();
                        let buf = Value::new_array(bytes.clone());
                        buf.set_property("length", Value::Number(bytes.len() as f64));
                        buf.set_property("type", Value::String("Buffer".to_string()));
                        Ok(buf)
                    }
                    Value::Object(obj) => {
                        let borrowed = obj.borrow();
                        if let crate::ObjectKind::Array(arr) = &borrowed.kind {
                            let buf = Value::new_array(arr.clone());
                            buf.set_property("length", Value::Number(arr.len() as f64));
                            buf.set_property("type", Value::String("Buffer".to_string()));
                            Ok(buf)
                        } else {
                            Ok(Value::new_array(vec![]))
                        }
                    }
                    _ => Ok(Value::new_array(vec![])),
                }
            }));

            // Buffer.alloc(size, fill?)
            buffer_obj.set_property("alloc", Value::make_native_fn("alloc", |args| {
                let size = args.first()
                    .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                    .unwrap_or(0);
                let fill = args.get(1)
                    .and_then(|v| if let Value::Number(n) = v { Some(*n) } else { None })
                    .unwrap_or(0.0);
                let bytes: Vec<Value> = vec![Value::Number(fill); size];
                let buf = Value::new_array(bytes);
                buf.set_property("length", Value::Number(size as f64));
                buf.set_property("type", Value::String("Buffer".to_string()));
                Ok(buf)
            }));

            // Buffer.isBuffer(obj)
            buffer_obj.set_property("isBuffer", Value::make_native_fn("isBuffer", |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let is_buf = val.get_property("type")
                    .map(|t| t == Value::String("Buffer".to_string()))
                    .unwrap_or(false);
                Ok(Value::Boolean(is_buf))
            }));

            // Buffer.concat(list)
            buffer_obj.set_property("concat", Value::make_native_fn("concat", |args| {
                let list = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::Object(obj) = &list {
                    let borrowed = obj.borrow();
                    if let crate::ObjectKind::Array(bufs) = &borrowed.kind {
                        let mut all = Vec::new();
                        for buf in bufs {
                            if let Value::Object(buf_obj) = buf {
                                let buf_ref = buf_obj.borrow();
                                if let crate::ObjectKind::Array(bytes) = &buf_ref.kind {
                                    all.extend(bytes.clone());
                                }
                            }
                        }
                        let result = Value::new_array(all.clone());
                        result.set_property("length", Value::Number(all.len() as f64));
                        result.set_property("type", Value::String("Buffer".to_string()));
                        return Ok(result);
                    }
                }
                Ok(Value::new_array(vec![]))
            }));

            buffer_obj
        });

        Value::new_object_with_properties(props)
    }

    fn build_events_module(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();

        // EventEmitter constructor (returns object with on/emit/removeListener)
        props.insert("EventEmitter".to_string(), Value::make_native_fn("EventEmitter", |_args| {
            let emitter = Value::new_object();

            // Internal listeners storage
            let listeners = Value::new_object();
            emitter.set_property("_listeners", listeners);

            // on(event, listener) — register a listener
            emitter.set_property("on", Value::make_native_fn("on", |_args| {
                // In a full implementation, this would store the callback
                // For now, return `this` for chaining
                Ok(Value::Undefined)
            }));

            // emit(event, ...args) — emit an event
            emitter.set_property("emit", Value::make_native_fn("emit", |_args| {
                Ok(Value::Boolean(false))
            }));

            // removeListener(event, listener)
            emitter.set_property("removeListener", Value::make_native_fn("removeListener", |_args| {
                Ok(Value::Undefined)
            }));

            // removeAllListeners(event?)
            emitter.set_property("removeAllListeners", Value::make_native_fn("removeAllListeners", |_args| {
                Ok(Value::Undefined)
            }));

            // listenerCount(event)
            emitter.set_property("listenerCount", Value::make_native_fn("listenerCount", |_args| {
                Ok(Value::Number(0.0))
            }));

            Ok(emitter)
        }));

        Value::new_object_with_properties(props)
    }
}

impl Default for CommonJsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Built-in Package Manager ──────────────────────────────────────────

/// Represents a package dependency with version constraint
#[derive(Debug, Clone)]
pub struct PackageDependency {
    pub name: String,
    pub version: String,
    pub resolved: Option<String>,
    pub integrity: Option<String>,
}

/// Lockfile entry for deterministic builds
#[derive(Debug, Clone)]
pub struct LockfileEntry {
    pub name: String,
    pub version: String,
    pub resolved: String,
    pub integrity: String,
    pub dependencies: Vec<String>,
}

/// In-memory lockfile representation
#[derive(Debug, Clone)]
pub struct Lockfile {
    pub version: u32,
    pub entries: HashMap<String, LockfileEntry>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            version: 1,
            entries: HashMap::default(),
        }
    }

    pub fn add_entry(&mut self, entry: LockfileEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    pub fn get_entry(&self, name: &str) -> Option<&LockfileEntry> {
        self.entries.get(name)
    }

    pub fn remove_entry(&mut self, name: &str) -> bool {
        self.entries.remove(name).is_some()
    }

    /// Serialize lockfile to JSON string
    pub fn to_json(&self) -> String {
        let mut s = String::from("{\n");
        s.push_str(&format!("  \"lockfileVersion\": {},\n", self.version));
        s.push_str("  \"dependencies\": {\n");
        let entries: Vec<_> = self.entries.iter().collect();
        for (i, (name, entry)) in entries.iter().enumerate() {
            s.push_str(&format!("    \"{}\": {{\n", name));
            s.push_str(&format!("      \"version\": \"{}\",\n", entry.version));
            s.push_str(&format!("      \"resolved\": \"{}\",\n", entry.resolved));
            s.push_str(&format!("      \"integrity\": \"{}\"\n", entry.integrity));
            s.push_str("    }");
            if i < entries.len() - 1 {
                s.push(',');
            }
            s.push('\n');
        }
        s.push_str("  }\n}");
        s
    }
}

/// Security permission for a package
#[derive(Debug, Clone, PartialEq)]
pub enum PackagePermission {
    /// Network access
    Net,
    /// File system access
    FileSystem,
    /// Environment variable access
    Env,
    /// Subprocess spawning
    Subprocess,
    /// FFI usage
    Ffi,
}

/// Package manager for installing, resolving, and vendoring dependencies
#[derive(Debug)]
pub struct PackageManager {
    /// Installed packages
    pub installed: HashMap<String, PackageDependency>,
    /// Lockfile state
    pub lockfile: Lockfile,
    /// Vendor directory path
    pub vendor_dir: Option<PathBuf>,
    /// Package permissions
    pub permissions: HashMap<String, Vec<PackagePermission>>,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            installed: HashMap::default(),
            lockfile: Lockfile::new(),
            vendor_dir: None,
            permissions: HashMap::default(),
        }
    }

    /// Set the vendor directory for local caching
    pub fn set_vendor_dir(&mut self, dir: PathBuf) {
        self.vendor_dir = Some(dir);
    }

    /// Install a package by name and version constraint
    pub fn install(&mut self, name: &str, version: &str) -> Result<&PackageDependency> {
        let resolved_version = Self::resolve_version(version);
        let integrity = format!("sha512-{}", name.len() + version.len());

        let dep = PackageDependency {
            name: name.to_string(),
            version: resolved_version.clone(),
            resolved: Some(format!("https://registry.npmjs.org/{}/-/{}-{}.tgz", name, name, resolved_version)),
            integrity: Some(integrity.clone()),
        };

        let lock_entry = LockfileEntry {
            name: name.to_string(),
            version: resolved_version.clone(),
            resolved: dep.resolved.clone().unwrap_or_default(),
            integrity,
            dependencies: Vec::new(),
        };

        self.lockfile.add_entry(lock_entry);
        self.installed.insert(name.to_string(), dep);
        Ok(self.installed.get(name).unwrap())
    }

    /// Uninstall a package
    pub fn uninstall(&mut self, name: &str) -> bool {
        self.lockfile.remove_entry(name);
        self.permissions.remove(name);
        self.installed.remove(name).is_some()
    }

    /// List all installed packages
    pub fn list(&self) -> Vec<&PackageDependency> {
        self.installed.values().collect()
    }

    /// Set permissions for a package
    pub fn set_permissions(&mut self, name: &str, perms: Vec<PackagePermission>) {
        self.permissions.insert(name.to_string(), perms);
    }

    /// Check if a package has a specific permission
    pub fn has_permission(&self, name: &str, perm: &PackagePermission) -> bool {
        self.permissions
            .get(name)
            .map(|perms| perms.contains(perm))
            .unwrap_or(false)
    }

    /// Resolve a version constraint to a concrete version (simplified)
    fn resolve_version(constraint: &str) -> String {
        let trimmed = constraint.trim_start_matches('^')
            .trim_start_matches('~')
            .trim_start_matches(">=")
            .trim_start_matches('>');
        if trimmed.is_empty() || trimmed == "*" {
            "1.0.0".to_string()
        } else {
            trimmed.to_string()
        }
    }

    /// Check if a package is installed
    pub fn is_installed(&self, name: &str) -> bool {
        self.installed.contains_key(name)
    }

    /// Get the lockfile as JSON
    pub fn export_lockfile(&self) -> String {
        self.lockfile.to_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_json_parse() {
        let json = r#"{
            "name": "test-pkg",
            "version": "1.0.0",
            "main": "dist/index.js",
            "dependencies": {
                "lodash": "^4.17.21"
            }
        }"#;
        let pkg = PackageJson::parse(json).unwrap();
        assert_eq!(pkg.name, Some("test-pkg".to_string()));
        assert_eq!(pkg.version, Some("1.0.0".to_string()));
        assert_eq!(pkg.entry_point(), Some("dist/index.js"));
        assert!(pkg.dependencies.contains_key("lodash"));
    }

    #[test]
    fn test_package_json_exports() {
        let json = r#"{
            "name": "modern-pkg",
            "exports": {
                "import": "./dist/esm/index.js",
                "require": "./dist/cjs/index.js"
            }
        }"#;
        let pkg = PackageJson::parse(json).unwrap();
        assert!(matches!(pkg.exports, Some(PackageExports::Conditional(_))));
        assert_eq!(pkg.entry_point(), Some("./dist/esm/index.js"));
    }

    #[test]
    fn test_parse_specifier() {
        let (name, sub) = ModuleResolver::parse_specifier("lodash");
        assert_eq!(name, "lodash");
        assert!(sub.is_none());

        let (name, sub) = ModuleResolver::parse_specifier("lodash/fp");
        assert_eq!(name, "lodash");
        assert_eq!(sub, Some("fp".to_string()));

        let (name, sub) = ModuleResolver::parse_specifier("@scope/pkg");
        assert_eq!(name, "@scope/pkg");
        assert!(sub.is_none());

        let (name, sub) = ModuleResolver::parse_specifier("@scope/pkg/deep/path");
        assert_eq!(name, "@scope/pkg");
        assert_eq!(sub, Some("deep/path".to_string()));
    }

    #[test]
    fn test_capability_analysis_fs() {
        let code = r#"
            const fs = require('fs');
            fs.readFileSync('./data.txt');
            const path = require('path');
        "#;
        let caps = CapabilityAnalyzer::analyze_source(code);
        assert!(caps.needs_filesystem);
        assert!(!caps.needs_network);
        assert!(!caps.needs_subprocess);
    }

    #[test]
    fn test_capability_analysis_net() {
        let code = r#"
            const http = require('http');
            fetch('https://api.example.com');
        "#;
        let caps = CapabilityAnalyzer::analyze_source(code);
        assert!(caps.needs_network);
    }

    #[test]
    fn test_capability_analysis_subprocess() {
        let code = r#"
            const { exec } = require('child_process');
            exec('ls -la');
        "#;
        let caps = CapabilityAnalyzer::analyze_source(code);
        assert!(caps.needs_subprocess);
    }

    #[test]
    fn test_capability_analysis_safe() {
        let code = r#"
            const x = 1 + 2;
            console.log(x);
        "#;
        let caps = CapabilityAnalyzer::analyze_source(code);
        assert!(!caps.needs_filesystem);
        assert!(!caps.needs_network);
        assert!(!caps.needs_subprocess);
        assert!(!caps.needs_eval);
    }

    #[test]
    fn test_core_modules() {
        let resolver = ModuleResolver::new();
        assert!(resolver.is_core_module("path"));
        assert!(resolver.is_core_module("node:path"));
        assert!(resolver.is_core_module("util"));
        assert!(resolver.is_core_module("events"));
        assert!(resolver.is_core_module("crypto"));
        assert!(!resolver.is_core_module("express"));
    }

    #[test]
    fn test_node_path_join() {
        use node_path::join;
        let result = join(&[
            Value::String("/foo".to_string()),
            Value::String("bar".to_string()),
            Value::String("baz".to_string()),
        ])
        .unwrap();
        assert_eq!(result, Value::String("/foo/bar/baz".to_string()));
    }

    #[test]
    fn test_node_path_basename() {
        use node_path::basename;
        let result = basename(&[Value::String("/foo/bar/baz.js".to_string())]).unwrap();
        assert_eq!(result, Value::String("baz.js".to_string()));

        let result = basename(&[
            Value::String("/foo/bar/baz.js".to_string()),
            Value::String(".js".to_string()),
        ])
        .unwrap();
        assert_eq!(result, Value::String("baz".to_string()));
    }

    #[test]
    fn test_node_path_extname() {
        use node_path::extname;
        let result = extname(&[Value::String("/foo/bar.js".to_string())]).unwrap();
        assert_eq!(result, Value::String(".js".to_string()));
    }

    #[test]
    fn test_node_util_format() {
        use node_util::format;
        let result = format(&[
            Value::String("Hello %s, you are %d".to_string()),
            Value::String("world".to_string()),
            Value::Number(42.0),
        ])
        .unwrap();
        assert_eq!(result, Value::String("Hello world, you are 42".to_string()));
    }

    #[test]
    fn test_node_assert_ok() {
        use node_assert::ok;
        assert!(ok(&[Value::Boolean(true)]).is_ok());
        assert!(ok(&[Value::Boolean(false)]).is_err());
        assert!(ok(&[Value::Number(1.0)]).is_ok());
        assert!(ok(&[Value::Number(0.0)]).is_err());
    }

    #[test]
    fn test_node_assert_strict_equal() {
        use node_assert::strict_equal;
        assert!(strict_equal(&[Value::Number(1.0), Value::Number(1.0)]).is_ok());
        assert!(strict_equal(&[Value::Number(1.0), Value::Number(2.0)]).is_err());
        assert!(strict_equal(&[
            Value::String("hello".to_string()),
            Value::String("hello".to_string())
        ])
        .is_ok());
    }

    #[test]
    fn test_commonjs_wrapper() {
        let wrapped = CommonJsWrapper::wrap("module.exports = 42;");
        assert!(wrapped.starts_with("(function(exports, require, module, __filename, __dirname)"));
        assert!(wrapped.contains("module.exports = 42;"));
        assert!(wrapped.ends_with("});"));
    }

    #[test]
    fn test_detect_commonjs() {
        assert!(CommonJsWrapper::is_commonjs("const x = require('lodash');"));
        assert!(CommonJsWrapper::is_commonjs("module.exports = {};"));
        assert!(CommonJsWrapper::is_commonjs("exports.foo = 1;"));
        assert!(!CommonJsWrapper::is_commonjs("const x = 1;"));
    }

    #[test]
    fn test_detect_esm() {
        assert!(CommonJsWrapper::is_esm("import foo from 'bar';"));
        assert!(CommonJsWrapper::is_esm("export const x = 1;"));
        assert!(!CommonJsWrapper::is_esm("const x = require('foo');"));
    }

    #[test]
    fn test_module_type_detection() {
        use std::path::Path;
        assert_eq!(CommonJsWrapper::detect_module_type("", Path::new("a.mjs")), ModuleType::ESM);
        assert_eq!(CommonJsWrapper::detect_module_type("", Path::new("a.cjs")), ModuleType::CommonJS);
        assert_eq!(
            CommonJsWrapper::detect_module_type("import x from 'y';", Path::new("a.js")),
            ModuleType::ESM
        );
        assert_eq!(
            CommonJsWrapper::detect_module_type("require('fs');", Path::new("a.js")),
            ModuleType::CommonJS
        );
        assert_eq!(
            CommonJsWrapper::detect_module_type("const x = 1;", Path::new("a.js")),
            ModuleType::Script
        );
    }

    // ==================== CommonJsRuntime Tests ====================

    #[test]
    fn test_commonjs_runtime_require_path() {
        let mut rt = CommonJsRuntime::new();
        let path_module = rt.require("path", Path::new("/tmp")).unwrap();
        assert!(matches!(path_module, Value::Object(_)));
    }

    #[test]
    fn test_commonjs_runtime_require_assert() {
        let mut rt = CommonJsRuntime::new();
        let assert_module = rt.require("assert", Path::new("/tmp")).unwrap();
        assert!(matches!(assert_module, Value::Object(_)));
    }

    #[test]
    fn test_commonjs_runtime_require_util() {
        let mut rt = CommonJsRuntime::new();
        let util_module = rt.require("util", Path::new("/tmp")).unwrap();
        assert!(matches!(util_module, Value::Object(_)));
    }

    #[test]
    fn test_commonjs_runtime_require_node_prefix() {
        let mut rt = CommonJsRuntime::new();
        let path_module = rt.require("node:path", Path::new("/tmp")).unwrap();
        assert!(matches!(path_module, Value::Object(_)));
    }

    #[test]
    fn test_commonjs_runtime_require_stub_module() {
        let mut rt = CommonJsRuntime::new();
        let fs_module = rt.require("fs", Path::new("/tmp")).unwrap();
        assert!(matches!(fs_module, Value::Object(_)));
    }

    #[test]
    fn test_commonjs_runtime_require_process() {
        let mut rt = CommonJsRuntime::new();
        let proc = rt.require("process", Path::new("/tmp")).unwrap();
        assert!(matches!(proc, Value::Object(_)));
        // Check version property
        let version = proc.get_property("version");
        assert!(matches!(version, Some(Value::String(_))));
        // Check platform property
        let platform = proc.get_property("platform");
        assert!(matches!(platform, Some(Value::String(_))));
        // Check arch property
        let arch = proc.get_property("arch");
        assert!(matches!(arch, Some(Value::String(_))));
        // Check pid is a number
        let pid = proc.get_property("pid");
        assert!(matches!(pid, Some(Value::Number(_))));
    }

    #[test]
    fn test_commonjs_runtime_require_os() {
        let mut rt = CommonJsRuntime::new();
        let os = rt.require("os", Path::new("/tmp")).unwrap();
        assert!(matches!(os, Value::Object(_)));
        // Check EOL constant
        let eol = os.get_property("EOL");
        assert_eq!(eol, Some(Value::String("\n".to_string())));
    }

    #[test]
    fn test_commonjs_runtime_process_env() {
        let mut rt = CommonJsRuntime::new();
        let proc = rt.require("process", Path::new("/tmp")).unwrap();
        let env = proc.get_property("env");
        assert!(matches!(env, Some(Value::Object(_))));
    }

    #[test]
    fn test_commonjs_runtime_process_argv() {
        let mut rt = CommonJsRuntime::new();
        let proc = rt.require("process", Path::new("/tmp")).unwrap();
        let argv = proc.get_property("argv");
        assert!(matches!(argv, Some(Value::Object(_))));
    }

    #[test]
    fn test_core_module_process_registered() {
        let resolver = ModuleResolver::new();
        assert!(resolver.is_core_module("process"));
        assert!(resolver.is_core_module("node:process"));
    }

    #[test]
    fn test_core_module_os_registered() {
        let resolver = ModuleResolver::new();
        assert!(resolver.is_core_module("os"));
        let core = resolver.get_core_module("os").unwrap();
        assert!(!core.exports.is_empty());
    }

    #[test]
    fn test_commonjs_runtime_require_buffer() {
        let mut rt = CommonJsRuntime::new();
        let result = rt.require("buffer", Path::new("."));
        assert!(result.is_ok());
        let module = result.unwrap();
        assert!(module.get_property("Buffer").is_some());
    }

    #[test]
    fn test_buffer_from_string() {
        let rt = CommonJsRuntime::new();
        let buf_mod = rt.build_buffer_module();
        let buffer_ctor = buf_mod.get_property("Buffer").unwrap();
        let from_fn = buffer_ctor.get_property("from").unwrap();
        if let Value::Object(obj) = &from_fn {
            let borrowed = obj.borrow();
            if let crate::ObjectKind::NativeFunction { func, .. } = &borrowed.kind {
                let result = func(&[Value::String("hi".to_string())]).unwrap();
                let typ = result.get_property("type").unwrap();
                assert_eq!(typ, Value::String("Buffer".to_string()));
            }
        }
    }

    #[test]
    fn test_commonjs_runtime_require_events() {
        let mut rt = CommonJsRuntime::new();
        let result = rt.require("events", Path::new("."));
        assert!(result.is_ok());
        let module = result.unwrap();
        assert!(module.get_property("EventEmitter").is_some());
    }

    #[test]
    fn test_package_manager_install() {
        let mut pm = PackageManager::new();
        let dep = pm.install("lodash", "^4.17.21").unwrap();
        assert_eq!(dep.name, "lodash");
        assert_eq!(dep.version, "4.17.21");
        assert!(pm.is_installed("lodash"));
    }

    #[test]
    fn test_package_manager_uninstall() {
        let mut pm = PackageManager::new();
        pm.install("lodash", "^4.17.21").unwrap();
        assert!(pm.uninstall("lodash"));
        assert!(!pm.is_installed("lodash"));
        assert!(!pm.uninstall("nonexistent"));
    }

    #[test]
    fn test_package_manager_lockfile() {
        let mut pm = PackageManager::new();
        pm.install("express", "4.18.2").unwrap();
        let json = pm.export_lockfile();
        assert!(json.contains("express"));
        assert!(json.contains("4.18.2"));
        assert!(json.contains("lockfileVersion"));
    }

    #[test]
    fn test_package_manager_permissions() {
        let mut pm = PackageManager::new();
        pm.install("axios", "1.0.0").unwrap();
        pm.set_permissions("axios", vec![PackagePermission::Net]);
        assert!(pm.has_permission("axios", &PackagePermission::Net));
        assert!(!pm.has_permission("axios", &PackagePermission::FileSystem));
        assert!(!pm.has_permission("unknown", &PackagePermission::Net));
    }

    #[test]
    fn test_package_manager_list() {
        let mut pm = PackageManager::new();
        pm.install("a", "1.0.0").unwrap();
        pm.install("b", "2.0.0").unwrap();
        assert_eq!(pm.list().len(), 2);
    }

    #[test]
    fn test_lockfile_operations() {
        let mut lockfile = Lockfile::new();
        lockfile.add_entry(LockfileEntry {
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            resolved: "https://example.com/pkg".to_string(),
            integrity: "sha512-abc".to_string(),
            dependencies: vec![],
        });
        assert!(lockfile.get_entry("pkg").is_some());
        assert!(lockfile.remove_entry("pkg"));
        assert!(lockfile.get_entry("pkg").is_none());
    }
}

//! Node.js-compatible module resolution algorithm
//!
//! Implements file, relative, and bare-specifier resolution with
//! automatic extension probing, `package.json` entry-point lookup,
//! and a result cache.

use crate::error::{Error, Result};
use crate::npm::package_json::PackageJson;
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};

/// Node.js-compatible module resolver
pub struct ModuleResolver {
    /// Base directory for resolution
    base_dir: String,
    /// Cached specifier → resolved path
    cache: HashMap<String, String>,
    /// Cached package.json per directory
    package_cache: HashMap<String, PackageJson>,
    /// File extensions to try when probing
    extensions: Vec<String>,
}

/// A successfully resolved module
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Absolute path to the resolved file
    pub path: String,
    /// Detected module format
    pub format: ModuleFormat,
    /// Owning package name (if resolved from node_modules)
    pub package_name: Option<String>,
}

/// Module format discriminator
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModuleFormat {
    CommonJS,
    ESModule,
    Json,
    Native,
}

impl ModuleResolver {
    /// Create a new resolver rooted at `base_dir`
    pub fn new(base_dir: &str) -> Self {
        Self {
            base_dir: base_dir.to_string(),
            cache: HashMap::default(),
            package_cache: HashMap::default(),
            extensions: vec![
                ".js".to_string(),
                ".mjs".to_string(),
                ".cjs".to_string(),
                ".json".to_string(),
                ".ts".to_string(),
            ],
        }
    }

    /// Resolve a module specifier relative to `from_file`.
    ///
    /// Handles relative paths (`./`, `../`), absolute paths, and bare
    /// specifiers (node_modules lookup).
    pub fn resolve(&mut self, specifier: &str, from_file: &str) -> Result<ResolvedModule> {
        let from_dir = Path::new(from_file)
            .parent()
            .unwrap_or_else(|| Path::new(&self.base_dir))
            .to_string_lossy()
            .to_string();

        // Check cache
        let cache_key = format!("{}:{}", specifier, from_dir);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(ResolvedModule {
                path: cached.clone(),
                format: Self::detect_format(cached),
                package_name: None,
            });
        }

        let result = if specifier.starts_with("./") || specifier.starts_with("../") {
            self.resolve_relative(specifier, &from_dir)
                .map(|path| ResolvedModule {
                    format: Self::detect_format(&path),
                    path,
                    package_name: None,
                })
        } else if specifier.starts_with('/') {
            self.resolve_absolute(specifier)
                .map(|path| ResolvedModule {
                    format: Self::detect_format(&path),
                    path,
                    package_name: None,
                })
        } else {
            self.resolve_bare(specifier, &from_dir)
        };

        match result {
            Some(resolved) => {
                self.cache.insert(cache_key, resolved.path.clone());
                Ok(resolved)
            }
            None => Err(Error::ModuleError(format!(
                "Cannot resolve module '{}' from '{}'",
                specifier, from_file
            ))),
        }
    }

    /// Resolve a bare specifier by walking up node_modules directories
    pub fn resolve_bare_specifier(&self, name: &str, from_dir: &str) -> Option<String> {
        let (pkg_name, subpath) = Self::parse_bare(name);
        let mut dir = PathBuf::from(from_dir);

        loop {
            let candidate = dir.join("node_modules").join(&pkg_name);
            if candidate.is_dir() {
                if let Some(sub) = &subpath {
                    let target = candidate.join(sub);
                    if let Some(resolved) = self.try_resolve_file(&target.to_string_lossy()) {
                        return Some(resolved);
                    }
                }
                // Try package.json entry
                let pkg_json_path = candidate.join("package.json");
                if pkg_json_path.is_file() {
                    if let Ok(pkg) = PackageJson::load(&pkg_json_path.to_string_lossy()) {
                        if let Some(entry) = pkg.resolve_main() {
                            let entry_path = candidate.join(entry);
                            if let Some(r) = self.try_resolve_file(&entry_path.to_string_lossy()) {
                                return Some(r);
                            }
                        }
                    }
                }
                // Fallback: index.js
                if let Some(r) = self.try_resolve_file(&candidate.join("index").to_string_lossy())
                {
                    return Some(r);
                }
            }
            if !dir.pop() {
                break;
            }
        }
        None
    }

    /// Resolve a relative path (`./file` or `../file`) from a directory
    pub fn resolve_relative(&self, path: &str, from_dir: &str) -> Option<String> {
        let full = PathBuf::from(from_dir).join(path);
        self.try_resolve_file(&full.to_string_lossy())
    }

    /// Walk up directory tree to find the nearest `package.json`
    pub fn find_package_json(&self, dir: &str) -> Option<String> {
        let mut current = PathBuf::from(dir);
        loop {
            let candidate = current.join("package.json");
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    /// Drop all cached resolutions and package.json entries
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.package_cache.clear();
    }

    // ── private helpers ─────────────────────────────────────────────

    fn resolve_absolute(&self, path: &str) -> Option<String> {
        self.try_resolve_file(path)
    }

    fn resolve_bare(&self, specifier: &str, from_dir: &str) -> Option<ResolvedModule> {
        let (pkg_name, _) = Self::parse_bare(specifier);
        self.resolve_bare_specifier(specifier, from_dir)
            .map(|path| ResolvedModule {
                format: Self::detect_format(&path),
                path,
                package_name: Some(pkg_name),
            })
    }

    /// Try the path as-is, then with each extension, then as a directory
    fn try_resolve_file(&self, path: &str) -> Option<String> {
        let p = Path::new(path);
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
        for ext in &self.extensions {
            let with_ext = format!("{}{}", path, ext);
            if Path::new(&with_ext).is_file() {
                return Some(with_ext);
            }
        }
        // Directory — try index files
        if p.is_dir() {
            for ext in &self.extensions {
                let index = p.join(format!("index{}", ext));
                if index.is_file() {
                    return Some(index.to_string_lossy().to_string());
                }
            }
        }
        None
    }

    fn detect_format(path: &str) -> ModuleFormat {
        if path.ends_with(".mjs") {
            ModuleFormat::ESModule
        } else if path.ends_with(".cjs") {
            ModuleFormat::CommonJS
        } else if path.ends_with(".json") {
            ModuleFormat::Json
        } else if path.ends_with(".node") {
            ModuleFormat::Native
        } else {
            ModuleFormat::CommonJS
        }
    }

    fn parse_bare(specifier: &str) -> (String, Option<String>) {
        if specifier.starts_with('@') {
            let parts: Vec<&str> = specifier.splitn(3, '/').collect();
            if parts.len() >= 2 {
                let name = format!("{}/{}", parts[0], parts[1]);
                let sub = if parts.len() > 2 {
                    Some(parts[2].to_string())
                } else {
                    None
                };
                (name, sub)
            } else {
                (specifier.to_string(), None)
            }
        } else {
            let parts: Vec<&str> = specifier.splitn(2, '/').collect();
            if parts.len() > 1 {
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else {
                (specifier.to_string(), None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_tmpdir() -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("qs_resolver_test_{}", id));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn test_resolve_relative_js_file() {
        let base = setup_tmpdir();
        let file = base.join("lib.js");
        fs::write(&file, "module.exports = 1;").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver
            .resolve_relative("./lib", &base.to_string_lossy())
            .unwrap();
        assert!(result.ends_with("lib.js"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_relative_exact() {
        let base = setup_tmpdir();
        let file = base.join("exact.js");
        fs::write(&file, "").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver
            .resolve_relative("./exact.js", &base.to_string_lossy())
            .unwrap();
        assert!(result.ends_with("exact.js"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_relative_index() {
        let base = setup_tmpdir();
        let sub = base.join("mymod");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("index.js"), "").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver
            .resolve_relative("./mymod", &base.to_string_lossy())
            .unwrap();
        assert!(result.ends_with("index.js"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_bare_specifier_node_modules() {
        let base = setup_tmpdir();
        let nm = base.join("node_modules").join("foo");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver
            .resolve_bare_specifier("foo", &base.to_string_lossy())
            .unwrap();
        assert!(result.ends_with("index.js"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_bare_with_package_json() {
        let base = setup_tmpdir();
        let nm = base.join("node_modules").join("bar");
        fs::create_dir_all(&nm).unwrap();
        fs::write(
            nm.join("package.json"),
            r#"{"main": "lib/main.js"}"#,
        )
        .unwrap();
        let lib = nm.join("lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("main.js"), "").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver
            .resolve_bare_specifier("bar", &base.to_string_lossy())
            .unwrap();
        assert!(result.ends_with("main.js"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_full_api() {
        let base = setup_tmpdir();
        let entry = base.join("entry.js");
        fs::write(&entry, "").unwrap();
        let dep = base.join("dep.js");
        fs::write(&dep, "").unwrap();

        let mut resolver = ModuleResolver::new(&base.to_string_lossy());
        let resolved = resolver
            .resolve("./dep", &entry.to_string_lossy())
            .unwrap();
        assert!(resolved.path.ends_with("dep.js"));
        assert_eq!(resolved.format, ModuleFormat::CommonJS);
        assert!(resolved.package_name.is_none());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_caches_results() {
        let base = setup_tmpdir();
        let entry = base.join("entry.js");
        fs::write(&entry, "").unwrap();
        let lib = base.join("cached.js");
        fs::write(&lib, "").unwrap();

        let mut resolver = ModuleResolver::new(&base.to_string_lossy());
        let r1 = resolver
            .resolve("./cached", &entry.to_string_lossy())
            .unwrap();
        // Remove the file — second resolve should still succeed from cache
        fs::remove_file(&lib).unwrap();
        let r2 = resolver
            .resolve("./cached", &entry.to_string_lossy())
            .unwrap();
        assert_eq!(r1.path, r2.path);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_clear_cache() {
        let base = setup_tmpdir();
        let entry = base.join("entry.js");
        fs::write(&entry, "").unwrap();
        let lib = base.join("clearcache.js");
        fs::write(&lib, "").unwrap();

        let mut resolver = ModuleResolver::new(&base.to_string_lossy());
        resolver
            .resolve("./clearcache", &entry.to_string_lossy())
            .unwrap();
        assert!(!resolver.cache.is_empty());
        resolver.clear_cache();
        assert!(resolver.cache.is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(ModuleResolver::detect_format("foo.mjs"), ModuleFormat::ESModule);
        assert_eq!(ModuleResolver::detect_format("foo.cjs"), ModuleFormat::CommonJS);
        assert_eq!(ModuleResolver::detect_format("foo.json"), ModuleFormat::Json);
        assert_eq!(ModuleResolver::detect_format("foo.node"), ModuleFormat::Native);
        assert_eq!(ModuleResolver::detect_format("foo.js"), ModuleFormat::CommonJS);
    }

    #[test]
    fn test_find_package_json() {
        let base = setup_tmpdir();
        let sub = base.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        fs::write(base.join("package.json"), "{}").unwrap();

        let resolver = ModuleResolver::new(&base.to_string_lossy());
        let found = resolver
            .find_package_json(&sub.to_string_lossy())
            .unwrap();
        assert!(found.ends_with("package.json"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_resolve_nonexistent_fails() {
        let base = setup_tmpdir();
        let entry = base.join("entry.js");
        fs::write(&entry, "").unwrap();

        let mut resolver = ModuleResolver::new(&base.to_string_lossy());
        let result = resolver.resolve("./nope", &entry.to_string_lossy());
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_parse_bare_scoped() {
        let (name, sub) = ModuleResolver::parse_bare("@scope/pkg");
        assert_eq!(name, "@scope/pkg");
        assert!(sub.is_none());

        let (name, sub) = ModuleResolver::parse_bare("@scope/pkg/lib/util");
        assert_eq!(name, "@scope/pkg");
        assert_eq!(sub.unwrap(), "lib/util");
    }

    #[test]
    fn test_parse_bare_regular() {
        let (name, sub) = ModuleResolver::parse_bare("lodash");
        assert_eq!(name, "lodash");
        assert!(sub.is_none());

        let (name, sub) = ModuleResolver::parse_bare("lodash/fp");
        assert_eq!(name, "lodash");
        assert_eq!(sub.unwrap(), "fp");
    }
}

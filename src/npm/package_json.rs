//! Full package.json representation with serde support
//!
//! Provides a comprehensive `PackageJson` type that covers the fields
//! commonly found in npm package manifests, including conditional exports,
//! bin entries, engines, and more.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full package.json representation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub package_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<PackageExports>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies", skip_serializing_if = "Option::is_none")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies", skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripts: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<serde_json::Value>,
}

/// Package exports field (supports simple and conditional forms)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PackageExports {
    /// Simple string export: `"exports": "./index.js"`
    Simple(String),
    /// Conditional exports: `"exports": { ".": { "import": "...", "require": "..." } }`
    Conditional(HashMap<String, serde_json::Value>),
}

/// `bin` field — either a single path or a name→path map
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinField {
    Single(String),
    Map(HashMap<String, String>),
}

impl PackageJson {
    /// Load a package.json from a file path
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::ModuleError(format!("Failed to read '{}': {}", path, e)))?;
        Self::from_json(&content)
    }

    /// Parse a package.json from a JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| Error::ModuleError(format!("Invalid package.json: {}", e)))
    }

    /// Resolve the package entry point.
    ///
    /// Priority: conditional exports → simple exports → `module` → `main`.
    pub fn resolve_main(&self) -> Option<&str> {
        if let Some(ref exports) = self.exports {
            match exports {
                PackageExports::Simple(s) => return Some(s.as_str()),
                PackageExports::Conditional(map) => {
                    // Try "." entry first (subpath root)
                    if let Some(dot) = map.get(".") {
                        if let Some(s) = dot.as_str() {
                            return Some(s);
                        }
                        if let Some(obj) = dot.as_object() {
                            for key in &["import", "require", "default"] {
                                if let Some(val) = obj.get(*key) {
                                    if let Some(s) = val.as_str() {
                                        return Some(s);
                                    }
                                }
                            }
                        }
                    }
                    // Top-level conditional (import/require/default)
                    for key in &["import", "require", "default"] {
                        if let Some(val) = map.get(*key) {
                            if let Some(s) = val.as_str() {
                                return Some(s);
                            }
                        }
                    }
                }
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

    /// Check if the package declares `"type": "module"` (ESM)
    pub fn is_esm(&self) -> bool {
        self.package_type.as_deref() == Some("module")
    }

    /// Merge all dependency types into a single map.
    ///
    /// `dependencies` < `devDependencies` < `peerDependencies`
    /// (later overwrites earlier on conflict).
    pub fn all_dependencies(&self) -> HashMap<String, String> {
        let mut all = HashMap::new();
        if let Some(ref deps) = self.dependencies {
            all.extend(deps.clone());
        }
        if let Some(ref deps) = self.dev_dependencies {
            all.extend(deps.clone());
        }
        if let Some(ref deps) = self.peer_dependencies {
            all.extend(deps.clone());
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_package_json() {
        let json = r#"{
            "name": "my-app",
            "version": "2.1.0",
            "description": "An example app",
            "main": "dist/index.js",
            "module": "dist/esm/index.js",
            "types": "dist/index.d.ts",
            "type": "module",
            "license": "MIT",
            "keywords": ["example", "app"],
            "files": ["dist"],
            "scripts": {
                "build": "tsc",
                "test": "jest"
            },
            "dependencies": {
                "express": "^4.18.2"
            },
            "devDependencies": {
                "typescript": "^5.0.0"
            },
            "peerDependencies": {
                "react": ">=18.0.0"
            },
            "engines": {
                "node": ">=18"
            }
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.name.as_deref(), Some("my-app"));
        assert_eq!(pkg.version.as_deref(), Some("2.1.0"));
        assert_eq!(pkg.description.as_deref(), Some("An example app"));
        assert_eq!(pkg.license.as_deref(), Some("MIT"));
        assert!(pkg.is_esm());
        assert_eq!(pkg.keywords.as_ref().unwrap().len(), 2);
        assert!(pkg.scripts.as_ref().unwrap().contains_key("build"));
        assert!(pkg.engines.as_ref().unwrap().contains_key("node"));
    }

    #[test]
    fn test_resolve_main_simple_exports() {
        let json = r#"{
            "name": "simple",
            "exports": "./lib/index.js"
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.resolve_main(), Some("./lib/index.js"));
    }

    #[test]
    fn test_resolve_main_conditional_exports() {
        let json = r#"{
            "name": "cond",
            "exports": {
                "import": "./esm/index.js",
                "require": "./cjs/index.js"
            }
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.resolve_main(), Some("./esm/index.js"));
    }

    #[test]
    fn test_resolve_main_dot_conditional_exports() {
        let json = r#"{
            "name": "dotcond",
            "exports": {
                ".": {
                    "import": "./esm.js",
                    "require": "./cjs.js"
                }
            }
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.resolve_main(), Some("./esm.js"));
    }

    #[test]
    fn test_resolve_main_module_fallback() {
        let json = r#"{
            "name": "modonly",
            "module": "./esm/index.js",
            "main": "./cjs/index.js"
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.resolve_main(), Some("./esm/index.js"));
    }

    #[test]
    fn test_resolve_main_main_fallback() {
        let json = r#"{
            "name": "mainonly",
            "main": "./index.js"
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        assert_eq!(pkg.resolve_main(), Some("./index.js"));
    }

    #[test]
    fn test_is_esm() {
        let esm: PackageJson = serde_json::from_str(r#"{"type": "module"}"#).unwrap();
        assert!(esm.is_esm());

        let cjs: PackageJson = serde_json::from_str(r#"{"type": "commonjs"}"#).unwrap();
        assert!(!cjs.is_esm());

        let none: PackageJson = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!none.is_esm());
    }

    #[test]
    fn test_all_dependencies() {
        let json = r#"{
            "dependencies": { "a": "1.0.0" },
            "devDependencies": { "b": "2.0.0" },
            "peerDependencies": { "c": "3.0.0" }
        }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        let all = pkg.all_dependencies();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get("a").unwrap(), "1.0.0");
        assert_eq!(all.get("b").unwrap(), "2.0.0");
        assert_eq!(all.get("c").unwrap(), "3.0.0");
    }

    #[test]
    fn test_bin_field_single() {
        let json = r#"{ "bin": "./cli.js" }"#;
        let pkg: PackageJson = serde_json::from_str(json).unwrap();
        assert!(matches!(pkg.bin, Some(BinField::Single(_))));
    }

    #[test]
    fn test_bin_field_map() {
        let json = r#"{ "bin": { "mycli": "./cli.js", "helper": "./helper.js" } }"#;
        let pkg: PackageJson = serde_json::from_str(json).unwrap();
        if let Some(BinField::Map(ref m)) = pkg.bin {
            assert_eq!(m.len(), 2);
            assert!(m.contains_key("mycli"));
        } else {
            panic!("expected BinField::Map");
        }
    }

    #[test]
    fn test_save_and_load() {
        let json = r#"{ "name": "roundtrip", "version": "1.0.0", "main": "index.js" }"#;
        let pkg = PackageJson::from_json(json).unwrap();
        let tmp = std::env::temp_dir().join("quicksilver_test_pkg.json");
        let path = tmp.to_string_lossy().to_string();
        std::fs::write(&path, json).unwrap();
        let loaded = PackageJson::load(&path).unwrap();
        assert_eq!(loaded.name, pkg.name);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_from_json_invalid() {
        assert!(PackageJson::from_json("not json at all").is_err());
    }

    #[test]
    fn test_empty_package_json() {
        let pkg = PackageJson::from_json("{}").unwrap();
        assert!(pkg.name.is_none());
        assert!(pkg.resolve_main().is_none());
        assert!(!pkg.is_esm());
        assert!(pkg.all_dependencies().is_empty());
    }
}

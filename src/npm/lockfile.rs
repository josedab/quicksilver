//! Package lock file for deterministic installs
//!
//! Provides serialization and validation of package lock files
//! for reproducible dependency resolution in Quicksilver.

use crate::error::{Error, Result};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

/// Package lock file for deterministic installs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLock {
    pub name: String,
    pub version: String,
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,
    pub packages: HashMap<String, LockedPackage>,
}

/// A single locked package entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub dev: bool,
}

impl PackageLock {
    /// Create a new empty package lock
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            lockfile_version: 3,
            packages: HashMap::default(),
        }
    }

    /// Add or update a package in the lock file
    pub fn add_package(&mut self, name: &str, pkg: LockedPackage) {
        self.packages.insert(name.to_string(), pkg);
    }

    /// Look up a locked package by name
    pub fn get_package(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.get(name)
    }

    /// Save the lock file to disk as JSON
    pub fn save(&self, path: &str) -> Result<()> {
        let json = self.to_json();
        std::fs::write(path, json)
            .map_err(|e| Error::ModuleError(format!("Failed to write lock file '{}': {}", path, e)))
    }

    /// Load a lock file from disk
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::ModuleError(format!("Failed to read lock file '{}': {}", path, e)))?;
        Self::from_json(&content)
    }

    /// Serialize to a JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Deserialize from a JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| Error::ModuleError(format!("Invalid lock file JSON: {}", e)))
    }

    /// Validate that all dependency references within the lock file
    /// resolve to packages that are actually present with matching versions.
    pub fn is_consistent(&self) -> bool {
        for pkg in self.packages.values() {
            for (dep_name, dep_version) in &pkg.dependencies {
                match self.packages.get(dep_name.as_str()) {
                    Some(resolved) if resolved.version == *dep_version => {}
                    _ => return false,
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_lock_new() {
        let lock = PackageLock::new("my-app", "1.0.0");
        assert_eq!(lock.name, "my-app");
        assert_eq!(lock.version, "1.0.0");
        assert_eq!(lock.lockfile_version, 3);
        assert!(lock.packages.is_empty());
    }

    #[test]
    fn test_add_and_get_package() {
        let mut lock = PackageLock::new("app", "1.0.0");
        lock.add_package(
            "lodash",
            LockedPackage {
                version: "4.17.21".to_string(),
                resolved: Some("https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz".to_string()),
                integrity: Some("sha512-abc123".to_string()),
                dependencies: HashMap::default(),
                dev: false,
            },
        );
        let pkg = lock.get_package("lodash").unwrap();
        assert_eq!(pkg.version, "4.17.21");
        assert!(pkg.resolved.is_some());
        assert!(!pkg.dev);
        assert!(lock.get_package("nonexistent").is_none());
    }

    #[test]
    fn test_to_json_and_from_json() {
        let mut lock = PackageLock::new("app", "2.0.0");
        lock.add_package(
            "express",
            LockedPackage {
                version: "4.18.2".to_string(),
                resolved: None,
                integrity: None,
                dependencies: HashMap::default(),
                dev: false,
            },
        );
        let json = lock.to_json();
        assert!(json.contains("express"));
        assert!(json.contains("4.18.2"));

        let restored = PackageLock::from_json(&json).unwrap();
        assert_eq!(restored.name, "app");
        assert_eq!(restored.version, "2.0.0");
        assert!(restored.get_package("express").is_some());
    }

    #[test]
    fn test_save_and_load() {
        let mut lock = PackageLock::new("test-app", "0.1.0");
        lock.add_package(
            "react",
            LockedPackage {
                version: "18.2.0".to_string(),
                resolved: Some("https://registry.npmjs.org/react/-/react-18.2.0.tgz".to_string()),
                integrity: Some("sha512-xyz".to_string()),
                dependencies: HashMap::default(),
                dev: false,
            },
        );

        let tmp = std::env::temp_dir().join("quicksilver_test_lockfile.json");
        let path = tmp.to_string_lossy().to_string();
        lock.save(&path).unwrap();
        let loaded = PackageLock::load(&path).unwrap();
        assert_eq!(loaded.name, "test-app");
        assert!(loaded.get_package("react").is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_consistency_check_valid() {
        let mut lock = PackageLock::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("b".to_string(), "2.0.0".to_string());
        lock.add_package(
            "a",
            LockedPackage {
                version: "1.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: deps,
                dev: false,
            },
        );
        lock.add_package(
            "b",
            LockedPackage {
                version: "2.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: HashMap::default(),
                dev: false,
            },
        );
        assert!(lock.is_consistent());
    }

    #[test]
    fn test_consistency_check_missing_dep() {
        let mut lock = PackageLock::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("missing".to_string(), "1.0.0".to_string());
        lock.add_package(
            "a",
            LockedPackage {
                version: "1.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: deps,
                dev: false,
            },
        );
        assert!(!lock.is_consistent());
    }

    #[test]
    fn test_consistency_check_version_mismatch() {
        let mut lock = PackageLock::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("b".to_string(), "3.0.0".to_string());
        lock.add_package(
            "a",
            LockedPackage {
                version: "1.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: deps,
                dev: false,
            },
        );
        lock.add_package(
            "b",
            LockedPackage {
                version: "2.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: HashMap::default(),
                dev: false,
            },
        );
        assert!(!lock.is_consistent());
    }

    #[test]
    fn test_from_json_invalid() {
        assert!(PackageLock::from_json("not json").is_err());
    }

    #[test]
    fn test_dev_dependency_flag() {
        let mut lock = PackageLock::new("app", "1.0.0");
        lock.add_package(
            "jest",
            LockedPackage {
                version: "29.0.0".to_string(),
                resolved: None,
                integrity: None,
                dependencies: HashMap::default(),
                dev: true,
            },
        );
        assert!(lock.get_package("jest").unwrap().dev);
    }
}

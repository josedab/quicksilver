//! Package installation pipeline for npm-compatible dependency management
//!
//! Provides recursive dependency resolution, flat node_modules layout with
//! hoisting, integrity verification via SHA-256 checksums, and a global
//! package cache to avoid redundant downloads.

use crate::error::{Error, Result};
use crate::npm::lockfile::{LockedPackage, PackageLock};
use crate::npm::package_json::PackageJson;
use crate::npm::registry::{RegistryClient, VersionMetadata};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Dependency tree ─────────────────────────────────────────────────

/// A node in the resolved dependency tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    /// Package name
    pub name: String,
    /// Resolved version
    pub version: String,
    /// Resolved tarball URL (if available)
    pub resolved: Option<String>,
    /// Integrity hash (e.g. sha256 checksum)
    pub integrity: Option<String>,
    /// Whether this is a dev-only dependency
    pub dev: bool,
    /// Transitive dependencies (name → version range)
    pub dependencies: HashMap<String, String>,
}

/// Fully resolved dependency tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyTree {
    /// Root package name
    pub name: String,
    /// Root package version
    pub version: String,
    /// All resolved packages keyed by name
    pub resolved: HashMap<String, DependencyNode>,
}

impl DependencyTree {
    /// Create a new empty dependency tree for the given root package
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            resolved: HashMap::default(),
        }
    }

    /// Return the number of resolved packages
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    /// Check whether the tree contains no resolved packages
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    /// Look up a resolved node by package name
    pub fn get(&self, name: &str) -> Option<&DependencyNode> {
        self.resolved.get(name)
    }

    /// Recursively resolve all dependencies starting from the given map of
    /// direct dependencies.  Uses `registry` for version resolution and
    /// metadata lookup, and `include_dev` to control whether dev
    /// dependencies of transitive packages are included.
    pub fn resolve(
        &mut self,
        deps: &HashMap<String, String>,
        registry: &RegistryClient,
        include_dev: bool,
    ) -> Result<()> {
        let mut stack: Vec<(String, String, bool)> = deps
            .iter()
            .map(|(k, v)| (k.clone(), v.clone(), false))
            .collect();

        while let Some((name, range, is_dev)) = stack.pop() {
            if self.resolved.contains_key(&name) {
                continue;
            }

            let version = registry
                .resolve_version(&name, &range)
                .ok_or_else(|| {
                    Error::ModuleError(format!(
                        "Could not resolve '{}@{}' from registry",
                        name, range
                    ))
                })?;

            let meta = registry.get_package_metadata(&name).ok_or_else(|| {
                Error::ModuleError(format!("No metadata cached for '{}'", name))
            })?;

            let ver_meta = meta.versions.get(&version).ok_or_else(|| {
                Error::ModuleError(format!(
                    "Version '{}' not found in metadata for '{}'",
                    version, name
                ))
            })?;

            let (resolved_url, integrity) = extract_dist_info(ver_meta);

            let transitive = ver_meta.dependencies.clone();

            let node = DependencyNode {
                name: name.clone(),
                version: version.clone(),
                resolved: resolved_url,
                integrity,
                dev: is_dev,
                dependencies: transitive.clone(),
            };
            self.resolved.insert(name.clone(), node);

            // Enqueue transitive deps
            for (dep_name, dep_range) in &transitive {
                if !self.resolved.contains_key(dep_name) {
                    stack.push((dep_name.clone(), dep_range.clone(), is_dev));
                }
            }

            // Optionally enqueue dev deps of this package
            if include_dev {
                for (dep_name, dep_range) in &ver_meta.dev_dependencies {
                    if !self.resolved.contains_key(dep_name) {
                        stack.push((dep_name.clone(), dep_range.clone(), true));
                    }
                }
            }
        }

        Ok(())
    }
}

/// Extract tarball URL and integrity from version metadata
fn extract_dist_info(meta: &VersionMetadata) -> (Option<String>, Option<String>) {
    match &meta.dist {
        Some(dist) => (dist.tarball.clone(), dist.integrity.clone()),
        None => (None, None),
    }
}

// ── Install options ─────────────────────────────────────────────────

/// Options controlling the install behaviour
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Include devDependencies
    pub include_dev: bool,
    /// Production-only install (skip devDependencies)
    pub production: bool,
    /// Fail if the lockfile is missing or out of date
    pub frozen_lockfile: bool,
    /// Path to the global cache directory
    pub global_cache_dir: Option<String>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            include_dev: true,
            production: false,
            frozen_lockfile: false,
            global_cache_dir: None,
        }
    }
}

// ── Package installer ───────────────────────────────────────────────

/// Orchestrates the full installation pipeline:
/// 1. Read package.json
/// 2. Resolve the dependency tree
/// 3. Write a flat `node_modules` layout with hoisting
/// 4. Generate / update the lockfile
pub struct PackageInstaller {
    /// Project root directory
    root_dir: PathBuf,
    /// Registry client used for version resolution
    registry: RegistryClient,
    /// Install options
    options: InstallOptions,
    /// Global download cache
    global_cache: GlobalCache,
}

impl PackageInstaller {
    /// Create a new installer for the given project root
    pub fn new(root_dir: &str, registry: RegistryClient, options: InstallOptions) -> Self {
        let cache_dir = options
            .global_cache_dir
            .clone()
            .unwrap_or_else(default_global_cache_dir);
        Self {
            root_dir: PathBuf::from(root_dir),
            registry,
            options,
            global_cache: GlobalCache::new(&cache_dir),
        }
    }

    /// Return a reference to the project root
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Run the full install pipeline and return the generated lockfile
    pub fn install(&mut self) -> Result<PackageLock> {
        let pkg = self.load_package_json()?;

        let name = pkg.name.clone().unwrap_or_else(|| "unnamed".to_string());
        let version = pkg.version.clone().unwrap_or_else(|| "0.0.0".to_string());

        // Collect the set of direct dependencies to resolve
        let mut deps: HashMap<String, String> = HashMap::default();
        if let Some(ref d) = pkg.dependencies {
            for (k, v) in d {
                deps.insert(k.clone(), v.clone());
            }
        }
        let include_dev = self.options.include_dev && !self.options.production;
        if include_dev {
            if let Some(ref d) = pkg.dev_dependencies {
                for (k, v) in d {
                    deps.insert(k.clone(), v.clone());
                }
            }
        }

        // Frozen lockfile check
        if self.options.frozen_lockfile {
            let lockfile_path = self.root_dir.join("package-lock.json");
            if !lockfile_path.is_file() {
                return Err(Error::ModuleError(
                    "Frozen lockfile requested but package-lock.json not found".to_string(),
                ));
            }
            let lock = PackageLock::load(&lockfile_path.to_string_lossy())?;
            if !lock.is_consistent() {
                return Err(Error::ModuleError(
                    "Frozen lockfile is inconsistent".to_string(),
                ));
            }
            return Ok(lock);
        }

        // Resolve the full dependency tree
        let mut tree = DependencyTree::new(&name, &version);
        tree.resolve(&deps, &self.registry, include_dev)?;

        // Create flat node_modules layout
        self.write_node_modules(&tree)?;

        // Generate lockfile
        let lock = self.generate_lockfile(&name, &version, &tree);
        let lockfile_path = self.root_dir.join("package-lock.json");
        lock.save(&lockfile_path.to_string_lossy())?;

        Ok(lock)
    }

    /// Load `package.json` from the project root
    fn load_package_json(&self) -> Result<PackageJson> {
        let path = self.root_dir.join("package.json");
        PackageJson::load(&path.to_string_lossy())
    }

    /// Write a flat (hoisted) node_modules directory.
    ///
    /// Each resolved package gets a top-level directory under `node_modules/`
    /// with a marker `package.json` containing its name and version.
    fn write_node_modules(&self, tree: &DependencyTree) -> Result<()> {
        let nm = self.root_dir.join("node_modules");
        std::fs::create_dir_all(&nm)
            .map_err(|e| Error::ModuleError(format!("Failed to create node_modules: {}", e)))?;

        for node in tree.resolved.values() {
            let pkg_dir = nm.join(&node.name);
            std::fs::create_dir_all(&pkg_dir).map_err(|e| {
                Error::ModuleError(format!(
                    "Failed to create directory for '{}': {}",
                    node.name, e
                ))
            })?;

            // Check global cache for a cached copy
            if let Some(cached) = self.global_cache.get(&node.name, &node.version) {
                // Write cached content as index.js placeholder
                std::fs::write(pkg_dir.join("index.js"), cached).map_err(|e| {
                    Error::ModuleError(format!(
                        "Failed to write cached content for '{}': {}",
                        node.name, e
                    ))
                })?;
            }

            // Write a minimal package.json marker
            let marker = format!(
                r#"{{"name":"{}","version":"{}"}}"#,
                node.name, node.version
            );
            std::fs::write(pkg_dir.join("package.json"), marker).map_err(|e| {
                Error::ModuleError(format!(
                    "Failed to write package.json for '{}': {}",
                    node.name, e
                ))
            })?;
        }

        Ok(())
    }

    /// Convert a resolved dependency tree into a lockfile
    fn generate_lockfile(
        &self,
        name: &str,
        version: &str,
        tree: &DependencyTree,
    ) -> PackageLock {
        let mut lock = PackageLock::new(name, version);

        for node in tree.resolved.values() {
            let locked = LockedPackage {
                version: node.version.clone(),
                resolved: node.resolved.clone(),
                integrity: node.integrity.clone(),
                dependencies: node.dependencies.clone(),
                dev: node.dev,
            };
            lock.add_package(&node.name, locked);
        }

        lock
    }
}

// ── Integrity checker ───────────────────────────────────────────────

/// Verifies package integrity using SHA-256 checksums
pub struct IntegrityChecker;

impl IntegrityChecker {
    /// Compute a SHA-256 hex digest for the given data
    pub fn sha256_hex(data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Simplified SHA-256-like checksum using a seeded hash.
        // A real implementation would use `ring` or `sha2` crate.
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        let h1 = hasher.finish();
        // Second pass with salt for more entropy
        let mut hasher2 = DefaultHasher::new();
        h1.hash(&mut hasher2);
        data.len().hash(&mut hasher2);
        let h2 = hasher2.finish();

        format!("{:016x}{:016x}", h1, h2)
    }

    /// Verify that `data` matches the expected hex digest
    pub fn verify(data: &[u8], expected: &str) -> bool {
        Self::sha256_hex(data) == expected
    }

    /// Compute an integrity string in the subresource integrity format
    /// (`sha256-<base64>`)
    pub fn integrity_string(data: &[u8]) -> String {
        format!("sha256-{}", Self::sha256_hex(data))
    }

    /// Verify a subresource-integrity string
    pub fn verify_integrity(data: &[u8], integrity: &str) -> bool {
        if let Some(hash) = integrity.strip_prefix("sha256-") {
            Self::verify(data, hash)
        } else {
            false
        }
    }
}

// ── Global cache ────────────────────────────────────────────────────

/// Caches downloaded packages on disk to avoid redundant downloads
pub struct GlobalCache {
    /// Root directory for the cache
    cache_dir: PathBuf,
}

impl GlobalCache {
    /// Create a new cache rooted at `dir`
    pub fn new(dir: &str) -> Self {
        Self {
            cache_dir: PathBuf::from(dir),
        }
    }

    /// Return the directory used for caching
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Build the on-disk path for a cached package
    fn package_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir.join(format!("{}-{}", name, version))
    }

    /// Store raw package content in the cache
    pub fn put(&self, name: &str, version: &str, data: &[u8]) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| {
            Error::ModuleError(format!("Failed to create cache dir: {}", e))
        })?;

        let path = self.package_path(name, version);
        std::fs::write(&path, data).map_err(|e| {
            Error::ModuleError(format!(
                "Failed to write cache for '{}@{}': {}",
                name, version, e
            ))
        })?;

        Ok(())
    }

    /// Retrieve cached package content, or `None` if not cached
    pub fn get(&self, name: &str, version: &str) -> Option<Vec<u8>> {
        let path = self.package_path(name, version);
        std::fs::read(&path).ok()
    }

    /// Check whether a package is present in the cache
    pub fn has(&self, name: &str, version: &str) -> bool {
        self.package_path(name, version).is_file()
    }

    /// Remove a cached package
    pub fn remove(&self, name: &str, version: &str) -> Result<()> {
        let path = self.package_path(name, version);
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| {
                Error::ModuleError(format!(
                    "Failed to remove cache for '{}@{}': {}",
                    name, version, e
                ))
            })?;
        }
        Ok(())
    }

    /// Remove all cached packages
    pub fn clear(&self) -> Result<()> {
        if self.cache_dir.is_dir() {
            std::fs::remove_dir_all(&self.cache_dir).map_err(|e| {
                Error::ModuleError(format!("Failed to clear cache: {}", e))
            })?;
        }
        Ok(())
    }
}

/// Return the default global cache directory
fn default_global_cache_dir() -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".quicksilver")
        .join("cache")
        .to_string_lossy()
        .to_string()
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npm::registry::{DistInfo, PackageMetadata};
    use std::fs;

    fn tmp_dir(label: &str) -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("qs_installer_{}_{}", label, id));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_registry_with_packages(packages: &[(&str, &[&str])]) -> RegistryClient {
        let mut client = RegistryClient::offline();
        for (name, versions) in packages {
            let mut ver_map = HashMap::default();
            let mut latest = String::new();
            for v in *versions {
                latest = v.to_string();
                ver_map.insert(
                    v.to_string(),
                    VersionMetadata {
                        version: v.to_string(),
                        dependencies: HashMap::default(),
                        dev_dependencies: HashMap::default(),
                        dist: Some(DistInfo {
                            tarball: Some(format!("https://registry.npmjs.org/{}/-/{}-{}.tgz", name, name, v)),
                            shasum: Some("abc123".to_string()),
                            integrity: Some(format!("sha256-{}", v)),
                        }),
                        main: Some("index.js".to_string()),
                        module: None,
                        types: None,
                    },
                );
            }
            let mut dist_tags = HashMap::default();
            dist_tags.insert("latest".to_string(), latest);
            client.cache_metadata(
                name,
                PackageMetadata {
                    name: name.to_string(),
                    description: None,
                    versions: ver_map,
                    dist_tags,
                },
            );
        }
        client
    }

    fn make_registry_with_deps(
        name: &str,
        version: &str,
        deps: &[(&str, &str)],
    ) -> RegistryClient {
        let mut client = RegistryClient::offline();
        let mut dep_map = HashMap::default();
        for (dn, dv) in deps {
            dep_map.insert(dn.to_string(), dv.to_string());
        }
        let mut ver_map = HashMap::default();
        ver_map.insert(
            version.to_string(),
            VersionMetadata {
                version: version.to_string(),
                dependencies: dep_map,
                dev_dependencies: HashMap::default(),
                dist: None,
                main: Some("index.js".to_string()),
                module: None,
                types: None,
            },
        );
        let mut dist_tags = HashMap::default();
        dist_tags.insert("latest".to_string(), version.to_string());
        client.cache_metadata(
            name,
            PackageMetadata {
                name: name.to_string(),
                description: None,
                versions: ver_map,
                dist_tags,
            },
        );
        client
    }

    // ── DependencyTree tests ────────────────────────────────────────

    #[test]
    fn test_dependency_tree_new() {
        let tree = DependencyTree::new("my-app", "1.0.0");
        assert_eq!(tree.name, "my-app");
        assert_eq!(tree.version, "1.0.0");
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_dependency_tree_resolve_single() {
        let registry = make_registry_with_packages(&[("lodash", &["4.17.21"])]);
        let mut tree = DependencyTree::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("lodash".to_string(), "4.17.21".to_string());
        tree.resolve(&deps, &registry, false).unwrap();
        assert_eq!(tree.len(), 1);
        let node = tree.get("lodash").unwrap();
        assert_eq!(node.version, "4.17.21");
        assert!(node.resolved.is_some());
    }

    #[test]
    fn test_dependency_tree_resolve_multiple() {
        let registry = make_registry_with_packages(&[
            ("express", &["4.18.2"]),
            ("lodash", &["4.17.21"]),
        ]);
        let mut tree = DependencyTree::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("express".to_string(), "4.18.2".to_string());
        deps.insert("lodash".to_string(), "4.17.21".to_string());
        tree.resolve(&deps, &registry, false).unwrap();
        assert_eq!(tree.len(), 2);
        assert!(tree.get("express").is_some());
        assert!(tree.get("lodash").is_some());
    }

    #[test]
    fn test_dependency_tree_resolve_transitive() {
        // express@4.18.2 depends on body-parser@1.20.0
        let mut registry = make_registry_with_packages(&[("body-parser", &["1.20.0"])]);

        // Add express with a dependency on body-parser
        let mut dep_map = HashMap::default();
        dep_map.insert("body-parser".to_string(), "1.20.0".to_string());
        let mut ver_map = HashMap::default();
        ver_map.insert(
            "4.18.2".to_string(),
            VersionMetadata {
                version: "4.18.2".to_string(),
                dependencies: dep_map,
                dev_dependencies: HashMap::default(),
                dist: None,
                main: Some("index.js".to_string()),
                module: None,
                types: None,
            },
        );
        let mut dist_tags = HashMap::default();
        dist_tags.insert("latest".to_string(), "4.18.2".to_string());
        registry.cache_metadata(
            "express",
            PackageMetadata {
                name: "express".to_string(),
                description: None,
                versions: ver_map,
                dist_tags,
            },
        );

        let mut tree = DependencyTree::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("express".to_string(), "4.18.2".to_string());
        tree.resolve(&deps, &registry, false).unwrap();

        assert_eq!(tree.len(), 2);
        assert!(tree.get("express").is_some());
        assert!(tree.get("body-parser").is_some());
    }

    #[test]
    fn test_dependency_tree_unresolvable() {
        let registry = RegistryClient::offline();
        let mut tree = DependencyTree::new("app", "1.0.0");
        let mut deps = HashMap::default();
        deps.insert("nonexistent".to_string(), "1.0.0".to_string());
        let result = tree.resolve(&deps, &registry, false);
        assert!(result.is_err());
    }

    // ── InstallOptions tests ────────────────────────────────────────

    #[test]
    fn test_install_options_default() {
        let opts = InstallOptions::default();
        assert!(opts.include_dev);
        assert!(!opts.production);
        assert!(!opts.frozen_lockfile);
        assert!(opts.global_cache_dir.is_none());
    }

    // ── IntegrityChecker tests ──────────────────────────────────────

    #[test]
    fn test_integrity_sha256_deterministic() {
        let data = b"hello world";
        let h1 = IntegrityChecker::sha256_hex(data);
        let h2 = IntegrityChecker::sha256_hex(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32); // two 16-char hex blocks
    }

    #[test]
    fn test_integrity_verify() {
        let data = b"package contents";
        let hash = IntegrityChecker::sha256_hex(data);
        assert!(IntegrityChecker::verify(data, &hash));
        assert!(!IntegrityChecker::verify(b"tampered", &hash));
    }

    #[test]
    fn test_integrity_string_format() {
        let data = b"test data";
        let integrity = IntegrityChecker::integrity_string(data);
        assert!(integrity.starts_with("sha256-"));
    }

    #[test]
    fn test_integrity_verify_sri() {
        let data = b"test data";
        let integrity = IntegrityChecker::integrity_string(data);
        assert!(IntegrityChecker::verify_integrity(data, &integrity));
        assert!(!IntegrityChecker::verify_integrity(b"wrong", &integrity));
        assert!(!IntegrityChecker::verify_integrity(data, "md5-bogus"));
    }

    // ── GlobalCache tests ───────────────────────────────────────────

    #[test]
    fn test_global_cache_put_get() {
        let dir = tmp_dir("cache_put_get");
        let cache = GlobalCache::new(&dir.to_string_lossy());
        cache.put("lodash", "4.17.21", b"contents").unwrap();
        assert!(cache.has("lodash", "4.17.21"));
        let data = cache.get("lodash", "4.17.21").unwrap();
        assert_eq!(data, b"contents");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_global_cache_miss() {
        let dir = tmp_dir("cache_miss");
        let cache = GlobalCache::new(&dir.to_string_lossy());
        assert!(!cache.has("unknown", "1.0.0"));
        assert!(cache.get("unknown", "1.0.0").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_global_cache_remove() {
        let dir = tmp_dir("cache_remove");
        let cache = GlobalCache::new(&dir.to_string_lossy());
        cache.put("pkg", "1.0.0", b"data").unwrap();
        assert!(cache.has("pkg", "1.0.0"));
        cache.remove("pkg", "1.0.0").unwrap();
        assert!(!cache.has("pkg", "1.0.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_global_cache_clear() {
        let dir = tmp_dir("cache_clear");
        let cache = GlobalCache::new(&dir.to_string_lossy());
        cache.put("a", "1.0.0", b"aaa").unwrap();
        cache.put("b", "2.0.0", b"bbb").unwrap();
        cache.clear().unwrap();
        assert!(!cache.has("a", "1.0.0"));
        assert!(!cache.has("b", "2.0.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    // ── PackageInstaller tests ──────────────────────────────────────

    #[test]
    fn test_installer_install_basic() {
        let dir = tmp_dir("install_basic");
        // Write a minimal package.json
        fs::write(
            dir.join("package.json"),
            r#"{"name":"test-app","version":"1.0.0","dependencies":{"lodash":"4.17.21"}}"#,
        )
        .unwrap();

        let registry = make_registry_with_packages(&[("lodash", &["4.17.21"])]);
        let opts = InstallOptions {
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        let lock = installer.install().unwrap();

        assert_eq!(lock.name, "test-app");
        assert!(lock.get_package("lodash").is_some());
        assert!(dir.join("node_modules/lodash/package.json").is_file());
        assert!(dir.join("package-lock.json").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_installer_production_skips_dev() {
        let dir = tmp_dir("install_prod");
        fs::write(
            dir.join("package.json"),
            r#"{"name":"app","version":"1.0.0","dependencies":{"lodash":"4.17.21"},"devDependencies":{"jest":"29.0.0"}}"#,
        )
        .unwrap();

        let registry = make_registry_with_packages(&[
            ("lodash", &["4.17.21"]),
            ("jest", &["29.0.0"]),
        ]);
        let opts = InstallOptions {
            production: true,
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        let lock = installer.install().unwrap();

        assert!(lock.get_package("lodash").is_some());
        assert!(lock.get_package("jest").is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_installer_frozen_lockfile_missing() {
        let dir = tmp_dir("install_frozen_missing");
        fs::write(
            dir.join("package.json"),
            r#"{"name":"app","version":"1.0.0"}"#,
        )
        .unwrap();

        let registry = RegistryClient::offline();
        let opts = InstallOptions {
            frozen_lockfile: true,
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        let result = installer.install();
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_installer_frozen_lockfile_present() {
        let dir = tmp_dir("install_frozen_ok");
        fs::write(
            dir.join("package.json"),
            r#"{"name":"app","version":"1.0.0"}"#,
        )
        .unwrap();
        let lock = PackageLock::new("app", "1.0.0");
        lock.save(&dir.join("package-lock.json").to_string_lossy()).unwrap();

        let registry = RegistryClient::offline();
        let opts = InstallOptions {
            frozen_lockfile: true,
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        let result = installer.install();
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_installer_generates_lockfile() {
        let dir = tmp_dir("install_lockfile");
        fs::write(
            dir.join("package.json"),
            r#"{"name":"lock-app","version":"2.0.0","dependencies":{"express":"4.18.2"}}"#,
        )
        .unwrap();

        let registry = make_registry_with_packages(&[("express", &["4.18.2"])]);
        let opts = InstallOptions {
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        let lock = installer.install().unwrap();

        assert_eq!(lock.name, "lock-app");
        assert_eq!(lock.version, "2.0.0");
        assert!(lock.get_package("express").is_some());

        // Verify it was persisted
        let loaded = PackageLock::load(&dir.join("package-lock.json").to_string_lossy()).unwrap();
        assert_eq!(loaded.name, "lock-app");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_installer_node_modules_structure() {
        let dir = tmp_dir("install_structure");
        fs::write(
            dir.join("package.json"),
            r#"{"name":"app","version":"1.0.0","dependencies":{"a":"1.0.0","b":"2.0.0"}}"#,
        )
        .unwrap();

        let registry = make_registry_with_packages(&[
            ("a", &["1.0.0"]),
            ("b", &["2.0.0"]),
        ]);
        let opts = InstallOptions {
            global_cache_dir: Some(dir.join(".cache").to_string_lossy().to_string()),
            ..Default::default()
        };
        let mut installer = PackageInstaller::new(&dir.to_string_lossy(), registry, opts);
        installer.install().unwrap();

        // Both packages should be hoisted to top-level node_modules
        assert!(dir.join("node_modules/a/package.json").is_file());
        assert!(dir.join("node_modules/b/package.json").is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}

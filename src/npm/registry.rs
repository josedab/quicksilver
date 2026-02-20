//! NPM registry client for package resolution
//!
//! Provides an offline-capable registry client that caches package metadata
//! and resolves semver version ranges.

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

/// NPM registry client for package resolution
pub struct RegistryClient {
    registry_url: String,
    cache: HashMap<String, PackageMetadata>,
    offline: bool,
}

/// Package metadata from the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub versions: HashMap<String, VersionMetadata>,
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: HashMap<String, String>,
}

/// Metadata for a single package version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMetadata {
    pub version: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(rename = "devDependencies", default)]
    pub dev_dependencies: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dist: Option<DistInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
}

/// Distribution info for a package tarball
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

impl RegistryClient {
    /// Create a new registry client pointing at the given URL
    pub fn new(url: &str) -> Self {
        Self {
            registry_url: url.to_string(),
            cache: HashMap::default(),
            offline: false,
        }
    }

    /// Create a registry client in offline mode (cache only)
    pub fn offline() -> Self {
        Self {
            registry_url: String::new(),
            cache: HashMap::default(),
            offline: true,
        }
    }

    /// Return the configured registry URL
    pub fn registry_url(&self) -> &str {
        &self.registry_url
    }

    /// Return whether the client is in offline mode
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Resolve a semver range to the best matching version from cached metadata.
    ///
    /// Supports: exact (`1.2.3`), caret (`^1.2.3`), tilde (`~1.2.3`),
    /// `>=` prefixes, wildcard (`*`), and `latest` dist-tag.
    pub fn resolve_version(&self, name: &str, range: &str) -> Option<String> {
        let meta = self.cache.get(name)?;

        // Handle dist-tag references like "latest"
        if let Some(tagged) = meta.dist_tags.get(range) {
            if meta.versions.contains_key(tagged) {
                return Some(tagged.clone());
            }
        }

        // Wildcard — pick latest tag or highest version
        if range == "*" {
            if let Some(latest) = meta.dist_tags.get("latest") {
                return Some(latest.clone());
            }
            return Self::highest_version(meta);
        }

        let (prefix, base) = Self::parse_range(range);

        let (major, minor, patch) = Self::parse_semver(&base)?;

        let mut best: Option<(u64, u64, u64, String)> = None;

        for ver_str in meta.versions.keys() {
            if let Some((vm, vn, vp)) = Self::parse_semver(ver_str) {
                let matches = match prefix {
                    RangeOp::Exact => vm == major && vn == minor && vp == patch,
                    RangeOp::Caret => {
                        vm == major && (vm > 0 || vn == minor || (vn >= minor && vp >= patch))
                            && (vm, vn, vp) >= (major, minor, patch)
                    }
                    RangeOp::Tilde => {
                        vm == major && vn == minor && vp >= patch
                    }
                    RangeOp::Gte => (vm, vn, vp) >= (major, minor, patch),
                    RangeOp::Gt => (vm, vn, vp) > (major, minor, patch),
                };

                if matches {
                    if let Some(ref b) = best {
                        if (vm, vn, vp) > (b.0, b.1, b.2) {
                            best = Some((vm, vn, vp, ver_str.clone()));
                        }
                    } else {
                        best = Some((vm, vn, vp, ver_str.clone()));
                    }
                }
            }
        }

        best.map(|(_, _, _, v)| v)
    }

    /// Get cached metadata for a package
    pub fn get_package_metadata(&self, name: &str) -> Option<&PackageMetadata> {
        self.cache.get(name)
    }

    /// Store metadata in the local cache
    pub fn cache_metadata(&mut self, name: &str, metadata: PackageMetadata) {
        self.cache.insert(name.to_string(), metadata);
    }

    /// Check if metadata for a package is already cached
    pub fn is_cached(&self, name: &str) -> bool {
        self.cache.contains_key(name)
    }

    // ── helpers ──────────────────────────────────────────────────────

    fn highest_version(meta: &PackageMetadata) -> Option<String> {
        let mut best: Option<(u64, u64, u64, String)> = None;
        for ver_str in meta.versions.keys() {
            if let Some((m, n, p)) = Self::parse_semver(ver_str) {
                if best.as_ref().is_none_or(|b| (m, n, p) > (b.0, b.1, b.2)) {
                    best = Some((m, n, p, ver_str.clone()));
                }
            }
        }
        best.map(|(_, _, _, v)| v)
    }

    fn parse_range(range: &str) -> (RangeOp, String) {
        if let Some(rest) = range.strip_prefix(">=") {
            (RangeOp::Gte, rest.to_string())
        } else if let Some(rest) = range.strip_prefix('>') {
            (RangeOp::Gt, rest.to_string())
        } else if let Some(rest) = range.strip_prefix('^') {
            (RangeOp::Caret, rest.to_string())
        } else if let Some(rest) = range.strip_prefix('~') {
            (RangeOp::Tilde, rest.to_string())
        } else {
            (RangeOp::Exact, range.to_string())
        }
    }

    fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() < 3 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        // Strip any pre-release suffix (e.g. "0-beta.1")
        let patch_str = parts[2].split('-').next().unwrap_or(parts[2]);
        let patch = patch_str.parse().ok()?;
        Some((major, minor, patch))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RangeOp {
    Exact,
    Caret,
    Tilde,
    Gte,
    Gt,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> PackageMetadata {
        let mut versions = HashMap::default();
        for v in &["4.17.20", "4.17.21", "4.18.0", "5.0.0"] {
            versions.insert(
                v.to_string(),
                VersionMetadata {
                    version: v.to_string(),
                    dependencies: HashMap::default(),
                    dev_dependencies: HashMap::default(),
                    dist: None,
                    main: Some("index.js".to_string()),
                    module: None,
                    types: None,
                },
            );
        }
        let mut dist_tags = HashMap::default();
        dist_tags.insert("latest".to_string(), "4.17.21".to_string());
        PackageMetadata {
            name: "lodash".to_string(),
            description: Some("Utility library".to_string()),
            versions,
            dist_tags,
        }
    }

    #[test]
    fn test_new_client() {
        let client = RegistryClient::new("https://registry.npmjs.org");
        assert_eq!(client.registry_url(), "https://registry.npmjs.org");
        assert!(!client.is_offline());
    }

    #[test]
    fn test_offline_client() {
        let client = RegistryClient::offline();
        assert!(client.is_offline());
        assert!(client.registry_url().is_empty());
    }

    #[test]
    fn test_cache_metadata_and_lookup() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        assert!(!client.is_cached("lodash"));
        client.cache_metadata("lodash", sample_metadata());
        assert!(client.is_cached("lodash"));
        let meta = client.get_package_metadata("lodash").unwrap();
        assert_eq!(meta.name, "lodash");
        assert!(meta.versions.contains_key("4.17.21"));
    }

    #[test]
    fn test_resolve_exact_version() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        assert_eq!(
            client.resolve_version("lodash", "4.17.21"),
            Some("4.17.21".to_string())
        );
        assert_eq!(client.resolve_version("lodash", "9.9.9"), None);
    }

    #[test]
    fn test_resolve_caret_version() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        let resolved = client.resolve_version("lodash", "^4.17.20").unwrap();
        // Should pick the highest 4.x match
        assert_eq!(resolved, "4.18.0");
    }

    #[test]
    fn test_resolve_tilde_version() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        let resolved = client.resolve_version("lodash", "~4.17.20").unwrap();
        // Tilde: same major.minor, patch >= base
        assert_eq!(resolved, "4.17.21");
    }

    #[test]
    fn test_resolve_gte_version() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        let resolved = client.resolve_version("lodash", ">=4.18.0").unwrap();
        assert_eq!(resolved, "5.0.0");
    }

    #[test]
    fn test_resolve_wildcard() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        let resolved = client.resolve_version("lodash", "*").unwrap();
        // Wildcard picks the "latest" dist-tag
        assert_eq!(resolved, "4.17.21");
    }

    #[test]
    fn test_resolve_dist_tag() {
        let mut client = RegistryClient::new("https://registry.npmjs.org");
        client.cache_metadata("lodash", sample_metadata());
        let resolved = client.resolve_version("lodash", "latest").unwrap();
        assert_eq!(resolved, "4.17.21");
    }

    #[test]
    fn test_resolve_unknown_package() {
        let client = RegistryClient::new("https://registry.npmjs.org");
        assert_eq!(client.resolve_version("unknown-pkg", "1.0.0"), None);
    }

    #[test]
    fn test_version_metadata_dist_info() {
        let dist = DistInfo {
            tarball: Some("https://example.com/pkg.tgz".to_string()),
            shasum: Some("abc123".to_string()),
            integrity: Some("sha512-xyz".to_string()),
        };
        let vm = VersionMetadata {
            version: "1.0.0".to_string(),
            dependencies: HashMap::default(),
            dev_dependencies: HashMap::default(),
            dist: Some(dist),
            main: Some("index.js".to_string()),
            module: Some("esm/index.js".to_string()),
            types: Some("index.d.ts".to_string()),
        };
        assert_eq!(vm.dist.as_ref().unwrap().shasum.as_deref(), Some("abc123"));
        assert_eq!(vm.module.as_deref(), Some("esm/index.js"));
    }
}

//! Permission Manifest for Declarative Security Configuration
//!
//! Loads and validates a `quicksilver.json` manifest that declares the
//! capabilities an application requires.  The manifest is converted into
//! a [`SandboxConfig`] at startup so that all permission decisions are
//! resolved before any user code executes.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::sandbox::SandboxConfig;

/// Current manifest schema version.
const CURRENT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// PermissionManifest
// ---------------------------------------------------------------------------

/// Permission manifest for declarative security configuration.
///
/// Typically stored as `quicksilver.json` alongside the application entry point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionManifest {
    /// Schema version.
    pub version: u32,
    /// Application name.
    pub name: Option<String>,
    /// Permission grants.
    pub permissions: ManifestPermissions,
    /// Resource limits.
    pub limits: Option<ManifestLimits>,
}

/// Declared permission grants.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestPermissions {
    /// File read paths (glob patterns).
    pub file_read: Option<Vec<String>>,
    /// File write paths (glob patterns).
    pub file_write: Option<Vec<String>>,
    /// Network hosts allowed.
    pub network: Option<Vec<String>>,
    /// Environment variables allowed.
    pub env: Option<Vec<String>>,
    /// Allow subprocess execution.
    pub subprocess: Option<bool>,
    /// Allow dynamic code eval.
    pub dynamic_code: Option<bool>,
    /// Allow FFI.
    pub ffi: Option<bool>,
    /// Allow high-resolution timers.
    pub high_res_time: Option<bool>,
}

/// Optional resource limits declared in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestLimits {
    /// Maximum memory in megabytes.
    pub max_memory_mb: Option<u64>,
    /// Maximum execution time in milliseconds.
    pub max_execution_time_ms: Option<u64>,
    /// Maximum call-stack depth.
    pub max_stack_depth: Option<usize>,
}

impl PermissionManifest {
    /// Load a manifest from a JSON file at `path`.
    pub fn load(path: &str) -> crate::error::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&contents).map_err(|e| {
            crate::error::Error::InternalError(format!("Invalid manifest: {}", e))
        })?;
        Ok(manifest)
    }

    /// Validate the manifest and return a list of human-readable warnings.
    ///
    /// An `Err` is returned for hard errors (e.g. unsupported version).
    /// Warnings are advisory — the manifest is still usable.
    pub fn validate(&self) -> crate::error::Result<Vec<String>> {
        let mut warnings = Vec::new();

        if self.version != CURRENT_VERSION {
            return Err(crate::error::Error::InternalError(format!(
                "Unsupported manifest version: {} (expected {})",
                self.version, CURRENT_VERSION
            )));
        }

        if self.name.is_none() {
            warnings.push("Manifest has no 'name' field".to_string());
        }

        if let Some(ref read) = self.permissions.file_read {
            if read.iter().any(|p| p == "*" || p == "**") {
                warnings.push("file_read contains wildcard '*' — grants read to all files".to_string());
            }
        }

        if let Some(ref write) = self.permissions.file_write {
            if write.iter().any(|p| p == "*" || p == "**") {
                warnings.push("file_write contains wildcard '*' — grants write to all files".to_string());
            }
        }

        if let Some(ref net) = self.permissions.network {
            if net.iter().any(|h| h == "*") {
                warnings.push("network contains wildcard '*' — grants access to all hosts".to_string());
            }
        }

        if let Some(ref limits) = self.limits {
            if let Some(mem) = limits.max_memory_mb {
                if mem == 0 {
                    warnings.push("max_memory_mb is 0 — execution will likely fail".to_string());
                }
            }
            if let Some(time) = limits.max_execution_time_ms {
                if time == 0 {
                    warnings.push("max_execution_time_ms is 0 — execution will likely fail".to_string());
                }
            }
            if let Some(depth) = limits.max_stack_depth {
                if depth == 0 {
                    warnings.push("max_stack_depth is 0 — execution will likely fail".to_string());
                }
            }
        }

        Ok(warnings)
    }

    /// Convert the manifest into a [`SandboxConfig`] suitable for runtime use.
    pub fn to_sandbox_config(&self) -> SandboxConfig {
        let mut config = SandboxConfig::default();

        if let Some(ref limits) = self.limits {
            if let Some(mem) = limits.max_memory_mb {
                config.max_memory = (mem as usize) * 1024 * 1024;
            }
            if let Some(time) = limits.max_execution_time_ms {
                config.max_duration = Duration::from_millis(time);
            }
        }

        config
    }

    /// Return a deny-all default manifest (no permissions granted).
    pub fn default_manifest() -> Self {
        Self {
            version: CURRENT_VERSION,
            name: None,
            permissions: ManifestPermissions::default(),
            limits: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_manifest_denies_all() {
        let m = PermissionManifest::default_manifest();
        assert_eq!(m.version, CURRENT_VERSION);
        assert!(m.name.is_none());
        assert!(m.permissions.file_read.is_none());
        assert!(m.permissions.file_write.is_none());
        assert!(m.permissions.network.is_none());
        assert!(m.permissions.env.is_none());
        assert!(m.permissions.subprocess.is_none());
        assert!(m.permissions.dynamic_code.is_none());
        assert!(m.permissions.ffi.is_none());
        assert!(m.permissions.high_res_time.is_none());
        assert!(m.limits.is_none());
    }

    #[test]
    fn test_load_valid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quicksilver.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{
                "version": 1,
                "name": "test-app",
                "permissions": {{
                    "file_read": ["./data"],
                    "network": ["api.example.com"],
                    "subprocess": false
                }},
                "limits": {{
                    "max_memory_mb": 128,
                    "max_execution_time_ms": 5000
                }}
            }}"#
        )
        .unwrap();

        let m = PermissionManifest::load(path.to_str().unwrap()).unwrap();
        assert_eq!(m.name.as_deref(), Some("test-app"));
        assert_eq!(m.permissions.file_read.as_ref().unwrap(), &["./data"]);
        assert_eq!(m.permissions.network.as_ref().unwrap(), &["api.example.com"]);
        assert_eq!(m.permissions.subprocess, Some(false));
        assert_eq!(m.limits.as_ref().unwrap().max_memory_mb, Some(128));
        assert_eq!(m.limits.as_ref().unwrap().max_execution_time_ms, Some(5000));
    }

    #[test]
    fn test_load_missing_file() {
        let result = PermissionManifest::load("/nonexistent/quicksilver.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();
        let result = PermissionManifest::load(path.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_unsupported_version() {
        let mut m = PermissionManifest::default_manifest();
        m.version = 99;
        let result = m.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_warnings() {
        let m = PermissionManifest {
            version: 1,
            name: None,
            permissions: ManifestPermissions {
                file_read: Some(vec!["*".to_string()]),
                file_write: Some(vec!["**".to_string()]),
                network: Some(vec!["*".to_string()]),
                ..Default::default()
            },
            limits: Some(ManifestLimits {
                max_memory_mb: Some(0),
                max_execution_time_ms: Some(0),
                max_stack_depth: Some(0),
            }),
        };
        let warnings = m.validate().unwrap();
        assert!(warnings.len() >= 5);
        assert!(warnings.iter().any(|w| w.contains("name")));
        assert!(warnings.iter().any(|w| w.contains("file_read")));
        assert!(warnings.iter().any(|w| w.contains("file_write")));
        assert!(warnings.iter().any(|w| w.contains("network")));
        assert!(warnings.iter().any(|w| w.contains("max_memory_mb")));
    }

    #[test]
    fn test_validate_clean_manifest() {
        let m = PermissionManifest {
            version: 1,
            name: Some("my-app".to_string()),
            permissions: ManifestPermissions {
                file_read: Some(vec!["./data".to_string()]),
                ..Default::default()
            },
            limits: None,
        };
        let warnings = m.validate().unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_to_sandbox_config_defaults() {
        let m = PermissionManifest::default_manifest();
        let cfg = m.to_sandbox_config();
        // Should use SandboxConfig defaults when no limits specified
        assert_eq!(cfg.max_duration, Duration::from_secs(30));
        assert_eq!(cfg.max_memory, 64 * 1024 * 1024);
    }

    #[test]
    fn test_to_sandbox_config_with_limits() {
        let m = PermissionManifest {
            version: 1,
            name: None,
            permissions: ManifestPermissions::default(),
            limits: Some(ManifestLimits {
                max_memory_mb: Some(256),
                max_execution_time_ms: Some(10_000),
                max_stack_depth: None,
            }),
        };
        let cfg = m.to_sandbox_config();
        assert_eq!(cfg.max_memory, 256 * 1024 * 1024);
        assert_eq!(cfg.max_duration, Duration::from_millis(10_000));
    }

    #[test]
    fn test_manifest_roundtrip_serialization() {
        let m = PermissionManifest {
            version: 1,
            name: Some("roundtrip".to_string()),
            permissions: ManifestPermissions {
                file_read: Some(vec!["./src".to_string()]),
                subprocess: Some(true),
                ..Default::default()
            },
            limits: Some(ManifestLimits {
                max_memory_mb: Some(64),
                max_execution_time_ms: None,
                max_stack_depth: Some(500),
            }),
        };
        let json = serde_json::to_string(&m).unwrap();
        let deserialized: PermissionManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name.as_deref(), Some("roundtrip"));
        assert_eq!(deserialized.permissions.subprocess, Some(true));
        assert_eq!(deserialized.limits.as_ref().unwrap().max_stack_depth, Some(500));
    }
}

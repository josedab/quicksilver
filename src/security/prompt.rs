//! Interactive Permission Prompts
//!
//! Provides a [`PermissionPrompt`] that decides how to handle permission
//! requests that are not covered by the manifest.  Four modes are supported:
//!
//! - **Interactive** — prompt the user (in non-interactive contexts falls back to deny).
//! - **DenyAll** — silently deny every unmanifested permission.
//! - **AllowAll** — allow everything (development mode).
//! - **ManifestOnly** — use the manifest exclusively; deny anything not listed.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// PromptMode
// ---------------------------------------------------------------------------

/// Permission prompt mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// Always prompt for unmanifested permissions.
    Interactive,
    /// Deny all unmanifested permissions silently.
    DenyAll,
    /// Allow all permissions (development mode).
    AllowAll,
    /// Use manifest only, deny everything not listed.
    ManifestOnly,
}

// ---------------------------------------------------------------------------
// PromptDecision
// ---------------------------------------------------------------------------

/// Result of a permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDecision {
    /// Allow this single request.
    AllowOnce,
    /// Allow and remember for the rest of the session.
    AllowAlways,
    /// Deny this single request.
    DenyOnce,
    /// Deny and remember for the rest of the session.
    DenyAlways,
}

impl PromptDecision {
    /// Whether the decision grants access.
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

// ---------------------------------------------------------------------------
// PermissionPrompt
// ---------------------------------------------------------------------------

/// Interactive permission prompt handler.
///
/// Maintains a map of remembered decisions so that the user is not asked
/// repeatedly for the same capability/resource pair.
pub struct PermissionPrompt {
    mode: PromptMode,
    remembered: HashMap<String, PromptDecision>,
}

impl PermissionPrompt {
    /// Create a new prompt handler with the given mode.
    pub fn new(mode: PromptMode) -> Self {
        Self {
            mode,
            remembered: HashMap::new(),
        }
    }

    /// Check permission for `capability` + `resource`.
    ///
    /// If a remembered decision exists for this key it is returned immediately.
    /// Otherwise the mode default is applied.
    pub fn check_permission(&mut self, capability: &str, resource: &str) -> PromptDecision {
        let key = format!("{}:{}", capability, resource);

        if let Some(&decision) = self.remembered.get(&key) {
            return decision;
        }

        match self.mode {
            PromptMode::AllowAll => PromptDecision::AllowOnce,
            PromptMode::DenyAll | PromptMode::ManifestOnly => PromptDecision::DenyOnce,
            // In a real interactive terminal we would prompt here; for now we
            // fall back to deny to maintain a secure default.
            PromptMode::Interactive => PromptDecision::DenyOnce,
        }
    }

    /// Remember a decision for a capability/resource key.
    pub fn remember(&mut self, key: String, decision: PromptDecision) {
        self.remembered.insert(key, decision);
    }

    /// Clear all remembered decisions.
    pub fn clear_remembered(&mut self) {
        self.remembered.clear();
    }

    /// Return the current mode.
    pub fn mode(&self) -> PromptMode {
        self.mode
    }

    /// Return the number of remembered decisions.
    pub fn remembered_count(&self) -> usize {
        self.remembered.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deny_all_mode() {
        let mut prompt = PermissionPrompt::new(PromptMode::DenyAll);
        let d = prompt.check_permission("file_read", "/etc/passwd");
        assert_eq!(d, PromptDecision::DenyOnce);
        assert!(!d.is_allow());
    }

    #[test]
    fn test_allow_all_mode() {
        let mut prompt = PermissionPrompt::new(PromptMode::AllowAll);
        let d = prompt.check_permission("network", "example.com");
        assert_eq!(d, PromptDecision::AllowOnce);
        assert!(d.is_allow());
    }

    #[test]
    fn test_manifest_only_mode() {
        let mut prompt = PermissionPrompt::new(PromptMode::ManifestOnly);
        let d = prompt.check_permission("subprocess", "bash");
        assert_eq!(d, PromptDecision::DenyOnce);
    }

    #[test]
    fn test_interactive_defaults_to_deny() {
        let mut prompt = PermissionPrompt::new(PromptMode::Interactive);
        let d = prompt.check_permission("ffi", "libfoo.so");
        assert_eq!(d, PromptDecision::DenyOnce);
    }

    #[test]
    fn test_remembered_decision_overrides_mode() {
        let mut prompt = PermissionPrompt::new(PromptMode::DenyAll);
        prompt.remember("file_read:/tmp".to_string(), PromptDecision::AllowAlways);
        let d = prompt.check_permission("file_read", "/tmp");
        assert_eq!(d, PromptDecision::AllowAlways);
        assert!(d.is_allow());
    }

    #[test]
    fn test_remembered_deny_always() {
        let mut prompt = PermissionPrompt::new(PromptMode::AllowAll);
        prompt.remember("network:evil.com".to_string(), PromptDecision::DenyAlways);
        let d = prompt.check_permission("network", "evil.com");
        assert_eq!(d, PromptDecision::DenyAlways);
        assert!(!d.is_allow());
    }

    #[test]
    fn test_clear_remembered() {
        let mut prompt = PermissionPrompt::new(PromptMode::DenyAll);
        prompt.remember("a:b".to_string(), PromptDecision::AllowAlways);
        assert_eq!(prompt.remembered_count(), 1);
        prompt.clear_remembered();
        assert_eq!(prompt.remembered_count(), 0);
        // After clearing, should fall back to mode default
        let d = prompt.check_permission("a", "b");
        assert_eq!(d, PromptDecision::DenyOnce);
    }

    #[test]
    fn test_different_resources_independent() {
        let mut prompt = PermissionPrompt::new(PromptMode::DenyAll);
        prompt.remember("file_read:/allowed".to_string(), PromptDecision::AllowAlways);
        assert_eq!(
            prompt.check_permission("file_read", "/allowed"),
            PromptDecision::AllowAlways
        );
        assert_eq!(
            prompt.check_permission("file_read", "/other"),
            PromptDecision::DenyOnce
        );
    }

    #[test]
    fn test_mode_accessor() {
        let prompt = PermissionPrompt::new(PromptMode::AllowAll);
        assert_eq!(prompt.mode(), PromptMode::AllowAll);
    }
}

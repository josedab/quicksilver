//! Audit Logger for Security Events
//!
//! Records every permission check so that operators can review what
//! capabilities were requested (and whether they were granted) during
//! a program's execution.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AuditEntry
// ---------------------------------------------------------------------------

/// A single audit log entry for a permission check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Capability that was checked (e.g. `"file_read"`).
    pub capability: String,
    /// Resource that was requested (e.g. `"/etc/passwd"`).
    pub resource: String,
    /// Outcome of the check (e.g. `"granted"`, `"denied"`).
    pub decision: String,
    /// Source file that triggered the check, if known.
    pub source_file: Option<String>,
    /// Source line that triggered the check, if known.
    pub source_line: Option<u32>,
}

// ---------------------------------------------------------------------------
// AuditLogger
// ---------------------------------------------------------------------------

/// Bounded audit logger for security events.
///
/// When the number of entries reaches `max_entries`, the oldest entry is
/// discarded to make room for the new one.
pub struct AuditLogger {
    entries: Vec<AuditEntry>,
    max_entries: usize,
}

impl AuditLogger {
    /// Create a new logger that retains at most `max_entries` entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// Append an entry, evicting the oldest if the log is full.
    pub fn log(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Return a slice of all stored entries.
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Serialize all entries to a JSON array string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Remove all stored entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Query entries by optional capability and/or decision filters.
    pub fn query<'a>(
        &'a self,
        capability: Option<&str>,
        decision: Option<&str>,
    ) -> Vec<&'a AuditEntry> {
        self.entries
            .iter()
            .filter(|e| {
                capability.is_none_or(|c| e.capability == c)
                    && decision.is_none_or(|d| e.decision == d)
            })
            .collect()
    }

    /// Return the total number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(cap: &str, resource: &str, decision: &str) -> AuditEntry {
        AuditEntry {
            timestamp: 1_000,
            capability: cap.to_string(),
            resource: resource.to_string(),
            decision: decision.to_string(),
            source_file: None,
            source_line: None,
        }
    }

    #[test]
    fn test_log_and_retrieve() {
        let mut logger = AuditLogger::new(100);
        logger.log(make_entry("file_read", "/tmp/a", "granted"));
        logger.log(make_entry("network", "example.com", "denied"));
        assert_eq!(logger.len(), 2);
        assert_eq!(logger.entries()[0].capability, "file_read");
        assert_eq!(logger.entries()[1].decision, "denied");
    }

    #[test]
    fn test_max_entries_eviction() {
        let mut logger = AuditLogger::new(2);
        logger.log(make_entry("a", "r1", "granted"));
        logger.log(make_entry("b", "r2", "granted"));
        logger.log(make_entry("c", "r3", "denied"));
        assert_eq!(logger.len(), 2);
        // Oldest entry ("a") should have been evicted
        assert_eq!(logger.entries()[0].capability, "b");
        assert_eq!(logger.entries()[1].capability, "c");
    }

    #[test]
    fn test_clear() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("x", "y", "granted"));
        assert!(!logger.is_empty());
        logger.clear();
        assert!(logger.is_empty());
        assert_eq!(logger.len(), 0);
    }

    #[test]
    fn test_to_json() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("file_read", "/etc/hosts", "denied"));
        let json = logger.to_json();
        assert!(json.contains("file_read"));
        assert!(json.contains("/etc/hosts"));
        assert!(json.contains("denied"));
        // Must be valid JSON
        let parsed: Vec<AuditEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn test_query_by_capability() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("file_read", "/a", "granted"));
        logger.log(make_entry("network", "/b", "granted"));
        logger.log(make_entry("file_read", "/c", "denied"));
        let results = logger.query(Some("file_read"), None);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.capability == "file_read"));
    }

    #[test]
    fn test_query_by_decision() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("file_read", "/a", "granted"));
        logger.log(make_entry("network", "/b", "denied"));
        logger.log(make_entry("env", "PATH", "denied"));
        let results = logger.query(None, Some("denied"));
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.decision == "denied"));
    }

    #[test]
    fn test_query_by_both() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("file_read", "/a", "granted"));
        logger.log(make_entry("file_read", "/b", "denied"));
        logger.log(make_entry("network", "/c", "denied"));
        let results = logger.query(Some("file_read"), Some("denied"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].resource, "/b");
    }

    #[test]
    fn test_query_no_filters() {
        let mut logger = AuditLogger::new(10);
        logger.log(make_entry("a", "x", "granted"));
        logger.log(make_entry("b", "y", "denied"));
        let results = logger.query(None, None);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_entry_with_source_info() {
        let entry = AuditEntry {
            timestamp: 42,
            capability: "subprocess".to_string(),
            resource: "bash".to_string(),
            decision: "denied".to_string(),
            source_file: Some("app.js".to_string()),
            source_line: Some(10),
        };
        let mut logger = AuditLogger::new(10);
        logger.log(entry);
        let e = &logger.entries()[0];
        assert_eq!(e.source_file.as_deref(), Some("app.js"));
        assert_eq!(e.source_line, Some(10));
    }

    #[test]
    fn test_empty_logger() {
        let logger = AuditLogger::new(10);
        assert!(logger.is_empty());
        assert_eq!(logger.len(), 0);
        assert_eq!(logger.to_json(), "[]");
        assert!(logger.query(None, None).is_empty());
    }
}

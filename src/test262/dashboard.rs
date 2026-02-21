//! Test262 Conformance Dashboard
//!
//! Provides a conformance dashboard and CI integration system for tracking
//! Test262 test results over time, detecting regressions, and generating
//! reports in multiple formats (Markdown, JSON, HTML, shields.io badges).

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::{Error, Result};

// ============================================================================
// Core Types
// ============================================================================

/// Status of a feature's conformance support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureStatus {
    /// All tests pass
    FullySupported,
    /// Some tests pass
    PartiallySupported,
    /// No tests pass
    NotSupported,
    /// Not yet evaluated
    Unknown,
}

impl std::fmt::Display for FeatureStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureStatus::FullySupported => write!(f, "✅ Fully Supported"),
            FeatureStatus::PartiallySupported => write!(f, "⚠️ Partially Supported"),
            FeatureStatus::NotSupported => write!(f, "❌ Not Supported"),
            FeatureStatus::Unknown => write!(f, "❓ Unknown"),
        }
    }
}

/// Severity of a regression
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegressionSeverity {
    /// Pass rate dropped by more than 10%
    Critical,
    /// Pass rate dropped by more than 5%
    Major,
    /// Pass rate dropped by threshold amount
    Minor,
}

impl std::fmt::Display for RegressionSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegressionSeverity::Critical => write!(f, "CRITICAL"),
            RegressionSeverity::Major => write!(f, "MAJOR"),
            RegressionSeverity::Minor => write!(f, "MINOR"),
        }
    }
}

/// Summary of a test run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errored: usize,
    pub timed_out: usize,
    pub pass_rate: f64,
}

impl RunSummary {
    /// Compute pass rate from counts (excludes skipped tests)
    pub fn compute_pass_rate(&mut self) {
        let runnable = self.total.saturating_sub(self.skipped);
        self.pass_rate = if runnable == 0 {
            0.0
        } else {
            self.passed as f64 / runnable as f64 * 100.0
        };
    }
}

/// Per-feature test result breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureResult {
    pub name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pass_rate: f64,
    pub status: FeatureStatus,
    pub failing_tests: Vec<String>,
}

impl FeatureResult {
    /// Compute pass rate and status from counts
    pub fn compute(&mut self) {
        let runnable = self.total.saturating_sub(self.skipped);
        self.pass_rate = if runnable == 0 {
            0.0
        } else {
            self.passed as f64 / runnable as f64 * 100.0
        };
        self.status = if runnable == 0 {
            FeatureStatus::Unknown
        } else if self.passed == runnable {
            FeatureStatus::FullySupported
        } else if self.passed == 0 {
            FeatureStatus::NotSupported
        } else {
            FeatureStatus::PartiallySupported
        };
    }
}

/// A single dashboard test run snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRun {
    pub id: String,
    pub timestamp: u64,
    pub git_sha: Option<String>,
    pub version: String,
    pub summary: RunSummary,
    pub feature_results: HashMap<String, FeatureResult>,
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    pub platform: String,
}

/// Serde helper for Duration (as milliseconds)
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(duration: &Duration, s: S) -> std::result::Result<S::Ok, S::Error> {
        duration.as_millis().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// A detected regression between runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    pub feature: String,
    pub previous_rate: f64,
    pub current_rate: f64,
    pub delta: f64,
    pub severity: RegressionSeverity,
    pub new_failures: Vec<String>,
}

/// Badge data for shields.io
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BadgeData {
    pub label: String,
    pub message: String,
    pub color: String,
}

// ============================================================================
// Dashboard Configuration
// ============================================================================

/// Configuration for the conformance dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub max_runs: usize,
    pub output_dir: String,
    pub track_regressions: bool,
    pub regression_threshold: f64,
    pub features_of_interest: Vec<String>,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            max_runs: 100,
            output_dir: "test262-dashboard".to_string(),
            track_regressions: true,
            regression_threshold: 1.0,
            features_of_interest: Vec::new(),
        }
    }
}

// ============================================================================
// Conformance Dashboard
// ============================================================================

/// Main dashboard for tracking Test262 conformance over time
pub struct ConformanceDashboard {
    runs: Vec<DashboardRun>,
    feature_matrix: HashMap<String, FeatureStatus>,
    config: DashboardConfig,
}

impl ConformanceDashboard {
    /// Create a new dashboard with default configuration
    pub fn new() -> Self {
        Self {
            runs: Vec::new(),
            feature_matrix: HashMap::default(),
            config: DashboardConfig::default(),
        }
    }

    /// Create a new dashboard with the given configuration
    pub fn with_config(config: DashboardConfig) -> Self {
        Self {
            runs: Vec::new(),
            feature_matrix: HashMap::default(),
            config,
        }
    }

    /// Add a run to the dashboard, trimming old runs if over max
    pub fn add_run(&mut self, run: DashboardRun) {
        // Update feature matrix from this run
        for (name, result) in &run.feature_results {
            self.feature_matrix.insert(name.clone(), result.status);
        }
        self.runs.push(run);
        // Trim oldest runs if over limit
        while self.runs.len() > self.config.max_runs {
            self.runs.remove(0);
        }
    }

    /// Get the latest run
    pub fn get_latest(&self) -> Option<&DashboardRun> {
        self.runs.last()
    }

    /// Get a run by its ID
    pub fn get_run_by_id(&self, id: &str) -> Option<&DashboardRun> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// Compare two runs and return regressions
    pub fn compare_runs(&self, prev_id: &str, curr_id: &str) -> Result<Vec<Regression>> {
        let prev = self.get_run_by_id(prev_id).ok_or_else(|| {
            Error::InternalError(format!("Run not found: {}", prev_id))
        })?;
        let curr = self.get_run_by_id(curr_id).ok_or_else(|| {
            Error::InternalError(format!("Run not found: {}", curr_id))
        })?;
        let detector = RegressionDetector::new(self.config.regression_threshold);
        Ok(detector.detect(prev, curr))
    }

    /// Get the trend across recent runs: "improving", "declining", or "stable"
    pub fn get_trend(&self) -> &'static str {
        if self.runs.len() < 2 {
            return "stable";
        }
        let recent: Vec<f64> = self.runs.iter().rev().take(5).map(|r| r.summary.pass_rate).collect();
        if recent.len() < 2 {
            return "stable";
        }
        let first = recent.last().unwrap();
        let last = recent.first().unwrap();
        let delta = last - first;
        if delta > 1.0 {
            "improving"
        } else if delta < -1.0 {
            "declining"
        } else {
            "stable"
        }
    }

    /// Export the dashboard to JSON
    pub fn export_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&DashboardExport {
            runs: &self.runs,
            feature_matrix: &self.feature_matrix,
        })
        .map_err(|e| Error::InternalError(format!("JSON serialization failed: {}", e)))
    }

    /// Import dashboard data from JSON
    pub fn import_json(&mut self, json: &str) -> Result<()> {
        let export: DashboardImport =
            serde_json::from_str(json).map_err(|e| Error::InternalError(format!("JSON parse failed: {}", e)))?;
        self.runs = export.runs;
        self.feature_matrix = export.feature_matrix;
        Ok(())
    }

    /// Get the current feature matrix
    pub fn get_feature_matrix(&self) -> &HashMap<String, FeatureStatus> {
        &self.feature_matrix
    }

    /// Get all runs
    pub fn runs(&self) -> &[DashboardRun] {
        &self.runs
    }
}

#[derive(Serialize)]
struct DashboardExport<'a> {
    runs: &'a [DashboardRun],
    feature_matrix: &'a HashMap<String, FeatureStatus>,
}

#[derive(Deserialize)]
struct DashboardImport {
    runs: Vec<DashboardRun>,
    feature_matrix: HashMap<String, FeatureStatus>,
}

// ============================================================================
// Regression Detector
// ============================================================================

/// Detects conformance regressions between test runs
pub struct RegressionDetector {
    threshold: f64,
}

impl RegressionDetector {
    /// Create a new detector with the given threshold (minimum pass rate drop to flag)
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    /// Detect regressions between two runs
    pub fn detect(&self, previous: &DashboardRun, current: &DashboardRun) -> Vec<Regression> {
        let mut regressions = Vec::new();

        // Check overall regression
        let overall_delta = current.summary.pass_rate - previous.summary.pass_rate;
        if overall_delta < -self.threshold {
            regressions.push(Regression {
                feature: "(overall)".to_string(),
                previous_rate: previous.summary.pass_rate,
                current_rate: current.summary.pass_rate,
                delta: overall_delta,
                severity: Self::classify_severity(overall_delta),
                new_failures: Vec::new(),
            });
        }

        // Check per-feature regressions
        for (name, prev_feat) in &previous.feature_results {
            if let Some(curr_feat) = current.feature_results.get(name) {
                if let Some(reg) = self.detect_feature_regressions(prev_feat, curr_feat) {
                    regressions.push(reg);
                }
            }
        }

        regressions
    }

    /// Detect regression for a single feature
    pub fn detect_feature_regressions(
        &self,
        prev: &FeatureResult,
        curr: &FeatureResult,
    ) -> Option<Regression> {
        let delta = curr.pass_rate - prev.pass_rate;
        if delta < -self.threshold {
            // Find new failures
            let prev_failures: std::collections::HashSet<&str> =
                prev.failing_tests.iter().map(|s| s.as_str()).collect();
            let new_failures: Vec<String> = curr
                .failing_tests
                .iter()
                .filter(|t| !prev_failures.contains(t.as_str()))
                .cloned()
                .collect();

            Some(Regression {
                feature: curr.name.clone(),
                previous_rate: prev.pass_rate,
                current_rate: curr.pass_rate,
                delta,
                severity: Self::classify_severity(delta),
                new_failures,
            })
        } else {
            None
        }
    }

    fn classify_severity(delta: f64) -> RegressionSeverity {
        let abs = delta.abs();
        if abs > 10.0 {
            RegressionSeverity::Critical
        } else if abs > 5.0 {
            RegressionSeverity::Major
        } else {
            RegressionSeverity::Minor
        }
    }
}

// ============================================================================
// CI Reporter
// ============================================================================

/// Generates CI-friendly reports from test runs
pub struct CiReporter;

impl CiReporter {
    /// Generate a Markdown summary of a run
    pub fn generate_summary(run: &DashboardRun) -> String {
        let mut s = String::new();
        s.push_str("## Test262 Conformance Report\n\n");
        s.push_str(&format!("**Version:** {}\n", run.version));
        if let Some(ref sha) = run.git_sha {
            s.push_str(&format!("**Commit:** `{}`\n", sha));
        }
        s.push_str(&format!("**Platform:** {}\n", run.platform));
        s.push_str(&format!("**Duration:** {:.2}s\n\n", run.duration.as_secs_f64()));

        s.push_str("### Summary\n\n");
        s.push_str(&format!(
            "| Metric | Count |\n|--------|-------|\n| Total | {} |\n| Passed | {} |\n| Failed | {} |\n| Skipped | {} |\n| Errored | {} |\n| Timed Out | {} |\n| **Pass Rate** | **{:.1}%** |\n\n",
            run.summary.total,
            run.summary.passed,
            run.summary.failed,
            run.summary.skipped,
            run.summary.errored,
            run.summary.timed_out,
            run.summary.pass_rate,
        ));

        if !run.feature_results.is_empty() {
            s.push_str("### Feature Results\n\n");
            s.push_str("| Feature | Pass Rate | Status |\n|---------|-----------|--------|\n");
            let mut features: Vec<_> = run.feature_results.iter().collect();
            features.sort_by_key(|(name, _)| (*name).clone());
            for (name, result) in features {
                s.push_str(&format!(
                    "| {} | {:.1}% ({}/{}) | {} |\n",
                    name, result.pass_rate, result.passed, result.total, result.status,
                ));
            }
        }

        s
    }

    /// Generate shields.io badge data
    pub fn generate_badge(run: &DashboardRun) -> BadgeData {
        let color = if run.summary.pass_rate >= 90.0 {
            "brightgreen".to_string()
        } else if run.summary.pass_rate >= 70.0 {
            "yellow".to_string()
        } else if run.summary.pass_rate >= 50.0 {
            "orange".to_string()
        } else {
            "red".to_string()
        };

        BadgeData {
            label: "test262".to_string(),
            message: format!("{:.1}%", run.summary.pass_rate),
            color,
        }
    }

    /// Generate a JSON report
    pub fn generate_json_report(run: &DashboardRun) -> String {
        serde_json::to_string_pretty(run).unwrap_or_else(|_| "{}".to_string())
    }

    /// Determine if CI should fail based on minimum pass rate
    pub fn should_fail_ci(run: &DashboardRun, min_pass_rate: f64) -> bool {
        run.summary.pass_rate < min_pass_rate
    }

    /// Format regressions as a human-readable string
    pub fn format_regressions(regressions: &[Regression]) -> String {
        if regressions.is_empty() {
            return "No regressions detected.".to_string();
        }

        let mut s = String::new();
        s.push_str(&format!("⚠️ {} regression(s) detected:\n\n", regressions.len()));
        for reg in regressions {
            s.push_str(&format!(
                "- **[{}]** {}: {:.1}% → {:.1}% (Δ {:.1}%)\n",
                reg.severity, reg.feature, reg.previous_rate, reg.current_rate, reg.delta,
            ));
            if !reg.new_failures.is_empty() {
                s.push_str(&format!(
                    "  New failures: {}\n",
                    reg.new_failures.join(", ")
                ));
            }
        }
        s
    }
}

// ============================================================================
// HTML Dashboard
// ============================================================================

/// Generates a static HTML dashboard
pub struct HtmlDashboard;

impl HtmlDashboard {
    /// Generate a full HTML dashboard page
    pub fn generate(dashboard: &ConformanceDashboard) -> String {
        let latest = dashboard.get_latest();
        let summary_html = match latest {
            Some(run) => format!(
                "<div class=\"summary\">\
                <h2>Latest Run: {}</h2>\
                <p>Version: {} | Platform: {} | Duration: {:.2}s</p>\
                <div class=\"pass-rate\">{:.1}%</div>\
                <p>{} passed / {} total ({} skipped)</p>\
                </div>",
                run.id,
                run.version,
                run.platform,
                run.duration.as_secs_f64(),
                run.summary.pass_rate,
                run.summary.passed,
                run.summary.total,
                run.summary.skipped,
            ),
            None => "<div class=\"summary\"><p>No runs recorded.</p></div>".to_string(),
        };

        let feature_html = match latest {
            Some(run) => Self::feature_table(&run.feature_results),
            None => String::new(),
        };

        let chart_data = Self::progress_chart_data(dashboard.runs());

        format!(
            "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"UTF-8\">\n\
            <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n\
            <title>Test262 Conformance Dashboard</title>\n\
            <style>\n\
            body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; \
            margin: 0; padding: 20px; background: #f5f5f5; }}\n\
            .container {{ max-width: 1200px; margin: 0 auto; }}\n\
            h1 {{ color: #333; }}\n\
            .summary {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; \
            box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}\n\
            .pass-rate {{ font-size: 48px; font-weight: bold; color: #2ea44f; }}\n\
            table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; \
            overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}\n\
            th, td {{ padding: 12px 16px; text-align: left; border-bottom: 1px solid #eee; }}\n\
            th {{ background: #f8f9fa; font-weight: 600; }}\n\
            .status-full {{ color: #2ea44f; }}\n\
            .status-partial {{ color: #d29922; }}\n\
            .status-none {{ color: #cb2431; }}\n\
            </style>\n</head>\n<body>\n<div class=\"container\">\n\
            <h1>🔬 Test262 Conformance Dashboard</h1>\n\
            {summary_html}\n\
            <h2>Feature Matrix</h2>\n\
            {feature_html}\n\
            <h2>Progress Over Time</h2>\n\
            <pre id=\"chart-data\">{chart_data}</pre>\n\
            </div>\n</body>\n</html>"
        )
    }

    /// Generate an HTML table of feature results
    pub fn feature_table(features: &HashMap<String, FeatureResult>) -> String {
        let mut s = String::new();
        s.push_str("<table>\n<thead><tr><th>Feature</th><th>Pass Rate</th><th>Passed</th><th>Failed</th><th>Status</th></tr></thead>\n<tbody>\n");

        let mut sorted: Vec<_> = features.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());

        for (name, result) in sorted {
            let status_class = match result.status {
                FeatureStatus::FullySupported => "status-full",
                FeatureStatus::PartiallySupported => "status-partial",
                _ => "status-none",
            };
            s.push_str(&format!(
                "<tr><td>{}</td><td>{:.1}%</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td></tr>\n",
                name, result.pass_rate, result.passed, result.failed, status_class, result.status,
            ));
        }

        s.push_str("</tbody>\n</table>");
        s
    }

    /// Generate JSON data for a progress chart
    pub fn progress_chart_data(runs: &[DashboardRun]) -> String {
        let data: Vec<serde_json::Value> = runs
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "timestamp": r.timestamp,
                    "pass_rate": r.summary.pass_rate,
                    "total": r.summary.total,
                    "passed": r.summary.passed,
                })
            })
            .collect();
        serde_json::to_string_pretty(&data).unwrap_or_else(|_| "[]".to_string())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_run(id: &str, pass_rate: f64, passed: usize, total: usize) -> DashboardRun {
        DashboardRun {
            id: id.to_string(),
            timestamp: 1700000000,
            git_sha: Some("abc123".to_string()),
            version: "0.1.0".to_string(),
            summary: RunSummary {
                total,
                passed,
                failed: total - passed,
                skipped: 0,
                errored: 0,
                timed_out: 0,
                pass_rate,
            },
            feature_results: HashMap::default(),
            duration: Duration::from_secs(42),
            platform: "linux-x86_64".to_string(),
        }
    }

    fn make_feature(name: &str, passed: usize, total: usize, failing: Vec<&str>) -> FeatureResult {
        let failed = total - passed;
        let pass_rate = if total == 0 { 0.0 } else { passed as f64 / total as f64 * 100.0 };
        let status = if total == 0 {
            FeatureStatus::Unknown
        } else if passed == total {
            FeatureStatus::FullySupported
        } else if passed == 0 {
            FeatureStatus::NotSupported
        } else {
            FeatureStatus::PartiallySupported
        };
        FeatureResult {
            name: name.to_string(),
            total,
            passed,
            failed,
            skipped: 0,
            pass_rate,
            status,
            failing_tests: failing.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_run_creation() {
        let run = make_run("run-1", 85.0, 85, 100);
        assert_eq!(run.id, "run-1");
        assert_eq!(run.summary.pass_rate, 85.0);
        assert_eq!(run.summary.total, 100);
        assert_eq!(run.summary.passed, 85);
    }

    #[test]
    fn test_dashboard_add_run() {
        let mut dash = ConformanceDashboard::new();
        dash.add_run(make_run("run-1", 80.0, 80, 100));
        dash.add_run(make_run("run-2", 85.0, 85, 100));
        assert_eq!(dash.runs().len(), 2);
        assert_eq!(dash.get_latest().unwrap().id, "run-2");
    }

    #[test]
    fn test_dashboard_max_runs() {
        let config = DashboardConfig {
            max_runs: 3,
            ..DashboardConfig::default()
        };
        let mut dash = ConformanceDashboard::with_config(config);
        for i in 0..5 {
            dash.add_run(make_run(&format!("run-{}", i), 80.0, 80, 100));
        }
        assert_eq!(dash.runs().len(), 3);
        assert_eq!(dash.runs()[0].id, "run-2");
    }

    #[test]
    fn test_get_run_by_id() {
        let mut dash = ConformanceDashboard::new();
        dash.add_run(make_run("alpha", 90.0, 90, 100));
        dash.add_run(make_run("beta", 92.0, 92, 100));
        assert!(dash.get_run_by_id("alpha").is_some());
        assert!(dash.get_run_by_id("gamma").is_none());
    }

    #[test]
    fn test_regression_detection_overall() {
        let detector = RegressionDetector::new(1.0);
        let prev = make_run("prev", 90.0, 90, 100);
        let curr = make_run("curr", 85.0, 85, 100);
        let regressions = detector.detect(&prev, &curr);
        assert!(!regressions.is_empty());
        assert_eq!(regressions[0].feature, "(overall)");
        assert!((regressions[0].delta - (-5.0)).abs() < 0.01);
    }

    #[test]
    fn test_regression_detection_no_regression() {
        let detector = RegressionDetector::new(1.0);
        let prev = make_run("prev", 85.0, 85, 100);
        let curr = make_run("curr", 90.0, 90, 100);
        let regressions = detector.detect(&prev, &curr);
        assert!(regressions.is_empty());
    }

    #[test]
    fn test_feature_regression_detection() {
        let detector = RegressionDetector::new(1.0);
        let prev = make_feature("arrow-function", 10, 10, vec![]);
        let curr = make_feature("arrow-function", 7, 10, vec!["test_a.js", "test_b.js", "test_c.js"]);
        let reg = detector.detect_feature_regressions(&prev, &curr);
        assert!(reg.is_some());
        let reg = reg.unwrap();
        assert_eq!(reg.feature, "arrow-function");
        assert_eq!(reg.new_failures.len(), 3);
    }

    #[test]
    fn test_regression_severity() {
        let detector = RegressionDetector::new(1.0);
        // Critical: >10% drop
        let prev = make_feature("f", 100, 100, vec![]);
        let curr = make_feature("f", 85, 100, vec![]);
        let reg = detector.detect_feature_regressions(&prev, &curr).unwrap();
        assert_eq!(reg.severity, RegressionSeverity::Critical);

        // Major: >5% drop
        let curr = make_feature("f", 93, 100, vec![]);
        let reg = detector.detect_feature_regressions(&prev, &curr).unwrap();
        assert_eq!(reg.severity, RegressionSeverity::Major);

        // Minor: small drop
        let curr = make_feature("f", 97, 100, vec![]);
        let reg = detector.detect_feature_regressions(&prev, &curr).unwrap();
        assert_eq!(reg.severity, RegressionSeverity::Minor);
    }

    #[test]
    fn test_ci_summary_generation() {
        let run = make_run("ci-run", 87.5, 70, 80);
        let summary = CiReporter::generate_summary(&run);
        assert!(summary.contains("Test262 Conformance Report"));
        assert!(summary.contains("87.5%"));
        assert!(summary.contains("0.1.0"));
    }

    #[test]
    fn test_badge_generation() {
        let run_high = make_run("b1", 95.0, 95, 100);
        let badge = CiReporter::generate_badge(&run_high);
        assert_eq!(badge.label, "test262");
        assert_eq!(badge.color, "brightgreen");
        assert!(badge.message.contains("95.0"));

        let run_low = make_run("b2", 40.0, 40, 100);
        let badge = CiReporter::generate_badge(&run_low);
        assert_eq!(badge.color, "red");
    }

    #[test]
    fn test_json_report_generation() {
        let run = make_run("json-run", 75.0, 75, 100);
        let json = CiReporter::generate_json_report(&run);
        assert!(json.contains("json-run"));
        assert!(json.contains("75"));
    }

    #[test]
    fn test_should_fail_ci() {
        let run = make_run("ci", 80.0, 80, 100);
        assert!(CiReporter::should_fail_ci(&run, 90.0));
        assert!(!CiReporter::should_fail_ci(&run, 70.0));
    }

    #[test]
    fn test_format_regressions() {
        let regressions = vec![Regression {
            feature: "let-const".to_string(),
            previous_rate: 100.0,
            current_rate: 88.0,
            delta: -12.0,
            severity: RegressionSeverity::Critical,
            new_failures: vec!["test1.js".to_string()],
        }];
        let output = CiReporter::format_regressions(&regressions);
        assert!(output.contains("CRITICAL"));
        assert!(output.contains("let-const"));
        assert!(output.contains("test1.js"));

        let empty = CiReporter::format_regressions(&[]);
        assert!(empty.contains("No regressions"));
    }

    #[test]
    fn test_html_dashboard_generation() {
        let mut dash = ConformanceDashboard::new();
        let mut run = make_run("html-run", 88.0, 88, 100);
        run.feature_results.insert(
            "arrow-function".to_string(),
            make_feature("arrow-function", 10, 10, vec![]),
        );
        dash.add_run(run);
        let html = HtmlDashboard::generate(&dash);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Test262 Conformance Dashboard"));
        assert!(html.contains("arrow-function"));
    }

    #[test]
    fn test_feature_table_html() {
        let mut features = HashMap::default();
        features.insert(
            "destructuring".to_string(),
            make_feature("destructuring", 8, 10, vec!["d1.js", "d2.js"]),
        );
        let html = HtmlDashboard::feature_table(&features);
        assert!(html.contains("<table>"));
        assert!(html.contains("destructuring"));
        assert!(html.contains("80.0%"));
    }

    #[test]
    fn test_progress_chart_data() {
        let runs = vec![
            make_run("r1", 80.0, 80, 100),
            make_run("r2", 85.0, 85, 100),
        ];
        let data = HtmlDashboard::progress_chart_data(&runs);
        assert!(data.contains("r1"));
        assert!(data.contains("r2"));
        assert!(data.contains("pass_rate"));
    }

    #[test]
    fn test_feature_matrix() {
        let mut dash = ConformanceDashboard::new();
        let mut run = make_run("m1", 90.0, 90, 100);
        run.feature_results.insert(
            "let".to_string(),
            make_feature("let", 10, 10, vec![]),
        );
        run.feature_results.insert(
            "const".to_string(),
            make_feature("const", 5, 10, vec![]),
        );
        dash.add_run(run);
        let matrix = dash.get_feature_matrix();
        assert_eq!(matrix.get("let"), Some(&FeatureStatus::FullySupported));
        assert_eq!(matrix.get("const"), Some(&FeatureStatus::PartiallySupported));
    }

    #[test]
    fn test_trend_analysis() {
        let mut dash = ConformanceDashboard::new();
        // Improving trend
        dash.add_run(make_run("t1", 80.0, 80, 100));
        dash.add_run(make_run("t2", 82.0, 82, 100));
        dash.add_run(make_run("t3", 85.0, 85, 100));
        assert_eq!(dash.get_trend(), "improving");

        // Declining trend
        let mut dash2 = ConformanceDashboard::new();
        dash2.add_run(make_run("t1", 90.0, 90, 100));
        dash2.add_run(make_run("t2", 87.0, 87, 100));
        dash2.add_run(make_run("t3", 85.0, 85, 100));
        assert_eq!(dash2.get_trend(), "declining");

        // Stable
        let mut dash3 = ConformanceDashboard::new();
        dash3.add_run(make_run("t1", 85.0, 85, 100));
        dash3.add_run(make_run("t2", 85.5, 85, 100));
        assert_eq!(dash3.get_trend(), "stable");
    }

    #[test]
    fn test_json_export_import() {
        let mut dash = ConformanceDashboard::new();
        let mut run = make_run("exp-1", 92.0, 92, 100);
        run.feature_results.insert(
            "arrow-function".to_string(),
            make_feature("arrow-function", 10, 10, vec![]),
        );
        dash.add_run(run);

        let json = dash.export_json().unwrap();
        assert!(json.contains("exp-1"));

        let mut dash2 = ConformanceDashboard::new();
        dash2.import_json(&json).unwrap();
        assert_eq!(dash2.runs().len(), 1);
        assert_eq!(dash2.runs()[0].id, "exp-1");
        assert!(dash2.get_feature_matrix().contains_key("arrow-function"));
    }

    #[test]
    fn test_pass_rate_computation() {
        let mut summary = RunSummary {
            total: 100,
            passed: 80,
            failed: 10,
            skipped: 10,
            errored: 0,
            timed_out: 0,
            pass_rate: 0.0,
        };
        summary.compute_pass_rate();
        // 80 / (100 - 10) = 88.88...%
        assert!((summary.pass_rate - 88.888).abs() < 0.01);

        // Edge case: all skipped
        let mut all_skipped = RunSummary {
            total: 10,
            passed: 0,
            failed: 0,
            skipped: 10,
            errored: 0,
            timed_out: 0,
            pass_rate: 0.0,
        };
        all_skipped.compute_pass_rate();
        assert_eq!(all_skipped.pass_rate, 0.0);
    }

    #[test]
    fn test_feature_result_compute() {
        let mut feat = FeatureResult {
            name: "test".to_string(),
            total: 10,
            passed: 10,
            failed: 0,
            skipped: 0,
            pass_rate: 0.0,
            status: FeatureStatus::Unknown,
            failing_tests: vec![],
        };
        feat.compute();
        assert_eq!(feat.status, FeatureStatus::FullySupported);
        assert!((feat.pass_rate - 100.0).abs() < 0.01);

        feat.passed = 0;
        feat.failed = 10;
        feat.compute();
        assert_eq!(feat.status, FeatureStatus::NotSupported);
    }

    #[test]
    fn test_compare_runs() {
        let mut dash = ConformanceDashboard::new();
        dash.add_run(make_run("a", 90.0, 90, 100));
        dash.add_run(make_run("b", 80.0, 80, 100));
        let regressions = dash.compare_runs("a", "b").unwrap();
        assert!(!regressions.is_empty());

        // Non-existent run
        assert!(dash.compare_runs("a", "nonexistent").is_err());
    }

    #[test]
    fn test_default_dashboard_config() {
        let config = DashboardConfig::default();
        assert_eq!(config.max_runs, 100);
        assert!(config.track_regressions);
        assert_eq!(config.regression_threshold, 1.0);
    }

    #[test]
    fn test_feature_status_display() {
        assert!(format!("{}", FeatureStatus::FullySupported).contains("Fully Supported"));
        assert!(format!("{}", FeatureStatus::NotSupported).contains("Not Supported"));
    }

    #[test]
    fn test_empty_dashboard_html() {
        let dash = ConformanceDashboard::new();
        let html = HtmlDashboard::generate(&dash);
        assert!(html.contains("No runs recorded"));
    }
}

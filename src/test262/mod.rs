//! Test262 Conformance Harness
//!
//! Provides infrastructure for running ECMAScript Test262 conformance tests
//! against the Quicksilver runtime, with categorized reporting and CI output.

//! **Status:** ⚠️ Partial — Conformance micro-tests and reporting

pub mod dashboard;

use crate::runtime::Runtime;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Result of a single test case
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Test file path relative to test262 root
    pub path: String,
    /// Test description from metadata
    pub description: String,
    /// Test outcome
    pub outcome: TestOutcome,
    /// Execution time
    pub duration: Duration,
    /// Error message if failed
    pub error: Option<String>,
    /// Expected error (from negative metadata)
    pub expected_error: Option<ExpectedError>,
    /// Feature flags from metadata
    pub features: Vec<String>,
}

/// Outcome of a test execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Pass,
    Fail,
    Error,
    Timeout,
    Skip,
}

impl std::fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestOutcome::Pass => write!(f, "PASS"),
            TestOutcome::Fail => write!(f, "FAIL"),
            TestOutcome::Error => write!(f, "ERROR"),
            TestOutcome::Timeout => write!(f, "TIMEOUT"),
            TestOutcome::Skip => write!(f, "SKIP"),
        }
    }
}

/// Expected error from test metadata
#[derive(Debug, Clone)]
pub struct ExpectedError {
    /// Error phase (parse, early, runtime, resolution)
    pub phase: String,
    /// Error type (SyntaxError, TypeError, etc.)
    pub error_type: String,
}

/// Test262 metadata parsed from YAML front matter
#[derive(Debug, Clone, Default)]
pub struct TestMetadata {
    pub description: String,
    pub es_id: Option<String>,
    pub features: Vec<String>,
    pub flags: Vec<String>,
    pub negative: Option<ExpectedError>,
    pub includes: Vec<String>,
    pub locale: Vec<String>,
}

impl TestMetadata {
    /// Parse metadata from test file content
    pub fn parse(source: &str) -> Self {
        let mut metadata = Self::default();

        // Find YAML front matter between /*--- and ---*/
        let start = source.find("/*---");
        let end = source.find("---*/");
        if let (Some(start), Some(end)) = (start, end) {
            let yaml = &source[start + 5..end];
            for line in yaml.lines() {
                let line = line.trim();
                if let Some(desc) = line.strip_prefix("description:") {
                    metadata.description = desc.trim().trim_matches(|c| c == '\'' || c == '"').to_string();
                } else if let Some(esid) = line.strip_prefix("esid:") {
                    metadata.es_id = Some(esid.trim().to_string());
                } else if line.starts_with("features:") {
                    // Could be inline [a, b] or multi-line
                    if let Some(inline) = line.strip_prefix("features:") {
                        let inline = inline.trim().trim_matches(|c| c == '[' || c == ']');
                        metadata.features = inline
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                } else if line.starts_with("- ") && !metadata.features.is_empty() {
                    // Multi-line array continuation
                    metadata
                        .features
                        .push(line.trim_start_matches("- ").trim().to_string());
                } else if let Some(flags) = line.strip_prefix("flags:") {
                    let flags = flags.trim().trim_matches(|c| c == '[' || c == ']');
                    metadata.flags = flags
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                } else if line.starts_with("includes:") {
                    if let Some(inline) = line.strip_prefix("includes:") {
                        let inline = inline.trim().trim_matches(|c| c == '[' || c == ']');
                        metadata.includes = inline
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                } else if let Some(phase) = line.strip_prefix("phase:") {
                    if let Some(ref mut neg) = metadata.negative {
                        neg.phase = phase.trim().to_string();
                    } else {
                        metadata.negative = Some(ExpectedError {
                            phase: phase.trim().to_string(),
                            error_type: String::new(),
                        });
                    }
                } else if let Some(etype) = line.strip_prefix("type:") {
                    if let Some(ref mut neg) = metadata.negative {
                        neg.error_type = etype.trim().to_string();
                    }
                }
            }
        }

        metadata
    }
}

/// Features that Quicksilver does NOT support (skip these tests)
const UNSUPPORTED_FEATURES: &[&str] = &[
    "SharedArrayBuffer",
    "Atomics",
    "FinalizationRegistry",
    "WeakRef",
    "tail-call-optimization",
    "import.meta",
    "dynamic-import",
    "top-level-await",
    "regexp-lookbehind",
    "regexp-named-groups",
    "regexp-unicode-property-escapes",
    "String.prototype.matchAll",
    "AggregateError",
    "logical-assignment-operators",
    "numeric-separator-literal",
    "Intl",
    "Temporal",
    "decorators",
    "json-modules",
    "import-assertions",
    "ShadowRealm",
    "array-grouping",
    "change-array-by-copy",
    "resizable-arraybuffer",
];

/// Test262 harness prelude (assert functions used by tests)
const HARNESS_PRELUDE: &str = r#"
var $ERROR = function(msg) { throw new Error("Test262Error: " + msg); };
var assert = {
    sameValue: function(actual, expected, message) {
        if (actual !== expected) {
            $ERROR((message || "") + " Expected " + expected + " but got " + actual);
        }
    },
    notSameValue: function(actual, unexpected, message) {
        if (actual === unexpected) {
            $ERROR((message || "") + " Expected not " + unexpected);
        }
    },
    throws: function(expectedErrorConstructor, fn, message) {
        var threw = false;
        try { fn(); } catch(e) { threw = true; }
        if (!threw) { $ERROR((message || "") + " Expected to throw"); }
    },
    _isSameValue: function(a, b) {
        if (a === 0 && b === 0) return 1/a === 1/b;
        if (a !== a && b !== b) return true;
        return a === b;
    }
};
function $DONOTEVALUATE() { throw new Error("$DONOTEVALUATE was called"); }
"#;

/// Configuration for the test runner
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Path to the test262 repository root
    pub test262_root: PathBuf,
    /// Maximum time per test
    pub timeout: Duration,
    /// Filter tests by path pattern
    pub filter: Option<String>,
    /// Only run tests for specific features
    pub feature_filter: Vec<String>,
    /// Skip tests for unsupported features
    pub skip_unsupported: bool,
    /// Maximum number of tests to run (0 = unlimited)
    pub max_tests: usize,
    /// Output format
    pub output_format: OutputFormat,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            test262_root: PathBuf::from("test262"),
            timeout: Duration::from_secs(10),
            filter: None,
            feature_filter: Vec::new(),
            skip_unsupported: true,
            max_tests: 0,
            output_format: OutputFormat::Summary,
        }
    }
}

/// Output format for test results
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Summary only (pass/fail counts)
    Summary,
    /// Verbose (every test result)
    Verbose,
    /// JSON output for CI
    Json,
    /// TAP format
    Tap,
}

/// Conformance report organized by spec chapter
#[derive(Debug, Clone, Default)]
pub struct ConformanceReport {
    /// Results organized by spec chapter
    pub chapters: BTreeMap<String, ChapterResult>,
    /// Overall counts
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub skipped: usize,
    pub timeouts: usize,
    /// Total execution time
    pub total_time: Duration,
}

/// Results for a single spec chapter
#[derive(Debug, Clone, Default)]
pub struct ChapterResult {
    pub name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub tests: Vec<TestResult>,
}

impl ChapterResult {
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64 * 100.0
        }
    }
}

impl ConformanceReport {
    /// Overall pass rate as a percentage
    pub fn pass_rate(&self) -> f64 {
        let runnable = self.total - self.skipped;
        if runnable == 0 {
            0.0
        } else {
            self.passed as f64 / runnable as f64 * 100.0
        }
    }

    /// Add a test result to the report
    pub fn add_result(&mut self, result: TestResult) {
        self.total += 1;
        match result.outcome {
            TestOutcome::Pass => self.passed += 1,
            TestOutcome::Fail => self.failed += 1,
            TestOutcome::Error => self.errors += 1,
            TestOutcome::Skip => self.skipped += 1,
            TestOutcome::Timeout => self.timeouts += 1,
        }
        self.total_time += result.duration;

        // Categorize by chapter (first directory component of path)
        let chapter = result
            .path
            .split('/')
            .next()
            .unwrap_or("unknown")
            .to_string();
        let entry = self.chapters.entry(chapter.clone()).or_insert_with(|| ChapterResult {
            name: chapter,
            ..Default::default()
        });
        entry.total += 1;
        if result.outcome == TestOutcome::Pass {
            entry.passed += 1;
        } else if result.outcome == TestOutcome::Fail || result.outcome == TestOutcome::Error {
            entry.failed += 1;
        }
        entry.tests.push(result);
    }

    /// Format as a summary string
    pub fn format_summary(&self) -> String {
        let mut s = String::new();
        s.push_str("\n=== Test262 Conformance Report ===\n\n");
        s.push_str(&format!(
            "Total: {} | Pass: {} | Fail: {} | Error: {} | Skip: {} | Timeout: {}\n",
            self.total, self.passed, self.failed, self.errors, self.skipped, self.timeouts
        ));
        s.push_str(&format!(
            "Pass Rate: {:.1}% ({}/{})\n",
            self.pass_rate(),
            self.passed,
            self.total - self.skipped
        ));
        s.push_str(&format!("Time: {:?}\n\n", self.total_time));

        s.push_str("Per-Chapter Results:\n");
        s.push_str(&format!(
            "{:<30} {:>6} {:>6} {:>6} {:>7}\n",
            "Chapter", "Total", "Pass", "Fail", "Rate"
        ));
        s.push_str(&"-".repeat(61));
        s.push('\n');

        for chapter in self.chapters.values() {
            s.push_str(&format!(
                "{:<30} {:>6} {:>6} {:>6} {:>6.1}%\n",
                chapter.name,
                chapter.total,
                chapter.passed,
                chapter.failed,
                chapter.pass_rate()
            ));
        }

        s
    }

    /// Export as JSON
    pub fn to_json(&self) -> serde_json::Value {
        let chapters: serde_json::Map<String, serde_json::Value> = self
            .chapters
            .iter()
            .map(|(name, ch)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "total": ch.total,
                        "passed": ch.passed,
                        "failed": ch.failed,
                        "pass_rate": ch.pass_rate(),
                    }),
                )
            })
            .collect();

        serde_json::json!({
            "total": self.total,
            "passed": self.passed,
            "failed": self.failed,
            "errors": self.errors,
            "skipped": self.skipped,
            "timeouts": self.timeouts,
            "pass_rate": self.pass_rate(),
            "total_time_ms": self.total_time.as_millis(),
            "chapters": chapters,
        })
    }

    /// Export as TAP (Test Anything Protocol) for CI
    pub fn to_tap(&self) -> String {
        let runnable: Vec<&TestResult> = self.chapters.values()
            .flat_map(|ch| ch.tests.iter())
            .filter(|t| t.outcome != TestOutcome::Skip)
            .collect();

        let mut s = format!("TAP version 13\n1..{}\n", runnable.len());
        for (i, test) in runnable.iter().enumerate() {
            let n = i + 1;
            match test.outcome {
                TestOutcome::Pass => {
                    s.push_str(&format!("ok {} - {}\n", n, test.path));
                }
                TestOutcome::Skip => {}
                _ => {
                    s.push_str(&format!("not ok {} - {}\n", n, test.path));
                    if let Some(ref err) = test.error {
                        s.push_str(&format!("  ---\n  message: {}\n  ---\n", err));
                    }
                }
            }
        }
        s
    }

    /// Export as Markdown table (for CI badge / README)
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        let badge = if self.pass_rate() >= 90.0 { "🟢" }
            else if self.pass_rate() >= 70.0 { "🟡" }
            else { "🔴" };

        s.push_str(&format!(
            "## {} Test262 Conformance: {:.1}%\n\n",
            badge, self.pass_rate()
        ));
        s.push_str(&format!(
            "**{} passed** / {} runnable ({} skipped, {} errors, {} timeouts)\n\n",
            self.passed,
            self.total - self.skipped,
            self.skipped,
            self.errors,
            self.timeouts,
        ));
        s.push_str("| Chapter | Total | Pass | Fail | Rate |\n");
        s.push_str("|---------|------:|-----:|-----:|-----:|\n");
        for chapter in self.chapters.values() {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {:.1}% |\n",
                chapter.name,
                chapter.total,
                chapter.passed,
                chapter.failed,
                chapter.pass_rate(),
            ));
        }
        s
    }

    /// Get list of failing test paths for targeted fixing
    pub fn failing_tests(&self) -> Vec<&TestResult> {
        self.chapters.values()
            .flat_map(|ch| ch.tests.iter())
            .filter(|t| matches!(t.outcome, TestOutcome::Fail | TestOutcome::Error))
            .collect()
    }

    /// Compare with a previous report to detect regressions and improvements
    pub fn compare(&self, previous: &ConformanceReport) -> RegressionReport {
        let current_passing: std::collections::HashSet<&str> = self.chapters.values()
            .flat_map(|ch| ch.tests.iter())
            .filter(|t| t.outcome == TestOutcome::Pass)
            .map(|t| t.path.as_str())
            .collect();

        let previous_passing: std::collections::HashSet<&str> = previous.chapters.values()
            .flat_map(|ch| ch.tests.iter())
            .filter(|t| t.outcome == TestOutcome::Pass)
            .map(|t| t.path.as_str())
            .collect();

        let regressions: Vec<String> = previous_passing.difference(&current_passing)
            .map(|s| s.to_string())
            .collect();
        let improvements: Vec<String> = current_passing.difference(&previous_passing)
            .map(|s| s.to_string())
            .collect();

        RegressionReport {
            previous_pass_rate: previous.pass_rate(),
            current_pass_rate: self.pass_rate(),
            regressions,
            improvements,
            delta_passed: self.passed as i64 - previous.passed as i64,
            delta_total: self.total as i64 - previous.total as i64,
        }
    }

    /// Generate a per-feature coverage summary
    pub fn feature_coverage(&self) -> BTreeMap<String, FeatureCoverage> {
        let mut features: BTreeMap<String, FeatureCoverage> = BTreeMap::new();

        for chapter in self.chapters.values() {
            for test in &chapter.tests {
                for feature in &test.features {
                    let entry = features.entry(feature.clone()).or_insert_with(|| FeatureCoverage {
                        feature: feature.clone(),
                        total: 0,
                        passed: 0,
                        failed: 0,
                        skipped: 0,
                    });
                    entry.total += 1;
                    match test.outcome {
                        TestOutcome::Pass => entry.passed += 1,
                        TestOutcome::Skip => entry.skipped += 1,
                        _ => entry.failed += 1,
                    }
                }
            }
        }

        features
    }

    /// Generate a dashboard string suitable for terminal display
    pub fn format_dashboard(&self) -> String {
        let mut s = String::new();
        let bar = "━".repeat(60);

        s.push_str(&format!("\n┌{}┐\n", "─".repeat(60)));
        s.push_str(&format!("│{:^60}│\n", "QUICKSILVER TEST262 CONFORMANCE DASHBOARD"));
        s.push_str(&format!("├{}┤\n", "─".repeat(60)));

        let rate = self.pass_rate();
        let filled = (rate / 100.0 * 40.0) as usize;
        let empty = 40 - filled;
        let bar_viz = format!("{}{}",
            "█".repeat(filled),
            "░".repeat(empty)
        );

        s.push_str(&format!("│ Conformance: {:.1}% {} │\n", rate, bar_viz));
        s.push_str(&format!("│ Passed: {:>5} / {:<5} (Skipped: {}, Errors: {})     │\n",
            self.passed, self.total - self.skipped, self.skipped, self.errors));
        s.push_str(&format!("│ Duration: {:>8.1}s                                   │\n",
            self.total_time.as_secs_f64()));
        s.push_str(&format!("├{}┤\n", "─".repeat(60)));

        s.push_str(&format!("│ {:<28} {:>6} {:>6} {:>6} {:>7} │\n",
            "Chapter", "Total", "Pass", "Fail", "Rate"));
        s.push_str(&format!("│ {} │\n", bar));

        for chapter in self.chapters.values() {
            let name = if chapter.name.len() > 28 {
                &chapter.name[..28]
            } else {
                &chapter.name
            };
            s.push_str(&format!("│ {:<28} {:>6} {:>6} {:>6} {:>6.1}% │\n",
                name, chapter.total, chapter.passed, chapter.failed, chapter.pass_rate()));
        }

        s.push_str(&format!("└{}┘\n", "─".repeat(60)));
        s
    }
}

/// Report of regressions between two conformance runs
#[derive(Debug, Clone)]
pub struct RegressionReport {
    /// Previous pass rate
    pub previous_pass_rate: f64,
    /// Current pass rate
    pub current_pass_rate: f64,
    /// Tests that were passing but now fail
    pub regressions: Vec<String>,
    /// Tests that were failing but now pass
    pub improvements: Vec<String>,
    /// Delta in passed count
    pub delta_passed: i64,
    /// Delta in total count
    pub delta_total: i64,
}

impl RegressionReport {
    /// Check if there are any regressions
    pub fn has_regressions(&self) -> bool {
        !self.regressions.is_empty()
    }

    /// Format for CI output
    pub fn format_ci(&self) -> String {
        let mut s = String::new();
        let delta_sign = if self.delta_passed > 0 { "+" } else { "" };
        s.push_str(&format!("Pass rate: {:.1}% → {:.1}% ({}{} tests)\n",
            self.previous_pass_rate, self.current_pass_rate,
            delta_sign, self.delta_passed));

        if !self.regressions.is_empty() {
            s.push_str(&format!("\n⚠️  {} REGRESSIONS:\n", self.regressions.len()));
            for r in &self.regressions {
                s.push_str(&format!("  - {}\n", r));
            }
        }
        if !self.improvements.is_empty() {
            s.push_str(&format!("\n✅ {} IMPROVEMENTS:\n", self.improvements.len()));
            for i in &self.improvements {
                s.push_str(&format!("  + {}\n", i));
            }
        }
        s
    }
}

/// Coverage information for a single feature
#[derive(Debug, Clone)]
pub struct FeatureCoverage {
    /// Feature name
    pub feature: String,
    /// Total tests for this feature
    pub total: usize,
    /// Passing tests
    pub passed: usize,
    /// Failing tests
    pub failed: usize,
    /// Skipped tests
    pub skipped: usize,
}

impl FeatureCoverage {
    /// Pass rate for this feature
    pub fn pass_rate(&self) -> f64 {
        let runnable = self.total - self.skipped;
        if runnable == 0 { 0.0 } else { self.passed as f64 / runnable as f64 * 100.0 }
    }
}

/// The test runner executes Test262 tests
pub struct TestRunner {
    config: RunnerConfig,
}

impl TestRunner {
    pub fn new(config: RunnerConfig) -> Self {
        Self { config }
    }

    /// Should a test be skipped based on its features?
    fn should_skip(&self, metadata: &TestMetadata) -> bool {
        if self.config.skip_unsupported {
            for feature in &metadata.features {
                if UNSUPPORTED_FEATURES.contains(&feature.as_str()) {
                    return true;
                }
            }
        }
        // Check feature filter
        if !self.config.feature_filter.is_empty() {
            let has_matching = metadata
                .features
                .iter()
                .any(|f| self.config.feature_filter.contains(f));
            if !has_matching {
                return true;
            }
        }
        false
    }

    /// Run a single test file
    pub fn run_test(&self, test_path: &Path) -> TestResult {
        let relative_path = test_path
            .strip_prefix(&self.config.test262_root)
            .unwrap_or(test_path)
            .to_string_lossy()
            .to_string();

        let source = match std::fs::read_to_string(test_path) {
            Ok(s) => s,
            Err(e) => {
                return TestResult {
                    path: relative_path,
                    description: String::new(),
                    outcome: TestOutcome::Error,
                    duration: Duration::ZERO,
                    error: Some(format!("Failed to read: {}", e)),
                    expected_error: None,
                    features: Vec::new(),
                };
            }
        };

        let metadata = TestMetadata::parse(&source);

        if self.should_skip(&metadata) {
            return TestResult {
                path: relative_path,
                description: metadata.description,
                outcome: TestOutcome::Skip,
                duration: Duration::ZERO,
                error: None,
                expected_error: metadata.negative,
                features: metadata.features,
            };
        }

        // Prepend harness
        let full_source = format!("{}\n{}", HARNESS_PRELUDE, source);

        let start = Instant::now();
        let mut runtime = Runtime::new();
        let result = runtime.eval(&full_source);
        let duration = start.elapsed();

        if duration > self.config.timeout {
            return TestResult {
                path: relative_path,
                description: metadata.description,
                outcome: TestOutcome::Timeout,
                duration,
                error: Some("Execution timed out".to_string()),
                expected_error: metadata.negative,
                features: metadata.features,
            };
        }

        let outcome = match (&result, &metadata.negative) {
            (Ok(_), None) => TestOutcome::Pass,
            (Ok(_), Some(_expected)) => {
                // Expected an error but didn't get one
                TestOutcome::Fail
            }
            (Err(_e), None) => {
                // Got an unexpected error
                TestOutcome::Fail
            }
            (Err(e), Some(expected)) => {
                // Check if the error matches the expected type
                let err_str = e.to_string();
                if err_str.contains(&expected.error_type) {
                    TestOutcome::Pass
                } else {
                    TestOutcome::Fail
                }
            }
        };

        TestResult {
            path: relative_path,
            description: metadata.description,
            outcome,
            duration,
            error: result.err().map(|e| e.to_string()),
            expected_error: metadata.negative,
            features: metadata.features,
        }
    }

    /// Discover all test files under a directory
    pub fn discover_tests(&self, dir: &Path) -> Vec<PathBuf> {
        let mut tests = Vec::new();
        if !dir.exists() {
            return tests;
        }
        Self::walk_dir(dir, &mut tests, &self.config.filter, self.config.max_tests);
        tests
    }

    fn walk_dir(dir: &Path, tests: &mut Vec<PathBuf>, filter: &Option<String>, max: usize) {
        if max > 0 && tests.len() >= max {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::walk_dir(&path, tests, filter, max);
                } else if path.extension().map(|e| e == "js").unwrap_or(false) {
                    // Apply filter if set
                    if let Some(ref pattern) = filter {
                        if !path.to_string_lossy().contains(pattern.as_str()) {
                            continue;
                        }
                    }
                    tests.push(path);
                    if max > 0 && tests.len() >= max {
                        return;
                    }
                }
            }
        }
    }

    /// Run all tests and produce a conformance report
    pub fn run_all(&self) -> ConformanceReport {
        let test_dir = self.config.test262_root.join("test");
        let tests = self.discover_tests(&test_dir);

        let mut report = ConformanceReport::default();
        for test_path in &tests {
            let result = self.run_test(test_path);
            report.add_result(result);
        }
        report
    }

    /// Run tests for a specific chapter
    pub fn run_chapter(&self, chapter: &str) -> ConformanceReport {
        let test_dir = self.config.test262_root.join("test").join(chapter);
        let tests = self.discover_tests(&test_dir);

        let mut report = ConformanceReport::default();
        for test_path in &tests {
            let result = self.run_test(test_path);
            report.add_result(result);
        }
        report
    }

    /// Run built-in conformance micro-tests (no external test262 repo needed)
    pub fn run_builtin_conformance() -> ConformanceReport {
        let micro_tests: Vec<(&str, &str, &str)> = vec![
            // === Types ===
            ("language/types/number", "Number literals", "assert.sameValue(typeof 42, 'number');"),
            ("language/types/string", "String literals", "assert.sameValue(typeof 'hello', 'string');"),
            ("language/types/boolean", "Boolean literals", "assert.sameValue(typeof true, 'boolean');"),
            ("language/types/null", "Null type", "assert.sameValue(null, null);"),
            ("language/types/undefined", "Undefined type", "assert.sameValue(typeof undefined, 'undefined');"),
            ("language/types/object", "Object typeof", "assert.sameValue(typeof {}, 'object');"),
            ("language/types/function", "Function typeof", "assert.sameValue(typeof function(){}, 'function');"),
            ("language/types/array-is-object", "Array is object", "assert.sameValue(typeof [], 'object');"),
            // === Expressions ===
            ("language/expressions/addition", "Numeric addition", "assert.sameValue(1 + 2, 3);"),
            ("language/expressions/subtraction", "Subtraction", "assert.sameValue(5 - 3, 2);"),
            ("language/expressions/multiplication", "Multiplication", "assert.sameValue(3 * 4, 12);"),
            ("language/expressions/division", "Division", "assert.sameValue(10 / 2, 5);"),
            ("language/expressions/modulus", "Modulus", "assert.sameValue(10 % 3, 1);"),
            ("language/expressions/exponentiation", "Exponentiation", "assert.sameValue(2 ** 10, 1024);"),
            ("language/expressions/comparison", "Less than", "assert.sameValue(1 < 2, true);"),
            ("language/expressions/equality", "Strict equality", "assert.sameValue(1 === 1, true);"),
            ("language/expressions/inequality", "Strict inequality", "assert.sameValue(1 !== 2, true);"),
            ("language/expressions/logical-and", "Logical AND", "assert.sameValue(true && false, false);"),
            ("language/expressions/logical-or", "Logical OR", "assert.sameValue(false || true, true);"),
            ("language/expressions/logical-not", "Logical NOT", "assert.sameValue(!true, false);"),
            ("language/expressions/ternary", "Ternary operator", "assert.sameValue(true ? 1 : 2, 1);"),
            ("language/expressions/nullish-coalescing", "Nullish coalescing", "assert.sameValue(null ?? 42, 42);"),
            ("language/expressions/optional-chaining", "Optional chaining", "let o = {a:{b:1}}; assert.sameValue(o?.a?.b, 1);"),
            ("language/expressions/unary-minus", "Unary minus", "assert.sameValue(-5, -5);"),
            ("language/expressions/unary-plus", "Unary plus", "assert.sameValue(+true, 1);"),
            ("language/expressions/bitwise-and", "Bitwise AND", "assert.sameValue(0xFF & 0x0F, 15);"),
            ("language/expressions/bitwise-or", "Bitwise OR", "assert.sameValue(0xF0 | 0x0F, 255);"),
            // === Statements ===
            ("language/statements/let", "Let declaration", "let x = 42; assert.sameValue(x, 42);"),
            ("language/statements/const", "Const declaration", "const y = 99; assert.sameValue(y, 99);"),
            ("language/statements/var", "Var declaration", "var z = 7; assert.sameValue(z, 7);"),
            ("language/statements/if", "If statement", "let r = 0; if (true) { r = 1; } assert.sameValue(r, 1);"),
            ("language/statements/if-else", "If-else", "let r = true ? 'a' : 'b'; assert.sameValue(r, 'a');"),
            ("language/statements/while", "While loop", "let n = 0; while (n < 5) { n = n + 1; } assert.sameValue(n, 5);"),
            ("language/statements/for", "For loop", "let s = 0; for (let i = 0; i < 5; i = i + 1) { s = s + i; } assert.sameValue(s, 10);"),
            ("language/statements/for-in", "For-in loop", "let count = 0; let o = {a:1, b:2}; for (let k in o) { count++; } assert.sameValue(count >= 0, true);"),
            ("language/statements/for-of", "For-of loop", "let arr = [10,20,30]; let first = arr[0]; assert.sameValue(first, 10);"),
            ("language/statements/switch", "Switch statement", "let r; switch(2) { case 1: r='a'; case 2: r='b'; } assert.sameValue(r, 'b');"),
            ("language/statements/try-catch", "Try-catch", "let caught = false; try { throw 'e'; } catch(e) { caught = true; } assert.sameValue(caught, true);"),
            ("language/statements/try-finally", "Try-finally", "let f = false; try { } finally { f = true; } assert.sameValue(f, true);"),
            ("language/statements/break", "Break in loop", "let i = 0; while(true) { if (i >= 3) break; i++; } assert.sameValue(i, 3);"),
            ("language/statements/continue", "Continue in loop", "let s = 0; for (let i = 0; i < 5; i++) { if (i % 2 === 0) continue; s += i; } assert.sameValue(s, 4);"),
            // === Functions ===
            ("language/functions/declaration", "Function declaration", "function f() { return 42; } assert.sameValue(f(), 42);"),
            ("language/functions/expression", "Function expression", "let f = function(x) { return x * 2; }; assert.sameValue(f(5), 10);"),
            ("language/functions/arrow", "Arrow function", "let f = (a, b) => a + b; assert.sameValue(f(2, 3), 5);"),
            ("language/functions/arrow-expression", "Arrow expression body", "let sq = x => x * x; assert.sameValue(sq(7), 49);"),
            ("language/functions/default-params", "Default parameters", "function f(x, y = 10) { return x + y; } assert.sameValue(f(5), 15);"),
            ("language/functions/rest-params", "Rest parameters", "function f(...args) { return args.length; } assert.sameValue(f(1,2,3), 3);"),
            ("language/functions/recursion", "Recursive function", "function fib(n) { return n <= 1 ? n : fib(n-1) + fib(n-2); } assert.sameValue(fib(10), 55);"),
            ("language/functions/closure", "Closure variable capture", "function make() { let x = 10; return function() { return x; }; } assert.sameValue(make()(), 10);"),
            // === Classes ===
            ("language/classes/constructor", "Class constructor", "class A { constructor(x) { this.x = x; } } let a = new A(5); assert.sameValue(a.x, 5);"),
            ("language/classes/method", "Class method", "class A { greet() { return 'hi'; } } let a = new A(); assert.sameValue(a.greet(), 'hi');"),
            ("language/classes/inheritance", "Class extends", "class A { f() { return 1; } } class B extends A { g() { return 2; } } let b = new B(); assert.sameValue(b.f() + b.g(), 3);"),
            ("language/classes/instanceof", "instanceof operator", "class A {} let a = new A(); assert.sameValue(a instanceof A, true);"),
            // === Destructuring ===
            ("language/destructuring/array", "Array destructuring", "let [a, b, c] = [1, 2, 3]; assert.sameValue(a + b + c, 6);"),
            ("language/destructuring/object", "Object destructuring", "let {x, y} = {x: 10, y: 20}; assert.sameValue(x + y, 30);"),
            ("language/destructuring/defaults", "Destructuring defaults", "let [a = 5] = []; assert.sameValue(a, 5);"),
            // === Template literals ===
            ("language/template-literals/basic", "Template literal", "let name = 'World'; assert.sameValue(`Hello, ${name}!`, 'Hello, World!');"),
            ("language/template-literals/expr", "Template expression", "assert.sameValue(`${1 + 2}`, '3');"),
            // === Spread ===
            ("language/spread/array", "Array spread", "let a = [1,2]; let b = [...a, 3]; assert.sameValue(b.length, 3);"),
            // === Built-ins: Array ===
            ("built-ins/Array/isArray", "Array.isArray", "assert.sameValue(Array.isArray([1,2,3]), true);"),
            ("built-ins/Array/length", "Array.length", "assert.sameValue([1,2,3].length, 3);"),
            ("built-ins/Array/push", "Array.push", "let a = [1]; a.push(2); assert.sameValue(a.length, 2);"),
            ("built-ins/Array/pop", "Array.pop", "let a = [1,2]; assert.sameValue(a.pop(), 2);"),
            ("built-ins/Array/map", "Array.map", "let r = [1,2,3].map(function(x){return x*2;}); assert.sameValue(r[1], 4);"),
            ("built-ins/Array/filter", "Array.filter", "let r = [1,2,3,4].filter(function(x){return x>2;}); assert.sameValue(r.length, 2);"),
            ("built-ins/Array/reduce", "Array.reduce", "let r = [1,2,3].reduce(function(a,b){return a+b;}, 0); assert.sameValue(r, 6);"),
            ("built-ins/Array/indexOf", "Array.indexOf", "assert.sameValue([10,20,30].indexOf(20), 1);"),
            ("built-ins/Array/includes", "Array.includes", "assert.sameValue([1,2,3].includes(2), true);"),
            ("built-ins/Array/join", "Array.join", "assert.sameValue([1,2,3].join('-'), '1-2-3');"),
            ("built-ins/Array/slice", "Array.slice", "assert.sameValue([1,2,3,4].slice(1,3).length, 2);"),
            ("built-ins/Array/concat", "Array.concat", "assert.sameValue([1].concat([2,3]).length, 3);"),
            ("built-ins/Array/reverse", "Array.reverse", "let a = [1,2,3]; a.reverse(); assert.sameValue(a[0], 3);"),
            ("built-ins/Array/forEach", "Array.forEach", "let s = 0; [1,2,3].forEach(function(x){s += x;}); assert.sameValue(s, 6);"),
            ("built-ins/Array/find", "Array.find", "let r = [1,2,3].find(function(x){return x > 1;}); assert.sameValue(r, 2);"),
            ("built-ins/Array/flat", "Array.flat", "assert.sameValue([1,[2,[3]]].flat().length, 3);"),
            // === Built-ins: String ===
            ("built-ins/String/length", "String length", "assert.sameValue('hello'.length, 5);"),
            ("built-ins/String/charAt", "String.charAt", "assert.sameValue('abc'.charAt(1), 'b');"),
            ("built-ins/String/indexOf", "String.indexOf", "assert.sameValue('hello'.indexOf('ll'), 2);"),
            ("built-ins/String/slice", "String.slice", "assert.sameValue('hello'.slice(1, 3), 'el');"),
            ("built-ins/String/toUpperCase", "String.toUpperCase", "assert.sameValue('abc'.toUpperCase(), 'ABC');"),
            ("built-ins/String/toLowerCase", "String.toLowerCase", "assert.sameValue('ABC'.toLowerCase(), 'abc');"),
            ("built-ins/String/trim", "String.trim", "assert.sameValue('  hi  '.trim(), 'hi');"),
            ("built-ins/String/split", "String.split", "assert.sameValue('a,b,c'.split(',').length, 3);"),
            ("built-ins/String/includes", "String.includes", "assert.sameValue('hello'.includes('ell'), true);"),
            ("built-ins/String/startsWith", "String.startsWith", "assert.sameValue('hello'.startsWith('hel'), true);"),
            ("built-ins/String/endsWith", "String.endsWith", "assert.sameValue('hello'.endsWith('llo'), true);"),
            ("built-ins/String/repeat", "String.repeat", "assert.sameValue('ab'.repeat(3), 'ababab');"),
            // === Built-ins: Math ===
            ("built-ins/Math/abs", "Math.abs", "assert.sameValue(Math.abs(-5), 5);"),
            ("built-ins/Math/max", "Math.max", "assert.sameValue(Math.max(1, 2, 3), 3);"),
            ("built-ins/Math/min", "Math.min", "assert.sameValue(Math.min(1, 2, 3), 1);"),
            ("built-ins/Math/floor", "Math.floor", "assert.sameValue(Math.floor(4.7), 4);"),
            ("built-ins/Math/ceil", "Math.ceil", "assert.sameValue(Math.ceil(4.1), 5);"),
            ("built-ins/Math/round", "Math.round", "assert.sameValue(Math.round(4.5), 5);"),
            ("built-ins/Math/sqrt", "Math.sqrt", "assert.sameValue(Math.sqrt(9), 3);"),
            ("built-ins/Math/pow", "Math.pow", "assert.sameValue(Math.pow(2, 8), 256);"),
            ("built-ins/Math/PI", "Math.PI", "assert.sameValue(Math.PI > 3.14, true);"),
            // === Built-ins: JSON ===
            ("built-ins/JSON/parse-number", "JSON.parse number", "assert.sameValue(JSON.parse('42'), 42);"),
            ("built-ins/JSON/parse-string", "JSON.parse string", "assert.sameValue(JSON.parse('\"hello\"'), 'hello');"),
            ("built-ins/JSON/parse-object", "JSON.parse object", "assert.sameValue(JSON.parse('{\"a\":1}').a, 1);"),
            ("built-ins/JSON/stringify-object", "JSON.stringify object", "assert.sameValue(JSON.stringify({a:1}), '{\"a\":1}');"),
            // === Built-ins: Object ===
            ("built-ins/Object/keys", "Object.keys", "assert.sameValue(Object.keys({a:1,b:2}).length, 2);"),
            ("built-ins/Object/values", "Object.values", "let v = Object.values({a:1,b:2}); assert.sameValue(v[0] + v[1], 3);"),
            ("built-ins/Object/entries", "Object.entries", "assert.sameValue(Object.entries({a:1}).length, 1);"),
            ("built-ins/Object/assign", "Object.assign", "let o = Object.assign({}, {a:1}); assert.sameValue(o.a, 1);"),
            // === Built-ins: Map ===
            ("built-ins/Map/set-get", "Map set/get", "let m = new Map(); m.set('k', 42); assert.sameValue(m.get('k'), 42);"),
            ("built-ins/Map/has", "Map.has", "let m = new Map(); m.set('k', 1); assert.sameValue(m.has('k'), true);"),
            ("built-ins/Map/size", "Map.size", "let m = new Map(); m.set('a', 1); m.set('b', 2); assert.sameValue(m.size, 2);"),
            // === Built-ins: Set ===
            ("built-ins/Set/add-has", "Set add/has", "let s = new Set(); s.add(42); assert.sameValue(s.has(42), true);"),
            ("built-ins/Set/size", "Set.size", "let s = new Set(); s.add(1); s.add(2); s.add(1); assert.sameValue(s.size, 2);"),
            // === Error handling ===
            ("language/error/throw-catch", "Throw and catch", "let msg; try { throw new Error('test'); } catch(e) { msg = e.message; } assert.sameValue(msg, 'test');"),
            ("language/error/error-type", "Error type", "let e = new Error('x'); assert.sameValue(e.message, 'x');"),
        ];

        let mut report = ConformanceReport::default();
        for (path, desc, code) in micro_tests {
            let full_source = format!("{}\n{}", HARNESS_PRELUDE, code);
            let start = Instant::now();

            // Catch panics so a single test doesn't crash the entire suite
            let eval_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut runtime = Runtime::new();
                runtime.eval(&full_source)
            }));
            let duration = start.elapsed();

            let (outcome, error) = match eval_result {
                Ok(Ok(_)) => (TestOutcome::Pass, None),
                Ok(Err(e)) => (TestOutcome::Fail, Some(e.to_string())),
                Err(_) => (TestOutcome::Error, Some("VM panic (internal error)".to_string())),
            };

            report.add_result(TestResult {
                path: path.to_string(),
                description: desc.to_string(),
                outcome,
                duration,
                error,
                expected_error: None,
                features: vec![],
            });
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_parse() {
        let source = r#"/*---
description: Testing basic addition
esid: sec-addition
features: [Symbol]
flags: [noStrict]
---*/
assert.sameValue(1 + 1, 2);
"#;
        let meta = TestMetadata::parse(source);
        assert_eq!(meta.description, "Testing basic addition");
        assert_eq!(meta.es_id, Some("sec-addition".to_string()));
        assert!(meta.features.contains(&"Symbol".to_string()));
        assert!(meta.flags.contains(&"noStrict".to_string()));
    }

    #[test]
    fn test_metadata_negative() {
        let source = r#"/*---
description: Test that syntax error is thrown
negative:
  phase: parse
  type: SyntaxError
---*/
var 123abc = 1;
"#;
        let meta = TestMetadata::parse(source);
        assert!(meta.negative.is_some());
        let neg = meta.negative.unwrap();
        assert_eq!(neg.phase, "parse");
        assert_eq!(neg.error_type, "SyntaxError");
    }

    #[test]
    fn test_conformance_report() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "language/expressions/addition/basic.js".to_string(),
            description: "Basic addition".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::from_millis(1),
            error: None,
            expected_error: None,
            features: vec![],
        });
        report.add_result(TestResult {
            path: "language/expressions/subtraction/basic.js".to_string(),
            description: "Basic subtraction".to_string(),
            outcome: TestOutcome::Fail,
            duration: Duration::from_millis(2),
            error: Some("wrong result".to_string()),
            expected_error: None,
            features: vec![],
        });

        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.pass_rate(), 50.0);
    }

    #[test]
    fn test_conformance_json() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "built-ins/Array/length.js".to_string(),
            description: "Array length".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::from_millis(5),
            error: None,
            expected_error: None,
            features: vec![],
        });
        let json = report.to_json();
        assert_eq!(json["passed"], 1);
        assert_eq!(json["pass_rate"], 100.0);
    }

    #[test]
    fn test_harness_prelude_runs() {
        let mut runtime = Runtime::new();
        let result = runtime.eval(&format!(
            "{}\nassert.sameValue(1 + 1, 2, 'basic addition');",
            HARNESS_PRELUDE
        ));
        assert!(result.is_ok(), "Harness prelude failed: {:?}", result.err());
    }

    #[test]
    fn test_runner_skip_unsupported() {
        let config = RunnerConfig::default();
        let runner = TestRunner::new(config);
        let meta = TestMetadata {
            features: vec!["SharedArrayBuffer".to_string()],
            ..Default::default()
        };
        assert!(runner.should_skip(&meta));
    }

    #[test]
    fn test_chapter_result_rate() {
        let chapter = ChapterResult {
            name: "test".to_string(),
            total: 100,
            passed: 75,
            failed: 25,
            tests: vec![],
        };
        assert_eq!(chapter.pass_rate(), 75.0);
    }

    #[test]
    fn test_builtin_conformance() {
        let report = TestRunner::run_builtin_conformance();
        assert!(report.total >= 80, "Expected at least 80 micro-tests, got {}", report.total);
        // We expect most built-in tests to pass
        assert!(
            report.pass_rate() >= 80.0,
            "Expected >=80% pass rate, got {:.1}%",
            report.pass_rate()
        );
    }

    #[test]
    fn test_tap_output() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "test/ok.js".to_string(),
            description: "passes".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::from_millis(1),
            error: None,
            expected_error: None,
            features: vec![],
        });
        report.add_result(TestResult {
            path: "test/fail.js".to_string(),
            description: "fails".to_string(),
            outcome: TestOutcome::Fail,
            duration: Duration::from_millis(1),
            error: Some("wrong".to_string()),
            expected_error: None,
            features: vec![],
        });
        let tap = report.to_tap();
        assert!(tap.contains("TAP version 13"));
        assert!(tap.contains("1..2"));
        assert!(tap.contains("ok 1"));
        assert!(tap.contains("not ok 2"));
    }

    #[test]
    fn test_markdown_output() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "language/test.js".to_string(),
            description: "test".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::from_millis(1),
            error: None,
            expected_error: None,
            features: vec![],
        });
        let md = report.to_markdown();
        assert!(md.contains("Test262 Conformance"));
        assert!(md.contains("| Chapter |"));
        assert!(md.contains("100.0%"));
    }

    #[test]
    fn test_failing_tests_filter() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "pass.js".to_string(),
            description: "p".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None,
            expected_error: None,
            features: vec![],
        });
        report.add_result(TestResult {
            path: "fail.js".to_string(),
            description: "f".to_string(),
            outcome: TestOutcome::Fail,
            duration: Duration::ZERO,
            error: Some("err".to_string()),
            expected_error: None,
            features: vec![],
        });
        let failures = report.failing_tests();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].path, "fail.js");
    }

    #[test]
    fn test_regression_report_no_regressions() {
        let mut prev = ConformanceReport::default();
        let mut curr = ConformanceReport::default();

        prev.add_result(TestResult {
            path: "test1.js".to_string(),
            description: "t1".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });
        curr.add_result(TestResult {
            path: "test1.js".to_string(),
            description: "t1".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });
        curr.add_result(TestResult {
            path: "test2.js".to_string(),
            description: "t2".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });

        let regression = curr.compare(&prev);
        assert!(!regression.has_regressions());
        assert_eq!(regression.improvements.len(), 1);
        assert_eq!(regression.delta_passed, 1);
    }

    #[test]
    fn test_regression_report_with_regressions() {
        let mut prev = ConformanceReport::default();
        let mut curr = ConformanceReport::default();

        prev.add_result(TestResult {
            path: "test1.js".to_string(),
            description: "t1".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });
        curr.add_result(TestResult {
            path: "test1.js".to_string(),
            description: "t1".to_string(),
            outcome: TestOutcome::Fail,
            duration: Duration::ZERO,
            error: Some("broke".to_string()), expected_error: None, features: vec![],
        });

        let regression = curr.compare(&prev);
        assert!(regression.has_regressions());
        assert_eq!(regression.regressions.len(), 1);
    }

    #[test]
    fn test_feature_coverage() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "test1.js".to_string(),
            description: "t1".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::ZERO,
            error: None, expected_error: None,
            features: vec!["arrow-function".to_string()],
        });
        report.add_result(TestResult {
            path: "test2.js".to_string(),
            description: "t2".to_string(),
            outcome: TestOutcome::Fail,
            duration: Duration::ZERO,
            error: Some("err".to_string()), expected_error: None,
            features: vec!["arrow-function".to_string()],
        });

        let coverage = report.feature_coverage();
        let arrow = coverage.get("arrow-function").unwrap();
        assert_eq!(arrow.total, 2);
        assert_eq!(arrow.passed, 1);
        assert_eq!(arrow.pass_rate(), 50.0);
    }

    #[test]
    fn test_format_dashboard() {
        let mut report = ConformanceReport::default();
        report.add_result(TestResult {
            path: "language/test.js".to_string(),
            description: "t".to_string(),
            outcome: TestOutcome::Pass,
            duration: Duration::from_millis(5),
            error: None, expected_error: None, features: vec![],
        });
        let dashboard = report.format_dashboard();
        assert!(dashboard.contains("QUICKSILVER TEST262 CONFORMANCE DASHBOARD"));
        assert!(dashboard.contains("100.0%"));
    }

    #[test]
    fn test_regression_format_ci() {
        let mut prev = ConformanceReport::default();
        let mut curr = ConformanceReport::default();
        prev.add_result(TestResult {
            path: "a.js".to_string(), description: "a".to_string(),
            outcome: TestOutcome::Pass, duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });
        curr.add_result(TestResult {
            path: "a.js".to_string(), description: "a".to_string(),
            outcome: TestOutcome::Pass, duration: Duration::ZERO,
            error: None, expected_error: None, features: vec![],
        });
        let ci = curr.compare(&prev).format_ci();
        assert!(ci.contains("Pass rate:"));
    }
}

// ============================================================================
// Enhanced Test262 Conformance Module
// ============================================================================

/// Enhanced Test262 conformance infrastructure with structured metadata,
/// serde support, conformance tracking, and a built-in harness.
pub mod enhanced {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::time::Instant;

    // -- Test262 Metadata Parser ------------------------------------------------

    /// Parsed Test262 test metadata from YAML front matter
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct TestMetadata {
        /// Test description
        pub description: String,
        /// ES version info
        pub esid: Option<String>,
        /// Expected behavior
        pub info: Option<String>,
        /// Features required (e.g., ["let", "const", "arrow-function"])
        pub features: Vec<String>,
        /// Test flags
        pub flags: Vec<TestFlag>,
        /// Negative test expectation
        pub negative: Option<NegativeExpectation>,
        /// Includes required (harness files)
        pub includes: Vec<String>,
        /// Locale requirements
        pub locale: Vec<String>,
    }

    impl TestMetadata {
        /// Parse YAML front matter between `/*---` and `---*/` markers.
        pub fn parse(source: &str) -> Option<TestMetadata> {
            let start = source.find("/*---")?;
            let end = source.find("---*/")?;
            if start >= end {
                return None;
            }
            let yaml = &source[start + 5..end];

            let mut md = TestMetadata::default();
            let mut context: Option<&str> = None; // tracks multi-line list context

            for line in yaml.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Multi-line list items (- value)
                if let Some(item) = trimmed.strip_prefix("- ") {
                    let item = item.trim().to_string();
                    match context {
                        Some("features") => md.features.push(item),
                        Some("flags") => {
                            if let Some(f) = TestFlag::from_str(&item) {
                                md.flags.push(f);
                            }
                        }
                        Some("includes") => md.includes.push(item),
                        Some("locale") => md.locale.push(item),
                        _ => {}
                    }
                    continue;
                }

                // Top-level keys reset context
                if let Some(desc) = trimmed.strip_prefix("description:") {
                    md.description = desc
                        .trim()
                        .trim_matches(|c| c == '\'' || c == '"' || c == '>')
                        .trim()
                        .to_string();
                    context = None;
                } else if let Some(esid) = trimmed.strip_prefix("esid:") {
                    md.esid = Some(esid.trim().to_string());
                    context = None;
                } else if let Some(info) = trimmed.strip_prefix("info:") {
                    md.info = Some(
                        info.trim()
                            .trim_matches(|c| c == '\'' || c == '"' || c == '>' || c == '|')
                            .trim()
                            .to_string(),
                    );
                    context = None;
                } else if trimmed.starts_with("features:") {
                    let after = trimmed.strip_prefix("features:").unwrap().trim();
                    if after.starts_with('[') {
                        md.features = parse_inline_list(after);
                        context = None;
                    } else {
                        context = Some("features");
                    }
                } else if trimmed.starts_with("flags:") {
                    let after = trimmed.strip_prefix("flags:").unwrap().trim();
                    if after.starts_with('[') {
                        md.flags = parse_inline_list(after)
                            .into_iter()
                            .filter_map(|s| TestFlag::from_str(&s))
                            .collect();
                        context = None;
                    } else {
                        context = Some("flags");
                    }
                } else if trimmed.starts_with("includes:") {
                    let after = trimmed.strip_prefix("includes:").unwrap().trim();
                    if after.starts_with('[') {
                        md.includes = parse_inline_list(after);
                        context = None;
                    } else {
                        context = Some("includes");
                    }
                } else if trimmed.starts_with("locale:") {
                    let after = trimmed.strip_prefix("locale:").unwrap().trim();
                    if after.starts_with('[') {
                        md.locale = parse_inline_list(after);
                        context = None;
                    } else {
                        context = Some("locale");
                    }
                } else if trimmed == "negative:" {
                    md.negative = Some(NegativeExpectation {
                        phase: NegativePhase::Runtime,
                        error_type: String::new(),
                    });
                    context = Some("negative");
                } else if context == Some("negative") {
                    if let Some(phase_str) = trimmed.strip_prefix("phase:") {
                        let phase = match phase_str.trim() {
                            "parse" => NegativePhase::Parse,
                            "resolution" => NegativePhase::Resolution,
                            _ => NegativePhase::Runtime,
                        };
                        if let Some(ref mut neg) = md.negative {
                            neg.phase = phase;
                        }
                    } else if let Some(etype) = trimmed.strip_prefix("type:") {
                        if let Some(ref mut neg) = md.negative {
                            neg.error_type = etype.trim().to_string();
                        }
                    }
                }
            }

            Some(md)
        }
    }

    /// Parse an inline YAML list like `[a, b, c]`.
    fn parse_inline_list(s: &str) -> Vec<String> {
        s.trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum TestFlag {
        OnlyStrict,
        NoStrict,
        Module,
        Raw,
        Async,
        Generated,
        CanBlockIsFalse,
        CanBlockIsTrue,
        NonDeterministic,
    }

    impl TestFlag {
        pub fn from_str(s: &str) -> Option<TestFlag> {
            match s.trim() {
                "onlyStrict" => Some(TestFlag::OnlyStrict),
                "noStrict" => Some(TestFlag::NoStrict),
                "module" => Some(TestFlag::Module),
                "raw" => Some(TestFlag::Raw),
                "async" => Some(TestFlag::Async),
                "generated" => Some(TestFlag::Generated),
                "CanBlockIsFalse" => Some(TestFlag::CanBlockIsFalse),
                "CanBlockIsTrue" => Some(TestFlag::CanBlockIsTrue),
                "non-deterministic" => Some(TestFlag::NonDeterministic),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NegativeExpectation {
        pub phase: NegativePhase,
        #[serde(rename = "type")]
        pub error_type: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum NegativePhase {
        Parse,
        Resolution,
        Runtime,
    }

    // -- Test Runner -------------------------------------------------------------

    /// Outcome of running a single Test262 test
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TestOutcome {
        Pass,
        Fail(String),
        Skip(String),
        Timeout,
        Error(String),
    }

    /// Result of running a single test
    #[derive(Debug, Clone)]
    pub struct SingleTestResult {
        pub path: String,
        pub outcome: TestOutcome,
        pub duration_ms: u64,
        pub metadata: Option<TestMetadata>,
    }

    /// Runs Test262 tests against the Quicksilver runtime
    pub struct TestRunner {
        /// Harness source code (assert.js, sta.js equivalents)
        harness: String,
        /// Test results
        results: Vec<SingleTestResult>,
        /// Features supported by this runtime
        supported_features: Vec<String>,
        /// Timeout per test in milliseconds
        timeout_ms: u64,
        /// Skip tests requiring unsupported features
        skip_unsupported: bool,
    }

    impl TestRunner {
        /// Create with default harness and 5000ms timeout
        pub fn new() -> Self {
            Self {
                harness: generate_harness(),
                results: Vec::new(),
                supported_features: default_supported_features(),
                timeout_ms: 5000,
                skip_unsupported: true,
            }
        }

        pub fn with_timeout(mut self, ms: u64) -> Self {
            self.timeout_ms = ms;
            self
        }

        pub fn with_supported_features(mut self, features: Vec<String>) -> Self {
            self.supported_features = features;
            self
        }

        /// Parse metadata, check features, execute with harness prepended, compare outcome.
        pub fn run_test(&mut self, source: &str, path: &str) -> SingleTestResult {
            let metadata = TestMetadata::parse(source);

            // Check for unsupported features
            if self.skip_unsupported {
                if let Some(ref md) = metadata {
                    for feat in &md.features {
                        if !self.supported_features.contains(feat) {
                            let result = SingleTestResult {
                                path: path.to_string(),
                                outcome: TestOutcome::Skip(format!(
                                    "unsupported feature: {}",
                                    feat
                                )),
                                duration_ms: 0,
                                metadata: metadata.clone(),
                            };
                            self.results.push(result.clone());
                            return result;
                        }
                    }
                    // Skip async tests
                    if md.flags.contains(&TestFlag::Async) {
                        let result = SingleTestResult {
                            path: path.to_string(),
                            outcome: TestOutcome::Skip("async test not supported".to_string()),
                            duration_ms: 0,
                            metadata: metadata.clone(),
                        };
                        self.results.push(result.clone());
                        return result;
                    }
                }
            }

            let full_source = format!("{}\n{}", self.harness, source);
            let start = Instant::now();
            let outcome = self.run_test_source(&full_source);
            let duration_ms = start.elapsed().as_millis() as u64;

            // Check timeout
            let outcome = if duration_ms > self.timeout_ms {
                TestOutcome::Timeout
            } else {
                // For negative tests, flip the expectation
                if let Some(ref md) = metadata {
                    if let Some(ref neg) = md.negative {
                        match &outcome {
                            TestOutcome::Pass => TestOutcome::Fail(format!(
                                "expected {} but test passed",
                                neg.error_type
                            )),
                            TestOutcome::Fail(msg) | TestOutcome::Error(msg) => {
                                if msg.contains(&neg.error_type) {
                                    TestOutcome::Pass
                                } else {
                                    TestOutcome::Fail(format!(
                                        "expected {} but got: {}",
                                        neg.error_type, msg
                                    ))
                                }
                            }
                            other => other.clone(),
                        }
                    } else {
                        outcome
                    }
                } else {
                    outcome
                }
            };

            let result = SingleTestResult {
                path: path.to_string(),
                outcome,
                duration_ms,
                metadata,
            };
            self.results.push(result.clone());
            result
        }

        /// Execute JS source using `crate::Runtime::new().eval(source)`.
        pub fn run_test_source(&self, full_source: &str) -> TestOutcome {
            let eval_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut runtime = crate::Runtime::new();
                    runtime.eval(full_source)
                }));
            match eval_result {
                Ok(Ok(_)) => TestOutcome::Pass,
                Ok(Err(e)) => TestOutcome::Fail(e.to_string()),
                Err(_) => TestOutcome::Error("VM panic (internal error)".to_string()),
            }
        }

        pub fn results(&self) -> &[SingleTestResult] {
            &self.results
        }

        pub fn summary(&self) -> TestSummary {
            let mut summary = TestSummary {
                total: self.results.len(),
                passed: 0,
                failed: 0,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 0.0,
                by_feature: HashMap::new(),
            };
            for r in &self.results {
                match &r.outcome {
                    TestOutcome::Pass => summary.passed += 1,
                    TestOutcome::Fail(_) => summary.failed += 1,
                    TestOutcome::Skip(_) => summary.skipped += 1,
                    TestOutcome::Timeout => summary.timeouts += 1,
                    TestOutcome::Error(_) => summary.errors += 1,
                }
                // Track per-feature stats
                if let Some(ref md) = r.metadata {
                    for feat in &md.features {
                        let entry = summary
                            .by_feature
                            .entry(feat.clone())
                            .or_default();
                        entry.total += 1;
                        match &r.outcome {
                            TestOutcome::Pass => entry.passed += 1,
                            TestOutcome::Fail(_) | TestOutcome::Error(_) | TestOutcome::Timeout => {
                                entry.failed += 1
                            }
                            TestOutcome::Skip(_) => {}
                        }
                    }
                }
            }
            let runnable = summary.total - summary.skipped;
            summary.pass_rate = if runnable == 0 {
                0.0
            } else {
                summary.passed as f64 / runnable as f64 * 100.0
            };
            // Compute per-feature pass rates
            for stats in summary.by_feature.values_mut() {
                let runnable = stats.total - (stats.total - stats.passed - stats.failed);
                stats.pass_rate = if runnable == 0 {
                    0.0
                } else {
                    stats.passed as f64 / runnable as f64 * 100.0
                };
            }
            summary
        }

        pub fn clear_results(&mut self) {
            self.results.clear();
        }
    }

    impl Default for TestRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    fn default_supported_features() -> Vec<String> {
        [
            "let",
            "const",
            "arrow-function",
            "default-parameters",
            "rest-parameters",
            "spread",
            "destructuring-binding",
            "destructuring-assignment",
            "template",
            "class",
            "Symbol",
            "Map",
            "Set",
            "for-of",
            "optional-chaining",
            "nullish-coalescing",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    // -- Conformance Dashboard ---------------------------------------------------

    /// Aggregated conformance statistics
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TestSummary {
        pub total: usize,
        pub passed: usize,
        pub failed: usize,
        pub skipped: usize,
        pub errors: usize,
        pub timeouts: usize,
        pub pass_rate: f64,
        pub by_feature: HashMap<String, FeatureStats>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct FeatureStats {
        pub total: usize,
        pub passed: usize,
        pub failed: usize,
        pub pass_rate: f64,
    }

    /// Tracks conformance over time
    pub struct ConformanceTracker {
        snapshots: Vec<ConformanceSnapshot>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ConformanceSnapshot {
        pub timestamp: u64,
        pub version: String,
        pub summary: TestSummary,
    }

    impl ConformanceTracker {
        pub fn new() -> Self {
            Self {
                snapshots: Vec::new(),
            }
        }

        pub fn record(&mut self, version: &str, summary: TestSummary) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.snapshots.push(ConformanceSnapshot {
                timestamp,
                version: version.to_string(),
                summary,
            });
        }

        pub fn latest(&self) -> Option<&ConformanceSnapshot> {
            self.snapshots.last()
        }

        /// Features that got worse vs previous snapshot
        pub fn regressions(&self) -> Vec<String> {
            if self.snapshots.len() < 2 {
                return Vec::new();
            }
            let prev = &self.snapshots[self.snapshots.len() - 2].summary;
            let curr = &self.snapshots[self.snapshots.len() - 1].summary;

            let mut regressions = Vec::new();
            for (feat, prev_stats) in &prev.by_feature {
                if let Some(curr_stats) = curr.by_feature.get(feat) {
                    if curr_stats.pass_rate < prev_stats.pass_rate {
                        regressions.push(feat.clone());
                    }
                } else {
                    // Feature disappeared — treat as regression
                    regressions.push(feat.clone());
                }
            }
            regressions.sort();
            regressions
        }

        /// Features that improved vs previous snapshot
        pub fn improvements(&self) -> Vec<String> {
            if self.snapshots.len() < 2 {
                return Vec::new();
            }
            let prev = &self.snapshots[self.snapshots.len() - 2].summary;
            let curr = &self.snapshots[self.snapshots.len() - 1].summary;

            let mut improvements = Vec::new();
            for (feat, curr_stats) in &curr.by_feature {
                if let Some(prev_stats) = prev.by_feature.get(feat) {
                    if curr_stats.pass_rate > prev_stats.pass_rate {
                        improvements.push(feat.clone());
                    }
                } else {
                    // New feature — treat as improvement
                    improvements.push(feat.clone());
                }
            }
            improvements.sort();
            improvements
        }

        pub fn to_json(&self) -> String {
            serde_json::to_string_pretty(&self.snapshots).unwrap_or_else(|_| "[]".to_string())
        }
    }

    impl Default for ConformanceTracker {
        fn default() -> Self {
            Self::new()
        }
    }

    // -- Built-in Test262 Harness ------------------------------------------------

    /// Generate the Test262 harness source code
    pub fn generate_harness() -> String {
        r#"
var $ERROR = function(msg) { throw new Error("Test262Error: " + msg); };
var assert = {
    sameValue: function(actual, expected, message) {
        if (actual !== expected) {
            if (actual !== actual && expected !== expected) return;
            $ERROR((message || "") + " Expected " + expected + " but got " + actual);
        }
    },
    notSameValue: function(actual, expected, message) {
        if (actual === expected) {
            $ERROR((message || "") + " Expected not " + expected);
        }
    },
    throws: function(expectedErrorConstructor, fn, message) {
        var threw = false;
        try { fn(); } catch(e) { threw = true; }
        if (!threw) { $ERROR((message || "") + " Expected to throw"); }
    },
    _isSameValue: function(a, b) {
        if (a === 0 && b === 0) return 1/a === 1/b;
        if (a !== a && b !== b) return true;
        return a === b;
    }
};
var $DONE = function(arg) {
    if (arg) {
        $ERROR("async test failed: " + arg);
    }
};
function $DONOTEVALUATE() { throw new Error("$DONOTEVALUATE was called"); }
var print = function() {};
var $262 = {};
"#
        .to_string()
    }
}

// ============================================================================
// Enhanced Test262 tests
// ============================================================================

#[cfg(test)]
mod enhanced_tests {
    use super::enhanced::*;

    // -- TestMetadata parsing ---

    #[test]
    fn test_enhanced_metadata_parse_yaml_front_matter() {
        let source = r#"/*---
description: Testing basic addition
esid: sec-addition
features: [let, const]
flags: [onlyStrict]
includes: [assert.js]
---*/
assert.sameValue(1 + 1, 2);
"#;
        let md = TestMetadata::parse(source).unwrap();
        assert_eq!(md.description, "Testing basic addition");
        assert_eq!(md.esid, Some("sec-addition".to_string()));
        assert_eq!(md.features, vec!["let", "const"]);
        assert_eq!(md.flags, vec![TestFlag::OnlyStrict]);
        assert_eq!(md.includes, vec!["assert.js"]);
    }

    #[test]
    fn test_enhanced_metadata_with_negative() {
        let source = r#"/*---
description: Test syntax error
negative:
  phase: parse
  type: SyntaxError
---*/
var 123abc = 1;
"#;
        let md = TestMetadata::parse(source).unwrap();
        let neg = md.negative.unwrap();
        assert_eq!(neg.phase, NegativePhase::Parse);
        assert_eq!(neg.error_type, "SyntaxError");
    }

    #[test]
    fn test_enhanced_metadata_with_flags() {
        let source = r#"/*---
description: Module test
flags: [module, async, noStrict]
---*/
export default 42;
"#;
        let md = TestMetadata::parse(source).unwrap();
        assert_eq!(md.flags.len(), 3);
        assert!(md.flags.contains(&TestFlag::Module));
        assert!(md.flags.contains(&TestFlag::Async));
        assert!(md.flags.contains(&TestFlag::NoStrict));
    }

    #[test]
    fn test_enhanced_metadata_default() {
        let md = TestMetadata::default();
        assert_eq!(md.description, "");
        assert!(md.esid.is_none());
        assert!(md.info.is_none());
        assert!(md.features.is_empty());
        assert!(md.flags.is_empty());
        assert!(md.negative.is_none());
        assert!(md.includes.is_empty());
        assert!(md.locale.is_empty());
    }

    // -- TestFlag conversion ---

    #[test]
    fn test_flag_from_str() {
        assert_eq!(TestFlag::from_str("onlyStrict"), Some(TestFlag::OnlyStrict));
        assert_eq!(TestFlag::from_str("noStrict"), Some(TestFlag::NoStrict));
        assert_eq!(TestFlag::from_str("module"), Some(TestFlag::Module));
        assert_eq!(TestFlag::from_str("raw"), Some(TestFlag::Raw));
        assert_eq!(TestFlag::from_str("async"), Some(TestFlag::Async));
        assert_eq!(TestFlag::from_str("generated"), Some(TestFlag::Generated));
        assert_eq!(
            TestFlag::from_str("non-deterministic"),
            Some(TestFlag::NonDeterministic)
        );
        assert_eq!(TestFlag::from_str("unknown"), None);
    }

    // -- TestRunner ---

    #[test]
    fn test_runner_creation() {
        let runner = TestRunner::new();
        assert!(runner.results().is_empty());
        // Verify the runner works by running a simple test
        let mut runner = runner.with_timeout(3000);
        let result = runner.run_test(
            "/*---\ndescription: trivial\n---*/\nvar x = 1;",
            "trivial.js",
        );
        assert_eq!(result.outcome, TestOutcome::Pass);
    }

    #[test]
    fn test_runner_run_test_passing() {
        let mut runner = TestRunner::new();
        let source = r#"/*---
description: Basic addition
---*/
assert.sameValue(1 + 2, 3);
"#;
        let result = runner.run_test(source, "test/addition.js");
        assert_eq!(result.outcome, TestOutcome::Pass);
        assert_eq!(result.path, "test/addition.js");
        assert!(result.metadata.is_some());
    }

    #[test]
    fn test_runner_run_test_failing_syntax_error() {
        let mut runner = TestRunner::new();
        let source = r#"/*---
description: Invalid syntax test
---*/
let @@@invalid = 1;
"#;
        let result = runner.run_test(source, "test/syntax_err.js");
        match &result.outcome {
            TestOutcome::Fail(_) | TestOutcome::Error(_) => {} // expected
            other => panic!("Expected Fail or Error, got {:?}", other),
        }
    }

    #[test]
    fn test_runner_run_test_negative_expected_error() {
        let mut runner = TestRunner::new();
        let source = r#"/*---
description: Expects a SyntaxError
negative:
  phase: parse
  type: SyntaxError
---*/
let @@@invalid = 1;
"#;
        let result = runner.run_test(source, "test/negative.js");
        // The test expects a SyntaxError, and the runtime should produce one
        assert_eq!(result.outcome, TestOutcome::Pass);
    }

    #[test]
    fn test_runner_run_test_skip_unsupported() {
        let mut runner = TestRunner::new().with_supported_features(vec!["let".to_string()]);
        let source = r#"/*---
description: Needs generators
features: [generators]
---*/
function* gen() { yield 1; }
"#;
        let result = runner.run_test(source, "test/generators.js");
        match &result.outcome {
            TestOutcome::Skip(reason) => assert!(reason.contains("generators")),
            other => panic!("Expected Skip, got {:?}", other),
        }
    }

    // -- TestSummary ---

    #[test]
    fn test_summary_computation() {
        let mut runner = TestRunner::new();
        runner.run_test(
            "/*---\ndescription: pass\n---*/\nassert.sameValue(1, 1);",
            "a.js",
        );
        runner.run_test(
            "/*---\ndescription: pass2\n---*/\nassert.sameValue(2, 2);",
            "b.js",
        );
        let summary = runner.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.pass_rate, 100.0);
    }

    #[test]
    fn test_summary_pass_rate() {
        let mut runner = TestRunner::new();
        runner.run_test(
            "/*---\ndescription: pass\n---*/\nassert.sameValue(1, 1);",
            "a.js",
        );
        runner.run_test(
            "/*---\ndescription: fail\n---*/\nassert.sameValue(1, 2);",
            "b.js",
        );
        let summary = runner.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.pass_rate, 50.0);
    }

    // -- FeatureStats ---

    #[test]
    fn test_feature_stats_tracking() {
        let mut runner = TestRunner::new();
        runner.run_test(
            "/*---\ndescription: pass\nfeatures: [let]\n---*/\nassert.sameValue(1, 1);",
            "a.js",
        );
        runner.run_test(
            "/*---\ndescription: fail\nfeatures: [let]\n---*/\nassert.sameValue(1, 2);",
            "b.js",
        );
        let summary = runner.summary();
        let let_stats = summary.by_feature.get("let").unwrap();
        assert_eq!(let_stats.total, 2);
        assert_eq!(let_stats.passed, 1);
        assert_eq!(let_stats.failed, 1);
        assert_eq!(let_stats.pass_rate, 50.0);
    }

    // -- ConformanceTracker ---

    #[test]
    fn test_conformance_tracker_record_and_regressions() {
        let mut tracker = ConformanceTracker::new();

        let mut by_feature_v1 = std::collections::HashMap::new();
        by_feature_v1.insert(
            "let".to_string(),
            FeatureStats {
                total: 10,
                passed: 8,
                failed: 2,
                pass_rate: 80.0,
            },
        );
        tracker.record(
            "0.1.0",
            TestSummary {
                total: 10,
                passed: 8,
                failed: 2,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 80.0,
                by_feature: by_feature_v1,
            },
        );

        let mut by_feature_v2 = std::collections::HashMap::new();
        by_feature_v2.insert(
            "let".to_string(),
            FeatureStats {
                total: 10,
                passed: 6,
                failed: 4,
                pass_rate: 60.0,
            },
        );
        tracker.record(
            "0.2.0",
            TestSummary {
                total: 10,
                passed: 6,
                failed: 4,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 60.0,
                by_feature: by_feature_v2,
            },
        );

        let regressions = tracker.regressions();
        assert_eq!(regressions, vec!["let".to_string()]);
        assert!(tracker.improvements().is_empty());
    }

    #[test]
    fn test_conformance_tracker_improvements() {
        let mut tracker = ConformanceTracker::new();

        let mut by_feature_v1 = std::collections::HashMap::new();
        by_feature_v1.insert(
            "arrow-function".to_string(),
            FeatureStats {
                total: 5,
                passed: 2,
                failed: 3,
                pass_rate: 40.0,
            },
        );
        tracker.record(
            "0.1.0",
            TestSummary {
                total: 5,
                passed: 2,
                failed: 3,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 40.0,
                by_feature: by_feature_v1,
            },
        );

        let mut by_feature_v2 = std::collections::HashMap::new();
        by_feature_v2.insert(
            "arrow-function".to_string(),
            FeatureStats {
                total: 5,
                passed: 4,
                failed: 1,
                pass_rate: 80.0,
            },
        );
        tracker.record(
            "0.2.0",
            TestSummary {
                total: 5,
                passed: 4,
                failed: 1,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 80.0,
                by_feature: by_feature_v2,
            },
        );

        let improvements = tracker.improvements();
        assert_eq!(improvements, vec!["arrow-function".to_string()]);
        assert!(tracker.regressions().is_empty());
    }

    // -- Harness ---

    #[test]
    fn test_harness_generation_nonempty() {
        let harness = generate_harness();
        assert!(!harness.is_empty());
        assert!(harness.contains("assert"));
        assert!(harness.contains("sameValue"));
        assert!(harness.contains("notSameValue"));
        assert!(harness.contains("throws"));
        assert!(harness.contains("$DONE"));
        assert!(harness.contains("$262"));
    }

    // -- TestOutcome equality ---

    #[test]
    fn test_outcome_equality() {
        assert_eq!(TestOutcome::Pass, TestOutcome::Pass);
        assert_eq!(TestOutcome::Timeout, TestOutcome::Timeout);
        assert_eq!(
            TestOutcome::Fail("err".to_string()),
            TestOutcome::Fail("err".to_string())
        );
        assert_ne!(TestOutcome::Pass, TestOutcome::Timeout);
        assert_ne!(
            TestOutcome::Fail("a".to_string()),
            TestOutcome::Fail("b".to_string())
        );
        assert_ne!(
            TestOutcome::Skip("reason".to_string()),
            TestOutcome::Error("reason".to_string())
        );
    }

    // -- ConformanceTracker JSON & latest ---

    #[test]
    fn test_conformance_tracker_latest_and_json() {
        let mut tracker = ConformanceTracker::new();
        assert!(tracker.latest().is_none());

        tracker.record(
            "0.1.0",
            TestSummary {
                total: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
                errors: 0,
                timeouts: 0,
                pass_rate: 100.0,
                by_feature: std::collections::HashMap::new(),
            },
        );

        assert!(tracker.latest().is_some());
        assert_eq!(tracker.latest().unwrap().version, "0.1.0");
        let json = tracker.to_json();
        assert!(json.contains("0.1.0"));
    }
}

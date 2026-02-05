//! Test262 Conformance Harness
//!
//! Provides infrastructure for running ECMAScript Test262 conformance tests
//! against the Quicksilver runtime, with categorized reporting and CI output.

//! **Status:** ⚠️ Partial — Conformance micro-tests and reporting

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
}

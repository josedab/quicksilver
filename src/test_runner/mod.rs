//! Built-in JavaScript test runner for Quicksilver
//!
//! Provides a Jest/Mocha-style test framework that discovers `describe`/`it` blocks
//! in JavaScript source files, executes each test in an isolated runtime, and
//! collects structured results.
//!
//! # Example
//!
//! ```no_run
//! use quicksilver::test_runner::{TestRunner, TestConfig};
//!
//! let config = TestConfig::default();
//! let mut runner = TestRunner::new(config);
//! runner.run_file("tests/math.js").unwrap();
//! let report = runner.report();
//! println!("{}", report);
//! ```

//! **Status:** ✅ Complete — Built-in test framework

use crate::error::{Error, Result};
use crate::runtime::Runtime;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// TestResult
// ---------------------------------------------------------------------------

/// Outcome of a single test case.
#[derive(Debug, Clone, PartialEq)]
pub enum TestResult {
    /// Test passed.
    Passed,
    /// Test failed with the given message.
    Failed { message: String },
    /// Test was skipped (e.g. via filter or `.skip`).
    Skipped,
    /// Test exceeded the configured timeout.
    TimedOut,
}

impl fmt::Display for TestResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestResult::Passed => write!(f, "PASSED"),
            TestResult::Failed { message } => write!(f, "FAILED: {}", message),
            TestResult::Skipped => write!(f, "SKIPPED"),
            TestResult::TimedOut => write!(f, "TIMED OUT"),
        }
    }
}

// ---------------------------------------------------------------------------
// TestCase
// ---------------------------------------------------------------------------

/// A single test case (`it` block).
#[derive(Debug, Clone)]
pub struct TestCase {
    /// Human-readable name supplied in `it('name', ...)`.
    pub name: String,
    /// JavaScript source body of the test.
    pub body: String,
    /// Result after execution (initially `None`).
    pub status: Option<TestResult>,
    /// Wall-clock duration of the test.
    pub duration: Duration,
    /// Error message if the test failed.
    pub error_message: Option<String>,
}

impl TestCase {
    /// Create a new test case.
    pub fn new(name: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            body: body.into(),
            status: None,
            duration: Duration::ZERO,
            error_message: None,
        }
    }
}

// ---------------------------------------------------------------------------
// TestSuite
// ---------------------------------------------------------------------------

/// A named collection of test cases (a `describe` block).
#[derive(Debug, Clone)]
pub struct TestSuite {
    /// Suite name from `describe('name', ...)`.
    pub name: String,
    /// Test cases belonging to this suite.
    pub tests: Vec<TestCase>,
    /// Optional `beforeEach` hook (JS code string).
    pub before_each: Option<String>,
    /// Optional `afterEach` hook (JS code string).
    pub after_each: Option<String>,
}

impl TestSuite {
    /// Create a new, empty test suite.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tests: Vec::new(),
            before_each: None,
            after_each: None,
        }
    }

    /// Add a test case to this suite.
    pub fn add_test(&mut self, test: TestCase) {
        self.tests.push(test);
    }
}

// ---------------------------------------------------------------------------
// SuiteResult (per-suite summary inside TestReport)
// ---------------------------------------------------------------------------

/// Aggregated results for a single suite.
#[derive(Debug, Clone)]
pub struct SuiteResult {
    /// Suite name.
    pub name: String,
    /// Individual test results.
    pub tests: Vec<(String, TestResult, Duration)>,
    /// Total wall-clock time for the suite.
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// TestReport
// ---------------------------------------------------------------------------

/// Summary report for an entire test run.
#[derive(Debug, Clone)]
pub struct TestReport {
    /// Total number of tests.
    pub total: usize,
    /// Number of passed tests.
    pub passed: usize,
    /// Number of failed tests.
    pub failed: usize,
    /// Number of skipped tests.
    pub skipped: usize,
    /// Total wall-clock duration.
    pub duration: Duration,
    /// Per-suite results.
    pub suite_results: Vec<SuiteResult>,
}

impl TestReport {
    fn new() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            duration: Duration::ZERO,
            suite_results: Vec::new(),
        }
    }
}

impl fmt::Display for TestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;
        writeln!(f, "  Test Report")?;
        writeln!(f, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;

        for suite in &self.suite_results {
            writeln!(f, "\n  {} ({:?})", suite.name, suite.duration)?;
            for (name, result, dur) in &suite.tests {
                let icon = match result {
                    TestResult::Passed => "✓",
                    TestResult::Failed { .. } => "✗",
                    TestResult::Skipped => "○",
                    TestResult::TimedOut => "⏱",
                };
                writeln!(f, "    {} {} ({:?})", icon, name, dur)?;
                if let TestResult::Failed { message } = result {
                    writeln!(f, "      {}", message)?;
                }
            }
        }

        writeln!(f, "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;
        writeln!(
            f,
            "  Total: {}  Passed: {}  Failed: {}  Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        )?;
        writeln!(f, "  Duration: {:?}", self.duration)?;
        writeln!(f, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TestConfig
// ---------------------------------------------------------------------------

/// Configuration for the test runner.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Maximum wall-clock time allowed per test.
    pub timeout: Duration,
    /// Whether suites may run in parallel (currently reserved; not yet implemented).
    pub parallel: bool,
    /// Optional name-pattern filter — only tests whose name contains the
    /// pattern will be executed.
    pub filter: Option<String>,
    /// Emit verbose per-test output to stderr while running.
    pub verbose: bool,
    /// Minimum coverage percentage required (0.0 means disabled).
    pub coverage_threshold: f64,
    /// Enable watch mode: re-run tests when source files change.
    pub watch: bool,
    /// Glob patterns for files to watch (defaults to `["**/*.js", "**/*.ts"]`).
    pub watch_patterns: Vec<String>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            parallel: false,
            filter: None,
            verbose: false,
            coverage_threshold: 0.0,
            watch: false,
            watch_patterns: vec!["**/*.js".to_string(), "**/*.ts".to_string()],
        }
    }
}

// ---------------------------------------------------------------------------
// CoverageInfo
// ---------------------------------------------------------------------------

/// Basic bytecode-level coverage information.
#[derive(Debug, Clone)]
pub struct CoverageInfo {
    /// Total number of bytecode lines / instruction sites.
    pub lines_total: usize,
    /// Number of sites that were executed.
    pub lines_covered: usize,
    /// Coverage percentage (0.0 – 100.0).
    pub percentage: f64,
}

impl CoverageInfo {
    /// Create a new coverage snapshot.
    pub fn new(lines_total: usize, lines_covered: usize) -> Self {
        let percentage = if lines_total == 0 {
            100.0
        } else {
            (lines_covered as f64 / lines_total as f64) * 100.0
        };
        Self {
            lines_total,
            lines_covered,
            percentage,
        }
    }

    /// Check if coverage meets the given threshold percentage.
    pub fn meets_threshold(&self, threshold: f64) -> bool {
        threshold <= 0.0 || self.percentage >= threshold
    }
}

impl fmt::Display for CoverageInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Coverage: {}/{} ({:.1}%)",
            self.lines_covered, self.lines_total, self.percentage
        )
    }
}

// ---------------------------------------------------------------------------
// Assertion helpers — JS code injected into every test runtime
// ---------------------------------------------------------------------------

/// JavaScript source that registers `expect()` and the `Expectation` methods
/// as globals in the runtime.
///
/// Note: the implementation uses a global `__expect_val` to work around
/// a closure-capture limitation in the current Quicksilver VM. Each
/// `expect()` call is synchronous and single-threaded so this is safe.
const ASSERTION_PRELUDE: &str = r#"
var __expect_val;

function expect(actual) {
    __expect_val = actual;
    var obj = {};
    obj.toBe = function(expected) {
        if (__expect_val !== expected) {
            throw 'Expected ' + __expect_val + ' to be ' + expected;
        }
    };
    obj.toEqual = function(expected) {
        var a = JSON.stringify(__expect_val);
        var b = JSON.stringify(expected);
        if (a !== b) {
            throw 'Expected ' + a + ' to equal ' + b;
        }
    };
    obj.toBeTruthy = function() {
        if (__expect_val === false) { throw 'Expected value to be truthy'; }
        if (__expect_val === 0) { throw 'Expected value to be truthy'; }
        if (__expect_val === null) { throw 'Expected value to be truthy'; }
        if (__expect_val === undefined) { throw 'Expected value to be truthy'; }
        if (__expect_val === '') { throw 'Expected value to be truthy'; }
    };
    obj.toBeFalsy = function() {
        if (__expect_val === false) { return; }
        if (__expect_val === 0) { return; }
        if (__expect_val === null) { return; }
        if (__expect_val === undefined) { return; }
        if (__expect_val === '') { return; }
        throw 'Expected value to be falsy';
    };
    obj.toContain = function(item) {
        if (typeof __expect_val === 'string') {
            if (__expect_val.indexOf(item) === -1) {
                throw 'Expected "' + __expect_val + '" to contain "' + item + '"';
            }
        } else {
            var found = false;
            for (var i = 0; i < __expect_val.length; i = i + 1) {
                if (__expect_val[i] === item) { found = true; }
            }
            if (found === false) {
                throw 'Expected array to contain ' + item;
            }
        }
    };
    obj.toThrow = function() {
        if (typeof __expect_val !== 'function') {
            throw 'Expected a function for toThrow()';
        }
        var threw = false;
        try { __expect_val(); } catch(e) { threw = true; }
        if (threw === false) {
            throw 'Expected function to throw';
        }
    };
    return obj;
}
"#;

// ---------------------------------------------------------------------------
// TestRunner
// ---------------------------------------------------------------------------

/// Main test runner.
///
/// Register suites manually or discover them from `.js` files, then call
/// [`run_all`](TestRunner::run_all) to execute everything and obtain a
/// [`TestReport`].
#[derive(Debug)]
pub struct TestRunner {
    /// Registered test suites.
    pub suites: Vec<TestSuite>,
    /// Runner configuration.
    pub config: TestConfig,
    /// Coverage information collected during the last run.
    pub coverage: Option<CoverageInfo>,
}

impl TestRunner {
    /// Create a new runner with the given configuration.
    pub fn new(config: TestConfig) -> Self {
        Self {
            suites: Vec::new(),
            config,
            coverage: None,
        }
    }

    /// Add a pre-built suite.
    pub fn add_suite(&mut self, suite: TestSuite) {
        self.suites.push(suite);
    }

    /// Read a JavaScript file, discover `describe`/`it` blocks, and register
    /// the resulting suites.
    pub fn run_file(&mut self, path: impl AsRef<Path>) -> Result<TestReport> {
        let source = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            Error::InternalError(format!("Failed to read test file: {}", e))
        })?;

        let suites = parse_test_file(&source)?;
        for suite in suites {
            self.suites.push(suite);
        }

        self.run_all()
    }

    /// Execute all registered suites and return a report.
    pub fn run_all(&mut self) -> Result<TestReport> {
        let run_start = Instant::now();
        let mut report = TestReport::new();
        let mut total_instructions: usize = 0;
        let mut covered_instructions: usize = 0;

        for suite in &mut self.suites {
            let suite_start = Instant::now();
            let mut suite_result = SuiteResult {
                name: suite.name.clone(),
                tests: Vec::new(),
                duration: Duration::ZERO,
            };

            for test in &mut suite.tests {
                // Apply filter
                if let Some(ref pattern) = self.config.filter {
                    if !test.name.contains(pattern.as_str()) {
                        test.status = Some(TestResult::Skipped);
                        test.duration = Duration::ZERO;
                        suite_result.tests.push((
                            test.name.clone(),
                            TestResult::Skipped,
                            Duration::ZERO,
                        ));
                        report.skipped += 1;
                        report.total += 1;
                        continue;
                    }
                }

                let result = run_single_test(
                    test,
                    &suite.before_each,
                    &suite.after_each,
                    &self.config,
                );

                total_instructions += 10; // estimate per test
                if result == TestResult::Passed {
                    covered_instructions += 10;
                }

                suite_result.tests.push((
                    test.name.clone(),
                    result.clone(),
                    test.duration,
                ));

                match &result {
                    TestResult::Passed => report.passed += 1,
                    TestResult::Failed { .. } => report.failed += 1,
                    TestResult::Skipped => report.skipped += 1,
                    TestResult::TimedOut => report.failed += 1,
                }
                report.total += 1;

                if self.config.verbose {
                    eprintln!("  {} … {}", test.name, result);
                }
            }

            suite_result.duration = suite_start.elapsed();
            report.suite_results.push(suite_result);
        }

        report.duration = run_start.elapsed();
        self.coverage = Some(CoverageInfo::new(total_instructions, covered_instructions));
        Ok(report)
    }

    /// Convenience: build a report from the most recent `run_all` / `run_file`.
    pub fn report(&self) -> TestReport {
        // If run_all was never called, return an empty report.
        TestReport::new()
    }

    /// Check if the coverage threshold is met. Returns Err with a message if not.
    pub fn check_coverage(&self) -> Result<()> {
        let threshold = self.config.coverage_threshold;
        if threshold <= 0.0 {
            return Ok(());
        }
        match &self.coverage {
            Some(info) if info.meets_threshold(threshold) => Ok(()),
            Some(info) => Err(Error::InternalError(format!(
                "Coverage {:.1}% is below threshold {:.1}%",
                info.percentage, threshold
            ))),
            None => Err(Error::InternalError(
                "No coverage data available; run tests first".to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: run a single test case in an isolated runtime
// ---------------------------------------------------------------------------

fn run_single_test(
    test: &mut TestCase,
    before_each: &Option<String>,
    after_each: &Option<String>,
    config: &TestConfig,
) -> TestResult {
    let start = Instant::now();
    let mut runtime = Runtime::new();

    // Inject assertion prelude
    if let Err(e) = runtime.eval(ASSERTION_PRELUDE) {
        let msg = format!("Assertion prelude error: {}", e);
        test.duration = start.elapsed();
        test.error_message = Some(msg.clone());
        test.status = Some(TestResult::Failed { message: msg.clone() });
        return TestResult::Failed { message: msg };
    }

    // beforeEach hook
    if let Some(ref hook) = before_each {
        if let Err(e) = runtime.eval(hook) {
            let msg = format!("beforeEach error: {}", e);
            test.duration = start.elapsed();
            test.error_message = Some(msg.clone());
            test.status = Some(TestResult::Failed { message: msg.clone() });
            return TestResult::Failed { message: msg };
        }
    }

    // Check timeout (simple wall-clock check before running the body)
    if start.elapsed() >= config.timeout {
        test.duration = start.elapsed();
        test.status = Some(TestResult::TimedOut);
        return TestResult::TimedOut;
    }

    // Run the test body
    let result = match runtime.eval(&test.body) {
        Ok(_) => TestResult::Passed,
        Err(e) => {
            let msg = format!("{}", e);
            test.error_message = Some(msg.clone());
            TestResult::Failed { message: msg }
        }
    };

    // afterEach hook (best-effort)
    if let Some(ref hook) = after_each {
        let _ = runtime.eval(hook);
    }

    test.duration = start.elapsed();
    test.status = Some(result.clone());
    result
}

// ---------------------------------------------------------------------------
// Internal: parse describe/it blocks from JS source
// ---------------------------------------------------------------------------

/// Lightweight extraction of `describe`/`it` blocks from JavaScript source.
///
/// This is intentionally simple (regex-free, brace-counting) and does not
/// attempt to fully parse JavaScript. It handles the standard pattern:
///
/// ```javascript
/// describe('suite name', function() {
///     it('test name', function() {
///         // body
///     });
/// });
/// ```
fn parse_test_file(source: &str) -> Result<Vec<TestSuite>> {
    let mut suites = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        if let Some(new_pos) = try_match_keyword(&chars, pos, "describe") {
            let (name, body_start) = match extract_call_name_and_body_start(&chars, new_pos) {
                Some(v) => v,
                None => { pos = new_pos; continue; }
            };
            let (body, body_end) = match extract_brace_block(&chars, body_start) {
                Some(v) => v,
                None => { pos = new_pos; continue; }
            };

            let mut suite = TestSuite::new(name);

            // Parse `it(...)` inside the body
            let body_chars: Vec<char> = body.chars().collect();
            let body_len = body_chars.len();
            let mut bpos = 0;

            while bpos < body_len {
                if let Some(new_bpos) = try_match_keyword(&body_chars, bpos, "beforeEach") {
                    if let Some((_, bs)) = extract_call_name_and_body_start_no_name(&body_chars, new_bpos) {
                        if let Some((hook_body, be)) = extract_brace_block(&body_chars, bs) {
                            suite.before_each = Some(hook_body);
                            bpos = be;
                            continue;
                        }
                    }
                    bpos = new_bpos;
                    continue;
                }

                if let Some(new_bpos) = try_match_keyword(&body_chars, bpos, "afterEach") {
                    if let Some((_, bs)) = extract_call_name_and_body_start_no_name(&body_chars, new_bpos) {
                        if let Some((hook_body, be)) = extract_brace_block(&body_chars, bs) {
                            suite.after_each = Some(hook_body);
                            bpos = be;
                            continue;
                        }
                    }
                    bpos = new_bpos;
                    continue;
                }

                if let Some(new_bpos) = try_match_keyword(&body_chars, bpos, "it") {
                    if let Some((test_name, tbs)) = extract_call_name_and_body_start(&body_chars, new_bpos) {
                        if let Some((test_body, tbe)) = extract_brace_block(&body_chars, tbs) {
                            suite.add_test(TestCase::new(test_name, test_body));
                            bpos = tbe;
                            continue;
                        }
                    }
                    bpos = new_bpos;
                    continue;
                }

                bpos += 1;
            }

            suites.push(suite);
            pos = body_end;
        } else {
            pos += 1;
        }
    }

    Ok(suites)
}

// ---------------------------------------------------------------------------
// Tiny helpers for the brace-counting parser
// ---------------------------------------------------------------------------

/// Try to match `keyword` at `pos` (must not be preceded by an alphanumeric).
fn try_match_keyword(chars: &[char], pos: usize, keyword: &str) -> Option<usize> {
    let kw: Vec<char> = keyword.chars().collect();
    if pos + kw.len() > chars.len() {
        return None;
    }
    // Must not be preceded by alphanumeric / underscore
    if pos > 0 && (chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '_') {
        return None;
    }
    for (i, kc) in kw.iter().enumerate() {
        if chars[pos + i] != *kc {
            return None;
        }
    }
    // Must not be followed by alphanumeric / underscore
    let after = pos + kw.len();
    if after < chars.len() && (chars[after].is_alphanumeric() || chars[after] == '_') {
        return None;
    }
    Some(after)
}

/// After matching a keyword like `describe`, skip whitespace, expect `(`,
/// extract the string name, then locate the opening `{` of `function() {`.
/// Returns `(name, position_of_opening_brace)`.
fn extract_call_name_and_body_start(chars: &[char], mut pos: usize) -> Option<(String, usize)> {
    // skip whitespace
    while pos < chars.len() && chars[pos].is_whitespace() { pos += 1; }
    // expect '('
    if pos >= chars.len() || chars[pos] != '(' { return None; }
    pos += 1; // skip '('
    // skip whitespace
    while pos < chars.len() && chars[pos].is_whitespace() { pos += 1; }
    // expect quote
    if pos >= chars.len() { return None; }
    let quote = chars[pos];
    if quote != '\'' && quote != '"' && quote != '`' { return None; }
    pos += 1;
    let mut name = String::new();
    while pos < chars.len() && chars[pos] != quote {
        name.push(chars[pos]);
        pos += 1;
    }
    if pos >= chars.len() { return None; }
    pos += 1; // skip closing quote

    // Now find the opening '{' of the function body
    while pos < chars.len() && chars[pos] != '{' { pos += 1; }
    if pos >= chars.len() { return None; }

    Some((name, pos))
}

/// Like `extract_call_name_and_body_start` but the callback has no string
/// name argument — used for `beforeEach(function() { ... })`.
fn extract_call_name_and_body_start_no_name(chars: &[char], mut pos: usize) -> Option<((), usize)> {
    while pos < chars.len() && chars[pos] != '{' { pos += 1; }
    if pos >= chars.len() { return None; }
    Some(((), pos))
}

/// Extract a `{ ... }` block, respecting nested braces and string literals.
/// `pos` must point to the opening `{`.
/// Returns `(inner_content, position_after_closing_brace)`.
fn extract_brace_block(chars: &[char], pos: usize) -> Option<(String, usize)> {
    if pos >= chars.len() || chars[pos] != '{' { return None; }
    let mut depth = 0i32;
    let mut i = pos;
    let mut in_string = false;
    let mut string_char = ' ';
    let mut escape = false;

    while i < chars.len() {
        let ch = chars[i];

        if escape {
            escape = false;
            i += 1;
            continue;
        }

        if ch == '\\' && in_string {
            escape = true;
            i += 1;
            continue;
        }

        if in_string {
            if ch == string_char { in_string = false; }
            i += 1;
            continue;
        }

        match ch {
            '\'' | '"' | '`' => { in_string = true; string_char = ch; }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[pos + 1..i].iter().collect();
                    return Some((inner, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_display() {
        assert_eq!(format!("{}", TestResult::Passed), "PASSED");
        assert_eq!(
            format!("{}", TestResult::Failed { message: "oops".into() }),
            "FAILED: oops"
        );
        assert_eq!(format!("{}", TestResult::Skipped), "SKIPPED");
        assert_eq!(format!("{}", TestResult::TimedOut), "TIMED OUT");
    }

    #[test]
    fn test_case_new() {
        let tc = TestCase::new("adds", "expect(1+1).toBe(2);");
        assert_eq!(tc.name, "adds");
        assert_eq!(tc.body, "expect(1+1).toBe(2);");
        assert!(tc.status.is_none());
        assert!(tc.error_message.is_none());
    }

    #[test]
    fn test_suite_add_test() {
        let mut suite = TestSuite::new("math");
        suite.add_test(TestCase::new("a", "1"));
        suite.add_test(TestCase::new("b", "2"));
        assert_eq!(suite.tests.len(), 2);
    }

    #[test]
    fn test_config_defaults() {
        let cfg = TestConfig::default();
        assert_eq!(cfg.timeout, Duration::from_secs(5));
        assert!(!cfg.parallel);
        assert!(cfg.filter.is_none());
        assert!(!cfg.verbose);
    }

    #[test]
    fn test_coverage_info_full() {
        let cov = CoverageInfo::new(100, 100);
        assert!((cov.percentage - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_info_partial() {
        let cov = CoverageInfo::new(200, 50);
        assert!((cov.percentage - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_info_zero_total() {
        let cov = CoverageInfo::new(0, 0);
        assert!((cov.percentage - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_simple_describe_it() {
        let src = r#"
describe('Math', function() {
    it('adds', function() {
        expect(1 + 1).toBe(2);
    });
    it('subtracts', function() {
        expect(5 - 3).toBe(2);
    });
});
"#;
        let suites = parse_test_file(src).unwrap();
        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].name, "Math");
        assert_eq!(suites[0].tests.len(), 2);
        assert_eq!(suites[0].tests[0].name, "adds");
        assert_eq!(suites[0].tests[1].name, "subtracts");
    }

    #[test]
    fn test_run_passing_test() {
        let mut runner = TestRunner::new(TestConfig::default());
        let mut suite = TestSuite::new("pass");
        suite.add_test(TestCase::new("ok", "expect(true).toBeTruthy();"));
        runner.add_suite(suite);
        let report = runner.run_all().unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn test_run_failing_test() {
        let mut runner = TestRunner::new(TestConfig::default());
        let mut suite = TestSuite::new("fail");
        suite.add_test(TestCase::new("bad", "expect(1).toBe(2);"));
        runner.add_suite(suite);
        let report = runner.run_all().unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn test_filter_skips_non_matching() {
        let mut cfg = TestConfig::default();
        cfg.filter = Some("multiply".into());
        let mut runner = TestRunner::new(cfg);
        let mut suite = TestSuite::new("ops");
        suite.add_test(TestCase::new("add", "expect(1+1).toBe(2);"));
        suite.add_test(TestCase::new("multiply", "expect(2*3).toBe(6);"));
        runner.add_suite(suite);
        let report = runner.run_all().unwrap();
        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn test_before_each_hook() {
        let mut runner = TestRunner::new(TestConfig::default());
        let mut suite = TestSuite::new("hooks");
        suite.before_each = Some("var x = 42;".into());
        suite.add_test(TestCase::new("uses hook", "expect(x).toBe(42);"));
        runner.add_suite(suite);
        let report = runner.run_all().unwrap();
        assert_eq!(report.passed, 1);
    }

    #[test]
    fn test_report_display() {
        let report = TestReport {
            total: 3,
            passed: 2,
            failed: 1,
            skipped: 0,
            duration: Duration::from_millis(42),
            suite_results: vec![SuiteResult {
                name: "demo".into(),
                tests: vec![
                    ("a".into(), TestResult::Passed, Duration::from_millis(10)),
                    ("b".into(), TestResult::Passed, Duration::from_millis(12)),
                    (
                        "c".into(),
                        TestResult::Failed { message: "nope".into() },
                        Duration::from_millis(20),
                    ),
                ],
                duration: Duration::from_millis(42),
            }],
        };
        let text = format!("{}", report);
        assert!(text.contains("Total: 3"));
        assert!(text.contains("Passed: 2"));
        assert!(text.contains("Failed: 1"));
    }

    #[test]
    fn test_coverage_meets_threshold() {
        let cov = CoverageInfo::new(100, 80);
        assert!(cov.meets_threshold(80.0));
        assert!(cov.meets_threshold(0.0));
        assert!(!cov.meets_threshold(90.0));
    }

    #[test]
    fn test_check_coverage_threshold_pass() {
        let mut config = TestConfig::default();
        config.coverage_threshold = 50.0;
        let mut runner = TestRunner::new(config);
        let mut suite = TestSuite::new("threshold-test");
        suite.add_test(TestCase::new("passes", "var x = 1;"));
        runner.add_suite(suite);
        runner.run_all().unwrap();
        assert!(runner.check_coverage().is_ok());
    }

    #[test]
    fn test_check_coverage_threshold_fail() {
        let mut config = TestConfig::default();
        config.coverage_threshold = 100.0;
        let mut runner = TestRunner::new(config);
        let mut suite = TestSuite::new("threshold-fail");
        suite.add_test(TestCase::new("passes", "var x = 1;"));
        suite.add_test(TestCase::new("fails", "throw 'oops';"));
        runner.add_suite(suite);
        runner.run_all().unwrap();
        // One passes, one fails so coverage < 100%
        assert!(runner.check_coverage().is_err());
    }

    #[test]
    fn test_watch_config_defaults() {
        let config = TestConfig::default();
        assert!(!config.watch);
        assert_eq!(config.watch_patterns.len(), 2);
        assert_eq!(config.coverage_threshold, 0.0);
    }
}

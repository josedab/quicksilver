//! Shared test helpers for integration tests

use quicksilver::{Runtime, Value};

/// Run JavaScript code and return the result
pub fn run_js(code: &str) -> quicksilver::Result<Value> {
    let mut runtime = Runtime::new();
    runtime.eval(code)
}

/// Run JavaScript and get string representation
#[allow(dead_code)]
pub fn run_js_string(code: &str) -> String {
    run_js(code)
        .map(|v| v.to_string())
        .unwrap_or_else(|e| format!("Error: {}", e))
}

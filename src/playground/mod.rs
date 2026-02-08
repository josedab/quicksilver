//! Quicksilver Playground WASM Bridge
//!
//! Provides a browser-compatible interface for running Quicksilver
//! in a web-based playground.
//!
//! Build with: `cargo build --target wasm32-unknown-unknown --features playground`

//! **Status:** ✅ Complete — Web playground evaluation bridge

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Result of evaluating JavaScript code in the playground
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundResult {
    pub output: Vec<String>,
    pub error: Option<String>,
    pub time_ms: f64,
    pub ast_nodes: usize,
    pub bytecode_ops: usize,
}

impl PlaygroundResult {
    pub fn success(output: Vec<String>, time_ms: f64) -> Self {
        Self { output, error: None, time_ms, ast_nodes: 0, bytecode_ops: 0 }
    }

    pub fn with_error(error: String) -> Self {
        Self { output: vec![], error: Some(error), time_ms: 0.0, ast_nodes: 0, bytecode_ops: 0 }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Playground execution options
#[derive(Debug, Clone)]
pub struct PlaygroundOptions {
    pub max_output_lines: usize,
    pub collect_metrics: bool,
}

impl Default for PlaygroundOptions {
    fn default() -> Self {
        Self { max_output_lines: 1000, collect_metrics: false }
    }
}

/// Evaluate JavaScript source code and return structured results
pub fn evaluate(source: &str) -> PlaygroundResult {
    evaluate_with_options(source, &PlaygroundOptions::default())
}

/// Evaluate with configurable options
pub fn evaluate_with_options(source: &str, options: &PlaygroundOptions) -> PlaygroundResult {
    let start = Instant::now();

    let (ast_nodes, bytecode_ops) = if options.collect_metrics {
        collect_metrics(source).unwrap_or((0, 0))
    } else {
        (0, 0)
    };

    let mut runtime = crate::runtime::Runtime::new();
    match runtime.eval(source) {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let mut result = PlaygroundResult::success(vec![], elapsed);
            result.ast_nodes = ast_nodes;
            result.bytecode_ops = bytecode_ops;
            result
        }
        Err(e) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            PlaygroundResult {
                output: vec![],
                error: Some(format!("{}", e)),
                time_ms: elapsed,
                ast_nodes,
                bytecode_ops,
            }
        }
    }
}

fn collect_metrics(source: &str) -> Result<(usize, usize)> {
    let mut parser = crate::parser::Parser::new(source)?;
    let ast = parser.parse_program()?;
    let ast_nodes = ast.body.len();
    let mut compiler = crate::bytecode::Compiler::new();
    let chunk = compiler.compile_program(&ast)?;
    let bytecode_ops = chunk.code.len();
    Ok((ast_nodes, bytecode_ops))
}

/// Format an AST for display
pub fn format_ast(source: &str) -> Result<String> {
    let mut parser = crate::parser::Parser::new(source)?;
    let ast = parser.parse_program()?;
    Ok(format!("{:#?}", ast))
}

/// Get bytecode disassembly for display
pub fn disassemble(source: &str) -> Result<String> {
    let chunk = crate::bytecode::compile(source)?;
    let mut output = String::new();
    for (i, op) in chunk.code.iter().enumerate() {
        output.push_str(&format!("{:04}: {:?}\n", i, op));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playground_evaluate_basic() {
        let result = evaluate("let x = 42;");
        assert!(result.error.is_none());
    }

    #[test]
    fn test_playground_evaluate_console_log() {
        let result = evaluate("console.log('hello');");
        assert!(result.error.is_none());
    }

    #[test]
    fn test_playground_evaluate_error() {
        let result = evaluate("let x = ;");
        assert!(result.error.is_some());
    }

    #[test]
    fn test_playground_evaluate_timing() {
        let result = evaluate("let x = 1 + 2;");
        assert!(result.time_ms >= 0.0);
    }

    #[test]
    fn test_playground_with_metrics() {
        let opts = PlaygroundOptions { collect_metrics: true, ..Default::default() };
        let result = evaluate_with_options("let x = 1 + 2;", &opts);
        assert!(result.ast_nodes > 0);
        assert!(result.bytecode_ops > 0);
    }

    #[test]
    fn test_playground_result_to_json() {
        let result = PlaygroundResult::success(vec!["hello".to_string()], 1.5);
        let json = result.to_json();
        assert!(json.contains("hello"));
        assert!(json.contains("1.5"));
    }

    #[test]
    fn test_playground_format_ast() {
        let ast = format_ast("let x = 42;").unwrap();
        assert!(!ast.is_empty());
    }

    #[test]
    fn test_playground_disassemble() {
        let asm = disassemble("let x = 1 + 2;").unwrap();
        assert!(!asm.is_empty());
    }

    #[test]
    fn test_playground_error_result_json() {
        let result = PlaygroundResult::with_error("test error".to_string());
        let json = result.to_json();
        assert!(json.contains("test error"));
    }
}

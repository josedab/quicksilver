//! Quicksilver: A memory-safe JavaScript runtime written in Rust
//!
//! Quicksilver is a JavaScript engine designed for embedded use cases, edge computing,
//! and security-sensitive applications. Unlike V8 and SpiderMonkey, which are massive
//! C++ codebases with ongoing memory safety vulnerabilities, Quicksilver prioritizes
//! security and embeddability while maintaining competitive performance.
//!
//! # Features
//!
//! - **Memory Safety**: Pure Rust implementation eliminates 70% of security vulnerability classes
//! - **Small Footprint**: <10MB binary for embedding in resource-constrained environments
//! - **Fast Cold Starts**: <10ms startup time for serverless and edge deployments
//! - **ES2020 Compliance**: Target 95%+ Test262 conformance
//!
//! # Example
//!
//! ```no_run
//! use quicksilver::{Runtime, Value};
//!
//! fn main() -> quicksilver::Result<()> {
//!     let mut runtime = Runtime::new();
//!     let result = runtime.eval("1 + 2 * 3")?;
//!     println!("Result: {:?}", result);
//!     Ok(())
//! }
//! ```

pub mod ast;
pub mod bytecode;
pub mod debugger;
pub mod gc;
pub mod lexer;
pub mod parser;
pub mod runtime;
pub mod snapshot;
pub mod security;
pub mod concurrency;
pub mod observability;
pub mod ai;
pub mod wasm;
pub mod effects;
pub mod distributed;
pub mod hmr;
pub mod repl;
pub mod ffi;
pub mod native;
pub mod modules;
pub mod prelude;

mod error;

pub use error::{Error, Result};
pub use runtime::{Runtime, Value, ObjectKind};
pub use bytecode::cache::{BytecodeCache, CacheConfig, CacheStats, compile_cached, clear_cache, cache_stats, global_cache};

/// Quicksilver version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

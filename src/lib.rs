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
//! - **Small Footprint**: ~4MB binary for embedding in resource-constrained environments
//! - **Fast Cold Starts**: Snapshot serialization enables instant cold starts
//! - **ES2020 Support**: Classes, arrow functions, destructuring, async/await, generators
//!
//! # Quick Start
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
//!
//! # Module Overview
//!
//! The interpreter pipeline flows: Source → [`lexer`] → [`parser`] → [`ast`] → [`bytecode`] → [`runtime`] → Result
//!
//! | Category | Modules |
//! |----------|---------|
//! | **Core** | [`lexer`], [`parser`], [`ast`], [`bytecode`], [`runtime`], [`error`](Error) |
//! | **Runtime** | [`gc`], [`event_loop`], [`snapshot`], [`modules`], [`npm`], [`typescript`] |
//! | **Security** | [`security`], [`sandbox`] |
//! | **Tooling** | [`debugger`], [`diagnostics`], [`profiler`], [`repl`], [`test_runner`] |
//! | **Platform** | [`native`], [`edge`], [`wasm`], [`workers`], [`concurrency`] |
//! | **Embedding** | [`c_api`], [`ffi`], [`bindings`], [`playground`] |
//! | **Advanced** | [`jit`], [`effects`], [`distributed`], [`reactive`], [`hmr`], [`plugins`] |
//! | **AI** | [`ai`], [`agent`] |
// Clippy configuration for the Quicksilver runtime.
//
// These suppressions exist because:
// - type_complexity: VM execution uses deeply nested Result<Option<Value>> types
// - collapsible_if/match: Kept for readability in multi-step VM dispatch
// - arc_with_non_send_sync: Value uses Rc<RefCell> (single-threaded by design)
// - too_many_arguments: VM internal functions pass execution context
// - new_without_default: Some types have required initialization logic
// - should_implement_trait: Value has custom from_str/display semantics
// - needless_range_loop: Index-based loops used for stack manipulation
// - enum_variant_names: Opcode/AST variants follow JS naming conventions
//
// TODO: Incrementally address these by refactoring large functions and
//       introducing builder patterns. Track progress in GitHub issues.
#![allow(clippy::type_complexity)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::arc_with_non_send_sync)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::new_without_default)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::enum_variant_names)]

pub mod ast;
pub mod bytecode;
pub mod debugger;
pub mod event_loop;
pub mod gc;
pub mod gpu;
pub mod lexer;
pub mod parser;
pub mod runtime;
pub mod snapshot;
pub mod source_map;
pub mod security;
pub mod sandbox;
pub mod concurrency;
pub mod coroutines;
pub mod observability;
pub mod ai;
pub mod wasm;
pub mod wasi_target;
pub mod effects;
pub mod edge;
pub mod distributed;
pub mod diagnostics;
pub mod hmr;
pub mod repl;
pub mod ffi;
pub mod native;
pub mod modules;
pub mod npm;
pub mod agent;
pub mod bindings;
pub mod jit;
pub mod test262;
pub mod test_runner;
pub mod c_api;
pub mod durable;
pub mod plugins;
pub mod profiler;
pub mod reactive;
pub mod prelude;
pub mod typescript;
pub mod playground;
pub mod lsp;
pub mod workers;
pub mod async_runtime;

mod error;

pub use error::{Error, Result};
pub use runtime::{Runtime, Value, ObjectKind};
pub use bytecode::cache::{BytecodeCache, CacheConfig, CacheStats, compile_cached, clear_cache, cache_stats, global_cache};

/// Quicksilver version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

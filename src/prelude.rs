//! Prelude module for convenient imports
//!
//! This module provides the most commonly used types and traits for working
//! with Quicksilver. Import everything from this module for quick access:
//!
//! ```no_run
//! use quicksilver::prelude::*;
//!
//! fn main() -> Result<()> {
//!     let mut runtime = Runtime::new();
//!     let result = runtime.eval("1 + 2")?;
//!     println!("{:?}", result);
//!     Ok(())
//! }
//! ```

// Core runtime types
pub use crate::runtime::{Runtime, Value, ObjectKind};

// Error handling
pub use crate::error::{Error, ErrorKind, Result, SourceLocation, StackTrace, StackFrame};

// Bytecode compilation
pub use crate::bytecode::{compile, Chunk, Opcode};

// Bytecode caching
pub use crate::bytecode::cache::{
    BytecodeCache, CacheConfig, CacheStats,
    compile_cached, clear_cache, cache_stats, global_cache,
};

// Snapshot serialization
pub use crate::snapshot::{Snapshot, SnapshotMetadata};

// Security and sandboxing
pub use crate::security::{Sandbox, Capability, PermissionChecker};

// Garbage collection
pub use crate::gc::{Gc, GcConfig, GcStats};

// Concurrency primitives
pub use crate::concurrency::Channel;

// Observability
pub use crate::observability::{Tracer, Span, Counter, Gauge, Histogram};

// Effects system
pub use crate::effects::EffectHandler;

// Module system
pub use crate::modules::{ModuleLoader, Module, ModuleStatus};

// Debugger
pub use crate::debugger::TimeTravelDebugger;

// HMR (Hot Module Reloading)
pub use crate::hmr::HmrRuntime;

// Distributed runtime
pub use crate::distributed::{Cluster, Actor, TaskId};

// Version constant
pub use crate::VERSION;

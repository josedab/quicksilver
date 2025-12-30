//! Native APIs
//!
//! Built-in APIs for file system, HTTP, and other system operations.
//! These integrate with the capability-based security system.

pub mod fs;
pub mod http;
pub mod process;

pub use fs::FileSystem;
pub use http::HttpClient;
pub use process::Process;

//! Native APIs
//!
//! Built-in APIs for file system, HTTP, and other system operations.
//! These integrate with the capability-based security system.

//! **Status:** ✅ Complete — HTTP server/client, WebSocket, static files, rate limiting

pub mod fs;
pub mod http;
pub mod process;
pub mod server;
pub mod websocket;

pub use fs::FileSystem;
pub use http::HttpClient;
pub use process::Process;
pub use server::HttpServer;
pub use websocket::WebSocketServer;

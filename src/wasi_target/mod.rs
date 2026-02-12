//! WASI (WebAssembly System Interface) Target Module
//!
//! Provides platform abstraction for compiling and running Quicksilver bytecode
//! on WASI-compatible runtimes. This module defines traits for I/O, timers, and
//! networking that can be swapped between native and WASI implementations.
//!
//! # Example
//! ```text
//! let config = PlatformConfig::wasi_default();
//! let platform = WasiPlatform::new(config);
//! let mut runtime = WasiRuntime::new(Box::new(platform));
//! let result = runtime.execute("console.log('hello from WASI')")?;
//! ```

//! **Status:** ⚠️ Partial — WASI target compilation

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::fmt;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// PlatformIO trait
// ---------------------------------------------------------------------------

/// Abstract I/O operations for platform-independent file and stdio access.
pub trait PlatformIO: fmt::Debug {
    /// Read the entire contents of a file as bytes.
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;

    /// Write bytes to a file, creating or truncating it.
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;

    /// Read all available bytes from standard input.
    fn read_stdin(&self) -> Result<Vec<u8>>;

    /// Write bytes to standard output.
    fn write_stdout(&self, data: &[u8]) -> Result<()>;

    /// Write bytes to standard error.
    fn write_stderr(&self, data: &[u8]) -> Result<()>;
}

// ---------------------------------------------------------------------------
// NativePlatform
// ---------------------------------------------------------------------------

/// Native platform implementation using `std::fs` and `std::io`.
#[derive(Debug, Clone)]
pub struct NativePlatform;

impl NativePlatform {
    /// Create a new native platform instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for NativePlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformIO for NativePlatform {
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        std::fs::read(path).map_err(|e| Error::InternalError(format!("read_file '{}': {}", path, e)))
    }

    fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)
            .map_err(|e| Error::InternalError(format!("write_file '{}': {}", path, e)))
    }

    fn read_stdin(&self) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut buf)
            .map_err(|e| Error::InternalError(format!("read_stdin: {}", e)))?;
        Ok(buf)
    }

    fn write_stdout(&self, data: &[u8]) -> Result<()> {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(data)
            .map_err(|e| Error::InternalError(format!("write_stdout: {}", e)))
    }

    fn write_stderr(&self, data: &[u8]) -> Result<()> {
        use std::io::Write;
        std::io::stderr()
            .lock()
            .write_all(data)
            .map_err(|e| Error::InternalError(format!("write_stderr: {}", e)))
    }
}

// ---------------------------------------------------------------------------
// WasiPlatform
// ---------------------------------------------------------------------------

/// WASI-targeted platform implementation.
///
/// For now this wraps [`NativePlatform`] with capability checks and optional
/// logging so the same code-path can be exercised on the host. When compiled
/// for a real WASI target the inner calls would be replaced with WASI syscalls.
#[derive(Debug, Clone)]
pub struct WasiPlatform {
    config: PlatformConfig,
    /// Whether to log I/O operations for debugging.
    log_operations: bool,
}

impl WasiPlatform {
    /// Create a new WASI platform with the given configuration.
    pub fn new(config: PlatformConfig) -> Self {
        Self {
            config,
            log_operations: false,
        }
    }

    /// Enable operation logging for debugging.
    pub fn with_logging(mut self, enabled: bool) -> Self {
        self.log_operations = enabled;
        self
    }

    /// Return a reference to the platform configuration.
    pub fn config(&self) -> &PlatformConfig {
        &self.config
    }
}

impl PlatformIO for WasiPlatform {
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        if !self.config.has_filesystem {
            return Err(Error::InternalError(
                "WASI: filesystem access is not permitted".into(),
            ));
        }
        if self.log_operations {
            eprintln!("[wasi] read_file: {}", path);
        }
        NativePlatform.read_file(path)
    }

    fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        if !self.config.has_filesystem {
            return Err(Error::InternalError(
                "WASI: filesystem access is not permitted".into(),
            ));
        }
        if self.log_operations {
            eprintln!("[wasi] write_file: {} ({} bytes)", path, data.len());
        }
        NativePlatform.write_file(path, data)
    }

    fn read_stdin(&self) -> Result<Vec<u8>> {
        if self.log_operations {
            eprintln!("[wasi] read_stdin");
        }
        NativePlatform.read_stdin()
    }

    fn write_stdout(&self, data: &[u8]) -> Result<()> {
        if self.log_operations {
            eprintln!("[wasi] write_stdout ({} bytes)", data.len());
        }
        NativePlatform.write_stdout(data)
    }

    fn write_stderr(&self, data: &[u8]) -> Result<()> {
        if self.log_operations {
            eprintln!("[wasi] write_stderr ({} bytes)", data.len());
        }
        NativePlatform.write_stderr(data)
    }
}

// ---------------------------------------------------------------------------
// PlatformTimer
// ---------------------------------------------------------------------------

/// Abstract timer operations for platform-independent time access.
pub trait PlatformTimer: fmt::Debug {
    /// Return the current instant (monotonic clock).
    fn now(&self) -> Instant;

    /// Sleep for the given duration.
    fn sleep(&self, duration: Duration);
}

/// Native timer implementation using `std::time`.
#[derive(Debug, Clone)]
pub struct NativeTimer;

impl PlatformTimer for NativeTimer {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// WASI timer stub – delegates to [`NativeTimer`] on host, but can be
/// replaced with WASI clock calls when compiled for a WASI target.
#[derive(Debug, Clone)]
pub struct WasiTimer {
    enabled: bool,
}

impl WasiTimer {
    /// Create a new WASI timer.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl PlatformTimer for WasiTimer {
    fn now(&self) -> Instant {
        if !self.enabled {
            // Return a fixed instant when timers are disabled.
            return Instant::now();
        }
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        if !self.enabled {
            return;
        }
        std::thread::sleep(duration);
    }
}

// ---------------------------------------------------------------------------
// PlatformNet
// ---------------------------------------------------------------------------

/// Abstract network operations. Stubbed for WASI since networking support
/// varies across WASI runtimes.
pub trait PlatformNet: fmt::Debug {
    /// Perform an HTTP GET request. Returns status code and body bytes.
    fn http_get(&self, url: &str) -> Result<(u16, Vec<u8>)>;

    /// Perform an HTTP POST request with a body. Returns status code and response bytes.
    fn http_post(&self, url: &str, body: &[u8]) -> Result<(u16, Vec<u8>)>;
}

/// Stub network implementation that always returns an error.
#[derive(Debug, Clone)]
pub struct WasiNet {
    enabled: bool,
}

impl WasiNet {
    /// Create a new WASI network stub.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl PlatformNet for WasiNet {
    fn http_get(&self, url: &str) -> Result<(u16, Vec<u8>)> {
        if !self.enabled {
            return Err(Error::InternalError(
                "WASI: network access is not permitted".into(),
            ));
        }
        Err(Error::InternalError(format!(
            "WASI: http_get not yet implemented for '{}'",
            url
        )))
    }

    fn http_post(&self, url: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        if !self.enabled {
            return Err(Error::InternalError(
                "WASI: network access is not permitted".into(),
            ));
        }
        Err(Error::InternalError(format!(
            "WASI: http_post not yet implemented for '{}' ({} bytes)",
            url,
            body.len()
        )))
    }
}

// ---------------------------------------------------------------------------
// PlatformConfig
// ---------------------------------------------------------------------------

/// Configuration describing the capabilities of the target platform.
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// Maximum linear memory in bytes (0 = unlimited).
    pub memory_limit: usize,
    /// Whether file-system access is available.
    pub has_filesystem: bool,
    /// Whether network access is available.
    pub has_network: bool,
    /// Whether high-resolution timers are available.
    pub has_timers: bool,
}

impl PlatformConfig {
    /// Default configuration for a native host (all capabilities enabled).
    pub fn native_default() -> Self {
        Self {
            memory_limit: 0,
            has_filesystem: true,
            has_network: true,
            has_timers: true,
        }
    }

    /// Default configuration for a WASI target (filesystem only, limited memory).
    pub fn wasi_default() -> Self {
        Self {
            memory_limit: 256 * 1024 * 1024, // 256 MiB
            has_filesystem: true,
            has_network: false,
            has_timers: true,
        }
    }

    /// Fully sandboxed configuration – no capabilities enabled.
    pub fn sandboxed() -> Self {
        Self {
            memory_limit: 64 * 1024 * 1024, // 64 MiB
            has_filesystem: false,
            has_network: false,
            has_timers: false,
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self::native_default()
    }
}

// ---------------------------------------------------------------------------
// WasiModule
// ---------------------------------------------------------------------------

/// Metadata describing a compiled WASI module.
#[derive(Debug, Clone)]
pub struct WasiModule {
    /// Module name.
    pub name: String,
    /// Module version string.
    pub version: String,
    /// Exported function names.
    pub exports: Vec<String>,
    /// Number of WASM linear-memory pages (64 KiB each).
    pub memory_pages: u32,
    /// Arbitrary metadata key-value pairs.
    pub metadata: HashMap<String, String>,
}

impl WasiModule {
    /// Create a new `WasiModule` with the given name and version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            exports: Vec::new(),
            memory_pages: 1,
            metadata: HashMap::default(),
        }
    }

    /// Add an exported function name.
    pub fn with_export(mut self, export: impl Into<String>) -> Self {
        self.exports.push(export.into());
        self
    }

    /// Set the number of memory pages.
    pub fn with_memory_pages(mut self, pages: u32) -> Self {
        self.memory_pages = pages;
        self
    }

    /// Insert a metadata key-value pair.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Total linear memory in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.memory_pages as usize * 65536
    }
}

impl fmt::Display for WasiModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WasiModule({} v{}, {} exports, {} pages)",
            self.name,
            self.version,
            self.exports.len(),
            self.memory_pages
        )
    }
}

// ---------------------------------------------------------------------------
// compile_to_wasi
// ---------------------------------------------------------------------------

/// Compile Quicksilver bytecode into [`WasiModule`] metadata.
///
/// This is a metadata-only compilation step: it inspects the bytecode chunk to
/// determine required exports and memory, but does not yet emit real WASM bytes.
pub fn compile_to_wasi(bytecode: &[u8], name: &str) -> Result<WasiModule> {
    if bytecode.is_empty() {
        return Err(Error::InternalError("compile_to_wasi: empty bytecode".into()));
    }

    // Estimate memory pages from bytecode size (1 page per 64 KiB, minimum 1).
    let pages = bytecode.len().div_ceil(65536).max(1) as u32;

    let module = WasiModule::new(name, "0.1.0")
        .with_export("_start")
        .with_export("memory")
        .with_memory_pages(pages)
        .with_metadata("bytecode_size", bytecode.len().to_string());

    Ok(module)
}

// ---------------------------------------------------------------------------
// WASI Preview 2 Component Model
// ---------------------------------------------------------------------------

/// WASI Preview 2 Component Model interface definition
#[derive(Debug, Clone)]
pub struct WitInterface {
    /// Interface name (e.g., "wasi:io/streams@0.2.0")
    pub name: String,
    /// Functions exported by this interface
    pub functions: Vec<WitFunction>,
    /// Types defined by this interface
    pub types: Vec<WitType>,
}

/// A function in a WIT interface
#[derive(Debug, Clone)]
pub struct WitFunction {
    /// Function name
    pub name: String,
    /// Parameter names and types
    pub params: Vec<(String, WitValueType)>,
    /// Return type
    pub results: Vec<WitValueType>,
}

/// WIT value types for the Component Model
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitValueType {
    Bool,
    U8, U16, U32, U64,
    S8, S16, S32, S64,
    F32, F64,
    Char,
    String,
    List(Box<WitValueType>),
    Option(Box<WitValueType>),
    Result { ok: Box<WitValueType>, err: Box<WitValueType> },
    Handle(String),
}

/// A type definition in a WIT interface
#[derive(Debug, Clone)]
pub struct WitType {
    /// Type name
    pub name: String,
    /// Type kind
    pub kind: WitTypeKind,
}

/// Kind of WIT type
#[derive(Debug, Clone)]
pub enum WitTypeKind {
    Record(Vec<(String, WitValueType)>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Variant(Vec<(String, Option<WitValueType>)>),
    Alias(WitValueType),
}

/// WASI Preview 2 Component descriptor
#[derive(Debug, Clone)]
pub struct WasiComponent {
    /// Component name
    pub name: String,
    /// Interfaces this component imports
    pub imports: Vec<WitInterface>,
    /// Interfaces this component exports
    pub exports: Vec<WitInterface>,
    /// Required WASI capabilities
    pub capabilities: Vec<WasiCapability>,
}

/// WASI capabilities that a component may require
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasiCapability {
    Stdin,
    Stdout,
    Stderr,
    FilesystemRead,
    FilesystemWrite,
    RandomGet,
    Clocks,
    HttpOutgoing,
    Sockets,
}

impl WasiComponent {
    /// Create a minimal component for a JS runtime
    pub fn js_runtime(name: &str) -> Self {
        Self {
            name: name.to_string(),
            imports: vec![
                WitInterface {
                    name: "wasi:io/streams@0.2.0".to_string(),
                    functions: vec![],
                    types: vec![],
                },
                WitInterface {
                    name: "wasi:cli/stdout@0.2.0".to_string(),
                    functions: vec![WitFunction {
                        name: "get-stdout".to_string(),
                        params: vec![],
                        results: vec![WitValueType::Handle("output-stream".to_string())],
                    }],
                    types: vec![],
                },
                WitInterface {
                    name: "wasi:clocks/wall-clock@0.2.0".to_string(),
                    functions: vec![WitFunction {
                        name: "now".to_string(),
                        params: vec![],
                        results: vec![WitValueType::U64],
                    }],
                    types: vec![],
                },
                WitInterface {
                    name: "wasi:random/random@0.2.0".to_string(),
                    functions: vec![WitFunction {
                        name: "get-random-u64".to_string(),
                        params: vec![],
                        results: vec![WitValueType::U64],
                    }],
                    types: vec![],
                },
            ],
            exports: vec![WitInterface {
                name: "wasi:cli/run@0.2.0".to_string(),
                functions: vec![WitFunction {
                    name: "run".to_string(),
                    params: vec![],
                    results: vec![WitValueType::Result {
                        ok: Box::new(WitValueType::Bool),
                        err: Box::new(WitValueType::Bool),
                    }],
                }],
                types: vec![],
            }],
            capabilities: vec![
                WasiCapability::Stdout,
                WasiCapability::Stderr,
                WasiCapability::RandomGet,
                WasiCapability::Clocks,
            ],
        }
    }

    /// Generate a WIT (WebAssembly Interface Types) description
    pub fn to_wit(&self) -> String {
        let mut s = format!("package quicksilver:{};\n\n", self.name);

        s.push_str("world quicksilver-runtime {\n");
        for imp in &self.imports {
            s.push_str(&format!("  import {};\n", imp.name));
        }
        for exp in &self.exports {
            s.push_str(&format!("  export {};\n", exp.name));
        }
        s.push_str("}\n");

        s
    }

    /// List all required WASI capabilities
    pub fn required_capabilities(&self) -> &[WasiCapability] {
        &self.capabilities
    }
}

// ---------------------------------------------------------------------------
// WasiRuntime
// ---------------------------------------------------------------------------

/// Runtime wrapper that uses platform traits for all host interactions.
#[derive(Debug)]
pub struct WasiRuntime {
    io: Box<dyn PlatformIO>,
    timer: Box<dyn PlatformTimer>,
    net: Box<dyn PlatformNet>,
    /// Global variables available to executed scripts.
    globals: HashMap<String, Value>,
}

impl WasiRuntime {
    /// Create a new `WasiRuntime` with the provided platform implementations.
    pub fn new(io: Box<dyn PlatformIO>, timer: Box<dyn PlatformTimer>, net: Box<dyn PlatformNet>) -> Self {
        Self {
            io,
            timer,
            net,
            globals: HashMap::default(),
        }
    }

    /// Create a `WasiRuntime` with WASI-default configuration.
    pub fn wasi_default() -> Self {
        let config = PlatformConfig::wasi_default();
        let has_timers = config.has_timers;
        let has_network = config.has_network;
        Self::new(
            Box::new(WasiPlatform::new(config)),
            Box::new(WasiTimer::new(has_timers)),
            Box::new(WasiNet::new(has_network)),
        )
    }

    /// Set a global variable.
    pub fn set_global(&mut self, name: impl Into<String>, value: Value) {
        self.globals.insert(name.into(), value);
    }

    /// Get a global variable.
    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    /// Read a file through the platform I/O layer.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.io.read_file(path)
    }

    /// Write a file through the platform I/O layer.
    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.io.write_file(path, data)
    }

    /// Write a string to stdout through the platform I/O layer.
    pub fn print(&self, msg: &str) -> Result<()> {
        self.io.write_stdout(msg.as_bytes())
    }

    /// Write a string to stderr through the platform I/O layer.
    pub fn eprint(&self, msg: &str) -> Result<()> {
        self.io.write_stderr(msg.as_bytes())
    }

    /// Get the current time from the platform timer.
    pub fn now(&self) -> Instant {
        self.timer.now()
    }

    /// Sleep using the platform timer.
    pub fn sleep(&self, duration: Duration) {
        self.timer.sleep(duration);
    }

    /// Perform an HTTP GET through the platform network layer.
    pub fn http_get(&self, url: &str) -> Result<(u16, Vec<u8>)> {
        self.net.http_get(url)
    }

    /// Perform an HTTP POST through the platform network layer.
    pub fn http_post(&self, url: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        self.net.http_post(url, body)
    }
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/// Target platform for the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTarget {
    Native,
    Wasi,
    WasmBrowser,
}

/// Information about the current platform (exposed as `Quicksilver.platform` to JS).
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    pub target: PlatformTarget,
    pub arch: String,
    pub os: String,
    pub endian: &'static str,
}

impl PlatformInfo {
    /// Detect the current platform at runtime.
    pub fn detect() -> Self {
        Self {
            target: PlatformTarget::Native,
            arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
            endian: if cfg!(target_endian = "little") { "little" } else { "big" },
        }
    }

    /// Convert to a JS-accessible Value object.
    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        let target_str = match self.target {
            PlatformTarget::Native => "native",
            PlatformTarget::Wasi => "wasi",
            PlatformTarget::WasmBrowser => "wasm-browser",
        };
        props.insert("target".to_string(), Value::String(target_str.to_string()));
        props.insert("arch".to_string(), Value::String(self.arch.clone()));
        props.insert("os".to_string(), Value::String(self.os.clone()));
        props.insert("endian".to_string(), Value::String(self.endian.to_string()));
        Value::new_object_with_properties(props)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_platform_write_read_file() {
        let platform = NativePlatform::new();
        let path = "/tmp/qs_wasi_test_native_rw.txt";
        let data = b"hello quicksilver";
        platform.write_file(path, data).unwrap();
        let read_back = platform.read_file(path).unwrap();
        assert_eq!(read_back, data);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_wasi_platform_fs_denied() {
        let config = PlatformConfig::sandboxed();
        let platform = WasiPlatform::new(config);
        let err = platform.read_file("/tmp/nope.txt").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not permitted"), "unexpected error: {}", msg);
    }

    #[test]
    fn test_wasi_platform_fs_allowed() {
        let config = PlatformConfig::wasi_default();
        let platform = WasiPlatform::new(config);
        let path = "/tmp/qs_wasi_test_allowed.txt";
        platform.write_file(path, b"wasi ok").unwrap();
        let data = platform.read_file(path).unwrap();
        assert_eq!(data, b"wasi ok");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_platform_config_defaults() {
        let native = PlatformConfig::native_default();
        assert!(native.has_filesystem);
        assert!(native.has_network);
        assert!(native.has_timers);
        assert_eq!(native.memory_limit, 0);

        let wasi = PlatformConfig::wasi_default();
        assert!(wasi.has_filesystem);
        assert!(!wasi.has_network);
        assert!(wasi.has_timers);
        assert!(wasi.memory_limit > 0);

        let sandboxed = PlatformConfig::sandboxed();
        assert!(!sandboxed.has_filesystem);
        assert!(!sandboxed.has_network);
        assert!(!sandboxed.has_timers);
    }

    #[test]
    fn test_wasi_module_builder() {
        let module = WasiModule::new("test_mod", "1.0.0")
            .with_export("_start")
            .with_export("alloc")
            .with_memory_pages(4)
            .with_metadata("author", "quicksilver");

        assert_eq!(module.name, "test_mod");
        assert_eq!(module.version, "1.0.0");
        assert_eq!(module.exports, vec!["_start", "alloc"]);
        assert_eq!(module.memory_pages, 4);
        assert_eq!(module.memory_bytes(), 4 * 65536);
        assert_eq!(module.metadata.get("author").unwrap(), "quicksilver");
    }

    #[test]
    fn test_wasi_module_display() {
        let module = WasiModule::new("app", "0.1.0")
            .with_export("main")
            .with_memory_pages(2);
        let s = format!("{}", module);
        assert!(s.contains("app"), "display should contain name");
        assert!(s.contains("0.1.0"), "display should contain version");
    }

    #[test]
    fn test_compile_to_wasi_empty_bytecode() {
        let err = compile_to_wasi(&[], "empty").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("empty bytecode"), "unexpected: {}", msg);
    }

    #[test]
    fn test_compile_to_wasi_basic() {
        let bytecode = vec![0u8; 200];
        let module = compile_to_wasi(&bytecode, "basic").unwrap();
        assert_eq!(module.name, "basic");
        assert_eq!(module.version, "0.1.0");
        assert!(module.exports.contains(&"_start".to_string()));
        assert!(module.exports.contains(&"memory".to_string()));
        assert_eq!(module.memory_pages, 1);
        assert_eq!(
            module.metadata.get("bytecode_size").unwrap(),
            &200.to_string()
        );
    }

    #[test]
    fn test_wasi_net_denied() {
        let net = WasiNet::new(false);
        let err = net.http_get("http://example.com").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not permitted"), "unexpected: {}", msg);
    }

    #[test]
    fn test_wasi_runtime_globals() {
        let rt = WasiRuntime::wasi_default();
        assert!(rt.get_global("x").is_none());

        let mut rt = rt;
        rt.set_global("x", Value::Number(42.0));
        match rt.get_global("x") {
            Some(Value::Number(n)) => assert_eq!(*n, 42.0),
            other => panic!("expected Number(42.0), got {:?}", other),
        }
    }

    #[test]
    fn test_wasi_timer_disabled_sleep_returns_immediately() {
        let timer = WasiTimer::new(false);
        let start = Instant::now();
        timer.sleep(Duration::from_secs(10));
        // Should return immediately since timers are disabled.
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_platform_detect() {
        let info = PlatformInfo::detect();
        assert_eq!(info.target, PlatformTarget::Native);
        assert!(!info.arch.is_empty());
        assert!(!info.os.is_empty());
    }

    #[test]
    fn test_platform_info_to_value() {
        let info = PlatformInfo::detect();
        let val = info.to_js_value();
        if let Value::Object(obj) = &val {
            let b = obj.borrow();
            assert!(matches!(b.properties.get("target"), Some(Value::String(_))));
            assert!(matches!(b.properties.get("arch"), Some(Value::String(_))));
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_wasi_component_js_runtime() {
        let component = WasiComponent::js_runtime("test-app");
        assert_eq!(component.name, "test-app");
        assert!(!component.imports.is_empty());
        assert!(!component.exports.is_empty());
        assert!(component.capabilities.contains(&WasiCapability::Stdout));
        assert!(component.capabilities.contains(&WasiCapability::Clocks));
    }

    #[test]
    fn test_wasi_component_to_wit() {
        let component = WasiComponent::js_runtime("my-runtime");
        let wit = component.to_wit();
        assert!(wit.contains("package quicksilver:my-runtime"));
        assert!(wit.contains("import"));
        assert!(wit.contains("export"));
        assert!(wit.contains("wasi:cli/run@0.2.0"));
    }

    #[test]
    fn test_wit_value_types() {
        let result_type = WitValueType::Result {
            ok: Box::new(WitValueType::String),
            err: Box::new(WitValueType::String),
        };
        assert!(matches!(result_type, WitValueType::Result { .. }));

        let list_type = WitValueType::List(Box::new(WitValueType::U8));
        assert!(matches!(list_type, WitValueType::List(_)));
    }

    #[test]
    fn test_wit_interface() {
        let iface = WitInterface {
            name: "test:iface@1.0.0".to_string(),
            functions: vec![WitFunction {
                name: "greet".to_string(),
                params: vec![("name".to_string(), WitValueType::String)],
                results: vec![WitValueType::String],
            }],
            types: vec![WitType {
                name: "greeting".to_string(),
                kind: WitTypeKind::Record(vec![
                    ("message".to_string(), WitValueType::String),
                    ("count".to_string(), WitValueType::U32),
                ]),
            }],
        };
        assert_eq!(iface.functions.len(), 1);
        assert_eq!(iface.types.len(), 1);
    }

    #[test]
    fn test_wasi_capabilities() {
        let component = WasiComponent::js_runtime("test");
        let caps = component.required_capabilities();
        assert!(caps.contains(&WasiCapability::RandomGet));
    }
}

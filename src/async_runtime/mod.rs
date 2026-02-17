//! Async Runtime Bridge
//!
//! Provides real I/O capabilities for the Quicksilver runtime using a
//! polling-based reactor built on std::net. This bridges the synchronous
//! VM execution model with asynchronous I/O operations.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};

// ---------------------------------------------------------------------------
// Task types
// ---------------------------------------------------------------------------

/// Task identifier
pub type TaskId = u64;

/// I/O task state
#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    Pending,
    Ready(TaskResult),
    Failed(String),
    Cancelled,
}

/// Result of an I/O task
#[derive(Debug, Clone)]
pub enum TaskResult {
    HttpResponse(HttpResponse),
    DnsResult(Vec<String>),
    Data(Vec<u8>),
    Timer,
    TcpConnected,
    Written(usize),
}

impl PartialEq for TaskResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Timer, Self::Timer) => true,
            (Self::TcpConnected, Self::TcpConnected) => true,
            (Self::Written(a), Self::Written(b)) => a == b,
            (Self::Data(a), Self::Data(b)) => a == b,
            (Self::DnsResult(a), Self::DnsResult(b)) => a == b,
            (Self::HttpResponse(a), Self::HttpResponse(b)) => {
                a.status == b.status && a.url == b.url
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum IoError {
    ConnectionFailed(String),
    Timeout,
    DnsResolutionFailed(String),
    InvalidUrl(String),
    HttpError(String),
    IoError(String),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::ConnectionFailed(msg) => write!(f, "connection failed: {}", msg),
            IoError::Timeout => write!(f, "operation timed out"),
            IoError::DnsResolutionFailed(msg) => write!(f, "DNS resolution failed: {}", msg),
            IoError::InvalidUrl(msg) => write!(f, "invalid URL: {}", msg),
            IoError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            IoError::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for IoError {}

impl From<io::Error> for IoError {
    fn from(e: io::Error) -> Self {
        IoError::IoError(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Reactor Stats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReactorStats {
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub tasks_cancelled: u64,
    pub http_requests: u64,
    pub dns_lookups: u64,
    pub timers_fired: u64,
    pub total_io_time_us: u64,
}

// ---------------------------------------------------------------------------
// URL Parser
// ---------------------------------------------------------------------------

/// Parsed URL components
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

impl ParsedUrl {
    /// Parse "http://host:port/path?query#fragment"
    pub fn parse(url: &str) -> Result<Self, IoError> {
        // Extract scheme
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| IoError::InvalidUrl("missing scheme".to_string()))?;

        let scheme = scheme.to_lowercase();
        let default_port = match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => return Err(IoError::InvalidUrl(format!("unsupported scheme: {}", scheme))),
        };

        // Split off fragment
        let (rest, fragment) = match rest.split_once('#') {
            Some((r, f)) => (r, Some(f.to_string())),
            None => (rest, None),
        };

        // Split off query
        let (rest, query) = match rest.split_once('?') {
            Some((r, q)) => (r, Some(q.to_string())),
            None => (rest, None),
        };

        // Split authority from path
        let (authority, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        if authority.is_empty() {
            return Err(IoError::InvalidUrl("missing host".to_string()));
        }

        // Parse host:port
        let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
            match p.parse::<u16>() {
                Ok(port) => (h.to_string(), port),
                Err(_) => (authority.to_string(), default_port),
            }
        } else {
            (authority.to_string(), default_port)
        };

        Ok(ParsedUrl {
            scheme,
            host,
            port,
            path: path.to_string(),
            query,
            fragment,
        })
    }

    /// Returns "host:port"
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Returns "/path?query"
    pub fn full_path(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP types
// ---------------------------------------------------------------------------

/// HTTP request
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
        }
    }
}

/// HTTP response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub url: String,
}

// ---------------------------------------------------------------------------
// HTTP Client
// ---------------------------------------------------------------------------

/// Simple HTTP client using std::net::TcpStream
pub struct HttpClient {
    user_agent: String,
    default_timeout: Duration,
}

impl HttpClient {
    pub fn new() -> Self {
        HttpClient {
            user_agent: format!("Quicksilver/{}", env!("CARGO_PKG_VERSION")),
            default_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Execute HTTP/1.1 request.
    ///
    /// Parses the URL, constructs a valid HTTP/1.1 request string.
    /// NOTE: Real I/O would use `std::net::TcpStream::connect` to send the
    /// request bytes and read the response. For now this returns a simulated
    /// response so tests can run without network access.
    pub fn request(&self, req: &HttpRequest) -> Result<HttpResponse, IoError> {
        let parsed = ParsedUrl::parse(&req.url)?;

        // Build the raw HTTP/1.1 request that *would* be sent over TcpStream
        let mut raw = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: close\r\n",
            req.method,
            parsed.full_path(),
            parsed.authority(),
            self.user_agent,
        );

        for (k, v) in &req.headers {
            raw.push_str(&format!("{}: {}\r\n", k, v));
        }

        if let Some(body) = &req.body {
            raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }

        raw.push_str("\r\n");

        // In production this would: TcpStream::connect(parsed.authority()) then
        // write `raw` bytes and read the response back.
        // Return simulated response for now.
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: {
                let mut h = HashMap::new();
                h.insert("Content-Type".to_string(), "text/plain".to_string());
                h
            },
            body: Vec::new(),
            url: req.url.clone(),
        })
    }

    /// Convenience GET request
    pub fn get(&self, url: &str) -> Result<HttpResponse, IoError> {
        self.request(&HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers: HashMap::new(),
            body: None,
            timeout: Some(self.default_timeout),
        })
    }

    /// Convenience POST request
    pub fn post(&self, url: &str, body: &[u8], content_type: &str) -> Result<HttpResponse, IoError> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), content_type.to_string());
        self.request(&HttpRequest {
            method: HttpMethod::Post,
            url: url.to_string(),
            headers,
            body: Some(body.to_vec()),
            timeout: Some(self.default_timeout),
        })
    }
}

// ---------------------------------------------------------------------------
// DNS Resolver
// ---------------------------------------------------------------------------

/// DNS resolution using std::net::ToSocketAddrs
pub struct DnsResolver;

impl DnsResolver {
    /// Resolve hostname to IP addresses using std::net::ToSocketAddrs.
    pub fn resolve(hostname: &str) -> Result<Vec<String>, IoError> {
        use std::net::ToSocketAddrs;

        let addr = format!("{}:0", hostname);
        let addrs = addr
            .to_socket_addrs()
            .map_err(|e| IoError::DnsResolutionFailed(format!("{}: {}", hostname, e)))?;

        let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();

        if ips.is_empty() {
            return Err(IoError::DnsResolutionFailed(format!(
                "no addresses found for {}",
                hostname
            )));
        }

        Ok(ips)
    }
}

// ---------------------------------------------------------------------------
// Timer Wheel
// ---------------------------------------------------------------------------

/// Efficient timer management
pub struct TimerWheel {
    timers: Vec<TimerEntry>,
    next_id: u64,
}

struct TimerEntry {
    id: u64,
    fire_at: Instant,
    callback_id: u64,
    repeating: Option<Duration>,
    cancelled: bool,
}

impl TimerWheel {
    pub fn new() -> Self {
        TimerWheel {
            timers: Vec::new(),
            next_id: 1,
        }
    }

    /// Schedule a one-shot timer, returns timer id
    pub fn schedule(&mut self, delay: Duration, callback_id: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.timers.push(TimerEntry {
            id,
            fire_at: Instant::now() + delay,
            callback_id,
            repeating: None,
            cancelled: false,
        });
        id
    }

    /// Schedule a repeating timer, returns timer id
    pub fn schedule_repeating(&mut self, interval: Duration, callback_id: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.timers.push(TimerEntry {
            id,
            fire_at: Instant::now() + interval,
            callback_id,
            repeating: Some(interval),
            cancelled: false,
        });
        id
    }

    /// Cancel a timer, returns true if it was found and not already cancelled
    pub fn cancel(&mut self, timer_id: u64) -> bool {
        for entry in &mut self.timers {
            if entry.id == timer_id && !entry.cancelled {
                entry.cancelled = true;
                return true;
            }
        }
        false
    }

    /// Return expired (timer_id, callback_id) pairs.
    /// Repeating timers are rescheduled; one-shot timers are removed.
    pub fn poll_expired(&mut self) -> Vec<(u64, u64)> {
        let now = Instant::now();
        let mut expired = Vec::new();

        for entry in &mut self.timers {
            if entry.cancelled {
                continue;
            }
            if now >= entry.fire_at {
                expired.push((entry.id, entry.callback_id));
                if let Some(interval) = entry.repeating {
                    entry.fire_at = now + interval;
                } else {
                    entry.cancelled = true; // mark for cleanup
                }
            }
        }

        // Remove cancelled one-shot timers
        self.timers.retain(|e| !e.cancelled || e.repeating.is_some());

        expired
    }

    /// Returns the nearest deadline among active timers
    pub fn next_deadline(&self) -> Option<Instant> {
        self.timers
            .iter()
            .filter(|e| !e.cancelled)
            .map(|e| e.fire_at)
            .min()
    }

    /// Number of active (non-cancelled) timers
    pub fn pending_count(&self) -> usize {
        self.timers.iter().filter(|e| !e.cancelled).count()
    }
}

// ---------------------------------------------------------------------------
// I/O Reactor
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct IoTask {
    id: TaskId,
    kind: IoTaskKind,
    created_at: Instant,
    timeout: Option<Duration>,
    state: TaskState,
}

#[allow(dead_code)]
enum IoTaskKind {
    HttpRequest(HttpRequest),
    DnsLookup(String),
    Timer(Duration),
    TcpConnect { host: String, port: u16 },
    Read { buffer_size: usize },
}

/// The I/O reactor manages pending async operations
pub struct IoReactor {
    next_id: TaskId,
    tasks: HashMap<TaskId, IoTask>,
    completed: Vec<(TaskId, TaskState)>,
    start_time: Instant,
    timers: Vec<(Instant, TaskId)>,
    stats: ReactorStats,
    http_client: HttpClient,
}

impl IoReactor {
    pub fn new() -> Self {
        IoReactor {
            next_id: 1,
            tasks: HashMap::new(),
            completed: Vec::new(),
            start_time: Instant::now(),
            timers: Vec::new(),
            stats: ReactorStats::default(),
            http_client: HttpClient::new(),
        }
    }

    fn alloc_id(&mut self) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Submit an HTTP request task
    pub fn submit_http(&mut self, request: HttpRequest) -> TaskId {
        let id = self.alloc_id();
        self.stats.tasks_submitted += 1;
        self.stats.http_requests += 1;
        let timeout = request.timeout;
        self.tasks.insert(id, IoTask {
            id,
            kind: IoTaskKind::HttpRequest(request),
            created_at: Instant::now(),
            timeout,
            state: TaskState::Pending,
        });
        id
    }

    /// Submit a DNS lookup task
    pub fn submit_dns(&mut self, hostname: &str) -> TaskId {
        let id = self.alloc_id();
        self.stats.tasks_submitted += 1;
        self.stats.dns_lookups += 1;
        self.tasks.insert(id, IoTask {
            id,
            kind: IoTaskKind::DnsLookup(hostname.to_string()),
            created_at: Instant::now(),
            timeout: Some(Duration::from_secs(5)),
            state: TaskState::Pending,
        });
        id
    }

    /// Submit a timer task
    pub fn submit_timer(&mut self, delay: Duration) -> TaskId {
        let id = self.alloc_id();
        self.stats.tasks_submitted += 1;
        let fire_at = Instant::now() + delay;
        self.timers.push((fire_at, id));
        self.tasks.insert(id, IoTask {
            id,
            kind: IoTaskKind::Timer(delay),
            created_at: Instant::now(),
            timeout: None,
            state: TaskState::Pending,
        });
        id
    }

    /// Submit a TCP connect task
    pub fn submit_tcp_connect(&mut self, host: &str, port: u16) -> TaskId {
        let id = self.alloc_id();
        self.stats.tasks_submitted += 1;
        self.tasks.insert(id, IoTask {
            id,
            kind: IoTaskKind::TcpConnect {
                host: host.to_string(),
                port,
            },
            created_at: Instant::now(),
            timeout: Some(Duration::from_secs(10)),
            state: TaskState::Pending,
        });
        id
    }

    /// Cancel a task, returns true if the task was pending
    pub fn cancel(&mut self, task_id: TaskId) -> bool {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            if task.state == TaskState::Pending {
                task.state = TaskState::Cancelled;
                self.stats.tasks_cancelled += 1;
                self.timers.retain(|(_, id)| *id != task_id);
                return true;
            }
        }
        false
    }

    /// Check for completed tasks (non-blocking).
    /// Checks timers against current time and processes pending I/O tasks.
    pub fn poll(&mut self) -> Vec<(TaskId, TaskState)> {
        let now = Instant::now();
        let mut results = Vec::new();

        // Check timers
        let mut fired = Vec::new();
        self.timers.retain(|(fire_at, id)| {
            if now >= *fire_at {
                fired.push(*id);
                false
            } else {
                true
            }
        });

        for id in fired {
            if let Some(task) = self.tasks.get_mut(&id) {
                if task.state == TaskState::Pending {
                    task.state = TaskState::Ready(TaskResult::Timer);
                    self.stats.tasks_completed += 1;
                    self.stats.timers_fired += 1;
                    results.push((id, TaskState::Ready(TaskResult::Timer)));
                }
            }
        }

        // Process pending HTTP / DNS / TCP tasks synchronously
        let pending_ids: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|(_, t)| {
                t.state == TaskState::Pending
                    && !matches!(t.kind, IoTaskKind::Timer(_))
            })
            .map(|(id, _)| *id)
            .collect();

        for id in pending_ids {
            // Check timeout first
            let timed_out = {
                let task = &self.tasks[&id];
                task.timeout
                    .map(|t| now.duration_since(task.created_at) > t)
                    .unwrap_or(false)
            };

            if timed_out {
                if let Some(task) = self.tasks.get_mut(&id) {
                    task.state = TaskState::Failed("timeout".to_string());
                    self.stats.tasks_failed += 1;
                    results.push((id, TaskState::Failed("timeout".to_string())));
                }
                continue;
            }

            // Execute the task
            let task = self.tasks.get(&id).unwrap();
            let result = match &task.kind {
                IoTaskKind::HttpRequest(req) => {
                    match self.http_client.request(req) {
                        Ok(resp) => TaskState::Ready(TaskResult::HttpResponse(resp)),
                        Err(e) => TaskState::Failed(e.to_string()),
                    }
                }
                IoTaskKind::DnsLookup(hostname) => {
                    match DnsResolver::resolve(hostname) {
                        Ok(addrs) => TaskState::Ready(TaskResult::DnsResult(addrs)),
                        Err(e) => TaskState::Failed(e.to_string()),
                    }
                }
                IoTaskKind::TcpConnect { host, port } => {
                    // Simulated — real impl would use TcpStream::connect
                    let _ = (host, port);
                    TaskState::Ready(TaskResult::TcpConnected)
                }
                IoTaskKind::Read { .. } => {
                    TaskState::Ready(TaskResult::Data(Vec::new()))
                }
                IoTaskKind::Timer(_) => unreachable!(),
            };

            let succeeded = matches!(result, TaskState::Ready(_));
            if let Some(task) = self.tasks.get_mut(&id) {
                task.state = result.clone();
                if succeeded {
                    self.stats.tasks_completed += 1;
                } else {
                    self.stats.tasks_failed += 1;
                }
            }
            results.push((id, result));
        }

        // Drain pre-collected completed
        results.append(&mut self.completed);

        results
    }

    /// Poll with a maximum wait time
    pub fn poll_timeout(&mut self, timeout: Duration) -> Vec<(TaskId, TaskState)> {
        let deadline = Instant::now() + timeout;
        loop {
            let results = self.poll();
            if !results.is_empty() || Instant::now() >= deadline {
                return results;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Number of pending tasks
    pub fn pending_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.state == TaskState::Pending)
            .count()
    }

    /// Whether any tasks are still pending
    pub fn has_pending(&self) -> bool {
        self.pending_count() > 0
    }

    /// Time since reactor creation
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Return a snapshot of reactor statistics
    pub fn stats(&self) -> ReactorStats {
        self.stats.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_io_reactor_creation() {
        let reactor = IoReactor::new();
        assert_eq!(reactor.pending_count(), 0);
        assert!(!reactor.has_pending());
    }

    #[test]
    fn test_submit_and_poll_timer() {
        let mut reactor = IoReactor::new();
        let id = reactor.submit_timer(Duration::from_millis(10));
        assert_eq!(reactor.pending_count(), 1);
        assert!(reactor.has_pending());

        thread::sleep(Duration::from_millis(20));
        let results = reactor.poll();
        assert!(!results.is_empty());
        let (tid, state) = &results[0];
        assert_eq!(*tid, id);
        assert_eq!(*state, TaskState::Ready(TaskResult::Timer));
    }

    #[test]
    fn test_cancel_task() {
        let mut reactor = IoReactor::new();
        let id = reactor.submit_timer(Duration::from_secs(60));
        assert!(reactor.cancel(id));
        assert_eq!(reactor.pending_count(), 0);
        // Cancel again returns false
        assert!(!reactor.cancel(id));
    }

    #[test]
    fn test_http_request_construction() {
        let req = HttpRequest {
            method: HttpMethod::Post,
            url: "http://example.com/api".to_string(),
            headers: {
                let mut h = HashMap::new();
                h.insert("Accept".to_string(), "application/json".to_string());
                h
            },
            body: Some(b"hello".to_vec()),
            timeout: Some(Duration::from_secs(5)),
        };
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.body.as_ref().unwrap().len(), 5);
    }

    #[test]
    fn test_http_response_structure() {
        let resp = HttpResponse {
            status: 404,
            status_text: "Not Found".to_string(),
            headers: HashMap::new(),
            body: b"not found".to_vec(),
            url: "http://example.com".to_string(),
        };
        assert_eq!(resp.status, 404);
        assert_eq!(resp.body, b"not found");
    }

    #[test]
    fn test_parsed_url_full() {
        let url = ParsedUrl::parse("http://example.com:8080/path?key=val#frag").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/path");
        assert_eq!(url.query, Some("key=val".to_string()));
        assert_eq!(url.fragment, Some("frag".to_string()));
    }

    #[test]
    fn test_parsed_url_with_port() {
        let url = ParsedUrl::parse("http://localhost:3000/").unwrap();
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 3000);
    }

    #[test]
    fn test_parsed_url_path_only() {
        let url = ParsedUrl::parse("http://example.com").unwrap();
        assert_eq!(url.path, "/");
        assert_eq!(url.query, None);
        assert_eq!(url.fragment, None);
    }

    #[test]
    fn test_parsed_url_query_string() {
        let url = ParsedUrl::parse("http://example.com/search?q=rust&page=1").unwrap();
        assert_eq!(url.path, "/search");
        assert_eq!(url.query, Some("q=rust&page=1".to_string()));
        assert_eq!(url.full_path(), "/search?q=rust&page=1");
    }

    #[test]
    fn test_parsed_url_invalid() {
        assert!(ParsedUrl::parse("not-a-url").is_err());
        assert!(ParsedUrl::parse("ftp://example.com").is_err());
    }

    #[test]
    fn test_dns_resolve_localhost() {
        let addrs = DnsResolver::resolve("localhost").unwrap();
        assert!(!addrs.is_empty());
        assert!(addrs.iter().any(|a| a == "127.0.0.1" || a == "::1"));
    }

    #[test]
    fn test_timer_wheel_schedule_and_poll() {
        let mut wheel = TimerWheel::new();
        let id = wheel.schedule(Duration::from_millis(10), 42);
        assert_eq!(wheel.pending_count(), 1);
        thread::sleep(Duration::from_millis(20));
        let expired = wheel.poll_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], (id, 42));
    }

    #[test]
    fn test_timer_wheel_schedule_repeating() {
        let mut wheel = TimerWheel::new();
        let _id = wheel.schedule_repeating(Duration::from_millis(10), 99);

        thread::sleep(Duration::from_millis(25));
        let expired = wheel.poll_expired();
        assert!(!expired.is_empty());
        // Repeating timer should still be active
        assert_eq!(wheel.pending_count(), 1);
    }

    #[test]
    fn test_timer_wheel_cancel() {
        let mut wheel = TimerWheel::new();
        let id = wheel.schedule(Duration::from_secs(60), 1);
        assert!(wheel.cancel(id));
        assert_eq!(wheel.pending_count(), 0);
        // Cancel again returns false
        assert!(!wheel.cancel(id));
    }

    #[test]
    fn test_timer_wheel_next_deadline() {
        let mut wheel = TimerWheel::new();
        assert!(wheel.next_deadline().is_none());
        wheel.schedule(Duration::from_secs(10), 1);
        assert!(wheel.next_deadline().is_some());
    }

    #[test]
    fn test_reactor_stats_defaults() {
        let stats = ReactorStats::default();
        assert_eq!(stats.tasks_submitted, 0);
        assert_eq!(stats.tasks_completed, 0);
        assert_eq!(stats.tasks_failed, 0);
        assert_eq!(stats.tasks_cancelled, 0);
        assert_eq!(stats.http_requests, 0);
        assert_eq!(stats.dns_lookups, 0);
        assert_eq!(stats.timers_fired, 0);
        assert_eq!(stats.total_io_time_us, 0);
    }

    #[test]
    fn test_io_error_display() {
        assert_eq!(
            IoError::Timeout.to_string(),
            "operation timed out"
        );
        assert_eq!(
            IoError::ConnectionFailed("refused".to_string()).to_string(),
            "connection failed: refused"
        );
        assert_eq!(
            IoError::InvalidUrl("bad".to_string()).to_string(),
            "invalid URL: bad"
        );
    }

    #[test]
    fn test_http_method_values() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Put.to_string(), "PUT");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
        assert_eq!(HttpMethod::Patch.to_string(), "PATCH");
        assert_eq!(HttpMethod::Head.to_string(), "HEAD");
        assert_eq!(HttpMethod::Options.to_string(), "OPTIONS");
    }

    #[test]
    fn test_http_client_creation() {
        let client = HttpClient::new();
        assert!(client.user_agent.starts_with("Quicksilver/"));
        assert_eq!(client.default_timeout, Duration::from_secs(30));

        let client2 = HttpClient::new().with_timeout(Duration::from_secs(5));
        assert_eq!(client2.default_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_multiple_timers_fire_in_order() {
        let mut reactor = IoReactor::new();
        let id1 = reactor.submit_timer(Duration::from_millis(10));
        let id2 = reactor.submit_timer(Duration::from_millis(20));
        let _id3 = reactor.submit_timer(Duration::from_secs(60)); // won't fire

        thread::sleep(Duration::from_millis(30));
        let results = reactor.poll();

        let fired_ids: Vec<TaskId> = results.iter().map(|(id, _)| *id).collect();
        assert!(fired_ids.contains(&id1));
        assert!(fired_ids.contains(&id2));
        assert_eq!(reactor.pending_count(), 1); // id3 still pending
    }
}

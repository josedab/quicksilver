//! HTTP Server API
//!
//! Deno.serve()-compatible HTTP server with routing, SSE, and stats.
//!
//! # Example
//! ```text
//! // Basic server
//! Deno.serve({ port: 8000 }, (req) => {
//!   return new Response("Hello, World!");
//! });
//!
//! // With routing
//! const server = new HttpServer({ port: 3000 });
//! server.get("/api/users", (req) => Response.json(users));
//! server.post("/api/users", (req) => Response.json(created));
//! server.start();
//! ```

use std::collections::HashMap;

/// HTTP server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub max_connections: usize,
    pub request_timeout_ms: u64,
    pub max_request_body_bytes: usize,
    pub keep_alive: bool,
    pub tls: Option<TlsConfig>,
}

/// TLS configuration for HTTPS
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            hostname: "127.0.0.1".to_string(),
            port: 8000,
            max_connections: 1024,
            request_timeout_ms: 30000,
            max_request_body_bytes: 1024 * 1024, // 1MB
            keep_alive: true,
            tls: None,
        }
    }
}

/// HTTP server instance
#[allow(dead_code)]
pub struct HttpServer {
    config: ServerConfig,
    state: ServerState,
    stats: ServerStats,
    routes: Vec<Route>,
}

/// Server lifecycle state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerState {
    Created,
    Running,
    Stopping,
    Stopped,
}

/// Server statistics
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    pub total_requests: u64,
    pub active_connections: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors: u64,
    pub avg_response_time_us: u64,
}

/// Route definition for pattern matching
#[derive(Debug, Clone)]
pub struct Route {
    pub method: HttpMethod,
    pub pattern: String,
    pub handler_name: String,
}

/// HTTP method for server-side routing
#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Any,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Any => "*",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "DELETE" => Some(Self::Delete),
            "PATCH" => Some(Self::Patch),
            "HEAD" => Some(Self::Head),
            "OPTIONS" => Some(Self::Options),
            "*" => Some(Self::Any),
            _ => None,
        }
    }

    fn matches(&self, other: &HttpMethod) -> bool {
        *self == HttpMethod::Any || *other == HttpMethod::Any || *self == *other
    }
}

/// Server-side request representation
#[derive(Debug, Clone)]
pub struct ServerRequest {
    pub method: HttpMethod,
    pub url: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub remote_addr: String,
}

impl ServerRequest {
    /// Parse a URL into path and query parameters
    pub fn parse_url(url: &str) -> (String, HashMap<String, String>) {
        let mut query = HashMap::new();

        let (path, query_str) = match url.find('?') {
            Some(idx) => (&url[..idx], Some(&url[idx + 1..])),
            None => (url, None),
        };

        if let Some(qs) = query_str {
            for pair in qs.split('&') {
                if pair.is_empty() {
                    continue;
                }
                let (key, value) = match pair.find('=') {
                    Some(idx) => (&pair[..idx], &pair[idx + 1..]),
                    None => (pair, ""),
                };
                query.insert(key.to_string(), value.to_string());
            }
        }

        (path.to_string(), query)
    }
}

/// Server-side response builder
#[derive(Debug, Clone)]
pub struct ServerResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: ResponseBody,
}

/// Response body types
#[derive(Debug, Clone)]
pub enum ResponseBody {
    Empty,
    Text(String),
    Json(String),
    Bytes(Vec<u8>),
    Stream(Vec<Vec<u8>>),
}

impl ServerResponse {
    /// Create a new response with the given status code
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: ResponseBody::Empty,
        }
    }

    /// Create a 200 OK response with text/plain body
    pub fn text(body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        Self {
            status: 200,
            headers,
            body: ResponseBody::Text(body.to_string()),
        }
    }

    /// Create a 200 OK response with application/json body
    pub fn json(body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        Self {
            status: 200,
            headers,
            body: ResponseBody::Json(body.to_string()),
        }
    }

    /// Create a 404 Not Found response
    pub fn not_found() -> Self {
        Self {
            status: 404,
            headers: HashMap::new(),
            body: ResponseBody::Text("Not Found".to_string()),
        }
    }

    /// Create an error response with the given status and message
    pub fn error(status: u16, msg: &str) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: ResponseBody::Text(msg.to_string()),
        }
    }

    /// Add a header to the response
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_lowercase(), value.to_string());
        self
    }
}

impl HttpServer {
    /// Create a new HTTP server with the given configuration
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            state: ServerState::Created,
            stats: ServerStats::default(),
            routes: Vec::new(),
        }
    }

    /// Add a route to the server
    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
    }

    /// Get the current server state
    pub fn state(&self) -> ServerState {
        self.state
    }

    /// Start the server (transition to Running state)
    pub fn start(&mut self) -> Result<(), String> {
        match self.state {
            ServerState::Created | ServerState::Stopped => {
                self.state = ServerState::Running;
                Ok(())
            }
            ServerState::Running => Err("Server is already running".to_string()),
            ServerState::Stopping => Err("Server is currently stopping".to_string()),
        }
    }

    /// Stop the server (transition to Stopped state)
    pub fn stop(&mut self) {
        if self.state == ServerState::Running {
            self.state = ServerState::Stopping;
        }
        self.state = ServerState::Stopped;
        self.stats.active_connections = 0;
    }

    /// Handle an incoming request by matching routes
    pub fn handle_request(&mut self, request: ServerRequest) -> ServerResponse {
        if self.state != ServerState::Running {
            return ServerResponse::error(503, "Server is not running");
        }

        self.stats.total_requests += 1;
        if let Some(ref body) = request.body {
            self.stats.bytes_received += body.len() as u64;
        }

        let matched = self.match_route(&request.method, &request.path).cloned();

        let response = match matched {
            Some(route) => {
                ServerResponse::text(&format!("Handled by: {}", route.handler_name))
            }
            None => ServerResponse::not_found(),
        };

        if response.status >= 400 {
            self.stats.errors += 1;
        }

        let body_size = match &response.body {
            ResponseBody::Empty => 0,
            ResponseBody::Text(s) => s.len(),
            ResponseBody::Json(s) => s.len(),
            ResponseBody::Bytes(b) => b.len(),
            ResponseBody::Stream(chunks) => chunks.iter().map(|c| c.len()).sum(),
        };
        self.stats.bytes_sent += body_size as u64;

        response
    }

    /// Get server statistics
    pub fn stats(&self) -> &ServerStats {
        &self.stats
    }

    /// Find a matching route for the given method and path
    pub fn match_route(&self, method: &HttpMethod, path: &str) -> Option<&Route> {
        self.routes.iter().find(|route| {
            if !route.method.matches(method) {
                return false;
            }
            Self::path_matches(&route.pattern, path)
        })
    }

    /// Check if a route pattern matches a path
    fn path_matches(pattern: &str, path: &str) -> bool {
        if pattern == path {
            return true;
        }

        // Wildcard match: /api/* matches /api/anything/here
        if let Some(prefix) = pattern.strip_suffix("/*") {
            return path.starts_with(prefix);
        }

        // Parameter match: /users/:id matches /users/42
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if pattern_parts.len() != path_parts.len() {
            return false;
        }

        pattern_parts.iter().zip(path_parts.iter()).all(|(p, r)| {
            p.starts_with(':') || *p == *r
        })
    }
}

// --- Server-Sent Events ---

/// Server-Sent Event
#[derive(Debug, Clone)]
pub struct ServerSentEvent {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
    pub retry: Option<u64>,
}

impl ServerSentEvent {
    /// Create a new SSE with the given data
    pub fn new(data: &str) -> Self {
        Self {
            id: None,
            event: None,
            data: data.to_string(),
            retry: None,
        }
    }

    /// Set the event type
    pub fn with_event(mut self, event: &str) -> Self {
        self.event = Some(event.to_string());
        self
    }

    /// Set the event ID
    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.to_string());
        self
    }

    /// Format as SSE wire protocol
    pub fn format(&self) -> String {
        let mut output = String::new();

        if let Some(ref id) = self.id {
            output.push_str(&format!("id: {}\n", id));
        }
        if let Some(ref event) = self.event {
            output.push_str(&format!("event: {}\n", event));
        }
        if let Some(retry) = self.retry {
            output.push_str(&format!("retry: {}\n", retry));
        }
        for line in self.data.lines() {
            output.push_str(&format!("data: {}\n", line));
        }
        output.push('\n');

        output
    }
}

/// SSE stream for push notifications
pub struct SseStream {
    events: Vec<ServerSentEvent>,
    last_event_id: Option<String>,
}

impl SseStream {
    /// Create a new empty SSE stream
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            last_event_id: None,
        }
    }

    /// Push an event onto the stream
    pub fn push(&mut self, event: ServerSentEvent) {
        if let Some(ref id) = event.id {
            self.last_event_id = Some(id.clone());
        }
        self.events.push(event);
    }

    /// Drain all events from the stream
    pub fn drain(&mut self) -> Vec<ServerSentEvent> {
        self.events.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_defaults() {
        let config = ServerConfig::default();
        assert_eq!(config.hostname, "127.0.0.1");
        assert_eq!(config.port, 8000);
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.request_timeout_ms, 30000);
        assert_eq!(config.max_request_body_bytes, 1024 * 1024);
        assert!(config.keep_alive);
        assert!(config.tls.is_none());
    }

    #[test]
    fn test_server_lifecycle() {
        let mut server = HttpServer::new(ServerConfig::default());
        assert_eq!(server.state(), ServerState::Created);

        assert!(server.start().is_ok());
        assert_eq!(server.state(), ServerState::Running);

        // Cannot start twice
        assert!(server.start().is_err());

        server.stop();
        assert_eq!(server.state(), ServerState::Stopped);

        // Can restart after stop
        assert!(server.start().is_ok());
        assert_eq!(server.state(), ServerState::Running);
    }

    #[test]
    fn test_route_matching_exact() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Get,
            pattern: "/api/users".to_string(),
            handler_name: "list_users".to_string(),
        });

        let matched = server.match_route(&HttpMethod::Get, "/api/users");
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().handler_name, "list_users");

        // Wrong method
        assert!(server.match_route(&HttpMethod::Post, "/api/users").is_none());

        // Wrong path
        assert!(server.match_route(&HttpMethod::Get, "/api/posts").is_none());
    }

    #[test]
    fn test_route_matching_wildcard() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Get,
            pattern: "/api/*".to_string(),
            handler_name: "api_handler".to_string(),
        });

        assert!(server.match_route(&HttpMethod::Get, "/api/users").is_some());
        assert!(server.match_route(&HttpMethod::Get, "/api/users/123").is_some());
        assert!(server.match_route(&HttpMethod::Get, "/other").is_none());
    }

    #[test]
    fn test_route_matching_params() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Get,
            pattern: "/users/:id".to_string(),
            handler_name: "get_user".to_string(),
        });

        assert!(server.match_route(&HttpMethod::Get, "/users/42").is_some());
        assert!(server.match_route(&HttpMethod::Get, "/users/abc").is_some());
        assert!(server.match_route(&HttpMethod::Get, "/users").is_none());
        assert!(server.match_route(&HttpMethod::Get, "/users/42/posts").is_none());
    }

    #[test]
    fn test_route_matching_any_method() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Any,
            pattern: "/health".to_string(),
            handler_name: "health_check".to_string(),
        });

        assert!(server.match_route(&HttpMethod::Get, "/health").is_some());
        assert!(server.match_route(&HttpMethod::Post, "/health").is_some());
        assert!(server.match_route(&HttpMethod::Delete, "/health").is_some());
    }

    #[test]
    fn test_request_handling() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Get,
            pattern: "/hello".to_string(),
            handler_name: "hello_handler".to_string(),
        });
        server.start().unwrap();

        let request = ServerRequest {
            method: HttpMethod::Get,
            url: "/hello".to_string(),
            path: "/hello".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
            remote_addr: "127.0.0.1:9999".to_string(),
        };

        let response = server.handle_request(request);
        assert_eq!(response.status, 200);
        assert_eq!(server.stats().total_requests, 1);
    }

    #[test]
    fn test_request_handling_not_found() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.start().unwrap();

        let request = ServerRequest {
            method: HttpMethod::Get,
            url: "/missing".to_string(),
            path: "/missing".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
            remote_addr: "127.0.0.1:9999".to_string(),
        };

        let response = server.handle_request(request);
        assert_eq!(response.status, 404);
        assert_eq!(server.stats().errors, 1);
    }

    #[test]
    fn test_request_handling_not_running() {
        let mut server = HttpServer::new(ServerConfig::default());

        let request = ServerRequest {
            method: HttpMethod::Get,
            url: "/hello".to_string(),
            path: "/hello".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
            remote_addr: "127.0.0.1:9999".to_string(),
        };

        let response = server.handle_request(request);
        assert_eq!(response.status, 503);
    }

    #[test]
    fn test_server_stats() {
        let mut server = HttpServer::new(ServerConfig::default());
        server.add_route(Route {
            method: HttpMethod::Post,
            pattern: "/data".to_string(),
            handler_name: "data_handler".to_string(),
        });
        server.start().unwrap();

        let request = ServerRequest {
            method: HttpMethod::Post,
            url: "/data".to_string(),
            path: "/data".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Some(b"hello world".to_vec()),
            remote_addr: "127.0.0.1:9999".to_string(),
        };

        server.handle_request(request);

        let stats = server.stats();
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.bytes_received, 11);
        assert!(stats.bytes_sent > 0);
    }

    #[test]
    fn test_response_text() {
        let response = ServerResponse::text("Hello, World!");
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type"),
            Some(&"text/plain".to_string())
        );
        match &response.body {
            ResponseBody::Text(s) => assert_eq!(s, "Hello, World!"),
            _ => panic!("Expected Text body"),
        }
    }

    #[test]
    fn test_response_json() {
        let response = ServerResponse::json(r#"{"key":"value"}"#);
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        match &response.body {
            ResponseBody::Json(s) => assert_eq!(s, r#"{"key":"value"}"#),
            _ => panic!("Expected Json body"),
        }
    }

    #[test]
    fn test_response_not_found() {
        let response = ServerResponse::not_found();
        assert_eq!(response.status, 404);
    }

    #[test]
    fn test_response_error() {
        let response = ServerResponse::error(500, "Internal Server Error");
        assert_eq!(response.status, 500);
        match &response.body {
            ResponseBody::Text(s) => assert_eq!(s, "Internal Server Error"),
            _ => panic!("Expected Text body"),
        }
    }

    #[test]
    fn test_response_with_header() {
        let response = ServerResponse::text("OK")
            .with_header("X-Request-Id", "abc123")
            .with_header("Cache-Control", "no-cache");

        assert_eq!(
            response.headers.get("x-request-id"),
            Some(&"abc123".to_string())
        );
        assert_eq!(
            response.headers.get("cache-control"),
            Some(&"no-cache".to_string())
        );
    }

    #[test]
    fn test_parse_url_simple() {
        let (path, query) = ServerRequest::parse_url("/api/users");
        assert_eq!(path, "/api/users");
        assert!(query.is_empty());
    }

    #[test]
    fn test_parse_url_with_query() {
        let (path, query) = ServerRequest::parse_url("/search?q=rust&page=2");
        assert_eq!(path, "/search");
        assert_eq!(query.get("q"), Some(&"rust".to_string()));
        assert_eq!(query.get("page"), Some(&"2".to_string()));
    }

    #[test]
    fn test_parse_url_empty_query() {
        let (path, query) = ServerRequest::parse_url("/path?");
        assert_eq!(path, "/path");
        assert!(query.is_empty());
    }

    #[test]
    fn test_parse_url_key_only() {
        let (_, query) = ServerRequest::parse_url("/path?flag");
        assert_eq!(query.get("flag"), Some(&"".to_string()));
    }

    #[test]
    fn test_http_method_from_str() {
        assert_eq!(HttpMethod::from_str("GET"), Some(HttpMethod::Get));
        assert_eq!(HttpMethod::from_str("post"), Some(HttpMethod::Post));
        assert_eq!(HttpMethod::from_str("*"), Some(HttpMethod::Any));
        assert_eq!(HttpMethod::from_str("INVALID"), None);
    }

    #[test]
    fn test_http_method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Any.as_str(), "*");
    }

    // --- SSE Tests ---

    #[test]
    fn test_sse_basic() {
        let event = ServerSentEvent::new("hello");
        let formatted = event.format();
        assert_eq!(formatted, "data: hello\n\n");
    }

    #[test]
    fn test_sse_with_event_type() {
        let event = ServerSentEvent::new("payload")
            .with_event("update");
        let formatted = event.format();
        assert!(formatted.contains("event: update\n"));
        assert!(formatted.contains("data: payload\n"));
    }

    #[test]
    fn test_sse_with_id() {
        let event = ServerSentEvent::new("data")
            .with_id("42");
        let formatted = event.format();
        assert!(formatted.contains("id: 42\n"));
        assert!(formatted.contains("data: data\n"));
    }

    #[test]
    fn test_sse_full() {
        let event = ServerSentEvent {
            id: Some("1".to_string()),
            event: Some("message".to_string()),
            data: "hello world".to_string(),
            retry: Some(5000),
        };
        let formatted = event.format();
        assert!(formatted.contains("id: 1\n"));
        assert!(formatted.contains("event: message\n"));
        assert!(formatted.contains("retry: 5000\n"));
        assert!(formatted.contains("data: hello world\n"));
        assert!(formatted.ends_with("\n\n"));
    }

    #[test]
    fn test_sse_multiline_data() {
        let event = ServerSentEvent::new("line1\nline2\nline3");
        let formatted = event.format();
        assert!(formatted.contains("data: line1\n"));
        assert!(formatted.contains("data: line2\n"));
        assert!(formatted.contains("data: line3\n"));
    }

    #[test]
    fn test_sse_stream() {
        let mut stream = SseStream::new();
        assert!(stream.drain().is_empty());

        stream.push(ServerSentEvent::new("first").with_id("1"));
        stream.push(ServerSentEvent::new("second").with_id("2"));

        let events = stream.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");

        // Stream should be empty after drain
        assert!(stream.drain().is_empty());

        // last_event_id should track the latest
        assert_eq!(stream.last_event_id, Some("2".to_string()));
    }
}

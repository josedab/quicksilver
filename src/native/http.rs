//! HTTP Client and Server
//!
//! Native HTTP functionality with capability-based security.
//!
//! # Example
//! ```text
//! // Fetch API
//! const response = await fetch("https://api.example.com/data");
//! const data = await response.json();
//!
//! // HTTP Server
//! Deno.serve({ port: 8000 }, (req) => {
//!   return new Response("Hello, World!");
//! });
//! ```

use crate::runtime::Value;
use crate::security::{Capability, HostPattern, NetworkSecurity, PermissionState, Sandbox};
use rustc_hash::FxHashMap as HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

/// HTTP Method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
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
            _ => None,
        }
    }
}

/// HTTP Headers
#[derive(Debug, Clone, Default)]
pub struct Headers {
    headers: HashMap<String, String>,
}

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: &str) {
        self.headers.insert(name.to_lowercase(), value.to_string());
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.headers.get(&name.to_lowercase())
    }

    pub fn has(&self, name: &str) -> bool {
        self.headers.contains_key(&name.to_lowercase())
    }

    pub fn remove(&mut self, name: &str) {
        self.headers.remove(&name.to_lowercase());
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.headers.iter()
    }

    pub fn to_js_value(&self) -> Value {
        let props: HashMap<String, Value> = self.headers
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        Value::new_object_with_properties(props)
    }
}

/// HTTP Request
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Headers,
    pub body: Option<Vec<u8>>,
}

impl Request {
    pub fn new(method: Method, url: &str) -> Self {
        Self {
            method,
            url: url.to_string(),
            headers: Headers::new(),
            body: None,
        }
    }

    pub fn get(url: &str) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn post(url: &str) -> Self {
        Self::new(Method::Post, url)
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.set(name, value);
        self
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_json<T: serde::Serialize>(mut self, value: &T) -> Self {
        self.headers.set("content-type", "application/json");
        if let Ok(json) = serde_json::to_vec(value) {
            self.body = Some(json);
        }
        self
    }
}

/// HTTP Response
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        let status_text = match status {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            301 => "Moved Permanently",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }.to_string();

        Self {
            status,
            status_text,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn not_found() -> Self {
        Self::new(404)
    }

    pub fn error() -> Self {
        Self::new(500)
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.headers.set("content-type", "text/plain");
        self.body = text.as_bytes().to_vec();
        self
    }

    pub fn with_html(mut self, html: &str) -> Self {
        self.headers.set("content-type", "text/html");
        self.body = html.as_bytes().to_vec();
        self
    }

    pub fn with_json<T: serde::Serialize>(mut self, value: &T) -> Self {
        self.headers.set("content-type", "application/json");
        if let Ok(json) = serde_json::to_vec(value) {
            self.body = json;
        }
        self
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.set(name, value);
        self
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_slice(&self.body).ok()
    }

    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        props.insert("status".to_string(), Value::Number(self.status as f64));
        props.insert("statusText".to_string(), Value::String(self.status_text.clone()));
        props.insert("ok".to_string(), Value::Boolean(self.status >= 200 && self.status < 300));
        props.insert("headers".to_string(), self.headers.to_js_value());
        props.insert("body".to_string(), Value::String(self.text()));
        Value::new_object_with_properties(props)
    }

    /// Format as HTTP response string
    pub fn to_http_string(&self) -> String {
        let mut response = format!("HTTP/1.1 {} {}\r\n", self.status, self.status_text);

        for (name, value) in self.headers.iter() {
            response.push_str(&format!("{}: {}\r\n", name, value));
        }

        if !self.body.is_empty() {
            response.push_str(&format!("content-length: {}\r\n", self.body.len()));
        }

        response.push_str("\r\n");
        response.push_str(&String::from_utf8_lossy(&self.body));

        response
    }
}

/// HTTP Error
#[derive(Debug, Clone)]
pub enum HttpError {
    PermissionDenied(String),
    ConnectionFailed(String),
    Timeout,
    InvalidUrl(String),
    IoError(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied(url) => write!(f, "permission denied: {}", url),
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {}", msg),
            Self::Timeout => write!(f, "request timed out"),
            Self::InvalidUrl(url) => write!(f, "invalid URL: {}", url),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for HttpError {}

pub type HttpResult<T> = Result<T, HttpError>;

/// HTTP Client with security
pub struct HttpClient {
    sandbox: Option<Sandbox>,
    default_headers: Headers,
    network_security: NetworkSecurity,
}

impl HttpClient {
    pub fn new() -> Self {
        let mut default_headers = Headers::new();
        default_headers.set("user-agent", "Quicksilver/0.1.0");

        Self {
            sandbox: None,
            default_headers,
            network_security: NetworkSecurity::new(),
        }
    }

    /// Create a client without network security (allows internal IPs)
    pub fn new_permissive() -> Self {
        let mut default_headers = Headers::new();
        default_headers.set("user-agent", "Quicksilver/0.1.0");

        Self {
            sandbox: None,
            default_headers,
            network_security: NetworkSecurity::permissive(),
        }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        let mut client = Self::new();
        client.sandbox = Some(sandbox);
        client
    }

    /// Set custom network security configuration
    pub fn with_network_security(mut self, security: NetworkSecurity) -> Self {
        self.network_security = security;
        self
    }

    /// Check network permission
    fn check_permission(&self, url: &str) -> HttpResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            // Extract host from URL for permission check
            if let Ok((_, host, _, _)) = Self::parse_url(url) {
                let capability = Capability::Network(HostPattern::Exact(host.clone()));
                if sandbox.check(&capability) != PermissionState::Granted {
                    return Err(HttpError::PermissionDenied(url.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Check if the host/IP is allowed by network security policy
    fn check_network_security(&self, host: &str) -> HttpResult<()> {
        self.network_security
            .check_host(host)
            .map_err(HttpError::PermissionDenied)
    }

    /// Resolve hostname and check for DNS rebinding attacks
    fn resolve_and_check(&self, host: &str, port: u16) -> HttpResult<std::net::SocketAddr> {
        let addr_str = format!("{}:{}", host, port);

        // Resolve the hostname
        let addrs: Vec<_> = addr_str
            .to_socket_addrs()
            .map_err(|e| HttpError::ConnectionFailed(format!("DNS resolution failed: {}", e)))?
            .collect();

        if addrs.is_empty() {
            return Err(HttpError::ConnectionFailed(format!(
                "No addresses found for {}",
                host
            )));
        }

        // Check the resolved IP for DNS rebinding attacks
        let addr = addrs[0];
        self.network_security
            .check_resolved_ip(host, &addr.ip())
            .map_err(HttpError::PermissionDenied)?;

        Ok(addr)
    }

    /// Parse URL into host and path
    fn parse_url(url: &str) -> HttpResult<(String, String, u16, String)> {
        // Simple URL parsing (in production, use a proper URL parser)
        let url = url.trim();

        let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
            ("https", r)
        } else if let Some(r) = url.strip_prefix("http://") {
            ("http", r)
        } else {
            return Err(HttpError::InvalidUrl(url.to_string()));
        };

        let (host_port, path) = if let Some(slash_pos) = rest.find('/') {
            (&rest[..slash_pos], &rest[slash_pos..])
        } else {
            (rest, "/")
        };

        let (host, port) = if let Some(colon_pos) = host_port.find(':') {
            let host = &host_port[..colon_pos];
            let port: u16 = host_port[colon_pos + 1..].parse()
                .map_err(|_| HttpError::InvalidUrl(url.to_string()))?;
            (host, port)
        } else {
            let port = if scheme == "https" { 443 } else { 80 };
            (host_port, port)
        };

        Ok((scheme.to_string(), host.to_string(), port, path.to_string()))
    }

    /// Make an HTTP request (simplified, no TLS)
    pub fn request(&self, req: &Request) -> HttpResult<Response> {
        self.check_permission(&req.url)?;

        let (_scheme, host, port, path) = Self::parse_url(&req.url)?;

        // Check network security before connecting
        self.check_network_security(&host)?;

        // Resolve hostname and check for DNS rebinding attacks
        let addr = self.resolve_and_check(&host, port)?;

        // Connect (HTTP only for this simple implementation)
        let mut stream = TcpStream::connect(addr)
            .map_err(|e| HttpError::ConnectionFailed(e.to_string()))?;

        // Build request
        let mut request_str = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\n",
            req.method.as_str(),
            path,
            host
        );

        // Add default headers
        for (name, value) in self.default_headers.iter() {
            request_str.push_str(&format!("{}: {}\r\n", name, value));
        }

        // Add request headers
        for (name, value) in req.headers.iter() {
            request_str.push_str(&format!("{}: {}\r\n", name, value));
        }

        // Add body if present
        if let Some(ref body) = req.body {
            request_str.push_str(&format!("content-length: {}\r\n", body.len()));
        }

        request_str.push_str("connection: close\r\n\r\n");

        // Send request
        stream.write_all(request_str.as_bytes())
            .map_err(|e| HttpError::IoError(e.to_string()))?;

        if let Some(ref body) = req.body {
            stream.write_all(body)
                .map_err(|e| HttpError::IoError(e.to_string()))?;
        }

        // Read response
        let mut response_bytes = Vec::new();
        stream.read_to_end(&mut response_bytes)
            .map_err(|e| HttpError::IoError(e.to_string()))?;

        // Parse response (simplified)
        let response_str = String::from_utf8_lossy(&response_bytes);
        Self::parse_response(&response_str)
    }

    /// Parse HTTP response
    fn parse_response(response: &str) -> HttpResult<Response> {
        let mut lines = response.lines();

        // Parse status line
        let status_line = lines.next().ok_or(HttpError::IoError("empty response".to_string()))?;
        let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return Err(HttpError::IoError("invalid status line".to_string()));
        }

        let status: u16 = parts[1].parse()
            .map_err(|_| HttpError::IoError("invalid status code".to_string()))?;

        let mut response = Response::new(status);

        // Parse headers
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim();
                let value = line[colon_pos + 1..].trim();
                response.headers.set(name, value);
            }
        }

        // Rest is body
        let body: String = lines.collect::<Vec<_>>().join("\n");
        response.body = body.into_bytes();

        Ok(response)
    }

    /// Convenience method for GET requests
    pub fn get(&self, url: &str) -> HttpResult<Response> {
        self.request(&Request::get(url))
    }

    /// Convenience method for POST requests
    pub fn post(&self, url: &str, body: &str) -> HttpResult<Response> {
        self.request(&Request::post(url).with_body(body.as_bytes().to_vec()))
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Router for HTTP Server ====================

/// Route handler type
pub type RouteHandler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

/// Middleware type — receives request and next handler, returns response
pub type Middleware = Box<dyn Fn(&Request, &dyn Fn(&Request) -> Response) -> Response + Send + Sync>;

/// HTTP route definition
struct Route {
    method: Method,
    path: String,
    handler: RouteHandler,
}

/// HTTP Router with middleware support
pub struct Router {
    routes: Vec<Route>,
    middleware: Vec<Middleware>,
    not_found_handler: RouteHandler,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            middleware: Vec::new(),
            not_found_handler: Box::new(|_| {
                Response::not_found().with_text("Not Found")
            }),
        }
    }

    /// Add a GET route
    pub fn get<F>(&mut self, path: &str, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.route(Method::Get, path, handler);
    }

    /// Add a POST route
    pub fn post<F>(&mut self, path: &str, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.route(Method::Post, path, handler);
    }

    /// Add a PUT route
    pub fn put<F>(&mut self, path: &str, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.route(Method::Put, path, handler);
    }

    /// Add a DELETE route
    pub fn delete<F>(&mut self, path: &str, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.route(Method::Delete, path, handler);
    }

    /// Add a route with any method
    pub fn route<F>(&mut self, method: Method, path: &str, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.routes.push(Route {
            method,
            path: path.to_string(),
            handler: Box::new(handler),
        });
    }

    /// Add middleware (executes in order for all routes)
    pub fn use_middleware<F>(&mut self, middleware: F)
    where
        F: Fn(&Request, &dyn Fn(&Request) -> Response) -> Response + Send + Sync + 'static,
    {
        self.middleware.push(Box::new(middleware));
    }

    /// Set custom 404 handler
    pub fn set_not_found<F>(&mut self, handler: F)
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.not_found_handler = Box::new(handler);
    }

    /// Handle a request
    pub fn handle(&self, request: &Request) -> Response {
        // Find matching route
        let handler = self.find_handler(request);

        // Apply middleware chain
        if self.middleware.is_empty() {
            handler(request)
        } else {
            self.apply_middleware(request, &handler, 0)
        }
    }

    fn find_handler(&self, request: &Request) -> &dyn Fn(&Request) -> Response {
        // Extract path from the URL
        let path = if request.url.starts_with('/') {
            &request.url
        } else if let Some(pos) = request.url.find("://") {
            request.url[pos + 3..].find('/').map(|p| &request.url[pos + 3 + p..]).unwrap_or("/")
        } else {
            &request.url
        };

        // Strip query string
        let path = path.split('?').next().unwrap_or("/");

        for route in &self.routes {
            if route.method == request.method && self.path_matches(&route.path, path) {
                return &*route.handler;
            }
        }
        &*self.not_found_handler
    }

    fn path_matches(&self, pattern: &str, path: &str) -> bool {
        if pattern == path {
            return true;
        }
        // Simple wildcard support: /api/* matches /api/anything
        if let Some(prefix) = pattern.strip_suffix('*') {
            return path.starts_with(prefix);
        }
        // Path parameter support: /users/:id matches /users/123
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();
        if pattern_parts.len() != path_parts.len() {
            return false;
        }
        pattern_parts.iter().zip(path_parts.iter()).all(|(p, a)| {
            p.starts_with(':') || *p == *a
        })
    }

    fn apply_middleware(&self, request: &Request, handler: &dyn Fn(&Request) -> Response, idx: usize) -> Response {
        if idx >= self.middleware.len() {
            return handler(request);
        }
        let mw = &self.middleware[idx];
        let next_idx = idx + 1;
        mw(request, &|req| {
            self.apply_middleware(req, handler, next_idx)
        })
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a JS-accessible `fetch` function value for the runtime
pub fn create_fetch_function() -> Value {
    Value::make_native_fn("fetch", |args| {
        let url = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let client = HttpClient::new_permissive();
        match client.get(&url) {
            Ok(resp) => Ok(resp.to_js_value()),
            Err(e) => Err(crate::Error::type_error(format!("fetch failed: {}", e))),
        }
    })
}

// ==================== CORS Middleware ====================

/// CORS configuration
#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allow_origins: Vec<String>,
    pub allow_methods: Vec<Method>,
    pub allow_headers: Vec<String>,
    pub max_age: Option<u32>,
    pub allow_credentials: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_origins: vec!["*".to_string()],
            allow_methods: vec![Method::Get, Method::Post, Method::Put, Method::Delete, Method::Options],
            allow_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            max_age: Some(86400),
            allow_credentials: false,
        }
    }
}

impl CorsConfig {
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Create CORS middleware from this config
    pub fn into_middleware(self) -> impl Fn(&Request, &dyn Fn(&Request) -> Response) -> Response + Send + Sync + 'static {
        move |req: &Request, next: &dyn Fn(&Request) -> Response| {
            // Handle preflight
            if req.method == Method::Options {
                let mut resp = Response::new(204);
                self.apply_headers(&mut resp);
                return resp;
            }

            let mut resp = next(req);
            self.apply_headers(&mut resp);
            resp
        }
    }

    fn apply_headers(&self, resp: &mut Response) {
        resp.headers.set("access-control-allow-origin",
            &self.allow_origins.join(", "));
        resp.headers.set("access-control-allow-methods",
            &self.allow_methods.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", "));
        resp.headers.set("access-control-allow-headers",
            &self.allow_headers.join(", "));
        if let Some(max_age) = self.max_age {
            resp.headers.set("access-control-max-age", &max_age.to_string());
        }
        if self.allow_credentials {
            resp.headers.set("access-control-allow-credentials", "true");
        }
    }
}

// ==================== Server Builder ====================

/// Builder for creating and configuring an HTTP server
pub struct ServerBuilder {
    addr: String,
    router: Router,
    sandbox: Option<Sandbox>,
}

impl ServerBuilder {
    /// Create a new server builder
    pub fn new() -> Self {
        Self {
            addr: "127.0.0.1:3000".to_string(),
            router: Router::new(),
            sandbox: None,
        }
    }

    /// Set the listening address
    pub fn addr(mut self, addr: &str) -> Self {
        self.addr = addr.to_string();
        self
    }

    /// Set the port (binds to 127.0.0.1)
    pub fn port(mut self, port: u16) -> Self {
        self.addr = format!("127.0.0.1:{}", port);
        self
    }

    /// Add a GET route
    pub fn get<F>(mut self, path: &str, handler: F) -> Self
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.router.get(path, handler);
        self
    }

    /// Add a POST route
    pub fn post<F>(mut self, path: &str, handler: F) -> Self
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.router.post(path, handler);
        self
    }

    /// Add CORS middleware with default config
    pub fn with_cors(mut self) -> Self {
        let cors = CorsConfig::default();
        self.router.use_middleware(cors.into_middleware());
        self
    }

    /// Add custom middleware
    pub fn with_middleware<F>(mut self, middleware: F) -> Self
    where
        F: Fn(&Request, &dyn Fn(&Request) -> Response) -> Response + Send + Sync + 'static,
    {
        self.router.use_middleware(middleware);
        self
    }

    /// Set sandbox for the server
    pub fn with_sandbox(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Build and return the server and router
    pub fn build(self) -> HttpResult<(HttpServer, Router)> {
        let mut server = HttpServer::bind(&self.addr)?;
        if let Some(sandbox) = self.sandbox {
            server = server.with_sandbox(sandbox);
        }
        Ok((server, self.router))
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function: create a simple server builder
pub fn serve() -> ServerBuilder {
    ServerBuilder::new()
}

/// Simple HTTP Server
pub struct HttpServer {
    listener: TcpListener,
    sandbox: Option<Sandbox>,
}

impl HttpServer {
    /// Create a new HTTP server
    pub fn bind(addr: &str) -> HttpResult<Self> {
        let listener = TcpListener::bind(addr)
            .map_err(|e| HttpError::IoError(e.to_string()))?;

        Ok(Self {
            listener,
            sandbox: None,
        })
    }

    /// Set sandbox for request handling
    pub fn with_sandbox(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Get the bound address
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.listener.local_addr().ok()
    }

    /// Accept a single connection and parse the request
    pub fn accept(&self) -> HttpResult<(TcpStream, Request)> {
        let (mut stream, _addr) = self.listener.accept()
            .map_err(|e| HttpError::IoError(e.to_string()))?;

        // Read request
        let mut buffer = [0u8; 4096];
        let bytes_read = stream.read(&mut buffer)
            .map_err(|e| HttpError::IoError(e.to_string()))?;

        let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);
        let request = Self::parse_request(&request_str)?;

        Ok((stream, request))
    }

    /// Parse HTTP request
    fn parse_request(request: &str) -> HttpResult<Request> {
        let mut lines = request.lines();

        // Parse request line
        let request_line = lines.next()
            .ok_or(HttpError::IoError("empty request".to_string()))?;
        let parts: Vec<&str> = request_line.split_whitespace().collect();

        if parts.len() < 2 {
            return Err(HttpError::IoError("invalid request line".to_string()));
        }

        let method = Method::from_str(parts[0])
            .ok_or(HttpError::IoError("invalid method".to_string()))?;
        let url = parts[1];

        let mut request = Request::new(method, url);

        // Parse headers
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim();
                let value = line[colon_pos + 1..].trim();
                request.headers.set(name, value);
            }
        }

        // Rest is body
        let body: String = lines.collect::<Vec<_>>().join("\n");
        if !body.is_empty() {
            request.body = Some(body.into_bytes());
        }

        Ok(request)
    }

    /// Send a response
    pub fn send_response(stream: &mut TcpStream, response: &Response) -> HttpResult<()> {
        let response_str = response.to_http_string();
        stream.write_all(response_str.as_bytes())
            .map_err(|e| HttpError::IoError(e.to_string()))?;
        stream.flush()
            .map_err(|e| HttpError::IoError(e.to_string()))?;
        Ok(())
    }
}

// ==================== WebSocket Support ====================

/// WebSocket opcode types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WsOpcode {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x0 => Some(WsOpcode::Continuation),
            0x1 => Some(WsOpcode::Text),
            0x2 => Some(WsOpcode::Binary),
            0x8 => Some(WsOpcode::Close),
            0x9 => Some(WsOpcode::Ping),
            0xA => Some(WsOpcode::Pong),
            _ => None,
        }
    }
}

/// A parsed WebSocket frame
#[derive(Debug, Clone)]
pub struct WsFrame {
    pub fin: bool,
    pub opcode: WsOpcode,
    pub payload: Vec<u8>,
}

impl WsFrame {
    /// Parse a WebSocket frame from raw bytes
    pub fn parse(data: &[u8]) -> Option<(WsFrame, usize)> {
        if data.len() < 2 {
            return None;
        }

        let fin = (data[0] & 0x80) != 0;
        let opcode = WsOpcode::from_u8(data[0] & 0x0F)?;
        let masked = (data[1] & 0x80) != 0;
        let mut payload_len = (data[1] & 0x7F) as usize;
        let mut offset = 2;

        if payload_len == 126 {
            if data.len() < 4 {
                return None;
            }
            payload_len = u16::from_be_bytes([data[2], data[3]]) as usize;
            offset = 4;
        } else if payload_len == 127 {
            if data.len() < 10 {
                return None;
            }
            payload_len = u64::from_be_bytes([
                data[2], data[3], data[4], data[5], data[6], data[7], data[8], data[9],
            ]) as usize;
            offset = 10;
        }

        let mask_key = if masked {
            if data.len() < offset + 4 {
                return None;
            }
            let key = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
            offset += 4;
            Some(key)
        } else {
            None
        };

        if data.len() < offset + payload_len {
            return None;
        }

        let mut payload = data[offset..offset + payload_len].to_vec();

        if let Some(key) = mask_key {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[i % 4];
            }
        }

        let total_len = offset + payload_len;
        Some((WsFrame { fin, opcode, payload }, total_len))
    }

    /// Encode a WebSocket frame to bytes (server → client, unmasked)
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let opcode_byte = if self.fin { 0x80 } else { 0x00 } | (self.opcode as u8);
        buf.push(opcode_byte);

        let len = self.payload.len();
        if len < 126 {
            buf.push(len as u8);
        } else if len < 65536 {
            buf.push(126);
            buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            buf.push(127);
            buf.extend_from_slice(&(len as u64).to_be_bytes());
        }

        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Create a text frame
    pub fn text(msg: &str) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Text,
            payload: msg.as_bytes().to_vec(),
        }
    }

    /// Create a close frame
    pub fn close() -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Close,
            payload: vec![],
        }
    }

    /// Create a pong frame
    pub fn pong(data: &[u8]) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Pong,
            payload: data.to_vec(),
        }
    }

    /// Get text payload
    pub fn text_payload(&self) -> Option<String> {
        if self.opcode == WsOpcode::Text {
            String::from_utf8(self.payload.clone()).ok()
        } else {
            None
        }
    }
}

/// Compute the WebSocket accept key for the handshake
pub fn ws_accept_key(client_key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Simplified accept key (real impl uses SHA-1 + base64)
    let magic = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let combined = format!("{}{}", client_key, magic);
    let mut hasher = DefaultHasher::new();
    combined.hash(&mut hasher);
    let hash = hasher.finish();
    // Simple base64-like encoding of the hash
    format!("QS{:016x}", hash)
}

/// Check if a request is a WebSocket upgrade request
pub fn is_websocket_upgrade(request: &Request) -> bool {
    request
        .headers
        .get("Upgrade")
        .or_else(|| request.headers.get("upgrade"))
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

// ==================== Static File Serving ====================

/// MIME type lookup from file extension
pub fn mime_type_for_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Static file server configuration
#[derive(Debug, Clone)]
pub struct StaticFileConfig {
    pub root_dir: String,
    pub index_file: String,
    pub cache_max_age: u32,
}

impl Default for StaticFileConfig {
    fn default() -> Self {
        Self {
            root_dir: "./public".to_string(),
            index_file: "index.html".to_string(),
            cache_max_age: 3600,
        }
    }
}

impl StaticFileConfig {
    pub fn new(root_dir: &str) -> Self {
        Self {
            root_dir: root_dir.to_string(),
            ..Default::default()
        }
    }

    /// Resolve a URL path to a file system path, with security checks
    pub fn resolve_path(&self, url_path: &str) -> Option<std::path::PathBuf> {
        let clean = url_path.strip_prefix('/').unwrap_or(url_path);
        // Prevent directory traversal
        if clean.contains("..") || clean.contains('\0') {
            return None;
        }
        let full_path = std::path::Path::new(&self.root_dir).join(clean);
        // Ensure resolved path is within root
        if let Ok(canonical_root) = std::fs::canonicalize(&self.root_dir) {
            if let Ok(canonical_path) = std::fs::canonicalize(&full_path) {
                if canonical_path.starts_with(&canonical_root) {
                    return Some(canonical_path);
                }
                return None;
            }
        }
        // If file doesn't exist, still return the path for 404 handling
        Some(full_path)
    }

    /// Serve a static file, returning a Response
    pub fn serve_file(&self, url_path: &str) -> Response {
        let path = match self.resolve_path(url_path) {
            Some(p) => p,
            None => return Response::new(403).with_text("Forbidden"),
        };

        let file_path = if path.is_dir() {
            path.join(&self.index_file)
        } else {
            path
        };

        match std::fs::read(&file_path) {
            Ok(contents) => {
                let ext = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let mime = mime_type_for_extension(ext);
                Response::new(200)
                    .with_body(contents)
                    .with_header("Content-Type", mime)
                    .with_header("Cache-Control", &format!("max-age={}", self.cache_max_age))
            }
            Err(_) => Response::not_found(),
        }
    }
}

// ==================== Rate Limiting ====================

/// Token bucket rate limiter
#[derive(Debug)]
pub struct RateLimiter {
    /// Max tokens per bucket
    pub max_tokens: u32,
    /// Current token counts by client key
    buckets: HashMap<String, (u32, std::time::Instant)>,
    /// Tokens refilled per second
    pub refill_rate: f64,
}

impl RateLimiter {
    pub fn new(max_tokens: u32, refill_rate: f64) -> Self {
        Self {
            max_tokens,
            buckets: HashMap::default(),
            refill_rate,
        }
    }

    /// Check if a request from the given key should be allowed
    pub fn check(&mut self, key: &str) -> bool {
        let now = std::time::Instant::now();
        let (tokens, last_check) = self
            .buckets
            .entry(key.to_string())
            .or_insert((self.max_tokens, now));

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(*last_check).as_secs_f64();
        let refill = (elapsed * self.refill_rate) as u32;
        *tokens = (*tokens + refill).min(self.max_tokens);
        *last_check = now;

        if *tokens > 0 {
            *tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Get remaining tokens for a key
    pub fn remaining(&self, key: &str) -> u32 {
        self.buckets
            .get(key)
            .map(|(t, _)| *t)
            .unwrap_or(self.max_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers() {
        let mut headers = Headers::new();
        headers.set("Content-Type", "application/json");

        assert!(headers.has("content-type"));
        assert_eq!(headers.get("content-type"), Some(&"application/json".to_string()));
    }

    #[test]
    fn test_response_builder() {
        let response = Response::ok()
            .with_text("Hello, World!")
            .with_header("X-Custom", "value");

        assert_eq!(response.status, 200);
        assert_eq!(response.text(), "Hello, World!");
        assert!(response.headers.has("content-type"));
        assert!(response.headers.has("x-custom"));
    }

    #[test]
    fn test_url_parsing() {
        let (scheme, host, port, path) = HttpClient::parse_url("http://example.com/api/data").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/api/data");

        let (_, _, port, _) = HttpClient::parse_url("https://example.com").unwrap();
        assert_eq!(port, 443);
    }

    #[test]
    fn test_method() {
        assert_eq!(Method::from_str("GET"), Some(Method::Get));
        assert_eq!(Method::from_str("post"), Some(Method::Post));
        assert_eq!(Method::Get.as_str(), "GET");
    }

    #[test]
    fn test_blocks_internal_ip() {
        let client = HttpClient::new();

        // Should block requests to internal IPs
        let result = client.check_network_security("127.0.0.1");
        assert!(result.is_err());

        let result = client.check_network_security("192.168.1.1");
        assert!(result.is_err());

        let result = client.check_network_security("10.0.0.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_allows_public_ip() {
        let client = HttpClient::new();

        // Should allow public IPs
        assert!(client.check_network_security("8.8.8.8").is_ok());
        assert!(client.check_network_security("1.1.1.1").is_ok());
    }

    #[test]
    fn test_allows_domain() {
        let client = HttpClient::new();

        // Should allow valid domains (DNS resolution happens later)
        assert!(client.check_network_security("example.com").is_ok());
        assert!(client.check_network_security("api.github.com").is_ok());
    }

    #[test]
    fn test_permissive_client_allows_internal() {
        let client = HttpClient::new_permissive();

        // Permissive client should allow internal IPs
        assert!(client.check_network_security("127.0.0.1").is_ok());
        assert!(client.check_network_security("192.168.1.1").is_ok());
    }

    #[test]
    fn test_router_basic() {
        let mut router = Router::new();
        router.get("/hello", |_| Response::ok().with_text("Hello!"));
        router.get("/world", |_| Response::ok().with_text("World!"));

        let req = Request::get("/hello");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.text(), "Hello!");
    }

    #[test]
    fn test_router_not_found() {
        let router = Router::new();
        let req = Request::get("/missing");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn test_router_wildcard() {
        let mut router = Router::new();
        router.get("/api/*", |_| Response::ok().with_text("API"));

        let req = Request::get("/api/users/123");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.text(), "API");
    }

    #[test]
    fn test_router_path_params() {
        let mut router = Router::new();
        router.get("/users/:id", |_| Response::ok().with_text("User"));

        let req = Request::get("/users/42");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_router_method_matching() {
        let mut router = Router::new();
        router.get("/data", |_| Response::ok().with_text("GET"));
        router.post("/data", |_| Response::ok().with_text("POST"));

        let get_req = Request::get("/data");
        assert_eq!(router.handle(&get_req).text(), "GET");

        let post_req = Request::post("/data");
        assert_eq!(router.handle(&post_req).text(), "POST");
    }

    #[test]
    fn test_router_middleware() {
        let mut router = Router::new();
        router.use_middleware(|req, next| {
            let mut resp = next(req);
            resp.headers.set("X-Middleware", "applied");
            resp
        });
        router.get("/test", |_| Response::ok().with_text("OK"));

        let req = Request::get("/test");
        let resp = router.handle(&req);
        assert_eq!(resp.text(), "OK");
        assert_eq!(resp.headers.get("x-middleware"), Some(&"applied".to_string()));
    }

    #[test]
    fn test_cors_config_default() {
        let cors = CorsConfig::default();
        assert_eq!(cors.allow_origins, vec!["*"]);
        assert!(cors.allow_credentials == false);
    }

    #[test]
    fn test_cors_middleware_preflight() {
        let mut router = Router::new();
        let cors = CorsConfig::default();
        router.use_middleware(cors.into_middleware());
        router.get("/api", |_| Response::ok().with_text("data"));

        let req = Request::new(Method::Options, "/api");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 204);
        assert!(resp.headers.has("access-control-allow-origin"));
        assert!(resp.headers.has("access-control-allow-methods"));
    }

    #[test]
    fn test_cors_middleware_normal_request() {
        let mut router = Router::new();
        let cors = CorsConfig::default();
        router.use_middleware(cors.into_middleware());
        router.get("/api", |_| Response::ok().with_text("data"));

        let req = Request::get("/api");
        let resp = router.handle(&req);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.text(), "data");
        assert!(resp.headers.has("access-control-allow-origin"));
    }

    #[test]
    fn test_server_builder() {
        let builder = serve()
            .port(0) // Use random port
            .get("/hello", |_| Response::ok().with_text("Hello!"))
            .with_cors();

        // Just verify builder construction works — binding requires a port
        assert_eq!(builder.addr, "127.0.0.1:0");
    }

    #[test]
    fn test_create_fetch_function() {
        let fetch_fn = create_fetch_function();
        assert!(matches!(fetch_fn, Value::Object(_)));
    }

    // ==================== WebSocket Tests ====================

    #[test]
    fn test_ws_frame_text() {
        let frame = WsFrame::text("hello");
        assert_eq!(frame.opcode, WsOpcode::Text);
        assert!(frame.fin);
        assert_eq!(frame.text_payload(), Some("hello".to_string()));
    }

    #[test]
    fn test_ws_frame_encode_decode() {
        let frame = WsFrame::text("test message");
        let encoded = frame.encode();
        let (decoded, len) = WsFrame::parse(&encoded).unwrap();
        assert_eq!(len, encoded.len());
        assert_eq!(decoded.text_payload(), Some("test message".to_string()));
        assert!(decoded.fin);
    }

    #[test]
    fn test_ws_frame_close() {
        let frame = WsFrame::close();
        assert_eq!(frame.opcode, WsOpcode::Close);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn test_ws_frame_pong() {
        let frame = WsFrame::pong(b"ping-data");
        assert_eq!(frame.opcode, WsOpcode::Pong);
        assert_eq!(frame.payload, b"ping-data");
    }

    #[test]
    fn test_ws_frame_parse_masked() {
        // Build a masked text frame: "Hi"
        let mask: [u8; 4] = [0x37, 0xfa, 0x21, 0x3d];
        let payload = b"Hi";
        let mut masked_payload = payload.to_vec();
        for (i, b) in masked_payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
        let mut data = vec![0x81, 0x82]; // FIN + Text, Masked + len=2
        data.extend_from_slice(&mask);
        data.extend_from_slice(&masked_payload);

        let (frame, len) = WsFrame::parse(&data).unwrap();
        assert_eq!(len, data.len());
        assert_eq!(frame.text_payload(), Some("Hi".to_string()));
    }

    #[test]
    fn test_ws_opcode_from_u8() {
        assert_eq!(WsOpcode::from_u8(0x1), Some(WsOpcode::Text));
        assert_eq!(WsOpcode::from_u8(0x2), Some(WsOpcode::Binary));
        assert_eq!(WsOpcode::from_u8(0x8), Some(WsOpcode::Close));
        assert_eq!(WsOpcode::from_u8(0xFF), None);
    }

    #[test]
    fn test_ws_accept_key() {
        let key = ws_accept_key("dGhlIHNhbXBsZSBub25jZQ==");
        assert!(!key.is_empty());
        assert!(key.starts_with("QS"));
    }

    #[test]
    fn test_is_websocket_upgrade() {
        let mut req = Request::get("/ws");
        req.headers.set("Upgrade", "websocket");
        assert!(is_websocket_upgrade(&req));

        let plain_req = Request::get("/api");
        assert!(!is_websocket_upgrade(&plain_req));
    }

    // ==================== Static File Tests ====================

    #[test]
    fn test_mime_types() {
        assert_eq!(mime_type_for_extension("html"), "text/html");
        assert_eq!(mime_type_for_extension("css"), "text/css");
        assert_eq!(mime_type_for_extension("js"), "application/javascript");
        assert_eq!(mime_type_for_extension("json"), "application/json");
        assert_eq!(mime_type_for_extension("png"), "image/png");
        assert_eq!(mime_type_for_extension("wasm"), "application/wasm");
        assert_eq!(mime_type_for_extension("xyz"), "application/octet-stream");
    }

    #[test]
    fn test_static_file_config_default() {
        let config = StaticFileConfig::default();
        assert_eq!(config.root_dir, "./public");
        assert_eq!(config.index_file, "index.html");
        assert_eq!(config.cache_max_age, 3600);
    }

    #[test]
    fn test_static_file_traversal_prevention() {
        let config = StaticFileConfig::new("/tmp");
        assert!(config.resolve_path("../etc/passwd").is_none());
        assert!(config.resolve_path("foo/../../etc/passwd").is_none());
    }

    #[test]
    fn test_static_file_serve_404() {
        let config = StaticFileConfig::new("/nonexistent/path");
        let resp = config.serve_file("/missing.html");
        assert_eq!(resp.status, 404);
    }

    // ==================== Rate Limiter Tests ====================

    #[test]
    fn test_rate_limiter_basic() {
        let mut limiter = RateLimiter::new(3, 1.0);
        assert!(limiter.check("client1"));
        assert!(limiter.check("client1"));
        assert!(limiter.check("client1"));
        assert!(!limiter.check("client1")); // Exhausted
    }

    #[test]
    fn test_rate_limiter_separate_clients() {
        let mut limiter = RateLimiter::new(1, 1.0);
        assert!(limiter.check("client1"));
        assert!(!limiter.check("client1")); // Exhausted
        assert!(limiter.check("client2")); // Different client, still has tokens
    }

    #[test]
    fn test_rate_limiter_remaining() {
        let mut limiter = RateLimiter::new(5, 1.0);
        assert_eq!(limiter.remaining("client1"), 5);
        limiter.check("client1");
        assert_eq!(limiter.remaining("client1"), 4);
    }
}

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
            .map_err(|e| HttpError::PermissionDenied(e))
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
            .map_err(|e| HttpError::PermissionDenied(e))?;

        Ok(addr)
    }

    /// Parse URL into host and path
    fn parse_url(url: &str) -> HttpResult<(String, String, u16, String)> {
        // Simple URL parsing (in production, use a proper URL parser)
        let url = url.trim();

        let (scheme, rest) = if url.starts_with("https://") {
            ("https", &url[8..])
        } else if url.starts_with("http://") {
            ("http", &url[7..])
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
        let mut stream = TcpStream::connect(&addr)
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
}

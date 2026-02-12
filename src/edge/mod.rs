//! Edge Function Runtime
//!
//! Cloudflare Workers-compatible API surface for running JavaScript
//! at the edge with low latency and high throughput.
//!
//! # Example
//! ```text
//! // Handle incoming fetch events
//! addEventListener("fetch", (event) => {
//!   event.respondWith(handleRequest(event.request));
//! });
//!
//! async function handleRequest(request) {
//!   const url = new URL(request.url);
//!   if (url.pathname === "/api/data") {
//!     const cached = await caches.default.match(request);
//!     if (cached) return cached;
//!     return new Response(JSON.stringify({ hello: "world" }), {
//!       headers: { "Content-Type": "application/json" }
//!     });
//!   }
//!   return new Response("Not Found", { status: 404 });
//! }
//! ```

//! **Status:** ⚠️ Partial — Cloudflare Workers-compatible API surface

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::time::{Duration, Instant};

/// HTTP methods supported by the edge runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl HttpMethod {
    /// Parse an HTTP method from a string
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            "PUT" => Ok(HttpMethod::Put),
            "DELETE" => Ok(HttpMethod::Delete),
            "PATCH" => Ok(HttpMethod::Patch),
            "HEAD" => Ok(HttpMethod::Head),
            "OPTIONS" => Ok(HttpMethod::Options),
            _ => Err(Error::type_error(format!("Unknown HTTP method: {}", s))),
        }
    }

    /// Return the method as an uppercase string
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }
}

/// HTTP headers collection, case-insensitive by key
#[derive(Debug, Clone)]
pub struct Headers {
    entries: HashMap<String, String>,
}

impl Headers {
    /// Create an empty headers collection
    pub fn new() -> Self {
        Self {
            entries: HashMap::default(),
        }
    }

    /// Set a header value, replacing any existing value
    pub fn set(&mut self, name: &str, value: &str) {
        self.entries
            .insert(name.to_lowercase(), value.to_string());
    }

    /// Get a header value by name
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    /// Check whether a header exists
    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_lowercase())
    }

    /// Delete a header by name
    pub fn delete(&mut self, name: &str) {
        self.entries.remove(&name.to_lowercase());
    }

    /// Return the number of headers
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check whether the headers collection is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all header entries
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Convert headers to a runtime Value
    pub fn to_value(&self) -> Value {
        let mut repr = Vec::new();
        for (k, v) in &self.entries {
            repr.push(format!("{}: {}", k, v));
        }
        Value::String(repr.join(", "))
    }
}

impl Default for Headers {
    fn default() -> Self {
        Self::new()
    }
}

/// Workers-compatible Request object
#[derive(Debug, Clone)]
pub struct Request {
    /// The request URL
    pub url: String,
    /// The HTTP method
    pub method: HttpMethod,
    /// Request headers
    pub headers: Headers,
    /// Request body (if any)
    pub body: Option<String>,
}

impl Request {
    /// Create a new GET request to the given URL
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            method: HttpMethod::Get,
            headers: Headers::new(),
            body: None,
        }
    }

    /// Create a request with a specific method and optional body
    pub fn with_method(url: &str, method: HttpMethod, body: Option<String>) -> Self {
        Self {
            url: url.to_string(),
            method,
            headers: Headers::new(),
            body,
        }
    }

    /// Convert the request to a runtime Value
    pub fn to_value(&self) -> Value {
        Value::String(format!(
            "Request {{ method: {}, url: {} }}",
            self.method.as_str(),
            self.url
        ))
    }

    /// Parse the URL into its components (WinterCG URL API)
    pub fn parse_url(&self) -> UrlInfo {
        UrlInfo::parse(&self.url)
    }
}

/// Parsed URL components (WinterCG-compatible)
#[derive(Debug, Clone)]
pub struct UrlInfo {
    /// Full URL string
    pub href: String,
    /// Protocol (e.g., "https:")
    pub protocol: String,
    /// Hostname without port
    pub hostname: String,
    /// Port string (empty if default)
    pub port: String,
    /// Path component
    pub pathname: String,
    /// Query string including '?'
    pub search: String,
    /// Parsed query parameters
    pub search_params: HashMap<String, String>,
    /// Hash/fragment
    pub hash: String,
}

impl UrlInfo {
    /// Parse a URL string into components
    pub fn parse(url: &str) -> Self {
        let mut protocol = String::new();
        let hostname;
        let mut port = String::new();
        let pathname;
        let mut search = String::new();
        let mut hash = String::new();
        let mut search_params = HashMap::default();

        let mut remaining = url;

        // Extract protocol
        if let Some(idx) = remaining.find("://") {
            protocol = format!("{}:", &remaining[..idx]);
            remaining = &remaining[idx + 3..];
        }

        // Extract hash
        if let Some(idx) = remaining.find('#') {
            hash = remaining[idx..].to_string();
            remaining = &remaining[..idx];
        }

        // Extract query string
        if let Some(idx) = remaining.find('?') {
            search = remaining[idx..].to_string();
            let query_str = &remaining[idx + 1..];
            for param in query_str.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    search_params.insert(key.to_string(), value.to_string());
                }
            }
            remaining = &remaining[..idx];
        }

        // Extract hostname and port from path
        if let Some(idx) = remaining.find('/') {
            pathname = remaining[idx..].to_string();
            remaining = &remaining[..idx];
        } else {
            pathname = "/".to_string();
        }

        // Extract hostname:port
        if let Some(idx) = remaining.find(':') {
            hostname = remaining[..idx].to_string();
            port = remaining[idx + 1..].to_string();
        } else {
            hostname = remaining.to_string();
        }

        Self {
            href: url.to_string(),
            protocol,
            hostname,
            port,
            pathname,
            search,
            search_params,
            hash,
        }
    }
}

/// Workers-compatible Response object
#[derive(Debug, Clone)]
pub struct Response {
    /// HTTP status code
    pub status: u16,
    /// HTTP status text
    pub status_text: String,
    /// Response headers
    pub headers: Headers,
    /// Response body
    pub body: String,
}

impl Response {
    /// Create a new 200 OK response with the given body
    pub fn new(body: &str) -> Self {
        Self {
            status: 200,
            status_text: "OK".to_string(),
            headers: Headers::new(),
            body: body.to_string(),
        }
    }

    /// Create a response with a specific status code
    pub fn with_status(body: &str, status: u16) -> Self {
        let status_text = default_status_text(status);
        Self {
            status,
            status_text: status_text.to_string(),
            headers: Headers::new(),
            body: body.to_string(),
        }
    }

    /// Create a JSON response with appropriate Content-Type
    pub fn json(data: &str) -> Self {
        let mut headers = Headers::new();
        headers.set("content-type", "application/json");
        Self {
            status: 200,
            status_text: "OK".to_string(),
            headers,
            body: data.to_string(),
        }
    }

    /// Create a redirect response
    pub fn redirect(url: &str, status: u16) -> Self {
        let mut headers = Headers::new();
        headers.set("location", url);
        let status_text = default_status_text(status);
        Self {
            status,
            status_text: status_text.to_string(),
            headers,
            body: String::new(),
        }
    }

    /// Convert the response to a runtime Value
    pub fn to_value(&self) -> Value {
        Value::String(format!(
            "Response {{ status: {}, body length: {} }}",
            self.status,
            self.body.len()
        ))
    }

    /// Create an HTML response with appropriate Content-Type
    pub fn html(body: &str) -> Self {
        let mut headers = Headers::new();
        headers.set("content-type", "text/html; charset=utf-8");
        Self {
            status: 200,
            status_text: "OK".to_string(),
            headers,
            body: body.to_string(),
        }
    }

    /// Create a plain text response with appropriate Content-Type
    pub fn text(body: &str) -> Self {
        let mut headers = Headers::new();
        headers.set("content-type", "text/plain; charset=utf-8");
        Self {
            status: 200,
            status_text: "OK".to_string(),
            headers,
            body: body.to_string(),
        }
    }

    /// Add CORS headers to the response (returns a new Response)
    pub fn with_cors(mut self, origin: &str) -> Self {
        self.headers.set("access-control-allow-origin", origin);
        self.headers.set("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS");
        self.headers.set("access-control-allow-headers", "Content-Type, Authorization");
        self.headers.set("access-control-max-age", "86400");
        self
    }
}

/// Map common HTTP status codes to their default text
fn default_status_text(status: u16) -> &'static str {
    match status {
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
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Fetch event representing an incoming HTTP request
#[derive(Debug, Clone)]
pub struct FetchEvent {
    /// The incoming request
    pub request: Request,
    /// The response (set via `respond_with`)
    response: Option<Response>,
}

impl FetchEvent {
    /// Create a new fetch event for the given request
    pub fn new(request: Request) -> Self {
        Self {
            request,
            response: None,
        }
    }

    /// Set the response for this event
    pub fn respond_with(&mut self, response: Response) {
        self.response = Some(response);
    }

    /// Consume the event and return the response, or a 500 error if none was set
    pub fn into_response(self) -> Response {
        self.response.unwrap_or_else(|| {
            Response::with_status("Internal Server Error", 500)
        })
    }
}

/// Entry in the in-memory edge cache
#[derive(Debug, Clone)]
struct CacheEntry {
    response: Response,
    expires_at: Option<Instant>,
}

/// Simple in-memory cache for edge functions (Cache API)
#[derive(Debug, Clone)]
pub struct Cache {
    entries: HashMap<String, CacheEntry>,
    /// Maximum number of cached entries
    max_entries: usize,
}

impl Cache {
    /// Create a new cache with the given capacity
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::default(),
            max_entries,
        }
    }

    /// Store a response in the cache, keyed by the request URL
    pub fn put(&mut self, request: &Request, response: Response, ttl: Option<Duration>) {
        // Evict expired entries before inserting
        self.evict_expired();

        if self.entries.len() >= self.max_entries {
            // Remove the first key found (simple eviction)
            if let Some(key) = self.entries.keys().next().cloned() {
                self.entries.remove(&key);
            }
        }

        let expires_at = ttl.map(|d| Instant::now() + d);
        self.entries.insert(
            request.url.clone(),
            CacheEntry {
                response,
                expires_at,
            },
        );
    }

    /// Match a cached response for the given request
    pub fn match_request(&self, request: &Request) -> Option<&Response> {
        self.entries.get(&request.url).and_then(|entry| {
            if let Some(exp) = entry.expires_at {
                if Instant::now() > exp {
                    return None;
                }
            }
            Some(&entry.response)
        })
    }

    /// Delete a cached entry by request URL
    pub fn delete(&mut self, request: &Request) -> bool {
        self.entries.remove(&request.url).is_some()
    }

    /// Return the number of currently cached entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check whether the cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all expired entries
    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.entries
            .retain(|_, entry| entry.expires_at.is_none_or(|exp| now <= exp));
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Key-value store interface (Workers KV)
#[derive(Debug, Clone)]
pub struct KVNamespace {
    name: String,
    store: HashMap<String, KVEntry>,
}

/// An entry in the KV store
#[derive(Debug, Clone)]
struct KVEntry {
    value: String,
    metadata: Option<String>,
    expires_at: Option<Instant>,
}

impl KVNamespace {
    /// Create a new KV namespace with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            store: HashMap::default(),
        }
    }

    /// Get the namespace name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a value by key, returning None if expired or missing
    pub fn get(&self, key: &str) -> Option<&str> {
        self.store.get(key).and_then(|entry| {
            if let Some(exp) = entry.expires_at {
                if Instant::now() > exp {
                    return None;
                }
            }
            Some(entry.value.as_str())
        })
    }

    /// Get a value with its metadata
    pub fn get_with_metadata(&self, key: &str) -> Option<(&str, Option<&str>)> {
        self.store.get(key).and_then(|entry| {
            if let Some(exp) = entry.expires_at {
                if Instant::now() > exp {
                    return None;
                }
            }
            Some((entry.value.as_str(), entry.metadata.as_deref()))
        })
    }

    /// Put a value into the store
    pub fn put(&mut self, key: &str, value: &str) {
        self.store.insert(
            key.to_string(),
            KVEntry {
                value: value.to_string(),
                metadata: None,
                expires_at: None,
            },
        );
    }

    /// Put a value with a TTL
    pub fn put_with_ttl(&mut self, key: &str, value: &str, ttl: Duration) {
        self.store.insert(
            key.to_string(),
            KVEntry {
                value: value.to_string(),
                metadata: None,
                expires_at: Some(Instant::now() + ttl),
            },
        );
    }

    /// Put a value with metadata
    pub fn put_with_metadata(&mut self, key: &str, value: &str, metadata: &str) {
        self.store.insert(
            key.to_string(),
            KVEntry {
                value: value.to_string(),
                metadata: Some(metadata.to_string()),
                expires_at: None,
            },
        );
    }

    /// Delete a key from the store
    pub fn delete(&mut self, key: &str) -> bool {
        self.store.remove(key).is_some()
    }

    /// List keys in the namespace, optionally filtered by prefix
    pub fn list(&self, prefix: Option<&str>) -> Vec<&str> {
        self.store
            .keys()
            .filter(|k| prefix.is_none_or(|p| k.starts_with(p)))
            .map(|k| k.as_str())
            .collect()
    }

    /// Convert a KV value to a runtime Value
    pub fn to_value(&self, key: &str) -> Value {
        match self.get(key) {
            Some(v) => Value::String(v.to_string()),
            None => Value::Undefined,
        }
    }
}

/// Configuration for the edge runtime
#[derive(Debug, Clone)]
pub struct EdgeConfig {
    /// Maximum request body size in bytes
    pub max_body_size: usize,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum number of cache entries
    pub max_cache_entries: usize,
    /// Enable cache
    pub cache_enabled: bool,
}

impl EdgeConfig {
    /// Create a new configuration with sensible defaults
    pub fn new() -> Self {
        Self {
            max_body_size: 1_048_576, // 1 MB
            timeout: Duration::from_secs(30),
            max_cache_entries: 1024,
            cache_enabled: true,
        }
    }

    /// Set the maximum request body size
    pub fn with_max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = size;
        self
    }

    /// Set the request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum number of cache entries
    pub fn with_max_cache_entries(mut self, max: usize) -> Self {
        self.max_cache_entries = max;
        self
    }
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Fetch event handler function type
pub type FetchHandler = Box<dyn Fn(&mut FetchEvent) -> Result<()>>;

/// Edge Function Runtime
///
/// Manages incoming requests, caching, and KV namespaces for
/// Cloudflare Workers-compatible edge function execution.
pub struct EdgeRuntime {
    config: EdgeConfig,
    cache: Cache,
    kv_namespaces: HashMap<String, KVNamespace>,
    handler: Option<FetchHandler>,
    /// Total number of requests handled
    pub requests_handled: u64,
}

impl std::fmt::Debug for EdgeRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeRuntime")
            .field("config", &self.config)
            .field("cache_size", &self.cache.len())
            .field("kv_namespaces", &self.kv_namespaces.keys().collect::<Vec<_>>())
            .field("requests_handled", &self.requests_handled)
            .finish()
    }
}

impl EdgeRuntime {
    /// Create a new edge runtime with the given configuration
    pub fn new(config: EdgeConfig) -> Self {
        let cache = Cache::new(config.max_cache_entries);
        Self {
            config,
            cache,
            kv_namespaces: HashMap::default(),
            handler: None,
            requests_handled: 0,
        }
    }

    /// Register a fetch event handler
    pub fn on_fetch(&mut self, handler: impl Fn(&mut FetchEvent) -> Result<()> + 'static) {
        self.handler = Some(Box::new(handler));
    }

    /// Handle an incoming request, returning a response
    pub fn handle_request(&mut self, request: Request) -> Result<Response> {
        // Check body size limit
        if let Some(ref body) = request.body {
            if body.len() > self.config.max_body_size {
                return Ok(Response::with_status("Payload Too Large", 413));
            }
        }

        // Check cache first
        if self.config.cache_enabled && request.method == HttpMethod::Get {
            if let Some(cached) = self.cache.match_request(&request) {
                self.requests_handled += 1;
                return Ok(cached.clone());
            }
        }

        let handler = self.handler.as_ref().ok_or_else(|| {
            Error::type_error("No fetch handler registered".to_string())
        })?;

        let mut event = FetchEvent::new(request);
        handler(&mut event)?;
        self.requests_handled += 1;

        Ok(event.into_response())
    }

    /// Get a mutable reference to the cache
    pub fn cache_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }

    /// Get a reference to the cache
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Bind a KV namespace to this runtime
    pub fn bind_kv(&mut self, namespace: KVNamespace) {
        self.kv_namespaces
            .insert(namespace.name().to_string(), namespace);
    }

    /// Get a reference to a KV namespace by name
    pub fn kv(&self, name: &str) -> Option<&KVNamespace> {
        self.kv_namespaces.get(name)
    }

    /// Get a mutable reference to a KV namespace by name
    pub fn kv_mut(&mut self, name: &str) -> Option<&mut KVNamespace> {
        self.kv_namespaces.get_mut(name)
    }

    /// Get the runtime configuration
    pub fn config(&self) -> &EdgeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_parsing() {
        assert_eq!(HttpMethod::from_str("GET").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::from_str("post").unwrap(), HttpMethod::Post);
        assert_eq!(HttpMethod::from_str("Delete").unwrap(), HttpMethod::Delete);
        assert!(HttpMethod::from_str("INVALID").is_err());
    }

    #[test]
    fn test_headers_case_insensitive() {
        let mut headers = Headers::new();
        headers.set("Content-Type", "text/html");
        assert_eq!(headers.get("content-type"), Some("text/html"));
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/html"));
        assert!(headers.has("Content-Type"));
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn test_headers_delete() {
        let mut headers = Headers::new();
        headers.set("X-Custom", "value");
        assert!(headers.has("x-custom"));
        headers.delete("X-Custom");
        assert!(!headers.has("x-custom"));
        assert!(headers.is_empty());
    }

    #[test]
    fn test_request_creation() {
        let req = Request::new("https://example.com/api");
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.url, "https://example.com/api");
        assert!(req.body.is_none());

        let req = Request::with_method(
            "https://example.com/api",
            HttpMethod::Post,
            Some(r#"{"key":"value"}"#.to_string()),
        );
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.body.as_deref(), Some(r#"{"key":"value"}"#));
    }

    #[test]
    fn test_response_constructors() {
        let resp = Response::new("Hello");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "Hello");

        let resp = Response::with_status("Not Found", 404);
        assert_eq!(resp.status, 404);
        assert_eq!(resp.status_text, "Not Found");

        let resp = Response::json(r#"{"ok":true}"#);
        assert_eq!(resp.headers.get("content-type"), Some("application/json"));

        let resp = Response::redirect("https://example.com", 301);
        assert_eq!(resp.status, 301);
        assert_eq!(resp.headers.get("location"), Some("https://example.com"));
    }

    #[test]
    fn test_fetch_event_lifecycle() {
        let req = Request::new("https://example.com");
        let mut event = FetchEvent::new(req);
        assert!(event.response.is_none());

        event.respond_with(Response::new("OK"));
        let resp = event.into_response();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "OK");
    }

    #[test]
    fn test_cache_put_and_match() {
        let mut cache = Cache::new(10);
        let req = Request::new("https://example.com/data");
        let resp = Response::json(r#"{"cached":true}"#);

        cache.put(&req, resp, None);
        assert_eq!(cache.len(), 1);

        let matched = cache.match_request(&req);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().body, r#"{"cached":true}"#);

        assert!(cache.delete(&req));
        assert!(cache.is_empty());
    }

    #[test]
    fn test_kv_namespace_operations() {
        let mut kv = KVNamespace::new("MY_KV");
        assert_eq!(kv.name(), "MY_KV");

        kv.put("key1", "value1");
        kv.put_with_metadata("key2", "value2", r#"{"version":1}"#);
        kv.put("prefix:a", "a_val");
        kv.put("prefix:b", "b_val");

        assert_eq!(kv.get("key1"), Some("value1"));
        let (val, meta) = kv.get_with_metadata("key2").unwrap();
        assert_eq!(val, "value2");
        assert_eq!(meta, Some(r#"{"version":1}"#));

        let prefixed = kv.list(Some("prefix:"));
        assert_eq!(prefixed.len(), 2);

        assert!(kv.delete("key1"));
        assert!(kv.get("key1").is_none());
    }

    #[test]
    fn test_edge_runtime_handle_request() {
        let config = EdgeConfig::new();
        let mut runtime = EdgeRuntime::new(config);

        runtime.on_fetch(|event| {
            let path = &event.request.url;
            if path.ends_with("/hello") {
                event.respond_with(Response::new("Hello, World!"));
            } else {
                event.respond_with(Response::with_status("Not Found", 404));
            }
            Ok(())
        });

        let resp = runtime
            .handle_request(Request::new("https://edge.example.com/hello"))
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "Hello, World!");

        let resp = runtime
            .handle_request(Request::new("https://edge.example.com/missing"))
            .unwrap();
        assert_eq!(resp.status, 404);

        assert_eq!(runtime.requests_handled, 2);
    }

    #[test]
    fn test_edge_runtime_body_size_limit() {
        let config = EdgeConfig::new().with_max_body_size(10);
        let mut runtime = EdgeRuntime::new(config);
        runtime.on_fetch(|event| {
            event.respond_with(Response::new("ok"));
            Ok(())
        });

        let req = Request::with_method(
            "https://example.com/upload",
            HttpMethod::Post,
            Some("x".repeat(100)),
        );
        let resp = runtime.handle_request(req).unwrap();
        assert_eq!(resp.status, 413);
    }

    #[test]
    fn test_edge_runtime_kv_binding() {
        let mut runtime = EdgeRuntime::new(EdgeConfig::new());
        let mut kv = KVNamespace::new("SETTINGS");
        kv.put("theme", "dark");
        runtime.bind_kv(kv);

        let ns = runtime.kv("SETTINGS").unwrap();
        assert_eq!(ns.get("theme"), Some("dark"));
    }

    #[test]
    fn test_cache_eviction_on_capacity() {
        let mut cache = Cache::new(2);
        let req1 = Request::new("https://a.com");
        let req2 = Request::new("https://b.com");
        let req3 = Request::new("https://c.com");

        cache.put(&req1, Response::new("a"), None);
        cache.put(&req2, Response::new("b"), None);
        assert_eq!(cache.len(), 2);

        // Inserting a third entry should evict one to stay at capacity
        cache.put(&req3, Response::new("c"), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_edge_config_builder() {
        let config = EdgeConfig::new()
            .with_max_body_size(512)
            .with_timeout(Duration::from_secs(5))
            .with_max_cache_entries(256);

        assert_eq!(config.max_body_size, 512);
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.max_cache_entries, 256);
    }

    #[test]
    fn test_response_html() {
        let resp = Response::html("<h1>Hello</h1>");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.headers.get("content-type"), Some("text/html; charset=utf-8"));
        assert_eq!(resp.body, "<h1>Hello</h1>");
    }

    #[test]
    fn test_response_text() {
        let resp = Response::text("plain text");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.headers.get("content-type"), Some("text/plain; charset=utf-8"));
    }

    #[test]
    fn test_response_cors() {
        let resp = Response::json("{}").with_cors("https://example.com");
        assert_eq!(resp.headers.get("access-control-allow-origin"), Some("https://example.com"));
        assert!(resp.headers.has("access-control-allow-methods"));
    }

    #[test]
    fn test_request_url_parsing() {
        let req = Request::new("https://api.example.com:8080/v1/users?page=2&limit=10");
        let url_info = req.parse_url();
        assert_eq!(url_info.pathname, "/v1/users");
        assert_eq!(url_info.hostname, "api.example.com");
        assert_eq!(url_info.search_params.get("page"), Some(&"2".to_string()));
        assert_eq!(url_info.search_params.get("limit"), Some(&"10".to_string()));
    }

    #[test]
    fn test_request_url_parsing_no_query() {
        let req = Request::new("https://example.com/path");
        let url_info = req.parse_url();
        assert_eq!(url_info.pathname, "/path");
        assert!(url_info.search_params.is_empty());
    }

    #[test]
    fn test_edge_runtime_caching_integration() {
        let config = EdgeConfig::new();
        let mut runtime = EdgeRuntime::new(config);

        runtime.on_fetch(|event| {
            event.respond_with(Response::json(r#"{"data":"fresh"}"#));
            Ok(())
        });

        // First request: cache miss
        let req = Request::new("https://example.com/api/data");
        let resp = runtime.handle_request(req).unwrap();
        assert_eq!(resp.status, 200);

        // Cache the response manually
        let req2 = Request::new("https://example.com/api/data");
        runtime.cache_mut().put(&req2, Response::json(r#"{"data":"cached"}"#), None);

        // Second request: cache hit
        let req3 = Request::new("https://example.com/api/data");
        let resp2 = runtime.handle_request(req3).unwrap();
        assert_eq!(resp2.body, r#"{"data":"cached"}"#);
    }
}

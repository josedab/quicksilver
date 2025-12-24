//! Capability-Based Security System
//!
//! Implements a fine-grained permission system that allows sandboxing JavaScript
//! execution with explicit capability grants. Inspired by Deno's permission model
//! but with more granular control.
//!
//! # Example
//! ```text
//! let mut sandbox = Sandbox::new()
//!     .allow_read(&["./data"])
//!     .allow_net(&["api.example.com"])
//!     .deny_write_all();
//!
//! runtime.execute_with_sandbox(code, sandbox)?;
//! ```

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

/// Capability types that can be granted or denied
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// File system read access
    FileRead(PathPattern),
    /// File system write access
    FileWrite(PathPattern),
    /// Network access
    Network(HostPattern),
    /// Environment variable access
    Env(EnvPattern),
    /// Subprocess execution
    Subprocess,
    /// High-resolution time (can be used for timing attacks)
    HighResTime,
    /// Eval and dynamic code execution
    DynamicCode,
    /// FFI / native code
    Ffi,
    /// System info access (CPU, memory, etc.)
    SystemInfo,
    /// Crypto operations
    Crypto,
}

/// Path pattern for file system capabilities
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathPattern {
    /// All paths
    All,
    /// Exact path
    Exact(PathBuf),
    /// Prefix match (directory and children)
    Prefix(PathBuf),
    /// Glob pattern
    Glob(String),
}

/// Host pattern for network capabilities
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostPattern {
    /// All hosts
    All,
    /// Exact host
    Exact(String),
    /// Domain and subdomains
    Domain(String),
    /// IP range (CIDR notation)
    IpRange(String),
}

/// Environment variable pattern
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EnvPattern {
    /// All environment variables
    All,
    /// Specific variable
    Exact(String),
    /// Prefix match
    Prefix(String),
}

/// Permission state for a capability
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// Explicitly granted
    Granted,
    /// Explicitly denied
    Denied,
    /// Not set, will prompt if interactive
    Prompt,
}

/// A sandbox configuration for executing JavaScript code
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Granted capabilities
    granted: HashSet<Capability>,
    /// Denied capabilities
    denied: HashSet<Capability>,
    /// Whether to allow prompting for permissions
    allow_prompts: bool,
    /// Maximum memory usage in bytes
    memory_limit: Option<usize>,
    /// Maximum execution time in milliseconds
    time_limit: Option<u64>,
    /// Maximum stack depth
    stack_limit: Option<usize>,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox {
    /// Create a new sandbox with no permissions (deny-all by default)
    pub fn new() -> Self {
        Self {
            granted: HashSet::new(),
            denied: HashSet::new(),
            allow_prompts: false,
            memory_limit: None,
            time_limit: None,
            stack_limit: None,
        }
    }

    /// Create an unrestricted sandbox (allow-all)
    pub fn unrestricted() -> Self {
        let mut sandbox = Self::new();
        sandbox.granted.insert(Capability::FileRead(PathPattern::All));
        sandbox.granted.insert(Capability::FileWrite(PathPattern::All));
        sandbox.granted.insert(Capability::Network(HostPattern::All));
        sandbox.granted.insert(Capability::Env(EnvPattern::All));
        sandbox.granted.insert(Capability::Subprocess);
        sandbox.granted.insert(Capability::HighResTime);
        sandbox.granted.insert(Capability::DynamicCode);
        sandbox.granted.insert(Capability::Ffi);
        sandbox.granted.insert(Capability::SystemInfo);
        sandbox.granted.insert(Capability::Crypto);
        sandbox
    }

    /// Allow reading from specific paths
    pub fn allow_read(mut self, paths: &[&str]) -> Self {
        for path in paths {
            self.granted.insert(Capability::FileRead(PathPattern::Prefix(PathBuf::from(path))));
        }
        self
    }

    /// Allow reading all files
    pub fn allow_read_all(mut self) -> Self {
        self.granted.insert(Capability::FileRead(PathPattern::All));
        self
    }

    /// Deny all file reading
    pub fn deny_read_all(mut self) -> Self {
        self.denied.insert(Capability::FileRead(PathPattern::All));
        self
    }

    /// Allow writing to specific paths
    pub fn allow_write(mut self, paths: &[&str]) -> Self {
        for path in paths {
            self.granted.insert(Capability::FileWrite(PathPattern::Prefix(PathBuf::from(path))));
        }
        self
    }

    /// Allow writing all files
    pub fn allow_write_all(mut self) -> Self {
        self.granted.insert(Capability::FileWrite(PathPattern::All));
        self
    }

    /// Deny all file writing
    pub fn deny_write_all(mut self) -> Self {
        self.denied.insert(Capability::FileWrite(PathPattern::All));
        self
    }

    /// Allow network access to specific hosts
    pub fn allow_net(mut self, hosts: &[&str]) -> Self {
        for host in hosts {
            self.granted.insert(Capability::Network(HostPattern::Exact(host.to_string())));
        }
        self
    }

    /// Allow all network access
    pub fn allow_net_all(mut self) -> Self {
        self.granted.insert(Capability::Network(HostPattern::All));
        self
    }

    /// Deny all network access
    pub fn deny_net_all(mut self) -> Self {
        self.denied.insert(Capability::Network(HostPattern::All));
        self
    }

    /// Allow environment variable access
    pub fn allow_env(mut self, vars: &[&str]) -> Self {
        for var in vars {
            self.granted.insert(Capability::Env(EnvPattern::Exact(var.to_string())));
        }
        self
    }

    /// Allow all environment variables
    pub fn allow_env_all(mut self) -> Self {
        self.granted.insert(Capability::Env(EnvPattern::All));
        self
    }

    /// Deny all environment variables
    pub fn deny_env_all(mut self) -> Self {
        self.denied.insert(Capability::Env(EnvPattern::All));
        self
    }

    /// Allow subprocess execution
    pub fn allow_subprocess(mut self) -> Self {
        self.granted.insert(Capability::Subprocess);
        self
    }

    /// Allow eval() and new Function()
    pub fn allow_dynamic_code(mut self) -> Self {
        self.granted.insert(Capability::DynamicCode);
        self
    }

    /// Allow high-resolution timing
    pub fn allow_hrtime(mut self) -> Self {
        self.granted.insert(Capability::HighResTime);
        self
    }

    /// Set memory limit in bytes
    pub fn with_memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// Set time limit in milliseconds
    pub fn with_time_limit(mut self, ms: u64) -> Self {
        self.time_limit = Some(ms);
        self
    }

    /// Set stack depth limit
    pub fn with_stack_limit(mut self, depth: usize) -> Self {
        self.stack_limit = Some(depth);
        self
    }

    /// Check if a capability is allowed
    pub fn check(&self, capability: &Capability) -> PermissionState {
        // Explicit deny takes precedence
        if self.is_denied(capability) {
            return PermissionState::Denied;
        }

        // Check for explicit grant
        if self.is_granted(capability) {
            return PermissionState::Granted;
        }

        // Default to prompt (or denied if prompts not allowed)
        if self.allow_prompts {
            PermissionState::Prompt
        } else {
            PermissionState::Denied
        }
    }

    /// Check if capability matches any granted pattern
    fn is_granted(&self, capability: &Capability) -> bool {
        for granted in &self.granted {
            if capability_matches(granted, capability) {
                return true;
            }
        }
        false
    }

    /// Check if capability matches any denied pattern
    fn is_denied(&self, capability: &Capability) -> bool {
        for denied in &self.denied {
            if capability_matches(denied, capability) {
                return true;
            }
        }
        false
    }

    /// Get memory limit
    pub fn memory_limit(&self) -> Option<usize> {
        self.memory_limit
    }

    /// Get time limit
    pub fn time_limit(&self) -> Option<u64> {
        self.time_limit
    }

    /// Get stack limit
    pub fn stack_limit(&self) -> Option<usize> {
        self.stack_limit
    }
}

/// Check if a pattern capability matches a specific capability
fn capability_matches(pattern: &Capability, specific: &Capability) -> bool {
    match (pattern, specific) {
        (Capability::FileRead(p1), Capability::FileRead(p2)) => path_pattern_matches(p1, p2),
        (Capability::FileWrite(p1), Capability::FileWrite(p2)) => path_pattern_matches(p1, p2),
        (Capability::Network(h1), Capability::Network(h2)) => host_pattern_matches(h1, h2),
        (Capability::Env(e1), Capability::Env(e2)) => env_pattern_matches(e1, e2),
        (a, b) => a == b,
    }
}

fn path_pattern_matches(pattern: &PathPattern, specific: &PathPattern) -> bool {
    match (pattern, specific) {
        (PathPattern::All, _) => true,
        (PathPattern::Exact(p1), PathPattern::Exact(p2)) => p1 == p2,
        (PathPattern::Prefix(prefix), PathPattern::Exact(path)) => path.starts_with(prefix),
        (PathPattern::Prefix(prefix), PathPattern::Prefix(other)) => other.starts_with(prefix),
        _ => pattern == specific,
    }
}

fn host_pattern_matches(pattern: &HostPattern, specific: &HostPattern) -> bool {
    match (pattern, specific) {
        (HostPattern::All, _) => true,
        (HostPattern::Exact(h1), HostPattern::Exact(h2)) => h1 == h2,
        (HostPattern::Domain(domain), HostPattern::Exact(host)) => {
            host == domain || host.ends_with(&format!(".{}", domain))
        }
        _ => pattern == specific,
    }
}

fn env_pattern_matches(pattern: &EnvPattern, specific: &EnvPattern) -> bool {
    match (pattern, specific) {
        (EnvPattern::All, _) => true,
        (EnvPattern::Exact(v1), EnvPattern::Exact(v2)) => v1 == v2,
        (EnvPattern::Prefix(prefix), EnvPattern::Exact(var)) => var.starts_with(prefix),
        _ => pattern == specific,
    }
}

/// Runtime permission checker
#[derive(Debug)]
pub struct PermissionChecker {
    sandbox: Sandbox,
    violation_count: usize,
}

impl PermissionChecker {
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox,
            violation_count: 0,
        }
    }

    /// Check a capability and return whether it's allowed
    pub fn check(&mut self, capability: &Capability) -> Result<(), PermissionError> {
        match self.sandbox.check(capability) {
            PermissionState::Granted => Ok(()),
            PermissionState::Denied => {
                self.violation_count += 1;
                Err(PermissionError::Denied(format!("{:?}", capability)))
            }
            PermissionState::Prompt => {
                Err(PermissionError::NeedsPrompt(format!("{:?}", capability)))
            }
        }
    }

    /// Check file read permission
    pub fn check_read(&mut self, path: &str) -> Result<(), PermissionError> {
        self.check(&Capability::FileRead(PathPattern::Exact(PathBuf::from(path))))
    }

    /// Check file write permission
    pub fn check_write(&mut self, path: &str) -> Result<(), PermissionError> {
        self.check(&Capability::FileWrite(PathPattern::Exact(PathBuf::from(path))))
    }

    /// Check network permission
    pub fn check_net(&mut self, host: &str) -> Result<(), PermissionError> {
        self.check(&Capability::Network(HostPattern::Exact(host.to_string())))
    }

    /// Check env permission
    pub fn check_env(&mut self, var: &str) -> Result<(), PermissionError> {
        self.check(&Capability::Env(EnvPattern::Exact(var.to_string())))
    }

    /// Get violation count
    pub fn violation_count(&self) -> usize {
        self.violation_count
    }
}

/// Permission error types
#[derive(Debug, Clone)]
pub enum PermissionError {
    /// Permission was explicitly denied
    Denied(String),
    /// Permission needs user prompt
    NeedsPrompt(String),
}

impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(cap) => write!(f, "Permission denied: {}", cap),
            Self::NeedsPrompt(cap) => write!(f, "Permission not granted, requires prompt: {}", cap),
        }
    }
}

impl std::error::Error for PermissionError {}

// ========== IP Address Security ==========

/// CIDR block representation for IP range matching
#[derive(Debug, Clone)]
pub struct CidrBlock {
    /// Base IP address
    base: IpAddr,
    /// Prefix length (subnet mask bits)
    prefix_len: u8,
}

impl CidrBlock {
    /// Parse a CIDR notation string (e.g., "192.168.0.0/16")
    pub fn parse(cidr: &str) -> Option<Self> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return None;
        }

        let base: IpAddr = parts[0].parse().ok()?;
        let prefix_len: u8 = parts[1].parse().ok()?;

        // Validate prefix length
        let max_prefix = match base {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };

        if prefix_len > max_prefix {
            return None;
        }

        Some(Self { base, prefix_len })
    }

    /// Check if an IP address is contained in this CIDR block
    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (&self.base, ip) {
            (IpAddr::V4(base), IpAddr::V4(addr)) => {
                let base_bits = u32::from_be_bytes(base.octets());
                let addr_bits = u32::from_be_bytes(addr.octets());
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - self.prefix_len)
                };
                (base_bits & mask) == (addr_bits & mask)
            }
            (IpAddr::V6(base), IpAddr::V6(addr)) => {
                let base_bits = u128::from_be_bytes(base.octets());
                let addr_bits = u128::from_be_bytes(addr.octets());
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    !0u128 << (128 - self.prefix_len)
                };
                (base_bits & mask) == (addr_bits & mask)
            }
            _ => false, // IPv4 and IPv6 don't mix
        }
    }
}

/// Internal/private IP ranges that should be blocked by default
/// These ranges are defined by RFC 1918, RFC 4193, RFC 3927, and RFC 4291
pub static INTERNAL_IP_RANGES: &[&str] = &[
    // IPv4 private/reserved ranges
    "127.0.0.0/8",      // Loopback
    "10.0.0.0/8",       // Private (Class A)
    "172.16.0.0/12",    // Private (Class B)
    "192.168.0.0/16",   // Private (Class C)
    "169.254.0.0/16",   // Link-local
    "0.0.0.0/8",        // "This" network
    "224.0.0.0/4",      // Multicast
    "240.0.0.0/4",      // Reserved for future use
    // IPv6 private/reserved ranges
    "::1/128",          // Loopback
    "fc00::/7",         // Unique local addresses
    "fe80::/10",        // Link-local
    "ff00::/8",         // Multicast
];

/// Check if an IP address is in an internal/private range
pub fn is_internal_ip(ip: &IpAddr) -> bool {
    for cidr_str in INTERNAL_IP_RANGES {
        if let Some(cidr) = CidrBlock::parse(cidr_str) {
            if cidr.contains(ip) {
                return true;
            }
        }
    }
    false
}

/// Parse an IP address from a string (handles both IPv4 and IPv6)
pub fn parse_ip(s: &str) -> Option<IpAddr> {
    // Try parsing as IPv4
    if let Ok(ipv4) = s.parse::<Ipv4Addr>() {
        return Some(IpAddr::V4(ipv4));
    }

    // Try parsing as IPv6 (including bracketed form like [::1])
    let s = s.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ipv6) = s.parse::<Ipv6Addr>() {
        return Some(IpAddr::V6(ipv6));
    }

    None
}

/// Validate a domain name format
pub fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }

    // Check for IP address (not a domain)
    if parse_ip(domain).is_some() {
        return false;
    }

    // Validate each label
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }

        // Labels must start with alphanumeric
        if !label.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false) {
            return false;
        }

        // Labels must end with alphanumeric
        if !label.chars().last().map(|c| c.is_alphanumeric()).unwrap_or(false) {
            return false;
        }

        // Labels can only contain alphanumeric and hyphens
        if !label.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return false;
        }
    }

    true
}

/// Network security checker for validating hosts/IPs
#[derive(Debug, Clone)]
pub struct NetworkSecurity {
    /// Whether to block internal/private IP ranges
    block_internal_ips: bool,
    /// Additional blocked CIDR ranges
    blocked_ranges: Vec<CidrBlock>,
    /// Allowed CIDR ranges (takes precedence over blocked)
    allowed_ranges: Vec<CidrBlock>,
}

impl Default for NetworkSecurity {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkSecurity {
    /// Create a new network security checker with default settings (blocks internal IPs)
    pub fn new() -> Self {
        Self {
            block_internal_ips: true,
            blocked_ranges: Vec::new(),
            allowed_ranges: Vec::new(),
        }
    }

    /// Create a permissive checker that doesn't block internal IPs
    pub fn permissive() -> Self {
        Self {
            block_internal_ips: false,
            blocked_ranges: Vec::new(),
            allowed_ranges: Vec::new(),
        }
    }

    /// Add a CIDR range to the blocked list
    pub fn block_range(mut self, cidr: &str) -> Self {
        if let Some(block) = CidrBlock::parse(cidr) {
            self.blocked_ranges.push(block);
        }
        self
    }

    /// Add a CIDR range to the allowed list (takes precedence)
    pub fn allow_range(mut self, cidr: &str) -> Self {
        if let Some(block) = CidrBlock::parse(cidr) {
            self.allowed_ranges.push(block);
        }
        self
    }

    /// Check if a host (IP or domain) should be blocked
    /// Returns Ok(()) if allowed, Err with reason if blocked
    pub fn check_host(&self, host: &str) -> Result<(), String> {
        // Check if host is an IP address
        if let Some(ip) = parse_ip(host) {
            return self.check_ip(&ip);
        }

        // Validate domain format
        if !is_valid_domain(host) {
            return Err(format!("Invalid domain format: {}", host));
        }

        // Domain is valid - DNS rebinding protection would happen at connection time
        // when we can check the resolved IP
        Ok(())
    }

    /// Check if an IP address should be blocked
    pub fn check_ip(&self, ip: &IpAddr) -> Result<(), String> {
        // Check allowed ranges first (takes precedence)
        for range in &self.allowed_ranges {
            if range.contains(ip) {
                return Ok(());
            }
        }

        // Check if IP is in internal ranges
        if self.block_internal_ips && is_internal_ip(ip) {
            return Err(format!(
                "Access to internal/private IP {} is blocked for security",
                ip
            ));
        }

        // Check additional blocked ranges
        for range in &self.blocked_ranges {
            if range.contains(ip) {
                return Err(format!("Access to IP {} is blocked by security policy", ip));
            }
        }

        Ok(())
    }

    /// DNS rebinding protection: check the resolved IP after DNS lookup
    pub fn check_resolved_ip(&self, hostname: &str, resolved_ip: &IpAddr) -> Result<(), String> {
        // If hostname is not an IP but resolves to an internal IP, it's likely DNS rebinding
        if parse_ip(hostname).is_none() && self.block_internal_ips && is_internal_ip(resolved_ip) {
            return Err(format!(
                "DNS rebinding attack detected: {} resolved to internal IP {}",
                hostname, resolved_ip
            ));
        }

        self.check_ip(resolved_ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_sandbox_denies_all() {
        let sandbox = Sandbox::new();
        assert_eq!(
            sandbox.check(&Capability::FileRead(PathPattern::Exact(PathBuf::from("/etc/passwd")))),
            PermissionState::Denied
        );
    }

    #[test]
    fn test_allow_read_specific_path() {
        let sandbox = Sandbox::new().allow_read(&["./data"]);
        let checker = sandbox.check(&Capability::FileRead(PathPattern::Exact(PathBuf::from("./data/file.txt"))));
        assert_eq!(checker, PermissionState::Granted);
    }

    #[test]
    fn test_deny_takes_precedence() {
        let sandbox = Sandbox::new()
            .allow_read_all()
            .deny_read_all();
        assert_eq!(
            sandbox.check(&Capability::FileRead(PathPattern::Exact(PathBuf::from("/any/path")))),
            PermissionState::Denied
        );
    }

    #[test]
    fn test_unrestricted_sandbox() {
        let sandbox = Sandbox::unrestricted();
        assert_eq!(
            sandbox.check(&Capability::FileRead(PathPattern::All)),
            PermissionState::Granted
        );
        assert_eq!(
            sandbox.check(&Capability::Network(HostPattern::All)),
            PermissionState::Granted
        );
    }

    // ========== Network Security Tests ==========

    #[test]
    fn test_cidr_parsing() {
        let cidr = CidrBlock::parse("192.168.0.0/16").unwrap();
        assert!(cidr.contains(&"192.168.1.1".parse().unwrap()));
        assert!(cidr.contains(&"192.168.255.255".parse().unwrap()));
        assert!(!cidr.contains(&"192.169.0.1".parse().unwrap()));
        assert!(!cidr.contains(&"10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_cidr_ipv6() {
        let cidr = CidrBlock::parse("fc00::/7").unwrap();
        assert!(cidr.contains(&"fd00::1".parse().unwrap()));
        assert!(cidr.contains(&"fc00::1".parse().unwrap()));
        assert!(!cidr.contains(&"2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn test_internal_ip_detection() {
        // IPv4 internal ranges
        assert!(is_internal_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_internal_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_internal_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_internal_ip(&"192.168.1.1".parse().unwrap()));
        assert!(is_internal_ip(&"169.254.1.1".parse().unwrap()));

        // Public IPs
        assert!(!is_internal_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_internal_ip(&"1.1.1.1".parse().unwrap()));
        assert!(!is_internal_ip(&"203.0.113.1".parse().unwrap()));

        // IPv6 internal ranges
        assert!(is_internal_ip(&"::1".parse().unwrap()));
        assert!(is_internal_ip(&"fd00::1".parse().unwrap()));
        assert!(is_internal_ip(&"fe80::1".parse().unwrap()));

        // IPv6 public
        assert!(!is_internal_ip(&"2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn test_network_security_blocks_internal() {
        let security = NetworkSecurity::new();

        // Should block internal IPs
        assert!(security.check_host("127.0.0.1").is_err());
        assert!(security.check_host("10.0.0.1").is_err());
        assert!(security.check_host("192.168.1.1").is_err());

        // Should allow public IPs
        assert!(security.check_host("8.8.8.8").is_ok());
        assert!(security.check_host("1.1.1.1").is_ok());

        // Should allow valid domains
        assert!(security.check_host("example.com").is_ok());
        assert!(security.check_host("api.example.com").is_ok());
    }

    #[test]
    fn test_network_security_permissive() {
        let security = NetworkSecurity::permissive();

        // Should allow internal IPs in permissive mode
        assert!(security.check_host("127.0.0.1").is_ok());
        assert!(security.check_host("192.168.1.1").is_ok());
    }

    #[test]
    fn test_network_security_allow_range() {
        let security = NetworkSecurity::new()
            .allow_range("192.168.1.0/24");

        // Should allow specific subnet
        assert!(security.check_host("192.168.1.100").is_ok());

        // Should still block other internal IPs
        assert!(security.check_host("192.168.2.1").is_err());
        assert!(security.check_host("10.0.0.1").is_err());
    }

    #[test]
    fn test_dns_rebinding_protection() {
        let security = NetworkSecurity::new();

        // Domain that resolves to internal IP should be blocked
        let result = security.check_resolved_ip("evil.com", &"127.0.0.1".parse().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DNS rebinding"));

        // Domain that resolves to public IP should be allowed
        assert!(security.check_resolved_ip("example.com", &"93.184.216.34".parse().unwrap()).is_ok());
    }

    #[test]
    fn test_domain_validation() {
        // Valid domains
        assert!(is_valid_domain("example.com"));
        assert!(is_valid_domain("api.example.com"));
        assert!(is_valid_domain("my-service.example.org"));
        assert!(is_valid_domain("a.b.c.d.e.f"));

        // Invalid domains
        assert!(!is_valid_domain("")); // Empty
        assert!(!is_valid_domain("192.168.1.1")); // IP address
        assert!(!is_valid_domain("-example.com")); // Starts with hyphen
        assert!(!is_valid_domain("example-.com")); // Ends with hyphen
        assert!(!is_valid_domain("exam ple.com")); // Contains space
        assert!(!is_valid_domain("example..com")); // Empty label
    }
}

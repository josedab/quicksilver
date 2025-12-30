//! Process API
//!
//! Deno-style process and subprocess management with capability-based security.
//!
//! # Example
//! ```text
//! // Run a command
//! const output = await Deno.run({
//!   cmd: ["echo", "Hello, World!"],
//! });
//!
//! // Get environment variable
//! const path = Deno.env.get("PATH");
//!
//! // Exit the process
//! Deno.exit(0);
//! ```

use crate::runtime::Value;
use crate::security::{Capability, EnvPattern, PermissionState, Sandbox};
use std::collections::{HashMap, HashSet};
use std::env;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Process error
#[derive(Debug, Clone)]
pub enum ProcessError {
    PermissionDenied(String),
    SpawnFailed(String),
    IoError(String),
    Timeout,
    InvalidCommand,
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied(msg) => write!(f, "permission denied: {}", msg),
            Self::SpawnFailed(msg) => write!(f, "spawn failed: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::Timeout => write!(f, "process timed out"),
            Self::InvalidCommand => write!(f, "invalid command"),
        }
    }
}

impl std::error::Error for ProcessError {}

pub type ProcessResult<T> = Result<T, ProcessError>;

/// Command output
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
    pub code: Option<i32>,
}

impl CommandOutput {
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }

    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        props.insert("stdout".to_string(), Value::String(self.stdout_string()));
        props.insert("stderr".to_string(), Value::String(self.stderr_string()));
        props.insert("success".to_string(), Value::Boolean(self.success));
        props.insert(
            "code".to_string(),
            self.code.map(|c| Value::Number(c as f64)).unwrap_or(Value::Null),
        );
        Value::new_object_with_properties(props)
    }
}

/// Run options
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub cmd: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub stdin: StdioMode,
    pub stdout: StdioMode,
    pub stderr: StdioMode,
    pub timeout: Option<Duration>,
}

/// Stdio mode
#[derive(Debug, Clone, Default)]
pub enum StdioMode {
    #[default]
    Inherit,
    Piped,
    Null,
}

// ========== Process Security ==========

/// Sensitive environment variables that should be filtered by default
/// These contain secrets, credentials, or security-sensitive information
pub static SENSITIVE_ENV_VARS: &[&str] = &[
    // API keys and tokens
    "API_KEY",
    "API_SECRET",
    "API_TOKEN",
    "AUTH_TOKEN",
    "ACCESS_TOKEN",
    "SECRET_KEY",
    "PRIVATE_KEY",
    // AWS credentials
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_SECURITY_TOKEN",
    // Cloud provider credentials
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_API_KEY",
    "AZURE_CLIENT_SECRET",
    "AZURE_TENANT_ID",
    "DIGITALOCEAN_ACCESS_TOKEN",
    // Database credentials
    "DATABASE_URL",
    "DATABASE_PASSWORD",
    "DB_PASSWORD",
    "POSTGRES_PASSWORD",
    "MYSQL_PASSWORD",
    "REDIS_PASSWORD",
    "MONGODB_URI",
    // Git and GitHub
    "GIT_TOKEN",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "GH_TOKEN",
    // SSH
    "SSH_PRIVATE_KEY",
    "SSH_AUTH_SOCK",
    // NPM
    "NPM_TOKEN",
    "NPM_AUTH_TOKEN",
    // Docker
    "DOCKER_PASSWORD",
    "DOCKER_AUTH_CONFIG",
    // JWT and session
    "JWT_SECRET",
    "SESSION_SECRET",
    "COOKIE_SECRET",
    // Encryption
    "ENCRYPTION_KEY",
    "MASTER_KEY",
    // Generic password patterns (checked by prefix)
    "PASSWORD",
    "PASSWD",
    "SECRET",
    "CREDENTIAL",
    "TOKEN",
];

/// Dangerous commands that should be blocked by default
/// These can be used for system compromise, privilege escalation, or data exfiltration
pub static DANGEROUS_COMMANDS: &[&str] = &[
    // Shell access
    "sh",
    "bash",
    "zsh",
    "fish",
    "csh",
    "ksh",
    "dash",
    // Remote access
    "ssh",
    "telnet",
    "nc",
    "netcat",
    "ncat",
    "socat",
    // Network tools that could exfiltrate data
    "curl",
    "wget",
    "ftp",
    "sftp",
    "scp",
    // Privilege escalation
    "sudo",
    "su",
    "doas",
    "pkexec",
    // System modification
    "chmod",
    "chown",
    "chgrp",
    "mount",
    "umount",
    // Process manipulation
    "kill",
    "killall",
    "pkill",
    // System information gathering
    "ps",
    "top",
    "htop",
    "lsof",
    "netstat",
    "ss",
    // Disk operations
    "dd",
    "mkfs",
    "fdisk",
    "parted",
    // Package managers (could install malware)
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "pacman",
    "brew",
    "npm",
    "pip",
    "gem",
    // Compilation (could compile exploits)
    "gcc",
    "g++",
    "clang",
    "make",
    "cmake",
    // Python and other interpreters
    "python",
    "python3",
    "ruby",
    "perl",
    "node",
    // System administration
    "systemctl",
    "service",
    "init",
    "crontab",
    "at",
];

/// Process security configuration
#[derive(Debug, Clone)]
pub struct ProcessSecurity {
    /// Whether to filter sensitive environment variables
    filter_sensitive_env: bool,
    /// Additional environment variables to filter
    filtered_env_vars: HashSet<String>,
    /// Environment variable prefixes to filter
    filtered_env_prefixes: Vec<String>,
    /// Whether to use command whitelist mode
    use_whitelist: bool,
    /// Allowed commands (whitelist mode)
    allowed_commands: HashSet<String>,
    /// Blocked commands (blacklist mode)
    blocked_commands: HashSet<String>,
    /// Maximum execution time in seconds
    max_execution_time: Option<Duration>,
}

impl Default for ProcessSecurity {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSecurity {
    /// Create new process security with default settings (filters sensitive env, blocks dangerous commands)
    pub fn new() -> Self {
        let mut blocked = HashSet::new();
        for cmd in DANGEROUS_COMMANDS {
            blocked.insert(cmd.to_string());
        }

        Self {
            filter_sensitive_env: true,
            filtered_env_vars: HashSet::new(),
            filtered_env_prefixes: vec![],
            use_whitelist: false,
            allowed_commands: HashSet::new(),
            blocked_commands: blocked,
            max_execution_time: Some(Duration::from_secs(30)), // 30 second default timeout
        }
    }

    /// Create a permissive security policy (no filtering or blocking)
    pub fn permissive() -> Self {
        Self {
            filter_sensitive_env: false,
            filtered_env_vars: HashSet::new(),
            filtered_env_prefixes: vec![],
            use_whitelist: false,
            allowed_commands: HashSet::new(),
            blocked_commands: HashSet::new(),
            max_execution_time: None,
        }
    }

    /// Enable strict whitelist mode (only allowed commands can run)
    pub fn whitelist_mode(mut self) -> Self {
        self.use_whitelist = true;
        self
    }

    /// Add a command to the whitelist
    pub fn allow_command(mut self, command: &str) -> Self {
        self.allowed_commands.insert(command.to_string());
        self
    }

    /// Add multiple commands to the whitelist
    pub fn allow_commands(mut self, commands: &[&str]) -> Self {
        for cmd in commands {
            self.allowed_commands.insert(cmd.to_string());
        }
        self
    }

    /// Block a specific command
    pub fn block_command(mut self, command: &str) -> Self {
        self.blocked_commands.insert(command.to_string());
        self
    }

    /// Add an environment variable to the filter list
    pub fn filter_env(mut self, var: &str) -> Self {
        self.filtered_env_vars.insert(var.to_string());
        self
    }

    /// Add an environment variable prefix to filter
    pub fn filter_env_prefix(mut self, prefix: &str) -> Self {
        self.filtered_env_prefixes.push(prefix.to_string());
        self
    }

    /// Disable sensitive environment filtering
    pub fn allow_sensitive_env(mut self) -> Self {
        self.filter_sensitive_env = false;
        self
    }

    /// Set maximum execution time
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.max_execution_time = Some(timeout);
        self
    }

    /// Remove execution time limit
    pub fn no_timeout(mut self) -> Self {
        self.max_execution_time = None;
        self
    }

    /// Check if a command is allowed
    pub fn check_command(&self, command: &str) -> Result<(), String> {
        // Extract just the command name (not full path)
        let cmd_name = std::path::Path::new(command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(command);

        if self.use_whitelist {
            // In whitelist mode, only explicitly allowed commands can run
            if !self.allowed_commands.contains(cmd_name) && !self.allowed_commands.contains(command) {
                return Err(format!(
                    "Command '{}' is not in the whitelist",
                    command
                ));
            }
        } else {
            // In blacklist mode, blocked commands cannot run
            if self.blocked_commands.contains(cmd_name) || self.blocked_commands.contains(command) {
                return Err(format!(
                    "Command '{}' is blocked for security reasons",
                    command
                ));
            }
        }

        Ok(())
    }

    /// Check if an environment variable should be filtered
    pub fn should_filter_env(&self, var: &str) -> bool {
        // Check explicit filter list
        if self.filtered_env_vars.contains(var) {
            return true;
        }

        // Check filtered prefixes
        let var_upper = var.to_uppercase();
        for prefix in &self.filtered_env_prefixes {
            if var_upper.starts_with(&prefix.to_uppercase()) {
                return true;
            }
        }

        // Check sensitive env vars if filtering is enabled
        if self.filter_sensitive_env {
            for sensitive in SENSITIVE_ENV_VARS {
                // Exact match
                if var_upper == *sensitive {
                    return true;
                }
                // Contains match for generic patterns
                if sensitive.len() <= 10 && var_upper.contains(sensitive) {
                    return true;
                }
            }
        }

        false
    }

    /// Filter environment variables, removing sensitive ones
    pub fn filter_env_vars(&self, env: HashMap<String, String>) -> HashMap<String, String> {
        env.into_iter()
            .filter(|(k, _)| !self.should_filter_env(k))
            .collect()
    }

    /// Get maximum execution time
    pub fn max_execution_time(&self) -> Option<Duration> {
        self.max_execution_time
    }
}

/// Environment variable access
pub struct Env {
    sandbox: Option<Sandbox>,
}

impl Env {
    pub fn new() -> Self {
        Self { sandbox: None }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }

    fn check_env_read(&self, key: &str) -> ProcessResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            let capability = Capability::Env(EnvPattern::Exact(key.to_string()));
            if sandbox.check(&capability) != PermissionState::Granted {
                return Err(ProcessError::PermissionDenied(
                    format!("environment read denied: {}", key),
                ));
            }
        }
        Ok(())
    }

    fn check_env_all(&self) -> ProcessResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            let capability = Capability::Env(EnvPattern::All);
            if sandbox.check(&capability) != PermissionState::Granted {
                return Err(ProcessError::PermissionDenied(
                    "environment access denied".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Get an environment variable
    pub fn get(&self, key: &str) -> ProcessResult<Option<String>> {
        self.check_env_read(key)?;
        Ok(env::var(key).ok())
    }

    /// Set an environment variable (requires env permission)
    pub fn set(&self, key: &str, value: &str) -> ProcessResult<()> {
        // Note: Env capability covers both read and write in this security model
        self.check_env_read(key)?;
        env::set_var(key, value);
        Ok(())
    }

    /// Delete an environment variable
    pub fn delete(&self, key: &str) -> ProcessResult<()> {
        self.check_env_read(key)?;
        env::remove_var(key);
        Ok(())
    }

    /// Get all environment variables
    pub fn to_object(&self) -> ProcessResult<HashMap<String, String>> {
        self.check_env_all()?;
        Ok(env::vars().collect())
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// Process API
pub struct Process {
    sandbox: Option<Sandbox>,
    security: ProcessSecurity,
}

impl Process {
    pub fn new() -> Self {
        Self {
            sandbox: None,
            security: ProcessSecurity::new(),
        }
    }

    /// Create a process runner without security restrictions
    pub fn new_permissive() -> Self {
        Self {
            sandbox: None,
            security: ProcessSecurity::permissive(),
        }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
            security: ProcessSecurity::new(),
        }
    }

    /// Set custom process security configuration
    pub fn with_security(mut self, security: ProcessSecurity) -> Self {
        self.security = security;
        self
    }

    /// Check subprocess permission (sandbox capability)
    fn check_sandbox_permission(&self, cmd: &str) -> ProcessResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            if sandbox.check(&Capability::Subprocess) != PermissionState::Granted {
                return Err(ProcessError::PermissionDenied(format!(
                    "subprocess '{}' not allowed by sandbox",
                    cmd
                )));
            }
        }
        Ok(())
    }

    /// Check command against security policy
    fn check_command_security(&self, cmd: &str) -> ProcessResult<()> {
        self.security
            .check_command(cmd)
            .map_err(ProcessError::PermissionDenied)
    }

    /// Run a command and wait for completion
    pub fn run(&self, options: RunOptions) -> ProcessResult<CommandOutput> {
        if options.cmd.is_empty() {
            return Err(ProcessError::InvalidCommand);
        }

        let program = &options.cmd[0];

        // Check sandbox permission first
        self.check_sandbox_permission(program)?;

        // Check command security policy
        self.check_command_security(program)?;

        let mut command = Command::new(program);
        command.args(&options.cmd[1..]);

        // Set working directory
        if let Some(ref cwd) = options.cwd {
            command.current_dir(cwd);
        }

        // Filter and set environment variables
        let filtered_env = self.security.filter_env_vars(options.env.clone());
        for (key, value) in &filtered_env {
            command.env(key, value);
        }

        // Configure stdio
        command.stdin(match options.stdin {
            StdioMode::Inherit => Stdio::inherit(),
            StdioMode::Piped => Stdio::piped(),
            StdioMode::Null => Stdio::null(),
        });
        command.stdout(match options.stdout {
            StdioMode::Inherit => Stdio::inherit(),
            StdioMode::Piped => Stdio::piped(),
            StdioMode::Null => Stdio::null(),
        });
        command.stderr(match options.stderr {
            StdioMode::Inherit => Stdio::inherit(),
            StdioMode::Piped => Stdio::piped(),
            StdioMode::Null => Stdio::null(),
        });

        // Spawn and wait
        let output = command
            .output()
            .map_err(|e| ProcessError::SpawnFailed(e.to_string()))?;

        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            success: output.status.success(),
            code: output.status.code(),
        })
    }

    /// Spawn a command without waiting
    pub fn spawn(&self, options: RunOptions) -> ProcessResult<Child> {
        if options.cmd.is_empty() {
            return Err(ProcessError::InvalidCommand);
        }

        let program = &options.cmd[0];

        // Check sandbox permission first
        self.check_sandbox_permission(program)?;

        // Check command security policy
        self.check_command_security(program)?;

        let mut command = Command::new(program);
        command.args(&options.cmd[1..]);

        if let Some(ref cwd) = options.cwd {
            command.current_dir(cwd);
        }

        // Filter and set environment variables
        let filtered_env = self.security.filter_env_vars(options.env.clone());
        for (key, value) in &filtered_env {
            command.env(key, value);
        }

        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let child = command
            .spawn()
            .map_err(|e| ProcessError::SpawnFailed(e.to_string()))?;

        Ok(Child {
            inner: Some(child),
        })
    }

    /// Get the current process ID
    pub fn pid() -> u32 {
        std::process::id()
    }

    /// Exit the process
    pub fn exit(code: i32) -> ! {
        std::process::exit(code)
    }

    /// Get command line arguments
    pub fn args() -> Vec<String> {
        env::args().collect()
    }

    /// Get the current working directory
    pub fn cwd() -> ProcessResult<String> {
        env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| ProcessError::IoError(e.to_string()))
    }

    /// Change the current working directory
    pub fn chdir(path: &str) -> ProcessResult<()> {
        env::set_current_dir(path).map_err(|e| ProcessError::IoError(e.to_string()))
    }
}

impl Default for Process {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawned child process
pub struct Child {
    inner: Option<std::process::Child>,
}

impl Child {
    /// Wait for the child to exit
    pub fn wait(&mut self) -> ProcessResult<CommandOutput> {
        let child = self
            .inner
            .take()
            .ok_or_else(|| ProcessError::IoError("child already consumed".to_string()))?;

        let output = child
            .wait_with_output()
            .map_err(|e| ProcessError::IoError(e.to_string()))?;

        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            success: output.status.success(),
            code: output.status.code(),
        })
    }

    /// Kill the child process
    pub fn kill(&mut self) -> ProcessResult<()> {
        if let Some(ref mut child) = self.inner {
            child
                .kill()
                .map_err(|e| ProcessError::IoError(e.to_string()))?;
        }
        Ok(())
    }

    /// Get the child's process ID
    pub fn id(&self) -> Option<u32> {
        self.inner.as_ref().map(|c| c.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_command() {
        // Use permissive mode for tests since "echo" might be blocked
        let process = Process::new_permissive();
        let output = process
            .run(RunOptions {
                cmd: vec!["echo".to_string(), "hello".to_string()],
                stdout: StdioMode::Piped,
                ..Default::default()
            })
            .unwrap();

        assert!(output.success);
        assert!(output.stdout_string().contains("hello"));
    }

    #[test]
    fn test_run_with_env() {
        let process = Process::new_permissive();
        let mut env = HashMap::default();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());

        let output = process
            .run(RunOptions {
                cmd: vec!["printenv".to_string(), "TEST_VAR".to_string()],
                env,
                stdout: StdioMode::Piped,
                ..Default::default()
            })
            .unwrap();

        assert!(output.success);
        assert!(output.stdout_string().contains("test_value"));
    }

    #[test]
    fn test_env_get() {
        let env = Env::new();
        // PATH should exist on most systems
        let path = env.get("PATH").unwrap();
        assert!(path.is_some());
    }

    #[test]
    fn test_process_pid() {
        let pid = Process::pid();
        assert!(pid > 0);
    }

    #[test]
    fn test_process_args() {
        let args = Process::args();
        assert!(!args.is_empty());
    }

    #[test]
    fn test_sandbox_blocks_subprocess() {
        let sandbox = Sandbox::new(); // Denies all by default
        let process = Process::with_sandbox(sandbox)
            .with_security(ProcessSecurity::permissive()); // Disable command security for this test

        let result = process.run(RunOptions {
            cmd: vec!["echo".to_string(), "test".to_string()],
            ..Default::default()
        });

        assert!(matches!(result, Err(ProcessError::PermissionDenied(_))));
    }

    #[test]
    fn test_invalid_command() {
        let process = Process::new_permissive();
        let result = process.run(RunOptions {
            cmd: vec![],
            ..Default::default()
        });

        assert!(matches!(result, Err(ProcessError::InvalidCommand)));
    }

    #[test]
    fn test_command_output_to_js_value() {
        let output = CommandOutput {
            stdout: b"hello\n".to_vec(),
            stderr: vec![],
            success: true,
            code: Some(0),
        };

        let value = output.to_js_value();
        // The value should be an object with the expected properties
        assert!(matches!(value, Value::Object { .. }));
    }

    // ========== Process Security Tests ==========

    #[test]
    fn test_blocks_dangerous_commands() {
        let security = ProcessSecurity::new();

        // Shell commands should be blocked
        assert!(security.check_command("bash").is_err());
        assert!(security.check_command("sh").is_err());
        assert!(security.check_command("/bin/bash").is_err());

        // Network tools should be blocked
        assert!(security.check_command("curl").is_err());
        assert!(security.check_command("wget").is_err());

        // Privilege escalation should be blocked
        assert!(security.check_command("sudo").is_err());
        assert!(security.check_command("su").is_err());
    }

    #[test]
    fn test_allows_safe_commands() {
        let security = ProcessSecurity::new();

        // Common safe commands should be allowed
        assert!(security.check_command("echo").is_ok());
        assert!(security.check_command("cat").is_ok());
        assert!(security.check_command("ls").is_ok());
        assert!(security.check_command("date").is_ok());
    }

    #[test]
    fn test_whitelist_mode() {
        let security = ProcessSecurity::new()
            .whitelist_mode()
            .allow_commands(&["echo", "cat"]);

        // Whitelisted commands should be allowed
        assert!(security.check_command("echo").is_ok());
        assert!(security.check_command("cat").is_ok());

        // Non-whitelisted commands should be blocked
        assert!(security.check_command("ls").is_err());
        assert!(security.check_command("date").is_err());
    }

    #[test]
    fn test_filters_sensitive_env_vars() {
        let security = ProcessSecurity::new();

        // Sensitive vars should be filtered
        assert!(security.should_filter_env("AWS_SECRET_ACCESS_KEY"));
        assert!(security.should_filter_env("DATABASE_PASSWORD"));
        assert!(security.should_filter_env("GITHUB_TOKEN"));
        assert!(security.should_filter_env("API_KEY"));
        assert!(security.should_filter_env("MY_SECRET"));

        // Normal vars should not be filtered
        assert!(!security.should_filter_env("PATH"));
        assert!(!security.should_filter_env("HOME"));
        assert!(!security.should_filter_env("USER"));
    }

    #[test]
    fn test_custom_env_filtering() {
        let security = ProcessSecurity::new()
            .filter_env("CUSTOM_VAR")
            .filter_env_prefix("MY_APP_");

        assert!(security.should_filter_env("CUSTOM_VAR"));
        assert!(security.should_filter_env("MY_APP_SECRET"));
        assert!(security.should_filter_env("MY_APP_CONFIG"));

        assert!(!security.should_filter_env("OTHER_VAR"));
    }

    #[test]
    fn test_env_filtering_in_process() {
        let mut env = HashMap::default();
        env.insert("SAFE_VAR".to_string(), "safe".to_string());
        env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
        env.insert("MY_PASSWORD".to_string(), "hidden".to_string());

        let security = ProcessSecurity::new();
        let filtered = security.filter_env_vars(env);

        // Safe var should be present
        assert!(filtered.contains_key("SAFE_VAR"));

        // Sensitive vars should be removed
        assert!(!filtered.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!filtered.contains_key("MY_PASSWORD"));
    }

    #[test]
    fn test_permissive_allows_all() {
        let security = ProcessSecurity::permissive();

        // Should allow dangerous commands
        assert!(security.check_command("bash").is_ok());
        assert!(security.check_command("curl").is_ok());
        assert!(security.check_command("sudo").is_ok());

        // Should not filter env vars
        assert!(!security.should_filter_env("AWS_SECRET_ACCESS_KEY"));
    }

    #[test]
    fn test_security_with_process() {
        let security = ProcessSecurity::new()
            .whitelist_mode()
            .allow_command("echo");

        let process = Process::new_permissive()
            .with_security(security);

        // echo should work
        let result = process.run(RunOptions {
            cmd: vec!["echo".to_string(), "test".to_string()],
            stdout: StdioMode::Piped,
            ..Default::default()
        });
        assert!(result.is_ok());

        // ls should be blocked
        let result = process.run(RunOptions {
            cmd: vec!["ls".to_string()],
            ..Default::default()
        });
        assert!(result.is_err());
    }
}

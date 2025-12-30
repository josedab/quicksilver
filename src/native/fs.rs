//! File System API
//!
//! Deno-style file system API with capability-based security.
//!
//! # Example
//! ```text
//! // Read a file
//! const content = await Deno.readTextFile("./data.txt");
//!
//! // Write a file
//! await Deno.writeTextFile("./output.txt", "Hello, World!");
//!
//! // List directory
//! for await (const entry of Deno.readDir("./")) {
//!   console.log(entry.name, entry.isFile);
//! }
//! ```

use crate::runtime::Value;
use crate::security::{Capability, Sandbox};
use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// File system error
#[derive(Debug, Clone)]
pub enum FsError {
    PermissionDenied(String),
    NotFound(String),
    IoError(String),
    InvalidPath(String),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied(path) => write!(f, "permission denied: {}", path),
            Self::NotFound(path) => write!(f, "not found: {}", path),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::InvalidPath(path) => write!(f, "invalid path: {}", path),
        }
    }
}

impl std::error::Error for FsError {}

pub type FsResult<T> = Result<T, FsError>;

/// File information
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub path: PathBuf,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub readonly: bool,
}

impl FileInfo {
    pub fn from_path(path: &Path) -> FsResult<Self> {
        let metadata = fs::metadata(path)
            .map_err(|e| FsError::IoError(e.to_string()))?;

        Ok(Self {
            name: path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            path: path.to_path_buf(),
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            readonly: metadata.permissions().readonly(),
        })
    }

    /// Convert to JavaScript object
    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        props.insert("name".to_string(), Value::String(self.name.clone()));
        props.insert("path".to_string(), Value::String(self.path.to_string_lossy().to_string()));
        props.insert("isFile".to_string(), Value::Boolean(self.is_file));
        props.insert("isDirectory".to_string(), Value::Boolean(self.is_directory));
        props.insert("isSymlink".to_string(), Value::Boolean(self.is_symlink));
        props.insert("size".to_string(), Value::Number(self.size as f64));
        props.insert("readonly".to_string(), Value::Boolean(self.readonly));
        Value::new_object_with_properties(props)
    }
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
}

impl DirEntry {
    pub fn to_js_value(&self) -> Value {
        let mut props = HashMap::default();
        props.insert("name".to_string(), Value::String(self.name.clone()));
        props.insert("isFile".to_string(), Value::Boolean(self.is_file));
        props.insert("isDirectory".to_string(), Value::Boolean(self.is_directory));
        props.insert("isSymlink".to_string(), Value::Boolean(self.is_symlink));
        Value::new_object_with_properties(props)
    }
}

/// File system API with security
pub struct FileSystem {
    sandbox: Option<Sandbox>,
    cwd: PathBuf,
    /// Allowed path prefixes - if set, all file operations must be within these directories
    allowed_prefixes: Vec<PathBuf>,
}

impl FileSystem {
    pub fn new() -> Self {
        Self {
            sandbox: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            allowed_prefixes: Vec::new(),
        }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            allowed_prefixes: Vec::new(),
        }
    }

    /// Create a new FileSystem with restricted path access
    pub fn with_allowed_prefixes(prefixes: Vec<PathBuf>) -> Self {
        // Canonicalize all prefixes upfront
        let allowed_prefixes = prefixes
            .into_iter()
            .filter_map(|p| fs::canonicalize(&p).ok())
            .collect();
        Self {
            sandbox: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            allowed_prefixes,
        }
    }

    /// Add an allowed path prefix (will be canonicalized)
    pub fn add_allowed_prefix(&mut self, prefix: PathBuf) -> FsResult<()> {
        let canonical = fs::canonicalize(&prefix)
            .map_err(|_| FsError::InvalidPath(prefix.to_string_lossy().to_string()))?;
        self.allowed_prefixes.push(canonical);
        Ok(())
    }

    /// Check read permission
    fn check_read(&self, path: &Path) -> FsResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            use crate::security::{PathPattern, PermissionState};
            let capability = Capability::FileRead(PathPattern::Exact(path.to_path_buf()));
            if sandbox.check(&capability) != PermissionState::Granted {
                return Err(FsError::PermissionDenied(path.to_string_lossy().to_string()));
            }
        }
        Ok(())
    }

    /// Check write permission
    fn check_write(&self, path: &Path) -> FsResult<()> {
        if let Some(ref sandbox) = self.sandbox {
            use crate::security::{PathPattern, PermissionState};
            let capability = Capability::FileWrite(PathPattern::Exact(path.to_path_buf()));
            if sandbox.check(&capability) != PermissionState::Granted {
                return Err(FsError::PermissionDenied(path.to_string_lossy().to_string()));
            }
        }
        Ok(())
    }

    /// Resolve a path relative to cwd with security validation
    ///
    /// This method:
    /// 1. Joins the path with cwd if relative
    /// 2. Canonicalizes the path (resolving symlinks and ..)
    /// 3. Validates the path is within allowed prefixes (if any are set)
    ///
    /// This prevents path traversal attacks like "../../../etc/passwd"
    fn resolve(&self, path: &str) -> FsResult<PathBuf> {
        let path = Path::new(path);
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };

        // Canonicalize the path to resolve symlinks and .. components
        // This is the key security fix - we resolve the REAL path before checking
        let canonical = self.canonicalize_path(&joined)?;

        // If allowed prefixes are set, validate the path is within them
        if !self.allowed_prefixes.is_empty() {
            self.validate_path_prefix(&canonical)?;
        }

        Ok(canonical)
    }

    /// Canonicalize a path, handling the case where the file doesn't exist yet
    ///
    /// For non-existent paths (e.g., when writing a new file), we canonicalize
    /// the parent directory and then append the filename.
    fn canonicalize_path(&self, path: &Path) -> FsResult<PathBuf> {
        // First try direct canonicalization (works if path exists)
        if let Ok(canonical) = fs::canonicalize(path) {
            return Ok(canonical);
        }

        // Path doesn't exist - canonicalize parent and append filename
        // This is safe because we're checking the real path of the parent
        let parent = path.parent().ok_or_else(|| {
            FsError::InvalidPath(path.to_string_lossy().to_string())
        })?;

        let file_name = path.file_name().ok_or_else(|| {
            FsError::InvalidPath(path.to_string_lossy().to_string())
        })?;

        // Validate filename doesn't contain path separators (defense in depth)
        let file_name_str = file_name.to_string_lossy();
        if file_name_str.contains('/') || file_name_str.contains('\\') {
            return Err(FsError::InvalidPath(format!(
                "invalid filename: {}",
                file_name_str
            )));
        }

        // Canonicalize the parent directory
        let canonical_parent = fs::canonicalize(parent).map_err(|e| {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    FsError::NotFound(parent.to_string_lossy().to_string())
                }
                _ => FsError::IoError(e.to_string()),
            }
        })?;

        Ok(canonical_parent.join(file_name))
    }

    /// Validate that a path is within the allowed prefixes
    fn validate_path_prefix(&self, canonical_path: &Path) -> FsResult<()> {
        for prefix in &self.allowed_prefixes {
            if canonical_path.starts_with(prefix) {
                return Ok(());
            }
        }
        Err(FsError::PermissionDenied(format!(
            "path '{}' is outside allowed directories",
            canonical_path.display()
        )))
    }

    /// Read a text file
    pub fn read_text_file(&self, path: &str) -> FsResult<String> {
        let path = self.resolve(path)?;
        self.check_read(&path)?;

        fs::read_to_string(&path)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => FsError::NotFound(path.to_string_lossy().to_string()),
                std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied(path.to_string_lossy().to_string()),
                _ => FsError::IoError(e.to_string()),
            })
    }

    /// Read a binary file
    pub fn read_file(&self, path: &str) -> FsResult<Vec<u8>> {
        let path = self.resolve(path)?;
        self.check_read(&path)?;

        fs::read(&path)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Write a text file
    pub fn write_text_file(&self, path: &str, content: &str) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        fs::write(&path, content)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Write a binary file
    pub fn write_file(&self, path: &str, content: &[u8]) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        fs::write(&path, content)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Append to a file
    pub fn append_file(&self, path: &str, content: &str) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| FsError::IoError(e.to_string()))?;

        file.write_all(content.as_bytes())
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Check if a path exists
    pub fn exists(&self, path: &str) -> FsResult<bool> {
        // For exists(), we need special handling since the path might not exist yet
        let path_obj = Path::new(path);
        let joined = if path_obj.is_absolute() {
            path_obj.to_path_buf()
        } else {
            self.cwd.join(path_obj)
        };

        // Try to canonicalize - if it fails, the path doesn't exist
        match self.canonicalize_path(&joined) {
            Ok(canonical) => {
                // Validate prefix if needed
                if !self.allowed_prefixes.is_empty() {
                    self.validate_path_prefix(&canonical)?;
                }
                self.check_read(&canonical)?;
                Ok(canonical.exists())
            }
            Err(FsError::NotFound(_)) => {
                // Parent doesn't exist, so the path doesn't exist
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Get file/directory info
    pub fn stat(&self, path: &str) -> FsResult<FileInfo> {
        let path = self.resolve(path)?;
        self.check_read(&path)?;
        FileInfo::from_path(&path)
    }

    /// Read directory contents
    pub fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>> {
        let path = self.resolve(path)?;
        self.check_read(&path)?;

        let entries = fs::read_dir(&path)
            .map_err(|e| FsError::IoError(e.to_string()))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| FsError::IoError(e.to_string()))?;
            let file_type = entry.file_type().map_err(|e| FsError::IoError(e.to_string()))?;

            result.push(DirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_file: file_type.is_file(),
                is_directory: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
            });
        }

        Ok(result)
    }

    /// Create a directory
    pub fn mkdir(&self, path: &str, recursive: bool) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        if recursive {
            fs::create_dir_all(&path)
        } else {
            fs::create_dir(&path)
        }
        .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Remove a file
    pub fn remove(&self, path: &str) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        if path.is_dir() {
            fs::remove_dir(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Remove a directory recursively
    pub fn remove_dir_all(&self, path: &str) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_write(&path)?;

        fs::remove_dir_all(&path)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Rename/move a file or directory
    pub fn rename(&self, from: &str, to: &str) -> FsResult<()> {
        let from = self.resolve(from)?;
        let to = self.resolve(to)?;
        self.check_read(&from)?;
        self.check_write(&to)?;

        fs::rename(&from, &to)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Copy a file
    pub fn copy(&self, from: &str, to: &str) -> FsResult<u64> {
        let from = self.resolve(from)?;
        let to = self.resolve(to)?;
        self.check_read(&from)?;
        self.check_write(&to)?;

        fs::copy(&from, &to)
            .map_err(|e| FsError::IoError(e.to_string()))
    }

    /// Get current working directory
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Change working directory
    pub fn chdir(&mut self, path: &str) -> FsResult<()> {
        let path = self.resolve(path)?;
        self.check_read(&path)?;

        if !path.is_dir() {
            return Err(FsError::NotFound(path.to_string_lossy().to_string()));
        }

        self.cwd = path;
        Ok(())
    }
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_write_text() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        let fs = FileSystem::new();
        let path_str = file_path.to_string_lossy().to_string();

        // Write
        fs.write_text_file(&path_str, "Hello, World!").unwrap();

        // Read
        let content = fs.read_text_file(&path_str).unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[test]
    fn test_exists() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("exists.txt");

        let fs = FileSystem::new();
        let path_str = file_path.to_string_lossy().to_string();

        assert!(!fs.exists(&path_str).unwrap());

        fs.write_text_file(&path_str, "test").unwrap();

        assert!(fs.exists(&path_str).unwrap());
    }

    #[test]
    fn test_read_dir() {
        let dir = tempdir().unwrap();

        // Create some files
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let fs = FileSystem::new();
        let entries = fs.read_dir(&dir.path().to_string_lossy()).unwrap();

        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|e| e.name == "a.txt" && e.is_file));
        assert!(entries.iter().any(|e| e.name == "subdir" && e.is_directory));
    }

    #[test]
    fn test_sandbox_permission() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("secret.txt");
        std::fs::write(&file_path, "secret data").unwrap();

        // Create a restrictive sandbox
        let sandbox = Sandbox::new(); // Denies all by default

        let fs = FileSystem::with_sandbox(sandbox);
        let result = fs.read_text_file(&file_path.to_string_lossy());

        assert!(matches!(result, Err(FsError::PermissionDenied(_))));
    }

    // ========== Path Traversal Security Tests ==========

    #[test]
    fn test_path_traversal_blocked() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();
        std::fs::write(allowed_dir.join("safe.txt"), "safe").unwrap();

        // Create file outside allowed directory
        let outside_file = dir.path().join("outside.txt");
        std::fs::write(&outside_file, "secret").unwrap();

        // Create FS with restricted prefix
        let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir.clone()]);

        // Accessing file within allowed dir should work
        let safe_path = allowed_dir.join("safe.txt");
        let content = fs.read_text_file(&safe_path.to_string_lossy()).unwrap();
        assert_eq!(content, "safe");

        // Path traversal attempt should be blocked
        let traversal_path = format!("{}/allowed/../outside.txt", dir.path().display());
        let result = fs.read_text_file(&traversal_path);
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));
    }

    #[test]
    fn test_path_traversal_with_dot_dot() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        let subdir = allowed_dir.join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join("file.txt"), "content").unwrap();

        // Create file outside allowed directory
        std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir.clone()]);

        // Try various traversal patterns
        let traversal_patterns = [
            format!("{}/subdir/../../secret.txt", allowed_dir.display()),
            format!("{}/../secret.txt", allowed_dir.display()),
            format!("{}/subdir/../../../secret.txt", allowed_dir.display()),
        ];

        for pattern in &traversal_patterns {
            let result = fs.read_text_file(pattern);
            assert!(
                matches!(result, Err(FsError::PermissionDenied(_))),
                "Pattern {} should be blocked",
                pattern
            );
        }
    }

    #[test]
    fn test_absolute_path_escape_blocked() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();

        // Create file outside allowed directory
        let outside_file = dir.path().join("secret.txt");
        std::fs::write(&outside_file, "secret").unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir]);

        // Absolute path to file outside allowed dir should be blocked
        let result = fs.read_text_file(&outside_file.to_string_lossy());
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));
    }

    #[test]
    fn test_write_path_traversal_blocked() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir.clone()]);

        // Try to write outside allowed directory
        let outside_path = format!("{}/allowed/../evil.txt", dir.path().display());
        let result = fs.write_text_file(&outside_path, "evil content");
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));

        // Verify file wasn't created
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn test_mkdir_path_traversal_blocked() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir.clone()]);

        // Try to create directory outside allowed area
        let outside_path = format!("{}/allowed/../evil_dir", dir.path().display());
        let result = fs.mkdir(&outside_path, false);
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));

        // Verify directory wasn't created
        assert!(!dir.path().join("evil_dir").exists());
    }

    #[test]
    fn test_symlink_attack_protection() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();

        // Create a file outside allowed dir
        let secret_file = dir.path().join("secret.txt");
        std::fs::write(&secret_file, "secret data").unwrap();

        // Create a symlink inside allowed dir pointing outside
        #[cfg(unix)]
        {
            let symlink_path = allowed_dir.join("symlink.txt");
            std::os::unix::fs::symlink(&secret_file, &symlink_path).unwrap();

            let fs = FileSystem::with_allowed_prefixes(vec![allowed_dir.clone()]);

            // Accessing the symlink should be blocked because it resolves outside
            let result = fs.read_text_file(&symlink_path.to_string_lossy());
            assert!(
                matches!(result, Err(FsError::PermissionDenied(_))),
                "Symlink attack should be blocked"
            );
        }
    }

    #[test]
    fn test_multiple_allowed_prefixes() {
        let dir = tempdir().unwrap();
        let allowed1 = dir.path().join("allowed1");
        let allowed2 = dir.path().join("allowed2");
        std::fs::create_dir(&allowed1).unwrap();
        std::fs::create_dir(&allowed2).unwrap();
        std::fs::write(allowed1.join("file1.txt"), "content1").unwrap();
        std::fs::write(allowed2.join("file2.txt"), "content2").unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![allowed1.clone(), allowed2.clone()]);

        // Both allowed directories should be accessible
        assert_eq!(
            fs.read_text_file(&allowed1.join("file1.txt").to_string_lossy()).unwrap(),
            "content1"
        );
        assert_eq!(
            fs.read_text_file(&allowed2.join("file2.txt").to_string_lossy()).unwrap(),
            "content2"
        );

        // Files outside both should be blocked
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        let result = fs.read_text_file(&outside.to_string_lossy());
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));
    }

    #[test]
    fn test_add_allowed_prefix() {
        let dir = tempdir().unwrap();
        let allowed_dir = dir.path().join("allowed");
        std::fs::create_dir(&allowed_dir).unwrap();
        std::fs::write(allowed_dir.join("file.txt"), "content").unwrap();

        let mut fs = FileSystem::new();

        // Initially no restrictions
        let file_path = allowed_dir.join("file.txt");
        assert!(fs.read_text_file(&file_path.to_string_lossy()).is_ok());

        // Add prefix restriction
        fs.add_allowed_prefix(allowed_dir.clone()).unwrap();

        // File in allowed dir still works
        assert!(fs.read_text_file(&file_path.to_string_lossy()).is_ok());

        // File outside is now blocked
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        let result = fs.read_text_file(&outside.to_string_lossy());
        assert!(matches!(result, Err(FsError::PermissionDenied(_))));
    }

    #[test]
    fn test_canonicalization_preserves_valid_paths() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("file.txt"), "content").unwrap();

        let fs = FileSystem::with_allowed_prefixes(vec![dir.path().to_path_buf()]);

        // Relative path with .. that stays within allowed area should work
        let path = format!("{}/subdir/../subdir/file.txt", dir.path().display());
        let content = fs.read_text_file(&path).unwrap();
        assert_eq!(content, "content");
    }
}

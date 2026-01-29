//! Bytecode caching for faster subsequent loads
//!
//! This module provides a filesystem-based cache for compiled bytecode.
//! When a JavaScript file is compiled, the bytecode is cached based on
//! a hash of the source code. On subsequent runs, if the source hasn't
//! changed, the cached bytecode is loaded instead of recompiling.

use crate::bytecode::Chunk;
use crate::snapshot::{Snapshot, SnapshotMetadata};
use crate::Error;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default cache directory name
const CACHE_DIR: &str = ".quicksilver_cache";

/// Cache file extension
const CACHE_EXT: &str = "qsc";

/// Bytecode cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Directory where cache files are stored
    pub cache_dir: PathBuf,
    /// Whether caching is enabled
    pub enabled: bool,
    /// Maximum cache size in bytes (0 = unlimited)
    pub max_size: usize,
    /// Maximum age of cache entries in seconds (0 = unlimited)
    pub max_age: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        let cache_dir = home_dir()
            .map(|h| h.join(CACHE_DIR))
            .unwrap_or_else(|| PathBuf::from(CACHE_DIR));

        Self {
            cache_dir,
            enabled: true,
            max_size: 100 * 1024 * 1024, // 100 MB
            max_age: 7 * 24 * 60 * 60,   // 7 days
        }
    }
}

/// Get home directory
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Bytecode cache manager
pub struct BytecodeCache {
    config: CacheConfig,
}

impl BytecodeCache {
    /// Create a new cache with default configuration
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Create a new cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        Self { config }
    }

    /// Ensure the cache directory exists
    fn ensure_cache_dir(&self) -> Result<(), Error> {
        if !self.config.cache_dir.exists() {
            fs::create_dir_all(&self.config.cache_dir)
                .map_err(|e| Error::InternalError(format!("Failed to create cache directory: {}", e)))?;
        }
        Ok(())
    }

    /// Generate a cache key from source code and optional filename
    fn cache_key(&self, source: &str, filename: Option<&str>) -> String {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        if let Some(name) = filename {
            name.hash(&mut hasher);
        }
        // Include runtime version in the hash for cache invalidation on updates
        crate::VERSION.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Get the cache file path for a given key
    fn cache_path(&self, key: &str) -> PathBuf {
        self.config.cache_dir.join(format!("{}.{}", key, CACHE_EXT))
    }

    /// Check if a cached entry is valid (not expired)
    fn is_valid_entry(&self, path: &Path) -> bool {
        if self.config.max_age == 0 {
            return true; // No age limit
        }

        if let Ok(metadata) = fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                    return elapsed.as_secs() < self.config.max_age;
                }
            }
        }
        false
    }

    /// Try to load bytecode from cache
    pub fn get(&self, source: &str, filename: Option<&str>) -> Option<Chunk> {
        if !self.config.enabled {
            return None;
        }

        let key = self.cache_key(source, filename);
        let path = self.cache_path(&key);

        if !path.exists() || !self.is_valid_entry(&path) {
            return None;
        }

        match Snapshot::load(&path) {
            Ok(snapshot) => {
                match snapshot.to_chunk() {
                    Ok(chunk) => Some(chunk),
                    Err(_) => {
                        // Cache entry is corrupted, remove it
                        let _ = fs::remove_file(&path);
                        None
                    }
                }
            }
            Err(_) => {
                // Failed to load, remove corrupted entry
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    /// Store bytecode in cache
    pub fn put(&self, source: &str, filename: Option<&str>, chunk: &Chunk) -> Result<(), Error> {
        if !self.config.enabled {
            return Ok(());
        }

        self.ensure_cache_dir()?;

        let key = self.cache_key(source, filename);
        let path = self.cache_path(&key);

        let mut snapshot = Snapshot::from_chunk(chunk, Some(source));
        snapshot.metadata = SnapshotMetadata {
            filename: filename.unwrap_or("<anonymous>").to_string(),
            created_at: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            runtime_version: crate::VERSION.to_string(),
            custom: Default::default(),
        };

        snapshot.save(&path)?;

        // Cleanup old entries if cache is too large
        self.maybe_cleanup()?;

        Ok(())
    }

    /// Compile source with caching
    ///
    /// This is the main entry point for cached compilation. It will:
    /// 1. Check if a valid cache entry exists
    /// 2. If so, return the cached bytecode
    /// 3. If not, compile the source and cache the result
    pub fn compile(&self, source: &str, filename: Option<&str>) -> Result<Chunk, Error> {
        // Try to get from cache
        if let Some(chunk) = self.get(source, filename) {
            return Ok(chunk);
        }

        // Compile the source
        let chunk = if let Some(file) = filename {
            crate::bytecode::compile_with_source_file(source, file)?
        } else {
            crate::bytecode::compile(source)?
        };

        // Store in cache (ignore errors, caching is optional)
        let _ = self.put(source, filename, &chunk);

        Ok(chunk)
    }

    /// Clear the entire cache
    pub fn clear(&self) -> Result<(), Error> {
        if self.config.cache_dir.exists() {
            for entry in (fs::read_dir(&self.config.cache_dir)
                .map_err(|e| Error::InternalError(format!("Failed to read cache directory: {}", e)))?).flatten()
            {
                let path = entry.path();
                if path.extension().map(|e| e == CACHE_EXT).unwrap_or(false) {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let mut stats = CacheStats::default();

        if !self.config.cache_dir.exists() {
            return stats;
        }

        if let Ok(entries) = fs::read_dir(&self.config.cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == CACHE_EXT).unwrap_or(false) {
                    stats.entry_count += 1;
                    if let Ok(metadata) = fs::metadata(&path) {
                        stats.total_size += metadata.len() as usize;
                    }
                }
            }
        }

        stats
    }

    /// Remove old or excess entries if needed
    fn maybe_cleanup(&self) -> Result<(), Error> {
        if self.config.max_size == 0 {
            return Ok(());
        }

        let stats = self.stats();
        if stats.total_size <= self.config.max_size {
            return Ok(());
        }

        // Collect entries with their modification times
        let mut entries: Vec<(PathBuf, SystemTime)> = Vec::new();
        if let Ok(dir_entries) = fs::read_dir(&self.config.cache_dir) {
            for entry in dir_entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == CACHE_EXT).unwrap_or(false) {
                    if let Ok(metadata) = fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            entries.push((path, modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
        entries.sort_by_key(|(_, time)| *time);

        // Remove oldest entries until we're under the limit
        let mut current_size = stats.total_size;
        for (path, _) in entries {
            if current_size <= self.config.max_size / 2 {
                break; // Leave some headroom
            }
            if let Ok(metadata) = fs::metadata(&path) {
                current_size = current_size.saturating_sub(metadata.len() as usize);
                let _ = fs::remove_file(&path);
            }
        }

        Ok(())
    }

    /// Remove a specific entry from cache
    pub fn invalidate(&self, source: &str, filename: Option<&str>) -> Result<(), Error> {
        let key = self.cache_key(source, filename);
        let path = self.cache_path(&key);
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| Error::InternalError(format!("Failed to remove cache entry: {}", e)))?;
        }
        Ok(())
    }
}

impl Default for BytecodeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    /// Number of entries in the cache
    pub entry_count: usize,
    /// Total size of all cached entries in bytes
    pub total_size: usize,
}

/// Global cache instance for convenience
static CACHE: std::sync::OnceLock<BytecodeCache> = std::sync::OnceLock::new();

/// Get the global bytecode cache instance
pub fn global_cache() -> &'static BytecodeCache {
    CACHE.get_or_init(BytecodeCache::new)
}

/// Compile source with caching (using global cache)
pub fn compile_cached(source: &str, filename: Option<&str>) -> Result<Chunk, Error> {
    global_cache().compile(source, filename)
}

/// Clear the global cache
pub fn clear_cache() -> Result<(), Error> {
    global_cache().clear()
}

/// Get global cache statistics
pub fn cache_stats() -> CacheStats {
    global_cache().stats()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn test_cache_with_unique_dir(test_name: &str) -> BytecodeCache {
        let cache_dir = temp_dir().join(format!("quicksilver_test_cache_{}", test_name));
        // Clean up any previous test runs
        let _ = fs::remove_dir_all(&cache_dir);
        BytecodeCache::with_config(CacheConfig {
            cache_dir,
            enabled: true,
            max_size: 10 * 1024 * 1024,
            max_age: 3600,
        })
    }

    #[test]
    fn test_cache_key_consistency() {
        let cache = test_cache_with_unique_dir("key_consistency");
        let source = "let x = 1 + 2;";

        let key1 = cache.cache_key(source, Some("test.js"));
        let key2 = cache.cache_key(source, Some("test.js"));
        assert_eq!(key1, key2);

        // Different source should have different key
        let key3 = cache.cache_key("let x = 3;", Some("test.js"));
        assert_ne!(key1, key3);

        // Different filename should have different key
        let key4 = cache.cache_key(source, Some("other.js"));
        assert_ne!(key1, key4);
    }

    #[test]
    fn test_cache_roundtrip() {
        let cache = test_cache_with_unique_dir("roundtrip");

        let source = "let x = 1 + 2;";
        let filename = Some("test_roundtrip.js");

        // First compile (cache miss)
        let chunk1 = cache.compile(source, filename).unwrap();

        // Second compile (cache hit)
        let chunk2 = cache.compile(source, filename).unwrap();

        // Results should be equivalent
        assert_eq!(chunk1.code.len(), chunk2.code.len());
        assert_eq!(chunk1.constants.len(), chunk2.constants.len());

        let _ = cache.clear();
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = test_cache_with_unique_dir("invalidation");

        let source = "let x = 42;";
        let filename = Some("test_invalidate.js");

        // Compile and cache
        let _ = cache.compile(source, filename).unwrap();
        assert!(cache.get(source, filename).is_some());

        // Invalidate
        cache.invalidate(source, filename).unwrap();
        assert!(cache.get(source, filename).is_none());

        let _ = cache.clear();
    }

    #[test]
    fn test_cache_stats() {
        let cache = test_cache_with_unique_dir("stats");

        let initial_stats = cache.stats();
        assert_eq!(initial_stats.entry_count, 0);

        // Add an entry
        let _ = cache.compile("let x = 1;", Some("stats1.js"));
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 1);
        assert!(stats.total_size > 0);

        let _ = cache.clear();
    }
}

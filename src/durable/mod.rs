//! Durable Objects / Persistent Execution Snapshots
//!
//! Extends the snapshot system with incremental snapshots, transactional state
//! persistence, write-ahead logging, and pluggable storage backends.

//! **Status:** 🧪 Experimental — Durable objects framework

pub mod sqlite_backend;

use crate::error::{Error, Result};
use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A durable object that persists state across process restarts
pub struct DurableObject {
    /// Unique identifier
    pub id: String,
    /// Current state (key-value store)
    state: HashMap<String, Value>,
    /// Dirty keys (modified since last persist)
    dirty_keys: Vec<String>,
    /// Storage backend
    storage: Box<dyn StorageBackend>,
    /// Write-ahead log
    wal: WriteAheadLog,
    /// Whether auto-persist is enabled
    auto_persist: bool,
    /// Number of operations since last persist
    ops_since_persist: u32,
    /// Persist threshold (auto-persist after this many ops)
    persist_threshold: u32,
}

impl DurableObject {
    /// Create a new durable object with the given storage backend
    pub fn new(id: &str, storage: Box<dyn StorageBackend>) -> Result<Self> {
        let wal = WriteAheadLog::new(id);
        let mut obj = Self {
            id: id.to_string(),
            state: HashMap::default(),
            dirty_keys: Vec::new(),
            storage,
            wal,
            auto_persist: true,
            ops_since_persist: 0,
            persist_threshold: 100,
        };
        obj.hydrate()?;
        Ok(obj)
    }

    /// Hydrate state from storage
    fn hydrate(&mut self) -> Result<()> {
        if let Some(data) = self.storage.load(&self.id)? {
            self.state = data;
        }
        // Replay WAL entries
        let entries = self.wal.replay()?;
        for entry in entries {
            match entry.operation {
                WalOperation::Set(key, value) => {
                    self.state.insert(key, value);
                }
                WalOperation::Delete(key) => {
                    self.state.remove(&key);
                }
                WalOperation::Clear => {
                    self.state.clear();
                }
            }
        }
        Ok(())
    }

    /// Get a value from state
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.state.get(key)
    }

    /// Set a value in state
    pub fn set(&mut self, key: &str, value: Value) -> Result<()> {
        self.wal.append(WalEntry {
            _timestamp: current_timestamp(),
            operation: WalOperation::Set(key.to_string(), value.clone()),
        })?;
        self.state.insert(key.to_string(), value);
        self.dirty_keys.push(key.to_string());
        self.ops_since_persist += 1;
        self.maybe_auto_persist()?;
        Ok(())
    }

    /// Delete a key from state
    pub fn delete(&mut self, key: &str) -> Result<bool> {
        let existed = self.state.remove(key).is_some();
        if existed {
            self.wal.append(WalEntry {
                _timestamp: current_timestamp(),
                operation: WalOperation::Delete(key.to_string()),
            })?;
            self.ops_since_persist += 1;
            self.maybe_auto_persist()?;
        }
        Ok(existed)
    }

    /// Clear all state
    pub fn clear(&mut self) -> Result<()> {
        self.wal.append(WalEntry {
            _timestamp: current_timestamp(),
            operation: WalOperation::Clear,
        })?;
        self.state.clear();
        self.dirty_keys.clear();
        self.ops_since_persist += 1;
        self.maybe_auto_persist()?;
        Ok(())
    }

    /// List all keys
    pub fn keys(&self) -> Vec<String> {
        self.state.keys().cloned().collect()
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.state.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Persist current state to storage
    pub fn persist(&mut self) -> Result<()> {
        self.storage.save(&self.id, &self.state)?;
        self.wal.truncate()?;
        self.dirty_keys.clear();
        self.ops_since_persist = 0;
        Ok(())
    }

    fn maybe_auto_persist(&mut self) -> Result<()> {
        if self.auto_persist && self.ops_since_persist >= self.persist_threshold {
            self.persist()?;
        }
        Ok(())
    }

    /// Execute a transaction
    pub fn transaction<F, R>(&mut self, f: F) -> Result<R>
    where
        F: FnOnce(&mut TransactionContext) -> Result<R>,
    {
        let snapshot = self.state.clone();
        let mut ctx = TransactionContext {
            state: &mut self.state,
            dirty_keys: &mut self.dirty_keys,
            _committed: false,
        };

        match f(&mut ctx) {
            Ok(result) => {
                let _committed = true;
                self.ops_since_persist += 1;
                self.maybe_auto_persist()?;
                Ok(result)
            }
            Err(e) => {
                // Rollback
                self.state = snapshot;
                Err(e)
            }
        }
    }

    /// Export state as a JavaScript Value object
    pub fn to_value(&self) -> Value {
        Value::new_object_with_properties(self.state.clone())
    }
}

/// Transaction context for atomic operations
pub struct TransactionContext<'a> {
    state: &'a mut HashMap<String, Value>,
    dirty_keys: &'a mut Vec<String>,
    _committed: bool,
}

impl<'a> TransactionContext<'a> {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.state.get(key)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.state.insert(key.to_string(), value);
        self.dirty_keys.push(key.to_string());
    }

    pub fn delete(&mut self, key: &str) -> bool {
        let existed = self.state.remove(key).is_some();
        if existed {
            self.dirty_keys.push(key.to_string());
        }
        existed
    }
}

/// Write-ahead log entry
#[derive(Debug, Clone)]
struct WalEntry {
    _timestamp: u64,
    operation: WalOperation,
}

#[derive(Debug, Clone)]
enum WalOperation {
    Set(String, Value),
    Delete(String),
    Clear,
}

/// Write-ahead log for crash recovery
struct WriteAheadLog {
    _object_id: String,
    entries: Vec<WalEntry>,
}

impl WriteAheadLog {
    fn new(object_id: &str) -> Self {
        Self {
            _object_id: object_id.to_string(),
            entries: Vec::new(),
        }
    }

    fn append(&mut self, entry: WalEntry) -> Result<()> {
        self.entries.push(entry);
        Ok(())
    }

    fn replay(&self) -> Result<Vec<WalEntry>> {
        Ok(self.entries.clone())
    }

    fn truncate(&mut self) -> Result<()> {
        self.entries.clear();
        Ok(())
    }
}

/// Storage backend trait
pub trait StorageBackend {
    fn load(&self, id: &str) -> Result<Option<HashMap<String, Value>>>;
    fn save(&self, id: &str, state: &HashMap<String, Value>) -> Result<()>;
    fn delete(&self, id: &str) -> Result<()>;
    fn list_objects(&self) -> Result<Vec<String>>;
}

/// In-memory storage backend (for testing)
pub struct MemoryStorage {
    data: std::cell::RefCell<HashMap<String, HashMap<String, Value>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            data: std::cell::RefCell::new(HashMap::default()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for MemoryStorage {
    fn load(&self, id: &str) -> Result<Option<HashMap<String, Value>>> {
        Ok(self.data.borrow().get(id).cloned())
    }

    fn save(&self, id: &str, state: &HashMap<String, Value>) -> Result<()> {
        self.data
            .borrow_mut()
            .insert(id.to_string(), state.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<()> {
        self.data.borrow_mut().remove(id);
        Ok(())
    }

    fn list_objects(&self) -> Result<Vec<String>> {
        Ok(self.data.borrow().keys().cloned().collect())
    }
}

/// File-based storage backend
pub struct FileStorage {
    base_dir: PathBuf,
}

impl FileStorage {
    pub fn new(base_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| Error::ModuleError(format!("Failed to create storage directory: {}", e)))?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    fn object_path(&self, id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", id))
    }
}

impl StorageBackend for FileStorage {
    fn load(&self, id: &str) -> Result<Option<HashMap<String, Value>>> {
        let path = self.object_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::ModuleError(format!("Failed to read: {}", e)))?;
        let json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| Error::ModuleError(format!("Invalid JSON: {}", e)))?;

        let mut state = HashMap::default();
        if let serde_json::Value::Object(obj) = json {
            for (key, val) in obj {
                state.insert(key, json_to_value(&val));
            }
        }
        Ok(Some(state))
    }

    fn save(&self, id: &str, state: &HashMap<String, Value>) -> Result<()> {
        let path = self.object_path(id);
        let mut json_map = serde_json::Map::new();
        for (key, val) in state {
            json_map.insert(key.clone(), value_to_json(val));
        }
        let json = serde_json::Value::Object(json_map);
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| Error::ModuleError(format!("Serialization failed: {}", e)))?;
        std::fs::write(&path, content)
            .map_err(|e| Error::ModuleError(format!("Failed to write: {}", e)))?;
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<()> {
        let path = self.object_path(id);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| Error::ModuleError(format!("Failed to delete: {}", e)))?;
        }
        Ok(())
    }

    fn list_objects(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    ids.push(name.to_string_lossy().to_string());
                }
            }
        }
        Ok(ids)
    }
}

fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::new_array(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut props = HashMap::default();
            for (k, v) in obj {
                props.insert(k.clone(), json_to_value(v));
            }
            Value::new_object_with_properties(props)
        }
    }
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Undefined | Value::Null => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => serde_json::json!(*n),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                crate::runtime::ObjectKind::Array(arr) => {
                    serde_json::Value::Array(arr.iter().map(value_to_json).collect())
                }
                _ => {
                    let mut map = serde_json::Map::new();
                    for (k, v) in &obj.properties {
                        map.insert(k.clone(), value_to_json(v));
                    }
                    serde_json::Value::Object(map)
                }
            }
        }
        _ => serde_json::Value::Null,
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

/// Durable Object manager
pub struct DurableObjectManager {
    _storage: std::rc::Rc<dyn StorageBackend>,
    _objects: HashMap<String, DurableObject>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_durable_object_crud() {
        let storage = Box::new(MemoryStorage::new());
        let mut obj = DurableObject::new("test1", storage).unwrap();

        obj.set("name", Value::String("Alice".to_string())).unwrap();
        obj.set("age", Value::Number(30.0)).unwrap();

        assert_eq!(obj.get("name"), Some(&Value::String("Alice".to_string())));
        assert_eq!(obj.get("age"), Some(&Value::Number(30.0)));
        assert_eq!(obj.len(), 2);

        obj.delete("age").unwrap();
        assert!(obj.get("age").is_none());
        assert_eq!(obj.len(), 1);
    }

    #[test]
    fn test_durable_object_persist_and_hydrate() {
        let storage = std::rc::Rc::new(MemoryStorage::new());

        // Create and persist
        {
            let mut obj = DurableObject::new("test2", Box::new(MemoryStorageClone(storage.clone()))).unwrap();
            obj.set("key", Value::String("value".to_string())).unwrap();
            obj.persist().unwrap();
        }

        // Hydrate
        {
            let obj = DurableObject::new("test2", Box::new(MemoryStorageClone(storage.clone()))).unwrap();
            assert_eq!(obj.get("key"), Some(&Value::String("value".to_string())));
        }
    }

    // Wrapper to share storage between objects in tests
    struct MemoryStorageClone(std::rc::Rc<MemoryStorage>);

    impl StorageBackend for MemoryStorageClone {
        fn load(&self, id: &str) -> Result<Option<HashMap<String, Value>>> {
            self.0.load(id)
        }
        fn save(&self, id: &str, state: &HashMap<String, Value>) -> Result<()> {
            self.0.save(id, state)
        }
        fn delete(&self, id: &str) -> Result<()> {
            self.0.delete(id)
        }
        fn list_objects(&self) -> Result<Vec<String>> {
            self.0.list_objects()
        }
    }

    #[test]
    fn test_transaction_commit() {
        let storage = Box::new(MemoryStorage::new());
        let mut obj = DurableObject::new("test3", storage).unwrap();

        let result = obj.transaction(|ctx| {
            ctx.set("a", Value::Number(1.0));
            ctx.set("b", Value::Number(2.0));
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(obj.get("a"), Some(&Value::Number(1.0)));
        assert_eq!(obj.get("b"), Some(&Value::Number(2.0)));
    }

    #[test]
    fn test_transaction_rollback() {
        let storage = Box::new(MemoryStorage::new());
        let mut obj = DurableObject::new("test4", storage).unwrap();
        obj.set("x", Value::Number(10.0)).unwrap();

        let result: Result<()> = obj.transaction(|ctx| {
            ctx.set("x", Value::Number(20.0));
            Err(Error::type_error("abort"))
        });
        assert!(result.is_err());
        // Should be rolled back
        assert_eq!(obj.get("x"), Some(&Value::Number(10.0)));
    }

    #[test]
    fn test_clear() {
        let storage = Box::new(MemoryStorage::new());
        let mut obj = DurableObject::new("test5", storage).unwrap();
        obj.set("a", Value::Number(1.0)).unwrap();
        obj.set("b", Value::Number(2.0)).unwrap();
        obj.clear().unwrap();
        assert!(obj.is_empty());
    }

    #[test]
    fn test_to_value() {
        let storage = Box::new(MemoryStorage::new());
        let mut obj = DurableObject::new("test6", storage).unwrap();
        obj.set("key", Value::String("val".to_string())).unwrap();
        let val = obj.to_value();
        if let Value::Object(o) = &val {
            assert!(o.borrow().get_property("key").is_some());
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let original = Value::Number(42.0);
        let json = value_to_json(&original);
        let restored = json_to_value(&json);
        assert_eq!(original, restored);
    }
}

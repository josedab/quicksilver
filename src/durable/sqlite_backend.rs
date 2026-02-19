//! Persisted Durable Objects with file-based SQL-like storage backend
//!
//! Provides a production-grade storage backend using JSON files organized by object ID,
//! WAL-style journaling for crash recovery, atomic writes via rename, and enhanced
//! lifecycle management including hibernation, eviction, transactions, and alarms.

use crate::error::{Error, Result};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// SqliteStorage — file-based persistent storage with WAL journaling
// ---------------------------------------------------------------------------

/// A WAL entry recording a single mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    pub timestamp: u64,
    pub op: WalOp,
}

/// Individual WAL operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOp {
    Set(String, String),
    Delete(String),
    Clear,
}

/// File-based storage that mimics SQLite-style persistence.
///
/// Data is kept in an in-memory cache and flushed to a JSON file per object.
/// A write-ahead log (`<id>.wal.json`) is used for crash recovery.
/// Writes are atomic: data is written to a temporary file and renamed.
pub struct SqliteStorage {
    path: String,
    data: HashMap<String, String>,
    wal: Vec<WalEntry>,
    dirty: bool,
}

impl SqliteStorage {
    /// Open (or create) storage rooted at `path`.
    pub fn new(path: &str) -> Result<Self> {
        std::fs::create_dir_all(path)
            .map_err(|e| Error::ModuleError(format!("Failed to create storage dir: {e}")))?;
        Ok(Self {
            path: path.to_string(),
            data: HashMap::default(),
            wal: Vec::new(),
            dirty: false,
        })
    }

    fn data_path(&self, id: &str) -> PathBuf {
        Path::new(&self.path).join(format!("{id}.json"))
    }

    fn wal_path(&self, id: &str) -> PathBuf {
        Path::new(&self.path).join(format!("{id}.wal.json"))
    }

    fn tmp_path(&self, id: &str) -> PathBuf {
        Path::new(&self.path).join(format!("{id}.tmp.json"))
    }

    /// Load state for `id` from disk, replaying any pending WAL entries.
    pub fn load(&mut self, id: &str) -> Result<()> {
        let data_path = self.data_path(id);
        if data_path.exists() {
            let content = std::fs::read_to_string(&data_path)
                .map_err(|e| Error::ModuleError(format!("Failed to read data: {e}")))?;
            let map: std::collections::HashMap<String, String> =
                serde_json::from_str(&content)
                    .map_err(|e| Error::ModuleError(format!("Invalid JSON data: {e}")))?;
            self.data = map.into_iter().collect();
        }

        // Replay WAL
        let wal_path = self.wal_path(id);
        if wal_path.exists() {
            let content = std::fs::read_to_string(&wal_path)
                .map_err(|e| Error::ModuleError(format!("Failed to read WAL: {e}")))?;
            let entries: Vec<WalEntry> = serde_json::from_str(&content)
                .map_err(|e| Error::ModuleError(format!("Invalid WAL JSON: {e}")))?;
            for entry in &entries {
                match &entry.op {
                    WalOp::Set(k, v) => {
                        self.data.insert(k.clone(), v.clone());
                    }
                    WalOp::Delete(k) => {
                        self.data.remove(k);
                    }
                    WalOp::Clear => {
                        self.data.clear();
                    }
                }
            }
            self.wal = entries;
            self.dirty = true;
        }
        Ok(())
    }

    /// Persist current in-memory state to disk atomically and truncate the WAL.
    pub fn save(&mut self, id: &str) -> Result<()> {
        let ordered: std::collections::BTreeMap<&String, &String> = self.data.iter().collect();
        let json = serde_json::to_string_pretty(&ordered)
            .map_err(|e| Error::ModuleError(format!("Serialization failed: {e}")))?;

        // Atomic write: write to tmp then rename
        let tmp = self.tmp_path(id);
        let data = self.data_path(id);
        std::fs::write(&tmp, &json)
            .map_err(|e| Error::ModuleError(format!("Failed to write tmp: {e}")))?;
        std::fs::rename(&tmp, &data)
            .map_err(|e| Error::ModuleError(format!("Failed to rename: {e}")))?;

        // Truncate WAL
        let wal = self.wal_path(id);
        if wal.exists() {
            let _ = std::fs::remove_file(&wal);
        }
        self.wal.clear();
        self.dirty = false;
        Ok(())
    }

    /// Append a WAL entry and flush the WAL file to disk.
    fn append_wal(&mut self, id: &str, op: WalOp) -> Result<()> {
        let entry = WalEntry {
            timestamp: current_timestamp(),
            op,
        };
        self.wal.push(entry);
        let json = serde_json::to_string(&self.wal)
            .map_err(|e| Error::ModuleError(format!("WAL serialization failed: {e}")))?;
        std::fs::write(self.wal_path(id), json)
            .map_err(|e| Error::ModuleError(format!("WAL write failed: {e}")))?;
        self.dirty = true;
        Ok(())
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    /// Set a key-value pair, appending to the WAL.
    pub fn set(&mut self, id: &str, key: &str, value: String) -> Result<()> {
        self.append_wal(id, WalOp::Set(key.to_string(), value.clone()))?;
        self.data.insert(key.to_string(), value);
        Ok(())
    }

    /// Delete a key, appending to the WAL.
    pub fn delete_key(&mut self, id: &str, key: &str) -> Result<bool> {
        let existed = self.data.remove(key).is_some();
        if existed {
            self.append_wal(id, WalOp::Delete(key.to_string()))?;
        }
        Ok(existed)
    }

    /// Clear all data, appending to the WAL.
    pub fn clear(&mut self, id: &str) -> Result<()> {
        self.data.clear();
        self.append_wal(id, WalOp::Clear)?;
        Ok(())
    }

    /// List all keys.
    pub fn keys(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Whether there are un-persisted mutations.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Delete the persisted files for an object.
    pub fn destroy(&self, id: &str) -> Result<()> {
        for p in [self.data_path(id), self.wal_path(id), self.tmp_path(id)] {
            if p.exists() {
                std::fs::remove_file(&p)
                    .map_err(|e| Error::ModuleError(format!("Failed to remove file: {e}")))?;
            }
        }
        Ok(())
    }

    /// List object IDs found in the storage directory.
    pub fn list_objects(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let entries = std::fs::read_dir(&self.path)
            .map_err(|e| Error::ModuleError(format!("Failed to list dir: {e}")))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") && !name.contains(".wal.") && !name.contains(".tmp.") {
                if let Some(stem) = name.strip_suffix(".json") {
                    ids.push(stem.to_string());
                }
            }
        }
        Ok(ids)
    }
}

// ---------------------------------------------------------------------------
// DurableConfig
// ---------------------------------------------------------------------------

/// Configuration for the durable object manager.
pub struct DurableConfig {
    pub storage_dir: String,
    pub max_objects: usize,
    pub hibernation_timeout: Duration,
    pub eviction_policy: EvictionPolicy,
    pub wal_enabled: bool,
    pub auto_persist_threshold: usize,
    pub max_object_size: usize,
}

impl Default for DurableConfig {
    fn default() -> Self {
        Self {
            storage_dir: "durable_data".to_string(),
            max_objects: 1024,
            hibernation_timeout: Duration::from_secs(300),
            eviction_policy: EvictionPolicy::LRU,
            wal_enabled: true,
            auto_persist_threshold: 100,
            max_object_size: 1024 * 1024, // 1 MiB
        }
    }
}

// ---------------------------------------------------------------------------
// EvictionPolicy
// ---------------------------------------------------------------------------

/// Strategy for evicting objects when the manager is at capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Least recently used
    LRU,
    /// Least frequently used
    LFU,
    /// Time-to-live based
    TTL,
    /// Manual eviction only
    Manual,
}

// ---------------------------------------------------------------------------
// ObjectState
// ---------------------------------------------------------------------------

/// Lifecycle state of a managed durable object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectState {
    Active,
    Hibernating,
    Evicted,
    Corrupted,
}

// ---------------------------------------------------------------------------
// ManagedObject
// ---------------------------------------------------------------------------

/// A durable object tracked by the manager.
pub struct ManagedObject {
    pub id: String,
    pub state: ObjectState,
    pub data: HashMap<String, serde_json::Value>,
    pub last_accessed: Instant,
    pub access_count: u64,
    pub dirty: bool,
}

impl ManagedObject {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            state: ObjectState::Active,
            data: HashMap::default(),
            last_accessed: Instant::now(),
            access_count: 0,
            dirty: false,
        }
    }

    fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }

    #[allow(dead_code)]
    fn estimated_size(&self) -> usize {
        self.data
            .iter()
            .map(|(k, v)| k.len() + v.to_string().len())
            .sum()
    }
}

// ---------------------------------------------------------------------------
// ManagerStats
// ---------------------------------------------------------------------------

/// Operational statistics for the manager.
#[derive(Debug, Clone, Default)]
pub struct ManagerStats {
    pub total_reads: u64,
    pub total_writes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub evictions: u64,
    pub hibernations: u64,
}

// ---------------------------------------------------------------------------
// DurableObjectManager
// ---------------------------------------------------------------------------

/// Manages the lifecycle of multiple durable objects.
pub struct DurableObjectManager {
    objects: HashMap<String, ManagedObject>,
    config: DurableConfig,
    storage_dir: String,
    stats: ManagerStats,
}

impl DurableObjectManager {
    /// Create a new manager.
    pub fn new(config: DurableConfig) -> Result<Self> {
        let storage_dir = config.storage_dir.clone();
        std::fs::create_dir_all(&storage_dir)
            .map_err(|e| Error::ModuleError(format!("Failed to create storage dir: {e}")))?;
        Ok(Self {
            objects: HashMap::default(),
            storage_dir,
            config,
            stats: ManagerStats::default(),
        })
    }

    /// Get (or create) a managed object by id.
    pub fn get_or_create(&mut self, id: &str) -> Result<&mut ManagedObject> {
        if self.objects.contains_key(id) {
            self.stats.cache_hits += 1;
            let obj = self.objects.get_mut(id).unwrap();
            if obj.state == ObjectState::Hibernating {
                obj.state = ObjectState::Active;
            }
            obj.touch();
            return Ok(obj);
        }

        self.stats.cache_misses += 1;

        // Evict if at capacity
        if self.objects.len() >= self.config.max_objects {
            self.evict_one()?;
        }

        let mut managed = ManagedObject::new(id);

        // Try to load persisted data from disk
        let path = Path::new(&self.storage_dir).join(format!("{id}.json"));
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| Error::ModuleError(format!("Failed to read: {e}")))?;
            let map: std::collections::HashMap<String, serde_json::Value> =
                serde_json::from_str(&content)
                    .map_err(|e| Error::ModuleError(format!("Invalid JSON: {e}")))?;
            managed.data = map.into_iter().collect();
        }
        managed.touch();

        self.objects.insert(id.to_string(), managed);
        Ok(self.objects.get_mut(id).unwrap())
    }

    /// Read a value from an object.
    pub fn read(&mut self, object_id: &str, key: &str) -> Result<Option<serde_json::Value>> {
        self.stats.total_reads += 1;
        let obj = self.get_or_create(object_id)?;
        Ok(obj.data.get(key).cloned())
    }

    /// Write a value into an object.
    pub fn write(
        &mut self,
        object_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<()> {
        if value.to_string().len() > self.config.max_object_size {
            return Err(Error::ModuleError("Value exceeds max_object_size".into()));
        }
        self.stats.total_writes += 1;
        let threshold = self.config.auto_persist_threshold;
        let obj = self.get_or_create(object_id)?;
        obj.data.insert(key.to_string(), value);
        obj.dirty = true;

        if obj.data.len() >= threshold {
            let id = obj.id.clone();
            self.persist_object(&id)?;
        }
        Ok(())
    }

    /// Delete a key from an object.
    pub fn delete_key(&mut self, object_id: &str, key: &str) -> Result<bool> {
        self.stats.total_writes += 1;
        let obj = self.get_or_create(object_id)?;
        let existed = obj.data.remove(key).is_some();
        if existed {
            obj.dirty = true;
        }
        Ok(existed)
    }

    /// Persist a single object to disk.
    pub fn persist_object(&mut self, id: &str) -> Result<()> {
        let obj = self
            .objects
            .get_mut(id)
            .ok_or_else(|| Error::ModuleError(format!("Object not found: {id}")))?;
        if !obj.dirty {
            return Ok(());
        }
        let ordered: std::collections::BTreeMap<&String, &serde_json::Value> =
            obj.data.iter().collect();
        let json = serde_json::to_string_pretty(&ordered)
            .map_err(|e| Error::ModuleError(format!("Serialization failed: {e}")))?;

        let tmp = Path::new(&self.storage_dir).join(format!("{id}.tmp.json"));
        let dest = Path::new(&self.storage_dir).join(format!("{id}.json"));
        std::fs::write(&tmp, &json)
            .map_err(|e| Error::ModuleError(format!("Write failed: {e}")))?;
        std::fs::rename(&tmp, &dest)
            .map_err(|e| Error::ModuleError(format!("Rename failed: {e}")))?;
        obj.dirty = false;
        Ok(())
    }

    /// Persist all dirty objects.
    pub fn persist_all(&mut self) -> Result<()> {
        let ids: Vec<String> = self
            .objects
            .iter()
            .filter(|(_, o)| o.dirty)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.persist_object(&id)?;
        }
        Ok(())
    }

    /// Hibernate objects that have been idle beyond the timeout.
    pub fn hibernate_idle(&mut self) -> Vec<String> {
        let timeout = self.config.hibernation_timeout;
        let mut hibernated = Vec::new();
        for (id, obj) in &mut self.objects {
            if obj.state == ObjectState::Active && obj.last_accessed.elapsed() > timeout {
                obj.state = ObjectState::Hibernating;
                self.stats.hibernations += 1;
                hibernated.push(id.clone());
            }
        }
        hibernated
    }

    /// Evict one object according to the configured policy.
    fn evict_one(&mut self) -> Result<()> {
        let victim = match self.config.eviction_policy {
            EvictionPolicy::LRU => self
                .objects
                .iter()
                .filter(|(_, o)| o.state != ObjectState::Corrupted)
                .min_by_key(|(_, o)| o.last_accessed)
                .map(|(id, _)| id.clone()),
            EvictionPolicy::LFU => self
                .objects
                .iter()
                .filter(|(_, o)| o.state != ObjectState::Corrupted)
                .min_by_key(|(_, o)| o.access_count)
                .map(|(id, _)| id.clone()),
            EvictionPolicy::TTL => self
                .objects
                .iter()
                .filter(|(_, o)| o.state == ObjectState::Hibernating)
                .min_by_key(|(_, o)| o.last_accessed)
                .map(|(id, _)| id.clone()),
            EvictionPolicy::Manual => None,
        };

        if let Some(id) = victim {
            // Persist before evicting
            if self.objects.get(&id).is_some_and(|o| o.dirty) {
                self.persist_object(&id)?;
            }
            self.objects.remove(&id);
            self.stats.evictions += 1;
        }
        Ok(())
    }

    /// Evict a specific object by id.
    pub fn evict(&mut self, id: &str) -> Result<()> {
        if let Some(obj) = self.objects.get(id) {
            if obj.dirty {
                let id_owned = id.to_string();
                self.persist_object(&id_owned)?;
            }
        }
        self.objects.remove(id);
        self.stats.evictions += 1;
        Ok(())
    }

    /// Mark an object as corrupted.
    pub fn mark_corrupted(&mut self, id: &str) {
        if let Some(obj) = self.objects.get_mut(id) {
            obj.state = ObjectState::Corrupted;
        }
    }

    /// Return a snapshot of current statistics.
    pub fn stats(&self) -> &ManagerStats {
        &self.stats
    }

    /// Number of currently tracked objects.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// List all tracked object ids.
    pub fn object_ids(&self) -> Vec<String> {
        self.objects.keys().cloned().collect()
    }

    /// Get the state of a managed object.
    pub fn object_state(&self, id: &str) -> Option<ObjectState> {
        self.objects.get(id).map(|o| o.state)
    }
}

// ---------------------------------------------------------------------------
// Transaction support
// ---------------------------------------------------------------------------

/// State of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    Active,
    Committed,
    RolledBack,
    Failed,
}

/// An individual operation inside a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionOp {
    Get(String),
    Put(String, serde_json::Value),
    Delete(String),
    List {
        prefix: Option<String>,
        limit: Option<usize>,
    },
}

/// A transaction against a managed durable object.
pub struct Transaction {
    pub id: String,
    pub object_id: String,
    pub operations: Vec<TransactionOp>,
    pub state: TransactionState,
    pub started_at: Instant,
    snapshot: HashMap<String, serde_json::Value>,
    results: Vec<Option<serde_json::Value>>,
}

impl Transaction {
    /// Begin a new transaction for `object_id`.
    pub fn begin(object_id: &str, data: &HashMap<String, serde_json::Value>) -> Self {
        Self {
            id: format!("txn-{}-{}", object_id, current_timestamp()),
            object_id: object_id.to_string(),
            operations: Vec::new(),
            state: TransactionState::Active,
            started_at: Instant::now(),
            snapshot: data.clone(),
            results: Vec::new(),
        }
    }

    /// Enqueue a Get operation and return the value from the snapshot.
    pub fn get(&mut self, key: &str) -> Option<serde_json::Value> {
        self.operations.push(TransactionOp::Get(key.to_string()));
        let val = self.snapshot.get(key).cloned();
        self.results.push(val.clone());
        val
    }

    /// Enqueue a Put operation.
    pub fn put(&mut self, key: &str, value: serde_json::Value) {
        self.operations
            .push(TransactionOp::Put(key.to_string(), value.clone()));
        self.snapshot.insert(key.to_string(), value);
        self.results.push(None);
    }

    /// Enqueue a Delete operation.
    pub fn delete(&mut self, key: &str) -> bool {
        self.operations
            .push(TransactionOp::Delete(key.to_string()));
        let existed = self.snapshot.remove(key).is_some();
        self.results.push(None);
        existed
    }

    /// Enqueue a List operation filtered by optional prefix and limit.
    pub fn list(&mut self, prefix: Option<&str>, limit: Option<usize>) -> Vec<String> {
        self.operations.push(TransactionOp::List {
            prefix: prefix.map(|s| s.to_string()),
            limit,
        });
        let mut keys: Vec<String> = self
            .snapshot
            .keys()
            .filter(|k| prefix.is_none_or(|p| k.starts_with(p)))
            .cloned()
            .collect();
        keys.sort();
        if let Some(lim) = limit {
            keys.truncate(lim);
        }
        self.results
            .push(Some(serde_json::Value::Array(
                keys.iter()
                    .map(|k| serde_json::Value::String(k.clone()))
                    .collect(),
            )));
        keys
    }

    /// Commit the transaction, applying all mutations to `target`.
    pub fn commit(
        mut self,
        target: &mut HashMap<String, serde_json::Value>,
    ) -> Result<Vec<Option<serde_json::Value>>> {
        if self.state != TransactionState::Active {
            return Err(Error::ModuleError(
                "Transaction is not active".to_string(),
            ));
        }
        *target = self.snapshot;
        self.state = TransactionState::Committed;
        Ok(self.results)
    }

    /// Roll back the transaction without applying any changes.
    pub fn rollback(mut self) -> TransactionState {
        self.state = TransactionState::RolledBack;
        self.state
    }

    /// Current state.
    pub fn transaction_state(&self) -> TransactionState {
        self.state
    }

    /// Number of queued operations.
    pub fn op_count(&self) -> usize {
        self.operations.len()
    }
}

// ---------------------------------------------------------------------------
// Alarm scheduler
// ---------------------------------------------------------------------------

/// A scheduled alarm for a durable object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledAlarm {
    pub object_id: String,
    pub scheduled_at: Duration,
    pub callback_name: String,
    pub data: Option<serde_json::Value>,
}

/// Manages alarms across durable objects (Cloudflare Workers compatible).
pub struct AlarmScheduler {
    alarms: HashMap<String, ScheduledAlarm>,
}

impl AlarmScheduler {
    pub fn new() -> Self {
        Self {
            alarms: HashMap::default(),
        }
    }

    /// Schedule an alarm. The key is `object_id`.
    pub fn set_alarm(
        &mut self,
        object_id: &str,
        delay: Duration,
        callback_name: &str,
        data: Option<serde_json::Value>,
    ) {
        let alarm = ScheduledAlarm {
            object_id: object_id.to_string(),
            scheduled_at: delay,
            callback_name: callback_name.to_string(),
            data,
        };
        self.alarms.insert(object_id.to_string(), alarm);
    }

    /// Cancel an alarm for a given object.
    pub fn cancel_alarm(&mut self, object_id: &str) -> bool {
        self.alarms.remove(object_id).is_some()
    }

    /// Get the alarm for a given object.
    pub fn get_alarm(&self, object_id: &str) -> Option<&ScheduledAlarm> {
        self.alarms.get(object_id)
    }

    /// Collect alarms whose scheduled time has elapsed relative to `now_elapsed`.
    pub fn collect_due(&mut self, now_elapsed: Duration) -> Vec<ScheduledAlarm> {
        let mut due = Vec::new();
        let mut remaining = HashMap::default();
        for (id, alarm) in self.alarms.drain() {
            if alarm.scheduled_at <= now_elapsed {
                due.push(alarm);
            } else {
                remaining.insert(id, alarm);
            }
        }
        self.alarms = remaining;
        due
    }

    /// Number of pending alarms.
    pub fn pending_count(&self) -> usize {
        self.alarms.len()
    }
}

impl Default for AlarmScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap()
    }

    // -- SqliteStorage tests -----------------------------------------------

    #[test]
    fn test_sqlite_storage_basic_crud() {
        let dir = tmp_dir();
        let mut s = SqliteStorage::new(dir.path().to_str().unwrap()).unwrap();
        s.set("obj1", "name", "Alice".into()).unwrap();
        s.set("obj1", "age", "30".into()).unwrap();
        assert_eq!(s.get("name"), Some(&"Alice".to_string()));
        assert_eq!(s.len(), 2);
        s.delete_key("obj1", "age").unwrap();
        assert!(s.get("age").is_none());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn test_sqlite_storage_persist_and_reload() {
        let dir = tmp_dir();
        let path = dir.path().to_str().unwrap().to_string();

        {
            let mut s = SqliteStorage::new(&path).unwrap();
            s.set("obj1", "key", "value".into()).unwrap();
            s.save("obj1").unwrap();
        }
        {
            let mut s = SqliteStorage::new(&path).unwrap();
            s.load("obj1").unwrap();
            assert_eq!(s.get("key"), Some(&"value".to_string()));
        }
    }

    #[test]
    fn test_sqlite_storage_wal_recovery() {
        let dir = tmp_dir();
        let path = dir.path().to_str().unwrap().to_string();

        // Write data + WAL but do NOT call save (simulate crash)
        {
            let mut s = SqliteStorage::new(&path).unwrap();
            s.set("obj1", "a", "1".into()).unwrap();
            // WAL file exists but no .json data file
        }
        // Recovery: load should replay WAL
        {
            let mut s = SqliteStorage::new(&path).unwrap();
            s.load("obj1").unwrap();
            assert_eq!(s.get("a"), Some(&"1".to_string()));
        }
    }

    #[test]
    fn test_sqlite_storage_clear() {
        let dir = tmp_dir();
        let mut s = SqliteStorage::new(dir.path().to_str().unwrap()).unwrap();
        s.set("obj1", "x", "1".into()).unwrap();
        s.set("obj1", "y", "2".into()).unwrap();
        s.clear("obj1").unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn test_sqlite_storage_list_objects() {
        let dir = tmp_dir();
        let path = dir.path().to_str().unwrap().to_string();
        let mut s = SqliteStorage::new(&path).unwrap();
        s.set("obj1", "k", "v".into()).unwrap();
        s.save("obj1").unwrap();
        s.data.clear();
        s.set("obj2", "k", "v".into()).unwrap();
        s.save("obj2").unwrap();
        let mut ids = s.list_objects().unwrap();
        ids.sort();
        assert_eq!(ids, vec!["obj1", "obj2"]);
    }

    #[test]
    fn test_sqlite_storage_dirty_flag() {
        let dir = tmp_dir();
        let mut s = SqliteStorage::new(dir.path().to_str().unwrap()).unwrap();
        assert!(!s.is_dirty());
        s.set("obj1", "k", "v".into()).unwrap();
        assert!(s.is_dirty());
        s.save("obj1").unwrap();
        assert!(!s.is_dirty());
    }

    // -- DurableObjectManager tests ----------------------------------------

    #[test]
    fn test_manager_create_and_read() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("obj1", "key", serde_json::json!("hello")).unwrap();
        let val = mgr.read("obj1", "key").unwrap();
        assert_eq!(val, Some(serde_json::json!("hello")));
    }

    #[test]
    fn test_manager_persist_and_reload() {
        let dir = tmp_dir();
        let path = dir.path().to_str().unwrap().to_string();

        {
            let config = DurableConfig {
                storage_dir: path.clone(),
                ..Default::default()
            };
            let mut mgr = DurableObjectManager::new(config).unwrap();
            mgr.write("obj1", "x", serde_json::json!(42)).unwrap();
            mgr.persist_all().unwrap();
        }
        {
            let config = DurableConfig {
                storage_dir: path,
                ..Default::default()
            };
            let mut mgr = DurableObjectManager::new(config).unwrap();
            let val = mgr.read("obj1", "x").unwrap();
            assert_eq!(val, Some(serde_json::json!(42)));
        }
    }

    #[test]
    fn test_manager_eviction_lru() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            max_objects: 2,
            eviction_policy: EvictionPolicy::LRU,
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("a", "k", serde_json::json!(1)).unwrap();
        mgr.write("b", "k", serde_json::json!(2)).unwrap();
        // Accessing "a" again makes it more recent
        mgr.read("a", "k").unwrap();
        // Adding "c" should evict "b" (least recently used)
        mgr.write("c", "k", serde_json::json!(3)).unwrap();
        assert_eq!(mgr.object_count(), 2);
        assert!(mgr.object_state("b").is_none());
        assert_eq!(mgr.stats().evictions, 1);
    }

    #[test]
    fn test_manager_eviction_lfu() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            max_objects: 2,
            eviction_policy: EvictionPolicy::LFU,
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("a", "k", serde_json::json!(1)).unwrap();
        mgr.write("b", "k", serde_json::json!(2)).unwrap();
        // Read "a" several times to boost its frequency
        for _ in 0..5 {
            mgr.read("a", "k").unwrap();
        }
        // Adding "c" should evict "b" (least frequently used)
        mgr.write("c", "k", serde_json::json!(3)).unwrap();
        assert!(mgr.object_state("b").is_none());
    }

    #[test]
    fn test_manager_hibernation() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            hibernation_timeout: Duration::from_millis(0), // instant hibernation
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("obj1", "k", serde_json::json!(1)).unwrap();
        // Allow a tiny bit of time to pass
        std::thread::sleep(Duration::from_millis(1));
        let hibernated = mgr.hibernate_idle();
        assert!(hibernated.contains(&"obj1".to_string()));
        assert_eq!(mgr.object_state("obj1"), Some(ObjectState::Hibernating));
        // Accessing again should reactivate
        mgr.read("obj1", "k").unwrap();
        assert_eq!(mgr.object_state("obj1"), Some(ObjectState::Active));
    }

    #[test]
    fn test_manager_mark_corrupted() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("obj1", "k", serde_json::json!(1)).unwrap();
        mgr.mark_corrupted("obj1");
        assert_eq!(mgr.object_state("obj1"), Some(ObjectState::Corrupted));
    }

    #[test]
    fn test_manager_stats() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("obj1", "a", serde_json::json!(1)).unwrap();
        mgr.read("obj1", "a").unwrap();
        mgr.read("obj1", "a").unwrap();
        assert_eq!(mgr.stats().total_writes, 1);
        assert_eq!(mgr.stats().total_reads, 2);
        assert_eq!(mgr.stats().cache_misses, 1); // first access
        assert!(mgr.stats().cache_hits >= 2); // subsequent
    }

    // -- Transaction tests -------------------------------------------------

    #[test]
    fn test_transaction_commit() {
        let mut data: HashMap<String, serde_json::Value> = HashMap::default();
        data.insert("x".to_string(), serde_json::json!(1));

        let mut txn = Transaction::begin("obj1", &data);
        txn.put("y", serde_json::json!(2));
        let val = txn.get("x");
        assert_eq!(val, Some(serde_json::json!(1)));

        let results = txn.commit(&mut data).unwrap();
        assert_eq!(data.get("y"), Some(&serde_json::json!(2)));
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_transaction_rollback() {
        let mut data: HashMap<String, serde_json::Value> = HashMap::default();
        data.insert("x".to_string(), serde_json::json!(1));

        let mut txn = Transaction::begin("obj1", &data);
        txn.put("x", serde_json::json!(999));
        txn.delete("x");
        let state = txn.rollback();
        assert_eq!(state, TransactionState::RolledBack);
        // Original data unchanged
        assert_eq!(data.get("x"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn test_transaction_list_with_prefix() {
        let mut data: HashMap<String, serde_json::Value> = HashMap::default();
        data.insert("user:1".to_string(), serde_json::json!("Alice"));
        data.insert("user:2".to_string(), serde_json::json!("Bob"));
        data.insert("post:1".to_string(), serde_json::json!("Hello"));

        let mut txn = Transaction::begin("obj1", &data);
        let keys = txn.list(Some("user:"), None);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"user:1".to_string()));
        assert!(keys.contains(&"user:2".to_string()));

        let limited = txn.list(Some("user:"), Some(1));
        assert_eq!(limited.len(), 1);
    }

    // -- Alarm tests -------------------------------------------------------

    #[test]
    fn test_alarm_schedule_and_cancel() {
        let mut scheduler = AlarmScheduler::new();
        scheduler.set_alarm("obj1", Duration::from_secs(60), "onAlarm", None);
        assert_eq!(scheduler.pending_count(), 1);
        assert!(scheduler.get_alarm("obj1").is_some());

        scheduler.cancel_alarm("obj1");
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_alarm_collect_due() {
        let mut scheduler = AlarmScheduler::new();
        scheduler.set_alarm("obj1", Duration::from_millis(10), "tick", None);
        scheduler.set_alarm("obj2", Duration::from_secs(3600), "later", None);

        let due = scheduler.collect_due(Duration::from_millis(50));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].object_id, "obj1");
        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_alarm_with_data() {
        let mut scheduler = AlarmScheduler::new();
        scheduler.set_alarm(
            "obj1",
            Duration::from_secs(1),
            "process",
            Some(serde_json::json!({"batch": 42})),
        );
        let alarm = scheduler.get_alarm("obj1").unwrap();
        assert_eq!(alarm.callback_name, "process");
        assert_eq!(alarm.data, Some(serde_json::json!({"batch": 42})));
    }

    #[test]
    fn test_manager_delete_key() {
        let dir = tmp_dir();
        let config = DurableConfig {
            storage_dir: dir.path().to_str().unwrap().to_string(),
            ..Default::default()
        };
        let mut mgr = DurableObjectManager::new(config).unwrap();
        mgr.write("obj1", "a", serde_json::json!(1)).unwrap();
        mgr.write("obj1", "b", serde_json::json!(2)).unwrap();
        assert!(mgr.delete_key("obj1", "a").unwrap());
        assert_eq!(mgr.read("obj1", "a").unwrap(), None);
        assert_eq!(mgr.read("obj1", "b").unwrap(), Some(serde_json::json!(2)));
    }
}

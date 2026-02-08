//! Hot Module Reloading
//!
//! **Status:** ⚠️ Partial — File watching, module graph tracking
//!
//! Update code without losing application state. Modules can accept updates
//! and preserve their state across reloads.
//!
//! # Example
//! ```text
//! // In module.js
//! let counter = 0;
//!
//! export function increment() {
//!   counter++;
//!   return counter;
//! }
//!
//! // Accept hot updates
//! if (module.hot) {
//!   module.hot.accept();
//!   module.hot.dispose((data) => {
//!     // Save state before unloading
//!     data.counter = counter;
//!   });
//!
//!   // Restore state after loading
//!   if (module.hot.data) {
//!     counter = module.hot.data.counter;
//!   }
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// Module identifier
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ModuleId(pub String);

impl ModuleId {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        Self(path.as_ref().to_string_lossy().to_string())
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Module version for tracking updates
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleVersion(pub u64);

impl ModuleVersion {
    pub fn new() -> Self {
        Self(1)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl Default for ModuleVersion {
    fn default() -> Self {
        Self::new()
    }
}

/// State that can be preserved across module updates
#[derive(Debug, Clone, Default)]
pub struct HotData {
    data: HashMap<String, Vec<u8>>,
}

impl HotData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set<T: serde::Serialize>(&mut self, key: &str, value: &T) -> Result<(), HmrError> {
        let bytes = bincode::serialize(value)
            .map_err(|e| HmrError::SerializationError(e.to_string()))?;
        self.data.insert(key.to_string(), bytes);
        Ok(())
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>, HmrError> {
        if let Some(bytes) = self.data.get(key) {
            let value = bincode::deserialize(bytes)
                .map_err(|e| HmrError::SerializationError(e.to_string()))?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn set_raw(&mut self, key: &str, data: Vec<u8>) {
        self.data.insert(key.to_string(), data);
    }

    pub fn get_raw(&self, key: &str) -> Option<&Vec<u8>> {
        self.data.get(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.data.remove(key)
    }
}

/// Module hot context (available as module.hot)
#[derive(Debug)]
pub struct HotContext {
    /// Module ID
    pub id: ModuleId,
    /// Data preserved from previous version
    pub data: Option<HotData>,
    /// Whether the module has accepted updates
    accepted: bool,
    /// Dependencies that trigger updates
    accepted_deps: HashSet<ModuleId>,
    /// Decline updates from these modules
    declined_deps: HashSet<ModuleId>,
    /// Self-decline (prevent updates to this module)
    self_declined: bool,
    /// Dispose callbacks to run before unload
    dispose_data: Option<HotData>,
}

impl HotContext {
    pub fn new(id: ModuleId) -> Self {
        Self {
            id,
            data: None,
            accepted: false,
            accepted_deps: HashSet::new(),
            declined_deps: HashSet::new(),
            self_declined: false,
            dispose_data: None,
        }
    }

    /// Accept hot updates for this module
    pub fn accept(&mut self) {
        self.accepted = true;
    }

    /// Accept updates from specific dependencies
    pub fn accept_deps(&mut self, deps: &[ModuleId]) {
        for dep in deps {
            self.accepted_deps.insert(dep.clone());
        }
    }

    /// Decline updates from specific dependencies
    pub fn decline(&mut self, deps: &[ModuleId]) {
        for dep in deps {
            self.declined_deps.insert(dep.clone());
        }
    }

    /// Decline updates to this module entirely
    pub fn decline_self(&mut self) {
        self.self_declined = true;
    }

    /// Register dispose callback data
    pub fn dispose(&mut self, data: HotData) {
        self.dispose_data = Some(data);
    }

    /// Check if this module can be updated
    pub fn can_update(&self) -> bool {
        !self.self_declined && self.accepted
    }

    /// Check if update from a dependency is accepted
    pub fn accepts_update_from(&self, dep: &ModuleId) -> bool {
        if self.declined_deps.contains(dep) {
            return false;
        }
        self.accepted || self.accepted_deps.contains(dep)
    }

    /// Get dispose data (consumes it)
    pub fn take_dispose_data(&mut self) -> Option<HotData> {
        self.dispose_data.take()
    }
}

/// Status of a module
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleStatus {
    /// Module is ready
    Ready,
    /// Module is being prepared for update
    Preparing,
    /// Module is being updated
    Updating,
    /// Module update failed
    Failed,
    /// Module was disposed
    Disposed,
}

/// Information about a loaded module
#[derive(Debug)]
pub struct ModuleInfo {
    pub id: ModuleId,
    pub path: PathBuf,
    pub version: ModuleVersion,
    pub status: ModuleStatus,
    pub hot: HotContext,
    pub dependencies: HashSet<ModuleId>,
    pub dependents: HashSet<ModuleId>,
    pub source_hash: u64,
    pub loaded_at: Instant,
    pub last_modified: SystemTime,
}

impl ModuleInfo {
    pub fn new(id: ModuleId, path: PathBuf) -> Self {
        let last_modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Self {
            id: id.clone(),
            path,
            version: ModuleVersion::new(),
            status: ModuleStatus::Ready,
            hot: HotContext::new(id),
            dependencies: HashSet::new(),
            dependents: HashSet::new(),
            source_hash: 0,
            loaded_at: Instant::now(),
            last_modified,
        }
    }

    pub fn add_dependency(&mut self, dep: ModuleId) {
        self.dependencies.insert(dep);
    }

    pub fn add_dependent(&mut self, dep: ModuleId) {
        self.dependents.insert(dep);
    }
}

/// Update propagation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// Only update the changed module
    Self_,
    /// Update module and direct dependents
    Bubble,
    /// Full page reload (all modules)
    Full,
}

/// An update to apply
#[derive(Debug)]
pub struct HotUpdate {
    pub module_id: ModuleId,
    pub new_source: String,
    pub source_hash: u64,
    pub mode: UpdateMode,
}

/// Result of applying an update
#[derive(Debug)]
pub struct UpdateResult {
    pub module_id: ModuleId,
    pub success: bool,
    pub affected_modules: Vec<ModuleId>,
    pub error: Option<String>,
    pub duration: Duration,
}

/// Module graph for tracking dependencies
#[derive(Debug, Default)]
pub struct ModuleGraph {
    modules: HashMap<ModuleId, ModuleInfo>,
    entry_points: HashSet<ModuleId>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a module
    pub fn register(&mut self, id: ModuleId, path: PathBuf) -> &mut ModuleInfo {
        self.modules.entry(id.clone()).or_insert_with(|| {
            ModuleInfo::new(id, path)
        })
    }

    /// Get a module
    pub fn get(&self, id: &ModuleId) -> Option<&ModuleInfo> {
        self.modules.get(id)
    }

    /// Get a module mutably
    pub fn get_mut(&mut self, id: &ModuleId) -> Option<&mut ModuleInfo> {
        self.modules.get_mut(id)
    }

    /// Register a dependency relationship
    pub fn add_dependency(&mut self, from: &ModuleId, to: &ModuleId) {
        if let Some(module) = self.modules.get_mut(from) {
            module.add_dependency(to.clone());
        }
        if let Some(module) = self.modules.get_mut(to) {
            module.add_dependent(from.clone());
        }
    }

    /// Add an entry point
    pub fn add_entry_point(&mut self, id: ModuleId) {
        self.entry_points.insert(id);
    }

    /// Check if a module is an entry point
    pub fn is_entry_point(&self, id: &ModuleId) -> bool {
        self.entry_points.contains(id)
    }

    /// Get all modules affected by updating a module
    pub fn get_affected_modules(&self, id: &ModuleId) -> Vec<ModuleId> {
        let mut affected = Vec::new();
        let mut visited = HashSet::new();
        self.collect_dependents(id, &mut affected, &mut visited);
        affected
    }

    fn collect_dependents(
        &self,
        id: &ModuleId,
        affected: &mut Vec<ModuleId>,
        visited: &mut HashSet<ModuleId>,
    ) {
        if visited.contains(id) {
            return;
        }
        visited.insert(id.clone());

        if let Some(module) = self.modules.get(id) {
            for dependent in &module.dependents {
                affected.push(dependent.clone());
                self.collect_dependents(dependent, affected, visited);
            }
        }
    }

    /// Find update boundary (modules that accept the update)
    pub fn find_update_boundary(&self, id: &ModuleId) -> Vec<ModuleId> {
        let mut boundary = Vec::new();
        let mut visited = HashSet::new();
        self.find_boundary_recursive(id, &mut boundary, &mut visited);
        boundary
    }

    fn find_boundary_recursive(
        &self,
        id: &ModuleId,
        boundary: &mut Vec<ModuleId>,
        visited: &mut HashSet<ModuleId>,
    ) {
        if visited.contains(id) {
            return;
        }
        visited.insert(id.clone());

        if let Some(module) = self.modules.get(id) {
            if module.hot.can_update() {
                boundary.push(id.clone());
                return; // Stop at this boundary
            }

            // Check if any dependent accepts updates from this module
            for dependent_id in &module.dependents {
                if let Some(dependent) = self.modules.get(dependent_id) {
                    if dependent.hot.accepts_update_from(id) {
                        boundary.push(dependent_id.clone());
                    } else {
                        // Continue searching up the tree
                        self.find_boundary_recursive(dependent_id, boundary, visited);
                    }
                }
            }
        }
    }

    /// Remove a module from the graph
    pub fn remove(&mut self, id: &ModuleId) -> Option<ModuleInfo> {
        if let Some(module) = self.modules.remove(id) {
            // Clean up references in other modules
            for dep_id in &module.dependencies {
                if let Some(dep) = self.modules.get_mut(dep_id) {
                    dep.dependents.remove(id);
                }
            }
            for dep_id in &module.dependents {
                if let Some(dep) = self.modules.get_mut(dep_id) {
                    dep.dependencies.remove(id);
                }
            }
            self.entry_points.remove(id);
            Some(module)
        } else {
            None
        }
    }

    /// Get all modules
    pub fn modules(&self) -> impl Iterator<Item = &ModuleInfo> {
        self.modules.values()
    }

    /// Get module count
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

/// File change event
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub kind: FileChangeKind,
    pub timestamp: SystemTime,
}

/// Type of file change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

/// File watcher for detecting changes
#[derive(Debug)]
pub struct FileWatcher {
    watched_paths: RwLock<HashSet<PathBuf>>,
    changes: Mutex<Vec<FileChange>>,
    poll_interval: Duration,
    file_timestamps: Mutex<HashMap<PathBuf, SystemTime>>,
}

impl FileWatcher {
    pub fn new(poll_interval: Duration) -> Self {
        Self {
            watched_paths: RwLock::new(HashSet::new()),
            changes: Mutex::new(Vec::new()),
            poll_interval,
            file_timestamps: Mutex::new(HashMap::default()),
        }
    }

    /// Watch a file for changes
    pub fn watch<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref().to_path_buf();
        self.watched_paths.write().unwrap().insert(path.clone());

        // Store initial timestamp
        if let Ok(metadata) = std::fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                self.file_timestamps.lock().unwrap().insert(path, modified);
            }
        }
    }

    /// Stop watching a file
    pub fn unwatch<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref().to_path_buf();
        self.watched_paths.write().unwrap().remove(&path);
        self.file_timestamps.lock().unwrap().remove(&path);
    }

    /// Poll for changes (should be called periodically)
    pub fn poll(&self) -> Vec<FileChange> {
        let paths = self.watched_paths.read().unwrap().clone();
        let mut timestamps = self.file_timestamps.lock().unwrap();
        let mut new_changes = Vec::new();

        for path in paths {
            let current_modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok();

            match (timestamps.get(&path), current_modified) {
                (Some(old), Some(new)) if new > *old => {
                    new_changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Modified,
                        timestamp: new,
                    });
                    timestamps.insert(path, new);
                }
                (None, Some(new)) => {
                    new_changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Created,
                        timestamp: new,
                    });
                    timestamps.insert(path, new);
                }
                (Some(_), None) => {
                    new_changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Deleted,
                        timestamp: SystemTime::now(),
                    });
                    timestamps.remove(&path);
                }
                _ => {}
            }
        }

        // Also store in changes buffer
        if !new_changes.is_empty() {
            self.changes.lock().unwrap().extend(new_changes.clone());
        }

        new_changes
    }

    /// Get and clear pending changes
    pub fn take_changes(&self) -> Vec<FileChange> {
        std::mem::take(&mut *self.changes.lock().unwrap())
    }

    /// Get poll interval
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new(Duration::from_millis(500))
    }
}

/// Hot Module Replacement runtime
pub struct HmrRuntime {
    graph: Arc<Mutex<ModuleGraph>>,
    watcher: Arc<FileWatcher>,
    pending_updates: Mutex<Vec<HotUpdate>>,
    update_handlers: Mutex<HashMap<ModuleId, Box<dyn Fn(&HotUpdate) + Send + Sync>>>,
}

impl std::fmt::Debug for HmrRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HmrRuntime")
            .field("graph", &self.graph)
            .field("watcher", &self.watcher)
            .field("pending_updates", &self.pending_updates)
            .field("update_handlers", &format!("<{} handlers>", self.update_handlers.lock().unwrap().len()))
            .finish()
    }
}

impl HmrRuntime {
    pub fn new() -> Self {
        Self {
            graph: Arc::new(Mutex::new(ModuleGraph::new())),
            watcher: Arc::new(FileWatcher::default()),
            pending_updates: Mutex::new(Vec::new()),
            update_handlers: Mutex::new(HashMap::default()),
        }
    }

    pub fn with_poll_interval(poll_interval: Duration) -> Self {
        Self {
            graph: Arc::new(Mutex::new(ModuleGraph::new())),
            watcher: Arc::new(FileWatcher::new(poll_interval)),
            pending_updates: Mutex::new(Vec::new()),
            update_handlers: Mutex::new(HashMap::default()),
        }
    }

    /// Register a module
    pub fn register_module<P: AsRef<Path>>(&self, path: P) -> ModuleId {
        let path = path.as_ref().to_path_buf();
        let id = ModuleId::from_path(&path);

        let mut graph = self.graph.lock().unwrap();
        graph.register(id.clone(), path.clone());

        // Watch the file
        self.watcher.watch(&path);

        id
    }

    /// Add module dependency
    pub fn add_dependency(&self, from: &ModuleId, to: &ModuleId) {
        self.graph.lock().unwrap().add_dependency(from, to);
    }

    /// Get module's hot context
    pub fn get_hot_context(&self, id: &ModuleId) -> Option<HotContext> {
        self.graph.lock().unwrap()
            .get(id)
            .map(|m| HotContext::new(m.id.clone()))
    }

    /// Check for file changes and queue updates
    pub fn check_for_updates(&self) -> Vec<FileChange> {
        let changes = self.watcher.poll();

        for change in &changes {
            if change.kind == FileChangeKind::Modified {
                let id = ModuleId::from_path(&change.path);
                if let Ok(source) = std::fs::read_to_string(&change.path) {
                    let hash = calculate_hash(&source);
                    self.queue_update(HotUpdate {
                        module_id: id,
                        new_source: source,
                        source_hash: hash,
                        mode: UpdateMode::Bubble,
                    });
                }
            }
        }

        changes
    }

    /// Queue an update
    pub fn queue_update(&self, update: HotUpdate) {
        self.pending_updates.lock().unwrap().push(update);
    }

    /// Get pending updates
    pub fn pending_updates(&self) -> Vec<HotUpdate> {
        std::mem::take(&mut *self.pending_updates.lock().unwrap())
    }

    /// Apply a single update
    pub fn apply_update(&self, update: &HotUpdate) -> UpdateResult {
        let start = Instant::now();
        let mut graph = self.graph.lock().unwrap();

        // Check if module accepts updates
        let can_update = graph.get(&update.module_id)
            .map(|m| m.hot.can_update())
            .unwrap_or(false);

        if !can_update {
            // Find update boundary
            let boundary = graph.find_update_boundary(&update.module_id);
            if boundary.is_empty() {
                return UpdateResult {
                    module_id: update.module_id.clone(),
                    success: false,
                    affected_modules: vec![],
                    error: Some("No module accepts this update, full reload required".to_string()),
                    duration: start.elapsed(),
                };
            }
        }

        // Get affected modules
        let affected = graph.get_affected_modules(&update.module_id);

        // Update module info
        if let Some(module) = graph.get_mut(&update.module_id) {
            // Run dispose handlers and save data
            let dispose_data = module.hot.take_dispose_data();

            // Update version and hash
            module.version = module.version.next();
            module.source_hash = update.source_hash;
            module.status = ModuleStatus::Updating;

            // Restore dispose data for next version
            if let Some(data) = dispose_data {
                module.hot.data = Some(data);
            }

            module.status = ModuleStatus::Ready;
        }

        // Call update handlers
        let handlers = self.update_handlers.lock().unwrap();
        if let Some(handler) = handlers.get(&update.module_id) {
            handler(update);
        }

        UpdateResult {
            module_id: update.module_id.clone(),
            success: true,
            affected_modules: affected,
            error: None,
            duration: start.elapsed(),
        }
    }

    /// Apply all pending updates
    pub fn apply_pending_updates(&self) -> Vec<UpdateResult> {
        let updates = self.pending_updates();
        updates.iter().map(|u| self.apply_update(u)).collect()
    }

    /// Register an update handler for a module
    pub fn on_update<F>(&self, id: ModuleId, handler: F)
    where
        F: Fn(&HotUpdate) + Send + Sync + 'static,
    {
        self.update_handlers.lock().unwrap().insert(id, Box::new(handler));
    }

    /// Invalidate a module (force reload)
    pub fn invalidate(&self, id: &ModuleId) {
        let mut graph = self.graph.lock().unwrap();
        if let Some(module) = graph.get_mut(id) {
            module.status = ModuleStatus::Disposed;
        }
    }

    /// Get module graph
    pub fn graph(&self) -> Arc<Mutex<ModuleGraph>> {
        Arc::clone(&self.graph)
    }

    /// Get file watcher
    pub fn watcher(&self) -> Arc<FileWatcher> {
        Arc::clone(&self.watcher)
    }
}

impl Default for HmrRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate a simple hash for source code
fn calculate_hash(source: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// HMR errors
#[derive(Debug, Clone)]
pub enum HmrError {
    ModuleNotFound(ModuleId),
    UpdateDeclined(ModuleId),
    UpdateFailed(String),
    SerializationError(String),
    IoError(String),
}

impl std::fmt::Display for HmrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotFound(id) => write!(f, "module not found: {}", id),
            Self::UpdateDeclined(id) => write!(f, "update declined by module: {}", id),
            Self::UpdateFailed(msg) => write!(f, "update failed: {}", msg),
            Self::SerializationError(msg) => write!(f, "serialization error: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for HmrError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_module_graph() {
        let mut graph = ModuleGraph::new();

        let id_a = ModuleId("a.js".to_string());
        let id_b = ModuleId("b.js".to_string());

        graph.register(id_a.clone(), PathBuf::from("a.js"));
        graph.register(id_b.clone(), PathBuf::from("b.js"));
        graph.add_dependency(&id_a, &id_b);

        // A depends on B, so B's dependents should include A
        let b = graph.get(&id_b).unwrap();
        assert!(b.dependents.contains(&id_a));

        // A's dependencies should include B
        let a = graph.get(&id_a).unwrap();
        assert!(a.dependencies.contains(&id_b));
    }

    #[test]
    fn test_hot_context_accept() {
        let mut hot = HotContext::new(ModuleId("test.js".to_string()));

        assert!(!hot.can_update());
        hot.accept();
        assert!(hot.can_update());
    }

    #[test]
    fn test_hot_context_decline_self() {
        let mut hot = HotContext::new(ModuleId("test.js".to_string()));

        hot.accept();
        assert!(hot.can_update());

        hot.decline_self();
        assert!(!hot.can_update());
    }

    #[test]
    fn test_file_watcher() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.js");

        // Create initial file
        {
            let mut file = std::fs::File::create(&file_path).unwrap();
            writeln!(file, "console.log('hello');").unwrap();
        }

        let watcher = FileWatcher::default();
        watcher.watch(&file_path);

        // Poll should return empty (no changes yet)
        let changes = watcher.poll();
        assert!(changes.is_empty());

        // Modify file
        std::thread::sleep(Duration::from_millis(10));
        {
            let mut file = std::fs::File::create(&file_path).unwrap();
            writeln!(file, "console.log('world');").unwrap();
        }

        // Poll should detect change
        let changes = watcher.poll();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
    }

    #[test]
    fn test_hmr_runtime() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("module.js");

        // Create module file
        {
            let mut file = std::fs::File::create(&file_path).unwrap();
            writeln!(file, "export const x = 1;").unwrap();
        }

        let runtime = HmrRuntime::new();
        let id = runtime.register_module(&file_path);

        // Module should be registered
        let graph = runtime.graph();
        let graph = graph.lock().unwrap();
        assert!(graph.get(&id).is_some());
    }

    #[test]
    fn test_update_boundary() {
        let mut graph = ModuleGraph::new();

        // Create module chain: A -> B -> C
        // Only B accepts updates
        let id_a = ModuleId("a.js".to_string());
        let id_b = ModuleId("b.js".to_string());
        let id_c = ModuleId("c.js".to_string());

        graph.register(id_a.clone(), PathBuf::from("a.js"));
        graph.register(id_b.clone(), PathBuf::from("b.js"));
        graph.register(id_c.clone(), PathBuf::from("c.js"));

        graph.add_dependency(&id_a, &id_b);
        graph.add_dependency(&id_b, &id_c);

        // Make B accept updates
        if let Some(b) = graph.get_mut(&id_b) {
            b.hot.accept();
        }

        // Update to C should bubble to B (which accepts)
        let boundary = graph.find_update_boundary(&id_c);
        assert!(boundary.contains(&id_b));
    }

    #[test]
    fn test_hot_data() {
        let mut data = HotData::new();

        data.set_raw("counter", vec![1, 2, 3, 4]);
        assert!(data.has("counter"));

        let retrieved = data.get_raw("counter");
        assert_eq!(retrieved, Some(&vec![1, 2, 3, 4]));

        data.remove("counter");
        assert!(!data.has("counter"));
    }
}

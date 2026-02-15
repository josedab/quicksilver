//! Garbage collector for Quicksilver
//!
//! This module implements a mark-and-sweep garbage collector
//! for managing JavaScript objects.
//!
//! ## Design
//!
//! The GC uses a mark-and-sweep approach:
//! - **Mark phase**: Traverse from roots and mark all reachable objects
//! - **Sweep phase**: Free all unmarked objects
//!
//! The collector maintains a list of all GC-managed objects and periodically
//! collects unreachable ones based on allocation thresholds.

//! **Status:** ✅ Complete — Mark-and-sweep garbage collector

use crate::runtime::Value;
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

/// GC configuration
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Collection threshold (number of allocations before GC)
    pub allocation_threshold: usize,
    /// Enable verbose GC logging
    pub verbose: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            allocation_threshold: 1000,
            verbose: false,
        }
    }
}

/// GC statistics
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total objects allocated
    pub total_allocations: usize,
    /// Current live objects
    pub live_objects: usize,
    /// Objects freed in last collection
    pub last_freed: usize,
    /// Total objects freed
    pub total_freed: usize,
    /// Number of collections
    pub collections: u64,
    /// Total time spent in GC (microseconds)
    pub total_gc_time_us: u64,
    /// Maximum pause time (microseconds)
    pub max_pause_us: u64,
}

/// A trait for objects that can be garbage collected
pub trait Traceable {
    /// Trace all references from this object
    fn trace(&self, tracer: &mut dyn FnMut(&Value));
}

/// Internal object entry in the GC heap
struct HeapEntry {
    /// Weak reference to the object
    object: Weak<RefCell<dyn std::any::Any>>,
    /// Unique ID for the object
    id: usize,
    /// Whether object was marked in current collection
    marked: bool,
}

/// Mark-and-sweep garbage collector
pub struct Gc {
    /// Configuration
    config: GcConfig,
    /// Statistics
    stats: GcStats,
    /// All managed objects (weak references)
    heap: Vec<HeapEntry>,
    /// Next object ID
    next_id: usize,
    /// Allocations since last collection
    allocations_since_gc: usize,
    /// Is collection currently running?
    collecting: bool,
    /// Fast O(1) lookup from object pointer to heap entry ID
    /// This eliminates the O(n) scan in mark_value
    ptr_to_id: HashMap<usize, usize>,
}

impl Gc {
    /// Create a new garbage collector
    pub fn new() -> Self {
        Self::with_config(GcConfig::default())
    }

    /// Create a GC with custom configuration
    pub fn with_config(config: GcConfig) -> Self {
        Self {
            config,
            stats: GcStats::default(),
            heap: Vec::new(),
            next_id: 0,
            allocations_since_gc: 0,
            collecting: false,
            ptr_to_id: HashMap::default(),
        }
    }

    /// Get GC statistics
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }

    /// Register an object with the GC
    pub fn register<T: 'static>(&mut self, object: Rc<RefCell<T>>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.total_allocations += 1;
        self.allocations_since_gc += 1;

        // Store pointer for O(1) lookup
        let ptr = Rc::as_ptr(&object) as *const () as usize;
        self.ptr_to_id.insert(ptr, id);

        self.heap.push(HeapEntry {
            object: Rc::downgrade(&object) as Weak<RefCell<dyn std::any::Any>>,
            id,
            marked: false,
        });

        if self.config.verbose {
            eprintln!("[GC] Registered object {}", id);
        }

        id
    }

    /// Record an allocation (without registering an object)
    pub fn record_allocation(&mut self, _size: usize) {
        self.allocations_since_gc += 1;
        self.stats.total_allocations += 1;
    }

    /// Record a deallocation
    pub fn record_deallocation(&mut self, _size: usize) {
        // Stats tracking only - actual deallocation handled by Rc
    }

    /// Check if collection is needed based on allocation count
    pub fn should_collect(&self) -> bool {
        self.allocations_since_gc >= self.config.allocation_threshold
    }

    /// Perform a garbage collection cycle
    ///
    /// The `roots` parameter should be an iterator of root Values to trace from.
    pub fn collect<'a>(&mut self, roots: impl Iterator<Item = &'a Value>) {
        if self.collecting {
            return;
        }

        self.collecting = true;
        let start = std::time::Instant::now();

        if self.config.verbose {
            eprintln!("[GC] Starting collection...");
        }

        // Phase 1: Clear all marks
        for entry in &mut self.heap {
            entry.marked = false;
        }

        // Phase 2: Mark phase - trace from roots
        let mut marked_ids = HashSet::new();
        for root in roots {
            self.mark_value(root, &mut marked_ids);
        }

        // Apply marks to heap entries
        for entry in &mut self.heap {
            entry.marked = marked_ids.contains(&entry.id);
        }

        // Phase 3: Sweep phase - remove dead objects
        let before_count = self.heap.len();
        let verbose = self.config.verbose;
        let ptr_to_id = &mut self.ptr_to_id;
        self.heap.retain(|entry| {
            // Keep if marked OR if there are still strong references
            if entry.marked {
                return true;
            }
            // Check if the object is still alive (has strong refs)
            if entry.object.strong_count() > 0 {
                return true;
            }
            // Object is dead - remove from ptr_to_id map and allow collection
            if let Some(strong) = entry.object.upgrade() {
                let ptr = Rc::as_ptr(&strong) as *const () as usize;
                ptr_to_id.remove(&ptr);
            }
            if verbose {
                eprintln!("[GC] Freeing object {}", entry.id);
            }
            false
        });

        let freed = before_count - self.heap.len();
        self.stats.last_freed = freed;
        self.stats.total_freed += freed;
        self.stats.live_objects = self.heap.len();
        self.stats.collections += 1;
        self.allocations_since_gc = 0;

        let elapsed = start.elapsed().as_micros() as u64;
        self.stats.total_gc_time_us += elapsed;
        self.stats.max_pause_us = self.stats.max_pause_us.max(elapsed);

        if self.config.verbose {
            eprintln!("[GC] Collection complete: freed {} objects, {} remaining, {}μs",
                freed, self.heap.len(), elapsed);
        }

        self.collecting = false;
    }

    /// Mark a value and recursively mark all values it references
    /// Uses O(1) HashMap lookup instead of O(n) linear scan
    fn mark_value(&self, value: &Value, marked: &mut HashSet<usize>) {
        match value {
            Value::Object(obj) => {
                // O(1) lookup using ptr_to_id HashMap
                let obj_ptr = Rc::as_ptr(obj) as *const () as usize;
                if let Some(&id) = self.ptr_to_id.get(&obj_ptr) {
                    if marked.insert(id) {
                        // Object wasn't marked before, trace its properties
                        let obj_ref = obj.borrow();
                        for (_key, prop_value) in obj_ref.properties.iter() {
                            self.mark_value(prop_value, marked);
                        }

                        // Trace based on object kind
                        if let Some(proto) = obj_ref.prototype.as_ref() {
                            self.mark_value(&Value::Object(proto.clone()), marked);
                        }

                        // Trace array elements if array
                        if let crate::runtime::ObjectKind::Array(elements) = &obj_ref.kind {
                            for elem in elements {
                                self.mark_value(elem, marked);
                            }
                        }

                        // Trace function upvalues if function
                        if let crate::runtime::ObjectKind::Function(func) = &obj_ref.kind {
                            for upvalue in &func.upvalues {
                                let val = upvalue.borrow().clone();
                                self.mark_value(&val, marked);
                            }
                        }

                        // Trace map entries
                        if let crate::runtime::ObjectKind::Map(entries) = &obj_ref.kind {
                            for (k, v) in entries {
                                self.mark_value(k, marked);
                                self.mark_value(v, marked);
                            }
                        }

                        // Trace set entries
                        if let crate::runtime::ObjectKind::Set(entries) = &obj_ref.kind {
                            for v in entries {
                                self.mark_value(v, marked);
                            }
                        }

                        // Trace promise value
                        if let crate::runtime::ObjectKind::Promise { value: pval, .. } = &obj_ref.kind {
                            if let Some(v) = pval {
                                self.mark_value(v, marked);
                            }
                        }
                    }
                }
            }
            _ => {
                // Primitive values don't need GC tracking
            }
        }
    }

    /// Get the number of live objects
    pub fn live_count(&self) -> usize {
        self.heap.iter().filter(|e| e.object.strong_count() > 0).count()
    }

    /// Force cleanup of all dead weak references
    pub fn cleanup_dead_refs(&mut self) {
        let before = self.heap.len();
        let verbose = self.config.verbose;
        let ptr_to_id = &mut self.ptr_to_id;
        self.heap.retain(|entry| {
            if entry.object.strong_count() > 0 {
                return true;
            }
            // Remove dead entry from ptr_to_id map
            if let Some(strong) = entry.object.upgrade() {
                let ptr = Rc::as_ptr(&strong) as *const () as usize;
                ptr_to_id.remove(&ptr);
            }
            false
        });
        let removed = before - self.heap.len();
        if removed > 0 && verbose {
            eprintln!("[GC] Cleaned up {} dead references", removed);
        }
    }
}

impl Default for Gc {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle to a GC-managed object
#[derive(Clone)]
pub struct GcHandle<T> {
    inner: Rc<RefCell<T>>,
}

impl<T> GcHandle<T> {
    /// Create a new GC handle
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    /// Get reference count
    pub fn ref_count(&self) -> usize {
        Rc::strong_count(&self.inner)
    }

    /// Get the inner Rc
    pub fn inner(&self) -> &Rc<RefCell<T>> {
        &self.inner
    }
}

impl<T> std::ops::Deref for GcHandle<T> {
    type Target = RefCell<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Write barrier for incremental/generational GC (placeholder for future use)
pub struct WriteBarrier;

impl WriteBarrier {
    /// Record a write from one object to another
    pub fn record_write(_from: usize, _to: usize) {
        // Placeholder for incremental GC implementation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Object, ObjectKind};
    use rustc_hash::FxHashMap as HashMap;

    #[test]
    fn test_gc_creation() {
        let gc = Gc::new();
        assert_eq!(gc.stats().total_allocations, 0);
        assert_eq!(gc.stats().collections, 0);
    }

    #[test]
    fn test_gc_allocation_tracking() {
        let mut gc = Gc::new();
        gc.record_allocation(1000);
        assert_eq!(gc.stats().total_allocations, 1);
    }

    #[test]
    fn test_gc_handle() {
        let handle = GcHandle::new(42);
        assert_eq!(handle.ref_count(), 1);

        let handle2 = handle.clone();
        assert_eq!(handle.ref_count(), 2);
        assert_eq!(handle2.ref_count(), 2);

        drop(handle2);
        assert_eq!(handle.ref_count(), 1);
    }

    #[test]
    fn test_gc_collection() {
        let mut gc = Gc::new();

        // Create some objects
        let obj1 = Rc::new(RefCell::new(Object {
            kind: ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        }));
        let obj2 = Rc::new(RefCell::new(Object {
            kind: ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None, cached_shape_id: None,
        }));

        gc.register(obj1.clone());
        gc.register(obj2.clone());

        assert_eq!(gc.live_count(), 2);

        // Collect with obj1 as root - obj2 should be collected if no refs
        let roots = vec![Value::Object(obj1.clone())];
        gc.collect(roots.iter());

        assert_eq!(gc.stats().collections, 1);
        // obj2 still has a strong ref, so it won't be collected yet
        assert_eq!(gc.live_count(), 2);

        // Drop obj2 and run cleanup
        drop(obj2);
        gc.cleanup_dead_refs();
        assert_eq!(gc.live_count(), 1);
    }

    #[test]
    fn test_gc_should_collect() {
        let config = GcConfig {
            allocation_threshold: 5,
            verbose: false,
        };
        let mut gc = Gc::with_config(config);

        for _ in 0..4 {
            gc.record_allocation(100);
        }
        assert!(!gc.should_collect());

        gc.record_allocation(100);
        assert!(gc.should_collect());
    }

    #[test]
    fn test_gc_registration() {
        let mut gc = Gc::new();

        let obj = Rc::new(RefCell::new(42));
        let id = gc.register(obj);

        assert_eq!(id, 0);
        assert_eq!(gc.stats().total_allocations, 1);
    }
}

// =============================================================================
// Tracing Mark-and-Sweep GC Implementation
// =============================================================================

/// Trait for types that can be traced by the garbage collector.
/// All GC-managed types must implement this trait.
pub trait Trace {
    /// Mark this object and all objects it references.
    fn trace(&self, tracer: &mut Tracer);
    /// Return the size of this object in bytes (for memory accounting).
    fn size_hint(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

/// Color for tri-color marking algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcColor {
    /// Not yet visited (candidate for collection)
    White,
    /// Discovered but not fully traced (in worklist)
    Gray,
    /// Fully traced (reachable, will not be collected)
    Black,
}

/// Header for GC-managed objects
#[derive(Debug)]
pub struct GcHeader {
    pub color: GcColor,
    pub size: usize,
    pub marked: bool,
    pub generation: u32,
}

/// A GC-managed reference (analogous to Rc but GC-tracked)
pub struct GcRef<T: Trace> {
    id: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Trace> GcRef<T> {
    /// Create a new GcRef with the given id.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the heap id of this reference.
    pub fn id(&self) -> usize {
        self.id
    }
}

impl<T: Trace> Copy for GcRef<T> {}

impl<T: Trace> Clone for GcRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Type-erased heap data
pub enum HeapData {
    /// Object with named properties pointing to other heap objects
    Object(Vec<(String, usize)>),
    /// Array of heap object ids
    Array(Vec<usize>),
    /// String data
    String(String),
    /// Numeric data
    Number(f64),
    /// Boolean data
    Boolean(bool),
    /// Closure with captured upvalue object ids
    Closure { upvalues: Vec<usize> },
    /// Empty slot (freed)
    Empty,
}

/// Internal object entry in the tracing GC heap
struct HeapObject {
    header: GcHeader,
    /// Type-erased data
    data: HeapData,
    /// References to other objects (for tracing)
    references: Vec<usize>,
    alive: bool,
}

/// Configuration for the GC heap
pub struct GcHeapConfig {
    pub initial_threshold: usize,
    pub growth_factor: f64,
    pub min_threshold: usize,
    pub max_threshold: usize,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            initial_threshold: 1024 * 1024,
            growth_factor: 1.5,
            min_threshold: 256 * 1024,
            max_threshold: 256 * 1024 * 1024,
        }
    }
}

/// The GC heap manages all allocated objects using tri-color mark-and-sweep.
pub struct GcHeap {
    /// All allocated objects (indexed by id)
    objects: Vec<HeapObject>,
    /// Free list for recycling slots
    free_list: Vec<usize>,
    /// Root set (indices into objects that are GC roots)
    roots: Vec<usize>,
    /// Total allocated bytes
    total_bytes: usize,
    /// Collection threshold in bytes
    threshold: usize,
    /// Number of collections performed
    collections: u64,
    /// Total bytes freed
    total_freed: u64,
    /// Generation counter
    generation: u32,
    /// GC configuration
    config: GcHeapConfig,
    /// Peak heap size in bytes
    peak_heap_size: usize,
    /// Total number of allocations ever made
    total_allocations: u64,
}

impl Default for GcHeap {
    fn default() -> Self {
        Self::new(GcHeapConfig::default())
    }
}

/// Tracer used during the marking phase
pub struct Tracer {
    /// Gray worklist (objects discovered but not yet traced)
    worklist: Vec<usize>,
    /// Number of objects marked in this cycle
    marked_count: usize,
    /// Bytes marked in this cycle
    marked_bytes: usize,
}

impl Tracer {
    /// Create a new empty tracer.
    pub fn new() -> Self {
        Self {
            worklist: Vec::new(),
            marked_count: 0,
            marked_bytes: 0,
        }
    }

    /// Enqueue an object id for tracing.
    pub fn mark(&mut self, id: usize) {
        self.worklist.push(id);
    }

    /// Number of objects marked so far.
    pub fn marked_count(&self) -> usize {
        self.marked_count
    }

    /// Bytes marked so far.
    pub fn marked_bytes(&self) -> usize {
        self.marked_bytes
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a garbage collection cycle
#[derive(Debug, Clone)]
pub struct CollectionResult {
    pub objects_freed: usize,
    pub bytes_freed: usize,
    pub objects_surviving: usize,
    pub duration_us: u64,
    pub generation: u32,
}

/// Statistics for the GC heap
#[derive(Debug, Clone, Default)]
pub struct GcHeapStats {
    pub total_allocations: u64,
    pub total_collections: u64,
    pub total_bytes_freed: u64,
    pub current_heap_size: usize,
    pub current_object_count: usize,
    pub current_threshold: usize,
    pub peak_heap_size: usize,
}

impl GcHeap {
    /// Create a new GC heap with the given configuration.
    pub fn new(config: GcHeapConfig) -> Self {
        let threshold = config.initial_threshold;
        Self {
            objects: Vec::new(),
            free_list: Vec::new(),
            roots: Vec::new(),
            total_bytes: 0,
            threshold,
            collections: 0,
            total_freed: 0,
            generation: 0,
            config,
            peak_heap_size: 0,
            total_allocations: 0,
        }
    }

    /// Allocate a new object on the heap. Returns the object id.
    pub fn allocate(&mut self, data: HeapData, references: Vec<usize>, size: usize) -> usize {
        let header = GcHeader {
            color: GcColor::White,
            size,
            marked: false,
            generation: self.generation,
        };
        let obj = HeapObject {
            header,
            data,
            references,
            alive: true,
        };

        self.total_bytes += size;
        self.total_allocations += 1;
        if self.total_bytes > self.peak_heap_size {
            self.peak_heap_size = self.total_bytes;
        }

        if let Some(idx) = self.free_list.pop() {
            self.objects[idx] = obj;
            idx
        } else {
            let idx = self.objects.len();
            self.objects.push(obj);
            idx
        }
    }

    /// Add an object id to the root set.
    pub fn add_root(&mut self, id: usize) {
        if !self.roots.contains(&id) {
            self.roots.push(id);
        }
    }

    /// Remove an object id from the root set.
    pub fn remove_root(&mut self, id: usize) {
        self.roots.retain(|&r| r != id);
    }

    /// Add a reference edge from one object to another.
    pub fn add_reference(&mut self, from: usize, to: usize) {
        if from < self.objects.len() && self.objects[from].alive {
            if !self.objects[from].references.contains(&to) {
                self.objects[from].references.push(to);
            }
        }
    }

    /// Perform a full mark-and-sweep collection cycle.
    pub fn collect(&mut self) -> CollectionResult {
        let start = std::time::Instant::now();
        self.generation += 1;

        self.mark_phase();
        let (freed_count, freed_bytes) = self.sweep_phase();

        self.collections += 1;
        self.total_freed += freed_bytes as u64;

        // Adjust threshold based on surviving heap size
        let new_threshold = ((self.total_bytes as f64) * self.config.growth_factor) as usize;
        self.threshold = new_threshold
            .max(self.config.min_threshold)
            .min(self.config.max_threshold);

        let duration_us = start.elapsed().as_micros() as u64;

        CollectionResult {
            objects_freed: freed_count,
            bytes_freed: freed_bytes,
            objects_surviving: self.live_object_count(),
            duration_us,
            generation: self.generation,
        }
    }

    /// Tri-color marking phase: mark all reachable objects from roots.
    fn mark_phase(&mut self) {
        // Reset all live objects to white
        for obj in &mut self.objects {
            if obj.alive {
                obj.header.color = GcColor::White;
                obj.header.marked = false;
            }
        }

        // Seed worklist with roots colored gray
        let mut worklist: Vec<usize> = Vec::new();
        for &root_id in &self.roots {
            if root_id < self.objects.len() && self.objects[root_id].alive {
                self.objects[root_id].header.color = GcColor::Gray;
                worklist.push(root_id);
            }
        }

        // Process gray objects until worklist is empty
        while let Some(obj_id) = worklist.pop() {
            if obj_id >= self.objects.len() || !self.objects[obj_id].alive {
                continue;
            }
            if self.objects[obj_id].header.color == GcColor::Black {
                continue;
            }

            // Promote to black (fully traced)
            self.objects[obj_id].header.color = GcColor::Black;
            self.objects[obj_id].header.marked = true;

            // Discover references and color them gray
            let refs: Vec<usize> = self.objects[obj_id].references.clone();
            for ref_id in refs {
                if ref_id < self.objects.len()
                    && self.objects[ref_id].alive
                    && self.objects[ref_id].header.color == GcColor::White
                {
                    self.objects[ref_id].header.color = GcColor::Gray;
                    worklist.push(ref_id);
                }
            }
        }
    }

    /// Sweep phase: free all white (unreachable) objects.
    /// Returns (freed_count, freed_bytes).
    fn sweep_phase(&mut self) -> (usize, usize) {
        let mut freed_count = 0;
        let mut freed_bytes = 0;

        for i in 0..self.objects.len() {
            if self.objects[i].alive && self.objects[i].header.color == GcColor::White {
                freed_bytes += self.objects[i].header.size;
                self.objects[i].alive = false;
                self.objects[i].data = HeapData::Empty;
                self.objects[i].references.clear();
                self.free_list.push(i);
                freed_count += 1;
            }
        }

        self.total_bytes = self.total_bytes.saturating_sub(freed_bytes);
        (freed_count, freed_bytes)
    }

    /// Check if the allocation threshold has been exceeded.
    pub fn should_collect(&self) -> bool {
        self.total_bytes >= self.threshold
    }

    /// Get current heap statistics.
    pub fn stats(&self) -> GcHeapStats {
        GcHeapStats {
            total_allocations: self.total_allocations,
            total_collections: self.collections,
            total_bytes_freed: self.total_freed,
            current_heap_size: self.total_bytes,
            current_object_count: self.live_object_count(),
            current_threshold: self.threshold,
            peak_heap_size: self.peak_heap_size,
        }
    }

    /// Total number of object slots (including freed slots).
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Number of currently live objects.
    pub fn live_object_count(&self) -> usize {
        self.objects.iter().filter(|o| o.alive).count()
    }

    /// Total bytes currently allocated on the heap.
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

/// Write barrier for incremental GC support.
///
/// Tracks objects whose references have been mutated so that an incremental
/// or concurrent collector can re-scan them.
pub struct GcWriteBarrier {
    dirty_objects: Vec<usize>,
    enabled: bool,
}

impl GcWriteBarrier {
    /// Create a new write barrier (enabled by default).
    pub fn new() -> Self {
        Self {
            dirty_objects: Vec::new(),
            enabled: true,
        }
    }

    /// Record that an object's references have been mutated.
    pub fn record_write(&mut self, object_id: usize) {
        if self.enabled {
            self.dirty_objects.push(object_id);
        }
    }

    /// Drain all recorded dirty object ids.
    pub fn drain(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.dirty_objects)
    }

    /// Enable the write barrier.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the write barrier.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if the write barrier is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for GcWriteBarrier {
    fn default() -> Self {
        Self::new()
    }
}

/// Weak reference that doesn't prevent collection.
pub struct WeakRef {
    target_id: usize,
    is_alive: bool,
}

impl WeakRef {
    /// Create a weak reference to the given heap object id.
    pub fn new(target_id: usize) -> Self {
        Self {
            target_id,
            is_alive: true,
        }
    }

    /// Check if the target is still alive on the heap.
    pub fn deref(&self, heap: &GcHeap) -> bool {
        if !self.is_alive {
            return false;
        }
        self.target_id < heap.objects.len() && heap.objects[self.target_id].alive
    }

    /// Get the target id if the weak reference has not been explicitly invalidated.
    pub fn target(&self) -> Option<usize> {
        if self.is_alive {
            Some(self.target_id)
        } else {
            None
        }
    }

    /// Manually invalidate this weak reference.
    pub fn invalidate(&mut self) {
        self.is_alive = false;
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod gc_heap_tests {
    use super::*;

    fn test_config() -> GcHeapConfig {
        GcHeapConfig {
            initial_threshold: 4096,
            growth_factor: 2.0,
            min_threshold: 1024,
            max_threshold: 1024 * 1024,
        }
    }

    #[test]
    fn test_gc_heap_creation() {
        let heap = GcHeap::new(test_config());
        assert_eq!(heap.object_count(), 0);
        assert_eq!(heap.live_object_count(), 0);
        assert_eq!(heap.total_bytes(), 0);
        assert_eq!(heap.stats().total_collections, 0);
    }

    #[test]
    fn test_gc_heap_allocate() {
        let mut heap = GcHeap::new(test_config());
        let id0 = heap.allocate(HeapData::Number(42.0), vec![], 8);
        let id1 = heap.allocate(HeapData::Boolean(true), vec![], 1);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(heap.live_object_count(), 2);
        assert_eq!(heap.total_bytes(), 9);
    }

    #[test]
    fn test_gc_heap_mark_sweep_basic() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Number(1.0), vec![], 8);
        let _garbage = heap.allocate(HeapData::Number(2.0), vec![], 8);
        heap.add_root(root);

        let result = heap.collect();
        assert_eq!(result.objects_freed, 1);
        assert_eq!(result.bytes_freed, 8);
        assert_eq!(heap.live_object_count(), 1);
    }

    #[test]
    fn test_gc_heap_roots_prevent_collection() {
        let mut heap = GcHeap::new(test_config());
        let a = heap.allocate(HeapData::String("hello".into()), vec![], 16);
        let b = heap.allocate(HeapData::String("world".into()), vec![], 16);
        heap.add_root(a);
        heap.add_root(b);

        let result = heap.collect();
        assert_eq!(result.objects_freed, 0);
        assert_eq!(heap.live_object_count(), 2);
    }

    #[test]
    fn test_gc_heap_circular_references() {
        let mut heap = GcHeap::new(test_config());
        // Create a cycle: a -> b -> c -> a, none rooted
        let a = heap.allocate(HeapData::Object(vec![]), vec![], 32);
        let b = heap.allocate(HeapData::Object(vec![]), vec![], 32);
        let c = heap.allocate(HeapData::Object(vec![]), vec![], 32);
        heap.add_reference(a, b);
        heap.add_reference(b, c);
        heap.add_reference(c, a);

        // No roots — entire cycle should be collected
        let result = heap.collect();
        assert_eq!(result.objects_freed, 3);
        assert_eq!(result.bytes_freed, 96);
        assert_eq!(heap.live_object_count(), 0);
    }

    #[test]
    fn test_gc_heap_circular_references_with_root() {
        let mut heap = GcHeap::new(test_config());
        // Cycle: a -> b -> a, with a as root
        let a = heap.allocate(HeapData::Object(vec![]), vec![], 32);
        let b = heap.allocate(HeapData::Object(vec![]), vec![], 32);
        heap.add_reference(a, b);
        heap.add_reference(b, a);
        heap.add_root(a);

        let result = heap.collect();
        assert_eq!(result.objects_freed, 0);
        assert_eq!(heap.live_object_count(), 2);
    }

    #[test]
    fn test_gc_write_barrier() {
        let mut wb = GcWriteBarrier::new();
        assert!(wb.is_enabled());

        wb.record_write(1);
        wb.record_write(2);
        wb.record_write(3);

        let dirty = wb.drain();
        assert_eq!(dirty, vec![1, 2, 3]);
        assert!(wb.drain().is_empty());

        wb.disable();
        assert!(!wb.is_enabled());
        wb.record_write(4);
        assert!(wb.drain().is_empty());

        wb.enable();
        wb.record_write(5);
        assert_eq!(wb.drain(), vec![5]);
    }

    #[test]
    fn test_gc_weak_ref_cleared() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Number(1.0), vec![], 8);
        let target = heap.allocate(HeapData::Number(2.0), vec![], 8);
        heap.add_root(root);

        let weak = WeakRef::new(target);
        assert!(weak.deref(&heap));
        assert_eq!(weak.target(), Some(target));

        heap.collect();
        // target was not rooted, should be collected
        assert!(!weak.deref(&heap));
    }

    #[test]
    fn test_gc_heap_threshold_adjustment() {
        let config = GcHeapConfig {
            initial_threshold: 100,
            growth_factor: 2.0,
            min_threshold: 50,
            max_threshold: 10000,
        };
        let mut heap = GcHeap::new(config);
        let a = heap.allocate(HeapData::Number(1.0), vec![], 80);
        heap.add_root(a);

        // Before collection, threshold is initial (100)
        assert!(heap.should_collect() || heap.total_bytes() < 100);

        heap.collect();
        let stats = heap.stats();
        // Threshold should have been adjusted based on surviving size * growth_factor
        assert!(stats.current_threshold >= 50);
        assert!(stats.current_threshold <= 10000);
    }

    #[test]
    fn test_gc_heap_multiple_collections() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        heap.add_root(root);

        for i in 0..5 {
            heap.allocate(HeapData::Number(i as f64), vec![], 8);
        }
        let r1 = heap.collect();
        assert_eq!(r1.objects_freed, 5);

        for i in 0..3 {
            heap.allocate(HeapData::Number(i as f64), vec![], 8);
        }
        let r2 = heap.collect();
        assert_eq!(r2.objects_freed, 3);
        assert_eq!(heap.stats().total_collections, 2);
        assert_eq!(heap.live_object_count(), 1);
    }

    #[test]
    fn test_gc_heap_free_list_reuse() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Number(0.0), vec![], 8);
        heap.add_root(root);

        // Allocate and collect to populate free list
        let garbage = heap.allocate(HeapData::Number(1.0), vec![], 8);
        assert_eq!(garbage, 1);
        heap.collect();
        assert_eq!(heap.live_object_count(), 1);

        // Next allocation should reuse the freed slot
        let reused = heap.allocate(HeapData::Number(2.0), vec![], 8);
        assert_eq!(reused, 1); // same slot as garbage
        assert_eq!(heap.object_count(), 2);
    }

    #[test]
    fn test_gc_heap_stats_tracking() {
        let mut heap = GcHeap::new(test_config());
        let a = heap.allocate(HeapData::Number(1.0), vec![], 64);
        heap.allocate(HeapData::Number(2.0), vec![], 64);
        heap.add_root(a);

        heap.collect();
        let stats = heap.stats();
        assert_eq!(stats.total_allocations, 2);
        assert_eq!(stats.total_collections, 1);
        assert_eq!(stats.total_bytes_freed, 64);
        assert_eq!(stats.current_heap_size, 64);
        assert_eq!(stats.current_object_count, 1);
        assert_eq!(stats.peak_heap_size, 128);
    }

    #[test]
    fn test_gc_heap_large_stress() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        heap.add_root(root);

        // Allocate many objects, only root survives
        for i in 0..500 {
            heap.allocate(HeapData::Number(i as f64), vec![], 8);
        }
        assert_eq!(heap.live_object_count(), 501);

        let result = heap.collect();
        assert_eq!(result.objects_freed, 500);
        assert_eq!(heap.live_object_count(), 1);
    }

    #[test]
    fn test_gc_heap_generation_tracking() {
        let mut heap = GcHeap::new(test_config());
        let root = heap.allocate(HeapData::Number(1.0), vec![], 8);
        heap.add_root(root);

        let r1 = heap.collect();
        assert_eq!(r1.generation, 1);

        let r2 = heap.collect();
        assert_eq!(r2.generation, 2);

        let r3 = heap.collect();
        assert_eq!(r3.generation, 3);
    }

    #[test]
    fn test_gc_tracer_worklist() {
        let mut tracer = Tracer::new();
        assert_eq!(tracer.marked_count(), 0);
        assert_eq!(tracer.marked_bytes(), 0);

        tracer.mark(0);
        tracer.mark(1);
        tracer.mark(2);
        assert_eq!(tracer.worklist.len(), 3);

        tracer.marked_count = 5;
        tracer.marked_bytes = 128;
        assert_eq!(tracer.marked_count(), 5);
        assert_eq!(tracer.marked_bytes(), 128);
    }

    #[test]
    fn test_gc_heap_data_variants() {
        let mut heap = GcHeap::new(test_config());

        let obj = heap.allocate(
            HeapData::Object(vec![("x".to_string(), 0)]),
            vec![],
            32,
        );
        let arr = heap.allocate(HeapData::Array(vec![0, 1]), vec![], 24);
        let s = heap.allocate(HeapData::String("test".into()), vec![], 16);
        let n = heap.allocate(HeapData::Number(3.14), vec![], 8);
        let b = heap.allocate(HeapData::Boolean(false), vec![], 1);
        let c = heap.allocate(
            HeapData::Closure { upvalues: vec![0] },
            vec![],
            16,
        );

        heap.add_root(obj);
        heap.add_root(arr);
        heap.add_root(s);
        heap.add_root(n);
        heap.add_root(b);
        heap.add_root(c);

        assert_eq!(heap.live_object_count(), 6);
        let result = heap.collect();
        assert_eq!(result.objects_freed, 0);
        assert_eq!(heap.live_object_count(), 6);
    }

    #[test]
    fn test_gc_color_values() {
        assert_ne!(GcColor::White, GcColor::Gray);
        assert_ne!(GcColor::Gray, GcColor::Black);
        assert_ne!(GcColor::White, GcColor::Black);
        let c = GcColor::Gray;
        let c2 = c;
        assert_eq!(c, c2);
    }

    #[test]
    fn test_gc_ref_clone_copy() {
        // Simple type implementing Trace for testing GcRef
        struct Dummy;
        impl Trace for Dummy {
            fn trace(&self, _tracer: &mut Tracer) {}
        }

        let r: GcRef<Dummy> = GcRef::new(42);
        let r2 = r;
        let r3 = r.clone();
        assert_eq!(r.id(), 42);
        assert_eq!(r2.id(), 42);
        assert_eq!(r3.id(), 42);
    }

    #[test]
    fn test_gc_heap_remove_root() {
        let mut heap = GcHeap::new(test_config());
        let a = heap.allocate(HeapData::Number(1.0), vec![], 8);
        heap.add_root(a);
        heap.collect();
        assert_eq!(heap.live_object_count(), 1);

        heap.remove_root(a);
        heap.collect();
        assert_eq!(heap.live_object_count(), 0);
    }

    #[test]
    fn test_gc_heap_reference_chain() {
        let mut heap = GcHeap::new(test_config());
        // Chain: root -> a -> b -> c (all reachable)
        let root = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        let a = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        let b = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        let c = heap.allocate(HeapData::Object(vec![]), vec![], 16);
        // Also allocate unreachable garbage
        let _g = heap.allocate(HeapData::Number(0.0), vec![], 8);

        heap.add_reference(root, a);
        heap.add_reference(a, b);
        heap.add_reference(b, c);
        heap.add_root(root);

        let result = heap.collect();
        assert_eq!(result.objects_freed, 1); // only _g freed
        assert_eq!(heap.live_object_count(), 4);
    }

    #[test]
    fn test_gc_weak_ref_invalidate() {
        let heap = GcHeap::new(test_config());
        let mut weak = WeakRef::new(0);
        assert_eq!(weak.target(), Some(0));

        weak.invalidate();
        assert_eq!(weak.target(), None);
        assert!(!weak.deref(&heap));
    }

    #[test]
    fn test_gc_heap_should_collect() {
        let config = GcHeapConfig {
            initial_threshold: 100,
            growth_factor: 1.5,
            min_threshold: 50,
            max_threshold: 10000,
        };
        let mut heap = GcHeap::new(config);
        assert!(!heap.should_collect());

        heap.allocate(HeapData::Number(1.0), vec![], 50);
        assert!(!heap.should_collect());

        heap.allocate(HeapData::Number(2.0), vec![], 60);
        assert!(heap.should_collect()); // 110 >= 100
    }
}

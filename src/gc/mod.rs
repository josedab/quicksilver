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

use crate::runtime::Value;
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
        self.heap.retain(|entry| {
            // Keep if marked OR if there are still strong references
            if entry.marked {
                return true;
            }
            // Check if the object is still alive (has strong refs)
            if entry.object.strong_count() > 0 {
                return true;
            }
            // Object is dead - allow it to be collected
            if self.config.verbose {
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
    fn mark_value(&self, value: &Value, marked: &mut HashSet<usize>) {
        match value {
            Value::Object(obj) => {
                // Find this object in our heap and mark it
                let obj_ptr = Rc::as_ptr(obj) as *const () as usize;
                for entry in &self.heap {
                    if let Some(strong) = entry.object.upgrade() {
                        let entry_ptr = Rc::as_ptr(&strong) as *const () as usize;
                        if entry_ptr == obj_ptr {
                            if marked.insert(entry.id) {
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
                            break;
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
        self.heap.retain(|entry| entry.object.strong_count() > 0);
        let removed = before - self.heap.len();
        if removed > 0 && self.config.verbose {
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
            prototype: None,
        }));
        let obj2 = Rc::new(RefCell::new(Object {
            kind: ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
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

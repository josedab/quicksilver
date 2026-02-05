//! Reactive State Primitives (Signals)
//!
//! Fine-grained reactivity system inspired by SolidJS signals. Provides
//! automatic dependency tracking, batched updates, and efficient change
//! propagation without a virtual DOM.
//!
//! # Example
//! ```text
//! let mut graph = ReactiveGraph::new();
//! let count = graph.create_signal(Value::Number(0.0));
//! let doubled = graph.create_computed(move |g| {
//!     match g.get_signal(count) {
//!         Value::Number(n) => Value::Number(n * 2.0),
//!         v => v,
//!     }
//! });
//! graph.create_effect(move |g| {
//!     println!("doubled = {}", g.get_signal(doubled));
//! });
//! graph.set_signal(count, Value::Number(5.0)); // logs "doubled = 10"
//! ```

//! **Status:** ✅ Complete — Reactive state management

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use crate::runtime::Value;

/// Unique identifier for a reactive node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub usize);

impl fmt::Display for SignalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signal({})", self.0)
    }
}

/// The kind of reactive node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A writable signal (source of reactivity)
    Signal,
    /// A computed/derived value
    Computed,
    /// A side effect
    Effect,
}

/// State of a reactive node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeState {
    /// Value is up to date
    Clean,
    /// Value may need recomputation (a dependency changed)
    Dirty,
    /// Currently being computed (for cycle detection)
    Computing,
}

/// Type for computed value functions
type ComputeFn = Rc<dyn Fn(&ReactiveGraph) -> Value>;

/// Type for effect functions
type EffectFn = Rc<dyn Fn(&ReactiveGraph)>;

/// A reactive node in the dependency graph
struct ReactiveNode {
    /// What kind of node this is
    kind: NodeKind,
    /// Current value
    value: Value,
    /// Current state
    state: NodeState,
    /// Nodes that this node depends on (sources) - RefCell for interior mutability
    sources: RefCell<Vec<SignalId>>,
    /// Nodes that depend on this node (observers) - RefCell for interior mutability
    observers: RefCell<Vec<SignalId>>,
    /// Computation function (for Computed and Effect nodes)
    compute: Option<ComputeFn>,
    /// Effect function (for Effect nodes)
    effect: Option<EffectFn>,
    /// Human-readable name for debugging
    name: Option<String>,
    /// Version counter (incremented on each change)
    version: u64,
}

impl fmt::Debug for ReactiveNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReactiveNode")
            .field("kind", &self.kind)
            .field("value", &self.value)
            .field("state", &self.state)
            .field("sources", &*self.sources.borrow())
            .field("observers", &*self.observers.borrow())
            .field("name", &self.name)
            .field("version", &self.version)
            .finish()
    }
}

/// The central reactive dependency graph
pub struct ReactiveGraph {
    /// All reactive nodes indexed by their SignalId
    nodes: Vec<ReactiveNode>,
    /// Currently tracking dependencies (RefCell for interior mutability in &self methods)
    tracking_stack: RefCell<Vec<SignalId>>,
    /// Batch depth counter (>0 means we're in a batch)
    batch_depth: u32,
    /// Pending effects to run after batch completes
    pending_effects: VecDeque<SignalId>,
    /// Whether we're currently flushing effects
    flushing: bool,
    /// Disposed signals (for reuse)
    free_list: Vec<usize>,
}

impl ReactiveGraph {
    /// Create a new reactive graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            tracking_stack: RefCell::new(Vec::new()),
            batch_depth: 0,
            pending_effects: VecDeque::new(),
            flushing: false,
            free_list: Vec::new(),
        }
    }

    /// Allocate a new node ID (reusing disposed slots)
    fn alloc_id(&mut self) -> SignalId {
        if let Some(idx) = self.free_list.pop() {
            SignalId(idx)
        } else {
            let id = SignalId(self.nodes.len());
            self.nodes.push(ReactiveNode {
                kind: NodeKind::Signal,
                value: Value::Undefined,
                state: NodeState::Clean,
                sources: RefCell::new(Vec::new()),
                observers: RefCell::new(Vec::new()),
                compute: None,
                effect: None,
                name: None,
                version: 0,
            });
            id
        }
    }

    /// Create a new signal with an initial value
    pub fn create_signal(&mut self, initial: Value) -> SignalId {
        let id = self.alloc_id();
        let node = &mut self.nodes[id.0];
        node.kind = NodeKind::Signal;
        node.value = initial;
        node.state = NodeState::Clean;
        node.version = 0;
        id
    }

    /// Create a named signal for debugging
    pub fn create_signal_named(&mut self, initial: Value, name: &str) -> SignalId {
        let id = self.create_signal(initial);
        self.nodes[id.0].name = Some(name.to_string());
        id
    }

    /// Create a computed signal (derived value)
    pub fn create_computed<F>(&mut self, compute: F) -> SignalId
    where
        F: Fn(&ReactiveGraph) -> Value + 'static,
    {
        let id = self.alloc_id();
        let compute_fn: ComputeFn = Rc::new(compute);
        let node = &mut self.nodes[id.0];
        node.kind = NodeKind::Computed;
        node.state = NodeState::Dirty;
        node.compute = Some(compute_fn);

        // Initial computation
        self.recompute(id);

        id
    }

    /// Create an effect (runs whenever dependencies change)
    pub fn create_effect<F>(&mut self, effect: F) -> SignalId
    where
        F: Fn(&ReactiveGraph) + 'static,
    {
        let id = self.alloc_id();
        let effect_fn: EffectFn = Rc::new(effect);
        let node = &mut self.nodes[id.0];
        node.kind = NodeKind::Effect;
        node.state = NodeState::Dirty;
        node.effect = Some(effect_fn);

        // Run effect initially
        self.run_effect(id);

        id
    }

    /// Get the current value of a signal, automatically tracking dependencies
    pub fn get_signal(&self, id: SignalId) -> Value {
        if id.0 >= self.nodes.len() {
            return Value::Undefined;
        }

        // Track dependency if we're inside a computation
        {
            let tracking = self.tracking_stack.borrow();
            if let Some(&tracker) = tracking.last() {
                drop(tracking); // Release borrow before mutating

                // Add id as source of tracker
                let mut sources = self.nodes[tracker.0].sources.borrow_mut();
                if !sources.contains(&id) {
                    sources.push(id);
                }
                drop(sources);

                // Add tracker as observer of id
                let mut observers = self.nodes[id.0].observers.borrow_mut();
                if !observers.contains(&tracker) {
                    observers.push(tracker);
                }
            }
        }

        self.nodes[id.0].value.clone()
    }

    /// Set the value of a signal, triggering updates
    pub fn set_signal(&mut self, id: SignalId, value: Value) {
        if id.0 >= self.nodes.len() { return; }
        if self.nodes[id.0].kind != NodeKind::Signal { return; }

        // Check if value actually changed
        if values_equal(&self.nodes[id.0].value, &value) {
            return;
        }

        self.nodes[id.0].value = value;
        self.nodes[id.0].version += 1;

        // Mark all observers as dirty
        let observers: Vec<SignalId> = self.nodes[id.0].observers.borrow().clone();
        for obs_id in observers {
            self.mark_dirty(obs_id);
        }

        // Flush effects if not in a batch
        if self.batch_depth == 0 {
            self.flush_effects();
        }
    }

    /// Update a signal using a function
    pub fn update_signal<F>(&mut self, id: SignalId, updater: F)
    where
        F: FnOnce(&Value) -> Value,
    {
        let current = self.get_signal(id);
        let new_value = updater(&current);
        self.set_signal(id, new_value);
    }

    /// Start a batch (defer effect execution)
    pub fn batch_start(&mut self) {
        self.batch_depth += 1;
    }

    /// End a batch (flush pending effects)
    pub fn batch_end(&mut self) {
        if self.batch_depth > 0 {
            self.batch_depth -= 1;
            if self.batch_depth == 0 {
                self.flush_effects();
            }
        }
    }

    /// Execute a function within a batch
    pub fn batch<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.batch_start();
        let result = f(self);
        self.batch_end();
        result
    }

    /// Dispose of a signal and clean up its dependencies
    pub fn dispose(&mut self, id: SignalId) {
        if id.0 >= self.nodes.len() { return; }

        // Remove from sources' observer lists
        let sources: Vec<SignalId> = self.nodes[id.0].sources.borrow().clone();
        for source_id in sources {
            self.nodes[source_id.0].observers.borrow_mut().retain(|o| *o != id);
        }

        // Remove from observers' source lists
        let observers: Vec<SignalId> = self.nodes[id.0].observers.borrow().clone();
        for obs_id in observers {
            self.nodes[obs_id.0].sources.borrow_mut().retain(|s| *s != id);
        }

        // Clear the node
        let node = &mut self.nodes[id.0];
        node.sources.borrow_mut().clear();
        node.observers.borrow_mut().clear();
        node.compute = None;
        node.effect = None;
        node.value = Value::Undefined;
        node.state = NodeState::Clean;

        // Add to free list for reuse
        self.free_list.push(id.0);
    }

    /// Mark a node as dirty and schedule effects
    fn mark_dirty(&mut self, id: SignalId) {
        if id.0 >= self.nodes.len() { return; }
        if self.nodes[id.0].state == NodeState::Dirty { return; }

        self.nodes[id.0].state = NodeState::Dirty;

        match self.nodes[id.0].kind {
            NodeKind::Effect => {
                if !self.pending_effects.contains(&id) {
                    self.pending_effects.push_back(id);
                }
            }
            NodeKind::Computed => {
                // Recompute immediately
                self.recompute(id);

                // Propagate to observers
                let observers: Vec<SignalId> = self.nodes[id.0].observers.borrow().clone();
                for obs_id in observers {
                    self.mark_dirty(obs_id);
                }
            }
            NodeKind::Signal => {} // Signals don't get marked dirty by propagation
        }
    }

    /// Recompute a computed node's value
    fn recompute(&mut self, id: SignalId) {
        if id.0 >= self.nodes.len() { return; }

        // Cycle detection
        if self.nodes[id.0].state == NodeState::Computing {
            return; // Break cycle silently
        }

        let compute_fn = match self.nodes[id.0].compute.clone() {
            Some(f) => f,
            None => return,
        };

        // Clear old sources (we'll re-track via get_signal's automatic tracking)
        let old_sources: Vec<SignalId> = self.nodes[id.0].sources.borrow().clone();
        for source in &old_sources {
            self.nodes[source.0].observers.borrow_mut().retain(|o| *o != id);
        }
        self.nodes[id.0].sources.borrow_mut().clear();

        // Push tracking context
        self.nodes[id.0].state = NodeState::Computing;
        self.tracking_stack.borrow_mut().push(id);

        // Compute new value - get_signal will automatically track dependencies
        let new_value = compute_fn(self);

        // Pop tracking context
        self.tracking_stack.borrow_mut().pop();

        let old_value = &self.nodes[id.0].value;
        let changed = !values_equal(old_value, &new_value);

        self.nodes[id.0].value = new_value;
        self.nodes[id.0].state = NodeState::Clean;

        if changed {
            self.nodes[id.0].version += 1;
        }
    }

    /// Run an effect
    fn run_effect(&mut self, id: SignalId) {
        if id.0 >= self.nodes.len() { return; }

        let effect_fn = match self.nodes[id.0].effect.clone() {
            Some(f) => f,
            None => return,
        };

        // Clear old dependencies
        let old_sources: Vec<SignalId> = self.nodes[id.0].sources.borrow().clone();
        for source in &old_sources {
            self.nodes[source.0].observers.borrow_mut().retain(|o| *o != id);
        }
        self.nodes[id.0].sources.borrow_mut().clear();

        // Track dependencies via get_signal's automatic tracking
        self.tracking_stack.borrow_mut().push(id);
        effect_fn(self);
        self.tracking_stack.borrow_mut().pop();

        self.nodes[id.0].state = NodeState::Clean;
    }

    /// Flush all pending effects
    fn flush_effects(&mut self) {
        if self.flushing { return; }
        self.flushing = true;

        while let Some(id) = self.pending_effects.pop_front() {
            if self.nodes[id.0].state == NodeState::Dirty {
                self.run_effect(id);
            }
        }

        self.flushing = false;
    }

    /// Get the number of nodes in the graph
    pub fn node_count(&self) -> usize {
        self.nodes.len() - self.free_list.len()
    }

    /// Get the version of a signal
    pub fn get_version(&self, id: SignalId) -> u64 {
        if id.0 < self.nodes.len() {
            self.nodes[id.0].version
        } else {
            0
        }
    }

    /// Get the kind of a node
    pub fn get_kind(&self, id: SignalId) -> Option<NodeKind> {
        if id.0 < self.nodes.len() {
            Some(self.nodes[id.0].kind)
        } else {
            None
        }
    }

    /// Get the name of a node (if set)
    pub fn get_name(&self, id: SignalId) -> Option<&str> {
        if id.0 < self.nodes.len() {
            self.nodes[id.0].name.as_deref()
        } else {
            None
        }
    }

    /// Get the observer count for a signal
    pub fn observer_count(&self, id: SignalId) -> usize {
        if id.0 < self.nodes.len() {
            self.nodes[id.0].observers.borrow().len()
        } else {
            0
        }
    }

    /// Get the source count for a node
    pub fn source_count(&self, id: SignalId) -> usize {
        if id.0 < self.nodes.len() {
            self.nodes[id.0].sources.borrow().len()
        } else {
            0
        }
    }

    /// Debug dump of the reactive graph
    pub fn debug_dump(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("ReactiveGraph ({} nodes, {} free)\n", self.nodes.len(), self.free_list.len()));

        for (i, node) in self.nodes.iter().enumerate() {
            if self.free_list.contains(&i) { continue; }
            let name = node.name.as_deref().unwrap_or("<unnamed>");
            s.push_str(&format!(
                "  [{}] {:?} '{}' = {:?} (v{}) sources={:?} observers={:?}\n",
                i, node.kind, name, node.value, node.version,
                *node.sources.borrow(), *node.observers.borrow()
            ));
        }

        s
    }
}

impl Default for ReactiveGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple value equality check
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}

/// A memo is a computed value that caches and only recomputes when dependencies change
pub fn create_memo<F>(graph: &mut ReactiveGraph, compute: F) -> SignalId
where
    F: Fn(&ReactiveGraph) -> Value + 'static,
{
    graph.create_computed(compute)
}

/// Helper to create a signal from a number
pub fn signal_number(graph: &mut ReactiveGraph, n: f64) -> SignalId {
    graph.create_signal(Value::Number(n))
}

/// Helper to create a signal from a string
pub fn signal_string(graph: &mut ReactiveGraph, s: &str) -> SignalId {
    graph.create_signal(Value::String(s.to_string()))
}

/// Helper to create a signal from a boolean
pub fn signal_bool(graph: &mut ReactiveGraph, b: bool) -> SignalId {
    graph.create_signal(Value::Boolean(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_signal() {
        let mut graph = ReactiveGraph::new();
        let id = graph.create_signal(Value::Number(42.0));
        assert!(matches!(graph.get_signal(id), Value::Number(n) if n == 42.0));
    }

    #[test]
    fn test_set_signal() {
        let mut graph = ReactiveGraph::new();
        let id = graph.create_signal(Value::Number(1.0));
        graph.set_signal(id, Value::Number(2.0));
        assert!(matches!(graph.get_signal(id), Value::Number(n) if n == 2.0));
    }

    #[test]
    fn test_computed_signal() {
        let mut graph = ReactiveGraph::new();
        let a = graph.create_signal(Value::Number(2.0));
        let b = graph.create_signal(Value::Number(3.0));

        let sum = graph.create_computed(move |g| {
            match (g.get_signal(a), g.get_signal(b)) {
                (Value::Number(x), Value::Number(y)) => Value::Number(x + y),
                _ => Value::Undefined,
            }
        });

        assert!(matches!(graph.get_signal(sum), Value::Number(n) if n == 5.0));
    }

    #[test]
    fn test_signal_update_propagation() {
        let mut graph = ReactiveGraph::new();
        let count = graph.create_signal(Value::Number(0.0));

        let doubled = graph.create_computed(move |g| {
            match g.get_signal(count) {
                Value::Number(n) => Value::Number(n * 2.0),
                v => v,
            }
        });

        assert!(matches!(graph.get_signal(doubled), Value::Number(n) if n == 0.0));

        graph.set_signal(count, Value::Number(5.0));
        assert!(matches!(graph.get_signal(doubled), Value::Number(n) if n == 10.0));

        graph.set_signal(count, Value::Number(10.0));
        assert!(matches!(graph.get_signal(doubled), Value::Number(n) if n == 20.0));
    }

    #[test]
    fn test_effect_runs_on_change() {
        let mut graph = ReactiveGraph::new();
        let count = graph.create_signal(Value::Number(0.0));
        let log = Rc::new(RefCell::new(Vec::new()));
        let log_clone = log.clone();

        graph.create_effect(move |g| {
            let val = g.get_signal(count);
            log_clone.borrow_mut().push(format!("{:?}", val));
        });

        // Effect runs initially
        assert_eq!(log.borrow().len(), 1);

        graph.set_signal(count, Value::Number(1.0));
        assert_eq!(log.borrow().len(), 2);

        graph.set_signal(count, Value::Number(2.0));
        assert_eq!(log.borrow().len(), 3);
    }

    #[test]
    fn test_no_update_on_same_value() {
        let mut graph = ReactiveGraph::new();
        let s = graph.create_signal(Value::Number(42.0));

        let version_before = graph.get_version(s);
        graph.set_signal(s, Value::Number(42.0)); // Same value
        let version_after = graph.get_version(s);

        assert_eq!(version_before, version_after);
    }

    #[test]
    fn test_batch_updates() {
        let mut graph = ReactiveGraph::new();
        let a = graph.create_signal(Value::Number(1.0));
        let b = graph.create_signal(Value::Number(2.0));
        let effect_count = Rc::new(RefCell::new(0u32));
        let ec = effect_count.clone();

        graph.create_effect(move |g| {
            let _ = g.get_signal(a);
            let _ = g.get_signal(b);
            *ec.borrow_mut() += 1;
        });

        // Effect runs once initially
        assert_eq!(*effect_count.borrow(), 1);

        // Batch update - effect should only run once at the end
        graph.batch(|g| {
            g.set_signal(a, Value::Number(10.0));
            g.set_signal(b, Value::Number(20.0));
        });

        // Effect ran once more during the batch flush
        assert!(*effect_count.borrow() >= 2);
    }

    #[test]
    fn test_dispose_signal() {
        let mut graph = ReactiveGraph::new();
        let s = graph.create_signal(Value::Number(1.0));
        let c = graph.create_computed(move |g| g.get_signal(s));

        assert_eq!(graph.node_count(), 2);

        graph.dispose(c);
        // After disposal, the signal should have no observers from c
        assert_eq!(graph.observer_count(s), 0);
    }

    #[test]
    fn test_update_signal() {
        let mut graph = ReactiveGraph::new();
        let count = graph.create_signal(Value::Number(0.0));

        graph.update_signal(count, |v| {
            match v {
                Value::Number(n) => Value::Number(n + 1.0),
                _ => Value::Number(1.0),
            }
        });

        assert!(matches!(graph.get_signal(count), Value::Number(n) if n == 1.0));
    }

    #[test]
    fn test_named_signal() {
        let mut graph = ReactiveGraph::new();
        let id = graph.create_signal_named(Value::Number(0.0), "counter");
        assert_eq!(graph.get_name(id), Some("counter"));
    }

    #[test]
    fn test_node_kind() {
        let mut graph = ReactiveGraph::new();
        let s = graph.create_signal(Value::Number(0.0));
        let c = graph.create_computed(move |g| g.get_signal(s));

        assert_eq!(graph.get_kind(s), Some(NodeKind::Signal));
        assert_eq!(graph.get_kind(c), Some(NodeKind::Computed));
    }

    #[test]
    fn test_signal_helpers() {
        let mut graph = ReactiveGraph::new();
        let n = signal_number(&mut graph, 42.0);
        let s = signal_string(&mut graph, "hello");
        let b = signal_bool(&mut graph, true);

        assert!(matches!(graph.get_signal(n), Value::Number(x) if x == 42.0));
        assert!(matches!(graph.get_signal(s), Value::String(ref x) if x == "hello"));
        assert!(matches!(graph.get_signal(b), Value::Boolean(true)));
    }

    #[test]
    fn test_values_equal() {
        assert!(values_equal(&Value::Undefined, &Value::Undefined));
        assert!(values_equal(&Value::Null, &Value::Null));
        assert!(values_equal(&Value::Number(1.0), &Value::Number(1.0)));
        assert!(!values_equal(&Value::Number(1.0), &Value::Number(2.0)));
        assert!(values_equal(&Value::String("a".into()), &Value::String("a".into())));
        assert!(!values_equal(&Value::String("a".into()), &Value::String("b".into())));
        assert!(values_equal(&Value::Boolean(true), &Value::Boolean(true)));
        assert!(!values_equal(&Value::Number(1.0), &Value::String("1".into())));
    }

    #[test]
    fn test_debug_dump() {
        let mut graph = ReactiveGraph::new();
        let _s = graph.create_signal_named(Value::Number(0.0), "count");
        let dump = graph.debug_dump();
        assert!(dump.contains("count"));
        assert!(dump.contains("Signal"));
    }

    #[test]
    fn test_chained_computed() {
        let mut graph = ReactiveGraph::new();
        let base = graph.create_signal(Value::Number(1.0));

        let times2 = graph.create_computed(move |g| {
            match g.get_signal(base) {
                Value::Number(n) => Value::Number(n * 2.0),
                v => v,
            }
        });

        let times4 = graph.create_computed(move |g| {
            match g.get_signal(times2) {
                Value::Number(n) => Value::Number(n * 2.0),
                v => v,
            }
        });

        assert!(matches!(graph.get_signal(times4), Value::Number(n) if n == 4.0));

        graph.set_signal(base, Value::Number(3.0));
        assert!(matches!(graph.get_signal(times4), Value::Number(n) if n == 12.0));
    }

    #[test]
    fn test_slot_reuse() {
        let mut graph = ReactiveGraph::new();
        let s1 = graph.create_signal(Value::Number(1.0));
        let s2 = graph.create_signal(Value::Number(2.0));

        graph.dispose(s1);

        // Next signal should reuse the disposed slot
        let s3 = graph.create_signal(Value::Number(3.0));
        assert_eq!(s3.0, s1.0); // Reused the same index

        // s2 should still work
        assert!(matches!(graph.get_signal(s2), Value::Number(n) if n == 2.0));
        assert!(matches!(graph.get_signal(s3), Value::Number(n) if n == 3.0));
    }
}

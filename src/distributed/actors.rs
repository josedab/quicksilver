//! Distributed Actor System
//!
//! Production-grade actor system with Erlang-style supervision, priority-aware
//! mailboxes, message routing, and lifecycle management.
//!
//! # Features
//! - Hierarchical actor supervision (OneForOne, OneForAll, RestForOne, Escalate)
//! - Priority-aware mailboxes with configurable overflow strategies
//! - Request-response messaging patterns
//! - Dead letter handling for undeliverable messages
//! - Actor routing with multiple strategies (RoundRobin, Random, Broadcast, ConsistentHash)
//! - Comprehensive lifecycle management and statistics

use rustc_hash::FxHashMap as HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Unique actor identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorId(pub u64);

impl ActorId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for ActorId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "actor-{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Priority levels for actor messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// Lowest priority
    Low = 0,
    /// Default priority
    Normal = 1,
    /// Elevated priority
    High = 2,
    /// Highest priority, reserved for system messages
    System = 3,
}

/// Application-level actor messages
#[derive(Debug, Clone)]
pub enum ActorMessage {
    /// Free-form user message
    User(serde_json::Value),
    /// Request expecting a response
    Request {
        id: String,
        payload: serde_json::Value,
    },
    /// Response to a prior request
    Response {
        id: String,
        payload: serde_json::Value,
    },
    /// Broadcast message
    Broadcast(serde_json::Value),
    /// Forward a message to another actor
    Forward {
        target: ActorId,
        message: Box<ActorMessage>,
    },
}

/// Envelope wrapping a message with metadata
#[derive(Debug, Clone)]
pub struct Envelope {
    pub sender: Option<ActorId>,
    pub message: ActorMessage,
    pub priority: MessagePriority,
    pub timestamp: Instant,
}

/// System-level control messages
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemMessage {
    Start,
    Stop,
    Restart,
    Suspend,
    Resume,
    Watch(ActorId),
    Unwatch(ActorId),
    Terminate,
}

// ---------------------------------------------------------------------------
// Mailbox
// ---------------------------------------------------------------------------

/// Strategy when the mailbox is full
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowStrategy {
    /// Drop the newest incoming message
    DropNewest,
    /// Drop the oldest message in the queue
    DropOldest,
    /// Return an error to the sender
    RejectNew,
}

/// Priority-aware message queue for an actor
#[derive(Debug)]
pub struct Mailbox {
    queue: VecDeque<Envelope>,
    system_queue: VecDeque<SystemMessage>,
    capacity: usize,
    overflow_strategy: OverflowStrategy,
}

impl Mailbox {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            system_queue: VecDeque::new(),
            capacity,
            overflow_strategy: OverflowStrategy::DropOldest,
        }
    }

    pub fn with_overflow_strategy(mut self, strategy: OverflowStrategy) -> Self {
        self.overflow_strategy = strategy;
        self
    }

    /// Enqueue an envelope respecting capacity and overflow strategy.
    /// Returns `true` if the message was accepted.
    pub fn enqueue(&mut self, envelope: Envelope) -> bool {
        if self.queue.len() >= self.capacity {
            match self.overflow_strategy {
                OverflowStrategy::DropNewest => return false,
                OverflowStrategy::DropOldest => {
                    self.queue.pop_front();
                }
                OverflowStrategy::RejectNew => return false,
            }
        }
        // Insert sorted by priority (higher priority toward front)
        let pos = self
            .queue
            .iter()
            .position(|e| e.priority < envelope.priority);
        match pos {
            Some(idx) => self.queue.insert(idx, envelope),
            None => self.queue.push_back(envelope),
        }
        true
    }

    /// Dequeue the next envelope (highest priority first)
    pub fn dequeue(&mut self) -> Option<Envelope> {
        self.queue.pop_front()
    }

    /// Push a system message (always accepted)
    pub fn push_system(&mut self, msg: SystemMessage) {
        self.system_queue.push_back(msg);
    }

    /// Pop the next system message
    pub fn pop_system(&mut self) -> Option<SystemMessage> {
        self.system_queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn system_len(&self) -> usize {
        self.system_queue.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Actor lifecycle states
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorLifecycle {
    Created,
    Starting,
    Running,
    Suspended,
    Stopping,
    Stopped,
    Failed(String),
}

// ---------------------------------------------------------------------------
// Actor cell
// ---------------------------------------------------------------------------

/// Internal representation of a live actor inside the system
#[derive(Debug)]
pub struct ActorCell {
    pub id: ActorId,
    pub name: String,
    pub mailbox: Mailbox,
    pub state: ActorLifecycle,
    pub parent: Option<ActorId>,
    pub children: Vec<ActorId>,
    pub restart_count: u32,
    pub created_at: Instant,
    pub last_message_at: Option<Instant>,
    pub message_count: u64,
}

impl ActorCell {
    pub fn new(id: ActorId, name: String, mailbox_capacity: usize) -> Self {
        Self {
            id,
            name,
            mailbox: Mailbox::new(mailbox_capacity),
            state: ActorLifecycle::Created,
            parent: None,
            children: Vec::new(),
            restart_count: 0,
            created_at: Instant::now(),
            last_message_at: None,
            message_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Supervision
// ---------------------------------------------------------------------------

/// Supervision strategy type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisionType {
    /// Restart only the failed child
    OneForOne,
    /// Restart all children when one fails
    OneForAll,
    /// Restart the failed child and all children started after it
    RestForOne,
    /// Escalate the failure to the parent supervisor
    Escalate,
}

/// Erlang-style supervisor strategy
#[derive(Debug, Clone)]
pub struct SupervisorStrategy {
    pub strategy: SupervisionType,
    pub max_restarts: u32,
    pub within_duration: Duration,
    pub restart_history: Vec<Instant>,
}

impl SupervisorStrategy {
    pub fn new(strategy: SupervisionType, max_restarts: u32, within: Duration) -> Self {
        Self {
            strategy,
            max_restarts,
            within_duration: within,
            restart_history: Vec::new(),
        }
    }

    /// Record a restart and return `true` if the limit has been exceeded.
    pub fn record_restart(&mut self) -> bool {
        let now = Instant::now();
        self.restart_history
            .retain(|t| now.duration_since(*t) < self.within_duration);
        self.restart_history.push(now);
        self.restart_history.len() as u32 > self.max_restarts
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Message routing strategies
#[derive(Debug, Clone)]
pub enum RoutingStrategy {
    RoundRobin { index: usize },
    Random,
    Broadcast,
    ConsistentHash,
}

/// Routes messages to groups of actors
#[derive(Debug)]
pub struct ActorRouter {
    routes: HashMap<String, Vec<ActorId>>,
    strategy: RoutingStrategy,
}

impl ActorRouter {
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self {
            routes: HashMap::default(),
            strategy,
        }
    }

    /// Register an actor for a given topic
    pub fn add_route(&mut self, topic: &str, actor_id: ActorId) {
        self.routes
            .entry(topic.to_string())
            .or_default()
            .push(actor_id);
    }

    /// Remove an actor from a topic
    pub fn remove_route(&mut self, topic: &str, actor_id: ActorId) {
        if let Some(actors) = self.routes.get_mut(topic) {
            actors.retain(|id| *id != actor_id);
        }
    }

    /// Resolve the target actor(s) for a topic
    pub fn resolve(&mut self, topic: &str) -> Vec<ActorId> {
        let actors = match self.routes.get(topic) {
            Some(a) if !a.is_empty() => a,
            _ => return Vec::new(),
        };

        match &mut self.strategy {
            RoutingStrategy::RoundRobin { index } => {
                let id = actors[*index % actors.len()];
                *index = index.wrapping_add(1);
                vec![id]
            }
            RoutingStrategy::Random => {
                // Deterministic "random" based on timestamp nanos
                let pick = Instant::now().elapsed().subsec_nanos() as usize % actors.len();
                vec![actors[pick]]
            }
            RoutingStrategy::Broadcast => actors.clone(),
            RoutingStrategy::ConsistentHash => {
                // Simple hash: use topic length as hash key
                let hash = topic.len();
                vec![actors[hash % actors.len()]]
            }
        }
    }

    pub fn topics(&self) -> Vec<String> {
        self.routes.keys().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Dead letters & stats
// ---------------------------------------------------------------------------

/// A message that could not be delivered
#[derive(Debug, Clone)]
pub struct DeadLetter {
    pub target: ActorId,
    pub message: ActorMessage,
    pub reason: String,
    pub timestamp: Instant,
}

/// Aggregate statistics for the actor system
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct ActorSystemStats {
    pub total_actors_created: u64,
    pub total_messages_sent: u64,
    pub total_messages_delivered: u64,
    pub total_dead_letters: u64,
    pub total_restarts: u64,
    pub total_failures: u64,
}


// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the actor system
#[derive(Debug, Clone)]
pub struct ActorSystemConfig {
    pub default_mailbox_capacity: usize,
    pub max_actors: usize,
    pub dead_letter_capacity: usize,
    pub default_overflow_strategy: OverflowStrategy,
}

impl Default for ActorSystemConfig {
    fn default() -> Self {
        Self {
            default_mailbox_capacity: 1000,
            max_actors: 10_000,
            dead_letter_capacity: 1000,
            default_overflow_strategy: OverflowStrategy::DropOldest,
        }
    }
}

// ---------------------------------------------------------------------------
// ActorSystem
// ---------------------------------------------------------------------------

/// Central coordinator for the actor system
#[derive(Debug)]
pub struct ActorSystem {
    actors: HashMap<ActorId, ActorCell>,
    supervisors: HashMap<ActorId, SupervisorStrategy>,
    dead_letters: Vec<DeadLetter>,
    config: ActorSystemConfig,
    stats: ActorSystemStats,
}

impl ActorSystem {
    /// Create a new actor system with the given configuration
    pub fn new(config: ActorSystemConfig) -> Self {
        Self {
            actors: HashMap::default(),
            supervisors: HashMap::default(),
            dead_letters: Vec::new(),
            config,
            stats: ActorSystemStats::default(),
        }
    }

    /// Spawn a new actor, returning its id
    pub fn spawn_actor(&mut self, name: &str, parent: Option<ActorId>) -> Result<ActorId, String> {
        if self.actors.len() >= self.config.max_actors {
            return Err("actor system capacity reached".to_string());
        }

        let id = ActorId::new();
        let mut cell = ActorCell::new(id, name.to_string(), self.config.default_mailbox_capacity);
        cell.parent = parent;
        cell.state = ActorLifecycle::Running;

        // Register as child of parent
        if let Some(pid) = parent {
            if let Some(parent_cell) = self.actors.get_mut(&pid) {
                parent_cell.children.push(id);
            }
        }

        self.actors.insert(id, cell);
        self.stats.total_actors_created += 1;
        Ok(id)
    }

    /// Stop an actor and all of its children
    pub fn stop_actor(&mut self, id: ActorId) -> Result<(), String> {
        // Collect children first to avoid borrow issues
        let children = self
            .actors
            .get(&id)
            .map(|c| c.children.clone())
            .unwrap_or_default();

        // Recursively stop children
        for child in children {
            let _ = self.stop_actor(child);
        }

        if let Some(cell) = self.actors.get_mut(&id) {
            cell.state = ActorLifecycle::Stopped;

            // Remove from parent's children list
            if let Some(pid) = cell.parent {
                if let Some(parent_cell) = self.actors.get_mut(&pid) {
                    parent_cell.children.retain(|c| *c != id);
                }
            }

            self.actors.remove(&id);
            Ok(())
        } else {
            Err(format!("actor {} not found", id))
        }
    }

    /// Send a message to an actor
    pub fn send_message(
        &mut self,
        target: ActorId,
        sender: Option<ActorId>,
        message: ActorMessage,
        priority: MessagePriority,
    ) -> Result<(), String> {
        self.stats.total_messages_sent += 1;

        // Handle Forward messages by re-routing
        if let ActorMessage::Forward {
            target: fwd_target,
            message: fwd_msg,
        } = message
        {
            return self.send_message(fwd_target, sender, *fwd_msg, priority);
        }

        if let Some(cell) = self.actors.get_mut(&target) {
            if cell.state != ActorLifecycle::Running {
                self.record_dead_letter(target, message, "actor not running".to_string());
                return Err("actor not running".to_string());
            }

            let envelope = Envelope {
                sender,
                message: message.clone(),
                priority,
                timestamp: Instant::now(),
            };

            if cell.mailbox.enqueue(envelope) {
                cell.message_count += 1;
                cell.last_message_at = Some(Instant::now());
                self.stats.total_messages_delivered += 1;
                Ok(())
            } else {
                self.record_dead_letter(target, message, "mailbox full".to_string());
                Err("mailbox full".to_string())
            }
        } else {
            self.record_dead_letter(target, message, "actor not found".to_string());
            Err(format!("actor {} not found", target))
        }
    }

    /// Request-response pattern: send a request and get back the request id
    pub fn ask(
        &mut self,
        target: ActorId,
        sender: ActorId,
        payload: serde_json::Value,
    ) -> Result<String, String> {
        static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = format!("req-{}", REQ_COUNTER.fetch_add(1, Ordering::SeqCst));
        let message = ActorMessage::Request {
            id: id.clone(),
            payload,
        };
        self.send_message(target, Some(sender), message, MessagePriority::Normal)?;
        Ok(id)
    }

    /// Assign a supervisor strategy to an actor
    pub fn set_supervisor(&mut self, actor_id: ActorId, strategy: SupervisorStrategy) {
        self.supervisors.insert(actor_id, strategy);
    }

    /// Process the next message in an actor's mailbox, returning it
    pub fn process_mailbox(&mut self, actor_id: ActorId) -> Option<Envelope> {
        // Process system messages first
        if let Some(cell) = self.actors.get_mut(&actor_id) {
            if let Some(sys_msg) = cell.mailbox.pop_system() {
                match sys_msg {
                    SystemMessage::Stop => cell.state = ActorLifecycle::Stopping,
                    SystemMessage::Suspend => cell.state = ActorLifecycle::Suspended,
                    SystemMessage::Resume => cell.state = ActorLifecycle::Running,
                    SystemMessage::Restart => {
                        cell.state = ActorLifecycle::Starting;
                        cell.restart_count += 1;
                        cell.state = ActorLifecycle::Running;
                    }
                    SystemMessage::Terminate => cell.state = ActorLifecycle::Stopped,
                    _ => {}
                }
            }
        }

        self.actors
            .get_mut(&actor_id)
            .and_then(|cell| cell.mailbox.dequeue())
    }

    /// Handle a failure for a supervised actor
    pub fn handle_actor_failure(
        &mut self,
        actor_id: ActorId,
        error: &str,
    ) -> Result<(), String> {
        self.stats.total_failures += 1;

        // Mark as failed
        if let Some(cell) = self.actors.get_mut(&actor_id) {
            cell.state = ActorLifecycle::Failed(error.to_string());
        }

        // Find the supervisor (the actor's parent)
        let parent = self
            .actors
            .get(&actor_id)
            .and_then(|c| c.parent);

        let parent_id = match parent {
            Some(pid) => pid,
            None => return Err("no supervisor for actor".to_string()),
        };

        let strategy = match self.supervisors.get_mut(&parent_id) {
            Some(s) => s,
            None => return Err("parent has no supervisor strategy".to_string()),
        };

        let exceeded = strategy.record_restart();
        if exceeded {
            return Err("max restarts exceeded".to_string());
        }

        let strategy_type = strategy.strategy;
        self.stats.total_restarts += 1;

        match strategy_type {
            SupervisionType::OneForOne => {
                self.restart_actor(actor_id);
            }
            SupervisionType::OneForAll => {
                let siblings = self
                    .actors
                    .get(&parent_id)
                    .map(|c| c.children.clone())
                    .unwrap_or_default();
                for child in siblings {
                    self.restart_actor(child);
                }
            }
            SupervisionType::RestForOne => {
                let siblings = self
                    .actors
                    .get(&parent_id)
                    .map(|c| c.children.clone())
                    .unwrap_or_default();
                let mut found = false;
                for child in siblings {
                    if child == actor_id {
                        found = true;
                    }
                    if found {
                        self.restart_actor(child);
                    }
                }
            }
            SupervisionType::Escalate => {
                // Propagate the failure upward
                return self.handle_actor_failure(parent_id, error);
            }
        }

        Ok(())
    }

    /// Route a message through a router to a topic
    pub fn route_message(
        &mut self,
        router: &mut ActorRouter,
        topic: &str,
        sender: Option<ActorId>,
        message: ActorMessage,
        priority: MessagePriority,
    ) -> Result<usize, String> {
        let targets = router.resolve(topic);
        if targets.is_empty() {
            return Err(format!("no routes for topic '{}'", topic));
        }
        let count = targets.len();
        for target in targets {
            self.send_message(target, sender, message.clone(), priority)?;
        }
        Ok(count)
    }

    /// Get statistics snapshot for an actor
    pub fn get_actor_stats(&self, actor_id: ActorId) -> Option<ActorCellStats> {
        self.actors.get(&actor_id).map(|cell| ActorCellStats {
            id: cell.id,
            name: cell.name.clone(),
            state: cell.state.clone(),
            mailbox_size: cell.mailbox.len(),
            message_count: cell.message_count,
            restart_count: cell.restart_count,
            children_count: cell.children.len(),
        })
    }

    /// Get overall system statistics
    pub fn stats(&self) -> &ActorSystemStats {
        &self.stats
    }

    /// Get the number of live actors
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }

    /// Get all dead letters
    pub fn dead_letters(&self) -> &[DeadLetter] {
        &self.dead_letters
    }

    // ---- internal helpers ----

    fn restart_actor(&mut self, id: ActorId) {
        if let Some(cell) = self.actors.get_mut(&id) {
            cell.state = ActorLifecycle::Running;
            cell.restart_count += 1;
        }
    }

    fn record_dead_letter(&mut self, target: ActorId, message: ActorMessage, reason: String) {
        self.stats.total_dead_letters += 1;
        if self.dead_letters.len() >= self.config.dead_letter_capacity {
            self.dead_letters.remove(0);
        }
        self.dead_letters.push(DeadLetter {
            target,
            message,
            reason,
            timestamp: Instant::now(),
        });
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new(ActorSystemConfig::default())
    }
}

/// Snapshot of per-actor statistics
#[derive(Debug, Clone)]
pub struct ActorCellStats {
    pub id: ActorId,
    pub name: String,
    pub state: ActorLifecycle,
    pub mailbox_size: usize,
    pub message_count: u64,
    pub restart_count: u32,
    pub children_count: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn system() -> ActorSystem {
        ActorSystem::default()
    }

    // ---- spawn / stop ----

    #[test]
    fn test_spawn_actor() {
        let mut sys = system();
        let id = sys.spawn_actor("worker", None).unwrap();
        assert_eq!(sys.actor_count(), 1);
        let stats = sys.get_actor_stats(id).unwrap();
        assert_eq!(stats.name, "worker");
        assert_eq!(stats.state, ActorLifecycle::Running);
    }

    #[test]
    fn test_spawn_with_parent() {
        let mut sys = system();
        let parent = sys.spawn_actor("supervisor", None).unwrap();
        let child = sys.spawn_actor("child", Some(parent)).unwrap();
        let ps = sys.get_actor_stats(parent).unwrap();
        assert_eq!(ps.children_count, 1);
        assert!(sys.get_actor_stats(child).is_some());
    }

    #[test]
    fn test_stop_actor() {
        let mut sys = system();
        let id = sys.spawn_actor("temp", None).unwrap();
        sys.stop_actor(id).unwrap();
        assert_eq!(sys.actor_count(), 0);
    }

    #[test]
    fn test_stop_actor_cascades_to_children() {
        let mut sys = system();
        let parent = sys.spawn_actor("parent", None).unwrap();
        let _child = sys.spawn_actor("child", Some(parent)).unwrap();
        sys.stop_actor(parent).unwrap();
        assert_eq!(sys.actor_count(), 0);
    }

    // ---- messaging ----

    #[test]
    fn test_send_and_receive_message() {
        let mut sys = system();
        let id = sys.spawn_actor("echo", None).unwrap();

        sys.send_message(
            id,
            None,
            ActorMessage::User(serde_json::json!("hello")),
            MessagePriority::Normal,
        )
        .unwrap();

        let env = sys.process_mailbox(id).unwrap();
        if let ActorMessage::User(v) = env.message {
            assert_eq!(v, serde_json::json!("hello"));
        } else {
            panic!("expected User message");
        }
    }

    #[test]
    fn test_priority_ordering() {
        let mut sys = system();
        let id = sys.spawn_actor("prio", None).unwrap();

        sys.send_message(
            id,
            None,
            ActorMessage::User(serde_json::json!("low")),
            MessagePriority::Low,
        )
        .unwrap();
        sys.send_message(
            id,
            None,
            ActorMessage::User(serde_json::json!("high")),
            MessagePriority::High,
        )
        .unwrap();

        let first = sys.process_mailbox(id).unwrap();
        if let ActorMessage::User(v) = first.message {
            assert_eq!(v, serde_json::json!("high"));
        } else {
            panic!("expected high-priority first");
        }
    }

    #[test]
    fn test_ask_request_response() {
        let mut sys = system();
        let server = sys.spawn_actor("server", None).unwrap();
        let client = sys.spawn_actor("client", None).unwrap();

        let req_id = sys
            .ask(server, client, serde_json::json!({"cmd": "ping"}))
            .unwrap();
        assert!(req_id.starts_with("req-"));

        let env = sys.process_mailbox(server).unwrap();
        if let ActorMessage::Request { id, payload } = env.message {
            assert_eq!(id, req_id);
            assert_eq!(payload, serde_json::json!({"cmd": "ping"}));
        } else {
            panic!("expected Request message");
        }
    }

    // ---- supervision ----

    #[test]
    fn test_one_for_one_supervision() {
        let mut sys = system();
        let sup = sys.spawn_actor("sup", None).unwrap();
        sys.set_supervisor(
            sup,
            SupervisorStrategy::new(SupervisionType::OneForOne, 5, Duration::from_secs(60)),
        );
        let child = sys.spawn_actor("w1", Some(sup)).unwrap();
        let _child2 = sys.spawn_actor("w2", Some(sup)).unwrap();

        sys.handle_actor_failure(child, "crash").unwrap();
        assert_eq!(
            sys.get_actor_stats(child).unwrap().state,
            ActorLifecycle::Running
        );
        assert_eq!(sys.get_actor_stats(child).unwrap().restart_count, 1);
    }

    #[test]
    fn test_one_for_all_supervision() {
        let mut sys = system();
        let sup = sys.spawn_actor("sup", None).unwrap();
        sys.set_supervisor(
            sup,
            SupervisorStrategy::new(SupervisionType::OneForAll, 5, Duration::from_secs(60)),
        );
        let c1 = sys.spawn_actor("c1", Some(sup)).unwrap();
        let c2 = sys.spawn_actor("c2", Some(sup)).unwrap();

        sys.handle_actor_failure(c1, "boom").unwrap();

        // Both children should have been restarted
        assert_eq!(sys.get_actor_stats(c1).unwrap().restart_count, 1);
        assert_eq!(sys.get_actor_stats(c2).unwrap().restart_count, 1);
    }

    #[test]
    fn test_rest_for_one_supervision() {
        let mut sys = system();
        let sup = sys.spawn_actor("sup", None).unwrap();
        sys.set_supervisor(
            sup,
            SupervisorStrategy::new(SupervisionType::RestForOne, 5, Duration::from_secs(60)),
        );
        let c1 = sys.spawn_actor("c1", Some(sup)).unwrap();
        let c2 = sys.spawn_actor("c2", Some(sup)).unwrap();
        let c3 = sys.spawn_actor("c3", Some(sup)).unwrap();

        sys.handle_actor_failure(c2, "err").unwrap();

        // c1 should NOT be restarted; c2 and c3 should
        assert_eq!(sys.get_actor_stats(c1).unwrap().restart_count, 0);
        assert_eq!(sys.get_actor_stats(c2).unwrap().restart_count, 1);
        assert_eq!(sys.get_actor_stats(c3).unwrap().restart_count, 1);
    }

    #[test]
    fn test_max_restarts_exceeded() {
        let mut sys = system();
        let sup = sys.spawn_actor("sup", None).unwrap();
        sys.set_supervisor(
            sup,
            SupervisorStrategy::new(SupervisionType::OneForOne, 1, Duration::from_secs(60)),
        );
        let child = sys.spawn_actor("fragile", Some(sup)).unwrap();

        // First failure – ok
        sys.handle_actor_failure(child, "err1").unwrap();
        // Second failure within window – exceeds limit
        let result = sys.handle_actor_failure(child, "err2");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max restarts exceeded"));
    }

    // ---- mailbox overflow ----

    #[test]
    fn test_mailbox_overflow_drop_oldest() {
        let mut mailbox = Mailbox::new(2);
        for i in 0..3 {
            mailbox.enqueue(Envelope {
                sender: None,
                message: ActorMessage::User(serde_json::json!(i)),
                priority: MessagePriority::Normal,
                timestamp: Instant::now(),
            });
        }
        // Capacity is 2, oldest (0) should have been dropped
        assert_eq!(mailbox.len(), 2);
        let first = mailbox.dequeue().unwrap();
        if let ActorMessage::User(v) = first.message {
            assert_eq!(v, serde_json::json!(1));
        }
    }

    #[test]
    fn test_mailbox_overflow_reject_new() {
        let mut mailbox = Mailbox::new(1).with_overflow_strategy(OverflowStrategy::RejectNew);
        mailbox.enqueue(Envelope {
            sender: None,
            message: ActorMessage::User(serde_json::json!(1)),
            priority: MessagePriority::Normal,
            timestamp: Instant::now(),
        });
        let accepted = mailbox.enqueue(Envelope {
            sender: None,
            message: ActorMessage::User(serde_json::json!(2)),
            priority: MessagePriority::Normal,
            timestamp: Instant::now(),
        });
        assert!(!accepted);
        assert_eq!(mailbox.len(), 1);
    }

    // ---- routing ----

    #[test]
    fn test_router_round_robin() {
        let mut sys = system();
        let a = sys.spawn_actor("a", None).unwrap();
        let b = sys.spawn_actor("b", None).unwrap();

        let mut router = ActorRouter::new(RoutingStrategy::RoundRobin { index: 0 });
        router.add_route("work", a);
        router.add_route("work", b);

        let first = router.resolve("work");
        assert_eq!(first, vec![a]);
        let second = router.resolve("work");
        assert_eq!(second, vec![b]);
    }

    #[test]
    fn test_router_broadcast() {
        let mut sys = system();
        let a = sys.spawn_actor("a", None).unwrap();
        let b = sys.spawn_actor("b", None).unwrap();

        let mut router = ActorRouter::new(RoutingStrategy::Broadcast);
        router.add_route("news", a);
        router.add_route("news", b);

        let targets = router.resolve("news");
        assert_eq!(targets.len(), 2);
    }

    // ---- dead letters ----

    #[test]
    fn test_dead_letter_on_missing_actor() {
        let mut sys = system();
        let fake = ActorId::new();
        let result = sys.send_message(
            fake,
            None,
            ActorMessage::User(serde_json::json!("lost")),
            MessagePriority::Normal,
        );
        assert!(result.is_err());
        assert_eq!(sys.dead_letters().len(), 1);
        assert_eq!(sys.stats().total_dead_letters, 1);
    }

    // ---- lifecycle ----

    #[test]
    fn test_system_message_suspend_resume() {
        let mut sys = system();
        let id = sys.spawn_actor("actor", None).unwrap();

        // Suspend
        sys.actors
            .get_mut(&id)
            .unwrap()
            .mailbox
            .push_system(SystemMessage::Suspend);
        sys.process_mailbox(id);
        assert_eq!(
            sys.get_actor_stats(id).unwrap().state,
            ActorLifecycle::Suspended
        );

        // Resume
        sys.actors
            .get_mut(&id)
            .unwrap()
            .mailbox
            .push_system(SystemMessage::Resume);
        sys.process_mailbox(id);
        assert_eq!(
            sys.get_actor_stats(id).unwrap().state,
            ActorLifecycle::Running
        );
    }

    // ---- stats ----

    #[test]
    fn test_system_stats() {
        let mut sys = system();
        let a = sys.spawn_actor("a", None).unwrap();
        sys.send_message(
            a,
            None,
            ActorMessage::User(serde_json::json!("msg")),
            MessagePriority::Normal,
        )
        .unwrap();

        assert_eq!(sys.stats().total_actors_created, 1);
        assert_eq!(sys.stats().total_messages_sent, 1);
        assert_eq!(sys.stats().total_messages_delivered, 1);
    }

    #[test]
    fn test_spawn_capacity_limit() {
        let mut sys = ActorSystem::new(ActorSystemConfig {
            max_actors: 2,
            ..Default::default()
        });
        sys.spawn_actor("a", None).unwrap();
        sys.spawn_actor("b", None).unwrap();
        let result = sys.spawn_actor("c", None);
        assert!(result.is_err());
    }
}

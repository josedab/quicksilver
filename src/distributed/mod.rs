//! Distributed Runtime
//!
//! Built-in primitives for distributed computing, enabling JavaScript code
//! to transparently run across multiple nodes with automatic serialization.
//!
//! # Example
//! ```text
//! // Define a distributed function
//! const cluster = Cluster.connect("quicksilver://cluster.example.com");
//!
//! // Run function on any available node
//! const result = await cluster.run(async () => {
//!   return heavyComputation();
//! });
//!
//! // Map-reduce style processing
//! const results = await cluster.mapReduce(
//!   largeDataset,
//!   (chunk) => processChunk(chunk),
//!   (results) => combineResults(results)
//! );
//! ```

use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

/// Unique node identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }

    pub fn local() -> Self {
        Self(0)
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

/// Node status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Starting,
    Ready,
    Busy,
    Draining,
    Offline,
}

/// Information about a cluster node
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: NodeId,
    pub address: String,
    pub status: NodeStatus,
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub current_tasks: u32,
    pub last_heartbeat: Instant,
    pub metadata: HashMap<String, String>,
}

impl NodeInfo {
    pub fn new(id: NodeId, address: &str) -> Self {
        Self {
            id,
            address: address.to_string(),
            status: NodeStatus::Starting,
            cpu_cores: num_cpus(),
            memory_mb: 1024,
            current_tasks: 0,
            last_heartbeat: Instant::now(),
            metadata: HashMap::default(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self.status, NodeStatus::Ready) &&
        self.last_heartbeat.elapsed() < Duration::from_secs(30)
    }
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|p| p.get() as u32)
        .unwrap_or(1)
}

/// Task identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// Task status
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Scheduled(NodeId),
    Running(NodeId),
    Completed(Vec<u8>),
    Failed(String),
    Cancelled,
}

/// A distributed task
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub code: Vec<u8>,
    pub args: Vec<u8>,
    pub status: TaskStatus,
    pub created_at: Instant,
    pub priority: i32,
    pub timeout: Option<Duration>,
}

impl Task {
    pub fn new(code: Vec<u8>, args: Vec<u8>) -> Self {
        Self {
            id: TaskId::new(),
            code,
            args,
            status: TaskStatus::Pending,
            created_at: Instant::now(),
            priority: 0,
            timeout: None,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Cluster name
    pub name: String,
    /// Maximum concurrent tasks per node
    pub max_tasks_per_node: u32,
    /// Task timeout
    pub default_timeout: Duration,
    /// Heartbeat interval
    pub heartbeat_interval: Duration,
    /// Node timeout threshold
    pub node_timeout: Duration,
    /// Enable task retry on failure
    pub enable_retry: bool,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            max_tasks_per_node: 10,
            default_timeout: Duration::from_secs(300),
            heartbeat_interval: Duration::from_secs(5),
            node_timeout: Duration::from_secs(30),
            enable_retry: true,
            max_retries: 3,
        }
    }
}

/// A cluster of nodes
#[derive(Debug)]
pub struct Cluster {
    config: ClusterConfig,
    nodes: Mutex<HashMap<NodeId, NodeInfo>>,
    tasks: Mutex<HashMap<TaskId, Task>>,
    local_node: NodeInfo,
}

impl Cluster {
    /// Create a new cluster
    pub fn new(config: ClusterConfig) -> Self {
        let local_node = NodeInfo::new(NodeId::local(), "localhost");
        Self {
            config,
            nodes: Mutex::new(HashMap::default()),
            tasks: Mutex::new(HashMap::default()),
            local_node,
        }
    }

    /// Connect to an existing cluster
    pub fn connect(_address: &str) -> Result<Arc<Self>, ClusterError> {
        // In a real implementation, this would connect to the cluster
        Ok(Arc::new(Self::new(ClusterConfig::default())))
    }

    /// Register a node
    pub fn register_node(&self, node: NodeInfo) {
        self.nodes.lock().unwrap().insert(node.id, node);
    }

    /// Remove a node
    pub fn remove_node(&self, node_id: NodeId) {
        self.nodes.lock().unwrap().remove(&node_id);
    }

    /// Get all nodes
    pub fn nodes(&self) -> Vec<NodeInfo> {
        self.nodes.lock().unwrap().values().cloned().collect()
    }

    /// Get available nodes
    pub fn available_nodes(&self) -> Vec<NodeInfo> {
        self.nodes.lock().unwrap()
            .values()
            .filter(|n| n.is_available())
            .cloned()
            .collect()
    }

    /// Submit a task to the cluster
    pub fn submit(&self, task: Task) -> TaskId {
        let id = task.id;
        self.tasks.lock().unwrap().insert(id, task);
        id
    }

    /// Get task status
    pub fn task_status(&self, id: TaskId) -> Option<TaskStatus> {
        self.tasks.lock().unwrap().get(&id).map(|t| t.status.clone())
    }

    /// Cancel a task
    pub fn cancel(&self, id: TaskId) -> bool {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(&id) {
            match &task.status {
                TaskStatus::Pending | TaskStatus::Scheduled(_) => {
                    task.status = TaskStatus::Cancelled;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Select the best node for a task
    pub fn select_node(&self) -> Option<NodeId> {
        let nodes = self.nodes.lock().unwrap();

        // Simple strategy: select node with least tasks
        nodes.values()
            .filter(|n| n.is_available())
            .min_by_key(|n| n.current_tasks)
            .map(|n| n.id)
    }

    /// Get cluster config
    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }

    /// Get local node info
    pub fn local_node(&self) -> &NodeInfo {
        &self.local_node
    }
}

/// Cluster errors
#[derive(Debug, Clone)]
pub enum ClusterError {
    ConnectionFailed(String),
    NodeNotFound(NodeId),
    TaskNotFound(TaskId),
    TaskFailed(String),
    Timeout,
    Serialization(String),
}

impl std::fmt::Display for ClusterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {}", msg),
            Self::NodeNotFound(id) => write!(f, "node not found: {:?}", id),
            Self::TaskNotFound(id) => write!(f, "task not found: {:?}", id),
            Self::TaskFailed(msg) => write!(f, "task failed: {}", msg),
            Self::Timeout => write!(f, "operation timed out"),
            Self::Serialization(msg) => write!(f, "serialization error: {}", msg),
        }
    }
}

impl std::error::Error for ClusterError {}

/// Message types for cluster communication
#[derive(Debug, Clone)]
pub enum Message {
    /// Heartbeat from node
    Heartbeat { node: NodeInfo },
    /// Submit a task
    SubmitTask { task: Task },
    /// Task result
    TaskResult { task_id: TaskId, result: Vec<u8> },
    /// Task failed
    TaskFailed { task_id: TaskId, error: String },
    /// Request to join cluster
    JoinRequest { node: NodeInfo },
    /// Acknowledge join
    JoinAck { cluster_config: ClusterConfig },
    /// Node leaving cluster
    Leave { node_id: NodeId },
}

/// Serializable function reference
#[derive(Debug, Clone)]
pub struct RemoteFunction {
    /// Serialized bytecode
    pub bytecode: Vec<u8>,
    /// Captured variables (closure)
    pub captures: Vec<u8>,
    /// Function name (for debugging)
    pub name: Option<String>,
}

impl RemoteFunction {
    pub fn new(bytecode: Vec<u8>) -> Self {
        Self {
            bytecode,
            captures: Vec::new(),
            name: None,
        }
    }

    pub fn with_captures(mut self, captures: Vec<u8>) -> Self {
        self.captures = captures;
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }
}

/// Work distribution strategies
#[derive(Debug, Clone, Copy)]
pub enum Distribution {
    /// Round-robin across nodes
    RoundRobin,
    /// Based on node load
    LeastLoaded,
    /// Based on data locality
    DataLocal,
    /// Random selection
    Random,
    /// All nodes (broadcast)
    Broadcast,
}

/// Map-reduce job
#[derive(Debug)]
pub struct MapReduceJob<T> {
    /// Input data chunks
    pub chunks: Vec<T>,
    /// Distribution strategy
    pub distribution: Distribution,
    /// Map phase timeout
    pub map_timeout: Duration,
    /// Reduce phase timeout
    pub reduce_timeout: Duration,
}

impl<T> MapReduceJob<T> {
    pub fn new(chunks: Vec<T>) -> Self {
        Self {
            chunks,
            distribution: Distribution::LeastLoaded,
            map_timeout: Duration::from_secs(60),
            reduce_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_distribution(mut self, distribution: Distribution) -> Self {
        self.distribution = distribution;
        self
    }
}

/// Actor-like message passing
#[derive(Debug)]
pub struct Actor {
    id: NodeId,
    mailbox: Mutex<Vec<Message>>,
}

impl Actor {
    pub fn new() -> Self {
        Self {
            id: NodeId::new(),
            mailbox: Mutex::new(Vec::new()),
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn send(&self, message: Message) {
        self.mailbox.lock().unwrap().push(message);
    }

    pub fn receive(&self) -> Option<Message> {
        self.mailbox.lock().unwrap().pop()
    }

    pub fn receive_all(&self) -> Vec<Message> {
        std::mem::take(&mut *self.mailbox.lock().unwrap())
    }
}

impl Default for Actor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_creation() {
        let cluster = Cluster::new(ClusterConfig::default());
        assert!(cluster.nodes().is_empty());
    }

    #[test]
    fn test_node_registration() {
        let cluster = Cluster::new(ClusterConfig::default());

        let mut node = NodeInfo::new(NodeId::new(), "192.168.1.1:8080");
        node.status = NodeStatus::Ready;

        cluster.register_node(node.clone());

        let nodes = cluster.nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].address, "192.168.1.1:8080");
    }

    #[test]
    fn test_task_submission() {
        let cluster = Cluster::new(ClusterConfig::default());

        let task = Task::new(vec![1, 2, 3], vec![4, 5, 6]);
        let id = cluster.submit(task);

        let status = cluster.task_status(id);
        assert!(matches!(status, Some(TaskStatus::Pending)));
    }

    #[test]
    fn test_task_cancellation() {
        let cluster = Cluster::new(ClusterConfig::default());

        let task = Task::new(vec![], vec![]);
        let id = cluster.submit(task);

        assert!(cluster.cancel(id));

        let status = cluster.task_status(id);
        assert!(matches!(status, Some(TaskStatus::Cancelled)));
    }

    #[test]
    fn test_actor_messaging() {
        let actor = Actor::new();

        actor.send(Message::Heartbeat {
            node: NodeInfo::new(NodeId::new(), "test"),
        });

        let msg = actor.receive();
        assert!(matches!(msg, Some(Message::Heartbeat { .. })));
    }
}

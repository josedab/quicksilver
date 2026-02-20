//! WebSocket Server API
//!
//! WebSocket server with connection management and message routing.
//!
//! # Example
//! ```text
//! // Accept WebSocket connections
//! const ws = new WebSocketServer({ maxConnections: 100 });
//! const connId = ws.accept("/chat");
//! ws.send(connId, "Welcome!");
//! ws.broadcast("New user joined");
//! ```

use std::collections::{HashMap, VecDeque};

/// WebSocket connection state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebSocketState {
    Connecting,
    Open,
    Closing,
    Closed,
}

/// WebSocket message types
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<u16>, Option<String>),
}

/// WebSocket connection
#[allow(dead_code)]
pub struct WebSocketConnection {
    pub id: u64,
    pub state: WebSocketState,
    pub url: String,
    outgoing: VecDeque<WebSocketMessage>,
    incoming: VecDeque<WebSocketMessage>,
    extensions: Vec<String>,
    protocol: Option<String>,
    close_code: Option<u16>,
    close_reason: Option<String>,
}

impl WebSocketConnection {
    /// Create a new WebSocket connection
    pub fn new(id: u64, url: &str) -> Self {
        Self {
            id,
            state: WebSocketState::Open,
            url: url.to_string(),
            outgoing: VecDeque::new(),
            incoming: VecDeque::new(),
            extensions: Vec::new(),
            protocol: None,
            close_code: None,
            close_reason: None,
        }
    }

    /// Send a message on this connection
    pub fn send(&mut self, msg: WebSocketMessage) -> Result<(), String> {
        match self.state {
            WebSocketState::Open => {
                self.outgoing.push_back(msg);
                Ok(())
            }
            _ => Err(format!(
                "Cannot send on connection {} in state {:?}",
                self.id, self.state
            )),
        }
    }

    /// Receive the next incoming message
    pub fn receive(&mut self) -> Option<WebSocketMessage> {
        self.incoming.pop_front()
    }

    /// Close this connection
    pub fn close(&mut self, code: u16, reason: &str) {
        if self.state == WebSocketState::Open || self.state == WebSocketState::Connecting {
            self.state = WebSocketState::Closing;
            self.close_code = Some(code);
            self.close_reason = Some(reason.to_string());
            self.outgoing.push_back(WebSocketMessage::Close(
                Some(code),
                Some(reason.to_string()),
            ));
        }
        self.state = WebSocketState::Closed;
    }

    /// Check if the connection is open
    pub fn is_open(&self) -> bool {
        self.state == WebSocketState::Open
    }

    /// Queue an incoming message (simulates receiving from network)
    pub fn queue_incoming(&mut self, msg: WebSocketMessage) {
        self.incoming.push_back(msg);
    }
}

/// WebSocket server managing multiple connections
#[allow(dead_code)]
pub struct WebSocketServer {
    connections: HashMap<u64, WebSocketConnection>,
    next_id: u64,
    max_connections: usize,
    max_message_size: usize,
    ping_interval_ms: u64,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: HashMap::new(),
            next_id: 1,
            max_connections,
            max_message_size: 64 * 1024, // 64KB default
            ping_interval_ms: 30000,
        }
    }

    /// Accept a new WebSocket connection, returning its ID
    pub fn accept(&mut self, url: &str) -> Result<u64, String> {
        if self.connections.len() >= self.max_connections {
            return Err("Max connections reached".to_string());
        }

        let id = self.next_id;
        self.next_id += 1;

        let conn = WebSocketConnection::new(id, url);
        self.connections.insert(id, conn);

        Ok(id)
    }

    /// Get a mutable reference to a connection by ID
    pub fn get_connection(&mut self, id: u64) -> Option<&mut WebSocketConnection> {
        self.connections.get_mut(&id)
    }

    /// Close and remove a connection
    pub fn close_connection(&mut self, id: u64) {
        if let Some(conn) = self.connections.get_mut(&id) {
            conn.close(1000, "Normal closure");
        }
        self.connections.remove(&id);
    }

    /// Get the number of active (open) connections
    pub fn active_connections(&self) -> usize {
        self.connections
            .values()
            .filter(|c| c.is_open())
            .count()
    }

    /// Broadcast a message to all open connections
    pub fn broadcast(&mut self, msg: WebSocketMessage) {
        let open_ids: Vec<u64> = self
            .connections
            .iter()
            .filter(|(_, c)| c.is_open())
            .map(|(id, _)| *id)
            .collect();

        for id in open_ids {
            if let Some(conn) = self.connections.get_mut(&id) {
                let cloned = match &msg {
                    WebSocketMessage::Text(s) => WebSocketMessage::Text(s.clone()),
                    WebSocketMessage::Binary(b) => WebSocketMessage::Binary(b.clone()),
                    WebSocketMessage::Ping(b) => WebSocketMessage::Ping(b.clone()),
                    WebSocketMessage::Pong(b) => WebSocketMessage::Pong(b.clone()),
                    WebSocketMessage::Close(c, r) => {
                        WebSocketMessage::Close(*c, r.clone())
                    }
                };
                let _ = conn.send(cloned);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_new() {
        let conn = WebSocketConnection::new(1, "/chat");
        assert_eq!(conn.id, 1);
        assert_eq!(conn.url, "/chat");
        assert_eq!(conn.state, WebSocketState::Open);
        assert!(conn.is_open());
    }

    #[test]
    fn test_connection_send_receive() {
        let mut conn = WebSocketConnection::new(1, "/ws");

        // Send a message
        assert!(conn.send(WebSocketMessage::Text("hello".to_string())).is_ok());

        // Queue and receive an incoming message
        conn.queue_incoming(WebSocketMessage::Text("world".to_string()));
        let msg = conn.receive();
        assert!(msg.is_some());
        match msg.unwrap() {
            WebSocketMessage::Text(s) => assert_eq!(s, "world"),
            _ => panic!("Expected Text message"),
        }

        // No more messages
        assert!(conn.receive().is_none());
    }

    #[test]
    fn test_connection_close() {
        let mut conn = WebSocketConnection::new(1, "/ws");
        assert!(conn.is_open());

        conn.close(1000, "Normal closure");
        assert!(!conn.is_open());
        assert_eq!(conn.state, WebSocketState::Closed);

        // Cannot send on closed connection
        let result = conn.send(WebSocketMessage::Text("fail".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_connection_send_binary() {
        let mut conn = WebSocketConnection::new(1, "/ws");
        assert!(conn.send(WebSocketMessage::Binary(vec![1, 2, 3])).is_ok());
    }

    #[test]
    fn test_connection_ping_pong() {
        let mut conn = WebSocketConnection::new(1, "/ws");
        conn.queue_incoming(WebSocketMessage::Ping(vec![1, 2]));

        let msg = conn.receive();
        assert!(msg.is_some());
        match msg.unwrap() {
            WebSocketMessage::Ping(data) => assert_eq!(data, vec![1, 2]),
            _ => panic!("Expected Ping message"),
        }
    }

    #[test]
    fn test_server_accept() {
        let mut server = WebSocketServer::new(10);
        let id = server.accept("/chat").unwrap();
        assert_eq!(id, 1);
        assert_eq!(server.active_connections(), 1);

        let id2 = server.accept("/chat").unwrap();
        assert_eq!(id2, 2);
        assert_eq!(server.active_connections(), 2);
    }

    #[test]
    fn test_server_max_connections() {
        let mut server = WebSocketServer::new(2);
        assert!(server.accept("/ws").is_ok());
        assert!(server.accept("/ws").is_ok());
        assert!(server.accept("/ws").is_err());
    }

    #[test]
    fn test_server_get_connection() {
        let mut server = WebSocketServer::new(10);
        let id = server.accept("/ws").unwrap();

        let conn = server.get_connection(id);
        assert!(conn.is_some());
        assert_eq!(conn.unwrap().url, "/ws");

        assert!(server.get_connection(999).is_none());
    }

    #[test]
    fn test_server_close_connection() {
        let mut server = WebSocketServer::new(10);
        let id = server.accept("/ws").unwrap();
        assert_eq!(server.active_connections(), 1);

        server.close_connection(id);
        assert_eq!(server.active_connections(), 0);
        assert!(server.get_connection(id).is_none());
    }

    #[test]
    fn test_server_broadcast() {
        let mut server = WebSocketServer::new(10);
        let id1 = server.accept("/ws").unwrap();
        let id2 = server.accept("/ws").unwrap();
        let id3 = server.accept("/ws").unwrap();

        // Close one connection
        server.close_connection(id2);

        // Broadcast should only reach open connections
        server.broadcast(WebSocketMessage::Text("hello everyone".to_string()));

        // Check that open connections received the message
        let conn1 = server.get_connection(id1).unwrap();
        assert_eq!(conn1.outgoing.len(), 1);

        let conn3 = server.get_connection(id3).unwrap();
        assert_eq!(conn3.outgoing.len(), 1);
    }

    #[test]
    fn test_server_connection_lifecycle() {
        let mut server = WebSocketServer::new(10);

        // Accept
        let id = server.accept("/chat").unwrap();
        assert_eq!(server.active_connections(), 1);

        // Send a message
        {
            let conn = server.get_connection(id).unwrap();
            assert!(conn.send(WebSocketMessage::Text("hi".to_string())).is_ok());
        }

        // Receive a message
        {
            let conn = server.get_connection(id).unwrap();
            conn.queue_incoming(WebSocketMessage::Text("reply".to_string()));
            let msg = conn.receive();
            assert!(msg.is_some());
        }

        // Close
        server.close_connection(id);
        assert_eq!(server.active_connections(), 0);
    }

    #[test]
    fn test_server_broadcast_binary() {
        let mut server = WebSocketServer::new(10);
        server.accept("/ws").unwrap();

        server.broadcast(WebSocketMessage::Binary(vec![0xFF, 0xFE]));

        let conn = server.get_connection(1).unwrap();
        assert_eq!(conn.outgoing.len(), 1);
    }
}

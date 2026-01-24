//! SSE session management for MCP Streamable HTTP

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Event sent over SSE stream
#[derive(Clone, Debug)]
pub struct SseEvent {
    pub event_type: String,
    pub data: String,
}

/// Session error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum SessionError {
    #[error("Too many concurrent sessions")]
    TooManySessions,
}

/// Manages SSE sessions for MCP Streamable HTTP
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, broadcast::Sender<SseEvent>>>>,
    max_sessions: usize,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_sessions: 100,
        }
    }

    /// Get or create a session, returning session ID and receiver
    pub async fn get_or_create(
        &self,
        session_id: Option<String>,
    ) -> Result<(String, broadcast::Receiver<SseEvent>), SessionError> {
        let id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let mut sessions = self.sessions.write().await;

        // Check if session exists
        if let Some(sender) = sessions.get(&id) {
            return Ok((id, sender.subscribe()));
        }

        // Check max sessions
        if sessions.len() >= self.max_sessions {
            return Err(SessionError::TooManySessions);
        }

        // Create new session with broadcast channel
        let (tx, rx) = broadcast::channel(256);
        sessions.insert(id.clone(), tx);

        Ok((id, rx))
    }

    /// Broadcast event to a specific session
    pub async fn broadcast(&self, session_id: &str, event: SseEvent) -> bool {
        let sessions = self.sessions.read().await;
        if let Some(sender) = sessions.get(session_id) {
            sender.send(event).is_ok()
        } else {
            false
        }
    }

    /// Broadcast event to all sessions
    pub async fn broadcast_all(&self, event: SseEvent) {
        let sessions = self.sessions.read().await;
        for sender in sessions.values() {
            let _ = sender.send(event.clone());
        }
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }

    /// Get active session count
    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

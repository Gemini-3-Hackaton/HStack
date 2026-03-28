use hstack_core::ticket::Ticket;
use hstack_core::provider::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use async_trait::async_trait;

/// Represents the agent's short-term scratchpad and reasoning history.
/// All tool results and intermediate thoughts live here first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemory {
    pub messages: Vec<Message>,
    pub technical_noise: Vec<Value>, // Raw tool outputs, etc.
}

impl WorkingMemory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            technical_noise: Vec::new(),
        }
    }
}

/// Represents the long-term, user-canonical state of the world.
/// This trait allows the harness to remain independent of the storage layer (Tauri vs Database).
#[async_trait]
pub trait HStackWorld: Send + Sync {
    /// Returns the current set of tickets in the world.
    async fn get_tickets(&self) -> Result<Vec<Ticket>, String>;
    
    /// Returns a subset of tickets based on a search query or filter.
    async fn search_tickets(&self, query: &str) -> Result<Vec<Ticket>, String>;
}

/// A simple in-memory implementation of HStackWorld for testing and basic use.
pub struct InMemoryWorld {
    pub tickets: Vec<Ticket>,
}

#[async_trait]
impl HStackWorld for InMemoryWorld {
    async fn get_tickets(&self) -> Result<Vec<Ticket>, String> {
        Ok(self.tickets.clone())
    }

    async fn search_tickets(&self, query: &str) -> Result<Vec<Ticket>, String> {
        let query = query.to_lowercase();
        Ok(self.tickets.iter()
            .filter(|t| t.title.to_lowercase().contains(&query) || t.notes.as_ref().map(|n| n.to_lowercase().contains(&query)).unwrap_or(false))
            .cloned()
            .collect())
    }
}

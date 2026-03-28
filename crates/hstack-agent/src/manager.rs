use crate::memory::{HStackWorld, WorkingMemory};
use hstack_core::provider::{Message, Role};
use async_trait::async_trait;
use crate::error::Error;

/// Constructs the prompt for the provider by fusing the persistent world (HStackWorld)
/// and the short-term reasoning context (WorkingMemory).
#[async_trait]
pub trait ContextManager: Send + Sync {
    async fn construct_context(
        &self,
        world: &dyn HStackWorld,
        memory: &WorkingMemory,
        base_prompt: &str,
    ) -> Result<Vec<Message>, Error>;
}

/// A simple implementation that appends the entire world state to the system prompt.
pub struct SimpleContextManager;

#[async_trait]
impl ContextManager for SimpleContextManager {
    async fn construct_context(
        &self,
        world: &dyn HStackWorld,
        memory: &WorkingMemory,
        base_prompt: &str,
    ) -> Result<Vec<Message>, Error> {
        let tickets = world.get_tickets().await.map_err(Error::World)?;
        let tickets_json = serde_json::to_string_pretty(&tickets).unwrap_or_else(|_| "[]".to_string());
        
        let mut messages = Vec::new();
        
        // Build the system prompt with context
        let system_content = format!(
            "{}\n\nCURRENT STACK CONTEXT:\n{}\n\nRECENT THOUGHTS:\n{:?}",
            base_prompt,
            tickets_json,
            memory.technical_noise
        );
        
        messages.push(Message {
            role: Role::System,
            content: Some(system_content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        
        // Add the conversation history from working memory
        messages.extend(memory.messages.clone());
        
        Ok(messages)
    }
}

use async_trait::async_trait;
use serde_json::Value;
use crate::action::{AgentAction, WorkingMemoryDelta};
use crate::error::Error;
use crate::memory::HStackWorld;

/// The interface for all agentic tools.
/// Tools produce an AgentAction (the transition function `a`).
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    
    /// Executes the tool logic and returns the resulting action (transition).
    async fn execute(&self, args: Value, world: &dyn HStackWorld) -> Result<AgentAction, Error>;
}

/// The Identity tool allows the agent to signal that it has finished its task.
pub struct IdentityTool;

#[async_trait]
impl Tool for IdentityTool {
    fn name(&self) -> &str { "identity" }
    fn description(&self) -> &str { "Signals that the task is complete and returns the final answer." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "answer": { "type": "string", "description": "The final natural language response to the user." }
            },
            "required": ["answer"]
        })
    }

    async fn execute(&self, args: Value, _world: &dyn HStackWorld) -> Result<AgentAction, Error> {
        let answer = args.get("answer").and_then(Value::as_str).unwrap_or("Done").to_string();
        Ok(AgentAction::Stop(answer))
    }
}

/// Allows the agent to search the HStack world for relevant tickets.
pub struct SearchStack;

#[async_trait]
impl Tool for SearchStack {
    fn name(&self) -> &str { "search_stack" }
    fn description(&self) -> &str { "Searches the user's stack (HStackWorld) for relevant tickets or habits." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query." }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, world: &dyn HStackWorld) -> Result<AgentAction, Error> {
        let query = args.get("query").and_then(Value::as_str).unwrap_or("");
        let results = world.search_tickets(query).await.map_err(Error::World)?;
        
        Ok(AgentAction::UpdateWorkingMemory(WorkingMemoryDelta::AddTechnicalNoise(
            format!("search_stack:{}", query),
            serde_json::to_value(results).unwrap_or(Value::Null)
        )))
    }
}

/// Allows the agent to store internal thoughts or intermediate reasoning in working memory.
pub struct ScratchThought;

#[async_trait]
impl Tool for ScratchThought {
    fn name(&self) -> &str { "scratch_thought" }
    fn description(&self) -> &str { "Saves a technical thought or intermediate result to the agent's working memory." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "thought": { "type": "string", "description": "The thought or reasoning step." },
                "metadata": { "type": "object", "description": "Optional structured data." }
            },
            "required": ["thought"]
        })
    }

    async fn execute(&self, args: Value, _world: &dyn HStackWorld) -> Result<AgentAction, Error> {
        let thought = args.get("thought").and_then(Value::as_str).unwrap_or("").to_string();
        let metadata = args.get("metadata").cloned().unwrap_or(Value::Null);
        
        Ok(AgentAction::UpdateWorkingMemory(WorkingMemoryDelta::AddTechnicalNoise(
            format!("thought:{}", thought),
            metadata
        )))
    }
}

use hstack_core::provider::Message;
use hstack_core::sync::SyncAction;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents a modification to the agent's internal working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkingMemoryDelta {
    /// Appends a message to the history.
    AppendMessage(Message),
    /// Injects technical context or raw tool output.
    AddTechnicalNoise(String, Value),
}

/// The "action" function `a` produced by the agent.
/// It represents the intent to transition the state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAction {
    /// Update short-term context.
    UpdateWorkingMemory(WorkingMemoryDelta),
    /// Propose changes to the long-term stack (requires safety control).
    /// Uses the canonical SyncAction from hstack-core.
    UpdateStack(SyncAction),
    /// A combination of multiple transitions.
    Compound(Vec<AgentAction>),
    /// Signal completion with a final answer.
    Stop(String),
}

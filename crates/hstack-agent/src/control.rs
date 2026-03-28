use hstack_core::sync::SyncAction;
use crate::error::Error;
use async_trait::async_trait;

/// The safety and control layer for agentic actions.
/// Callers (Tauri client or Axum server) implement this to gatekeep destructive changes.
#[async_trait]
pub trait AgentControlSystem: Send + Sync {
    /// Validates a proposed StackAction/SyncAction (mutation to the long-term state).
    /// Returns Ok(()) if permitted, or Err(Error::Denied) if blocked.
    async fn validate_stack_action(&self, action: &SyncAction) -> Result<(), Error>;
}

/// A default permissive control system.
pub struct AllowAllControl;

#[async_trait]
impl AgentControlSystem for AllowAllControl {
    async fn validate_stack_action(&self, _action: &SyncAction) -> Result<(), Error> {
        Ok(())
    }
}

/// A control system that denies all StackActions (read-only agent).
pub struct ReadOnlyControl;

#[async_trait]
impl AgentControlSystem for ReadOnlyControl {
    async fn validate_stack_action(&self, action: &SyncAction) -> Result<(), Error> {
        Err(Error::Denied(format!("Sync action {:?} is forbidden in read-only mode", action.action_id)))
    }
}

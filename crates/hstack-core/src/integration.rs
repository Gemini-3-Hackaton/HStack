// Shared integration contract types.
// Review docs/public-private-contract.md before widening this surface for private-only infrastructure needs.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    Password,
    Google,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationProvider {
    GoogleCalendar,
    GoogleTasks,
    Gmail,
    GitHub,
    Jira,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResourceKind {
    CalendarEvent,
    Task,
    EmailThread,
    Issue,
    PullRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Active,
    NeedsReconnect,
    Revoked,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BindingSyncMode {
    ReadOnly,
    Bidirectional,
    ExportOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BindingStatus {
    Healthy,
    PendingPush,
    Failed,
    Conflict,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutboxJobKind {
    CreateRemote,
    UpdateRemote,
    DeleteRemote,
    RefreshRemote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthIdentity {
    pub provider: AuthProvider,
    pub provider_user_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationConnection {
    pub id: String,
    pub provider: IntegrationProvider,
    pub account_label: String,
    pub status: ConnectionStatus,
    pub scopes: Vec<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketBinding {
    pub id: String,
    pub ticket_id: String,
    pub connection_id: String,
    pub provider: IntegrationProvider,
    pub resource_kind: ExternalResourceKind,
    pub remote_resource_id: String,
    pub sync_mode: BindingSyncMode,
    pub status: BindingStatus,
    pub last_error: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalResource {
    pub id: String,
    pub connection_id: String,
    pub provider: IntegrationProvider,
    pub resource_kind: ExternalResourceKind,
    pub remote_resource_id: String,
    pub remote_version: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub payload: serde_json::Value,
    pub remote_updated_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationOutboxJob {
    pub id: String,
    pub binding_id: String,
    pub job_kind: OutboxJobKind,
    pub attempt_count: i32,
    pub next_attempt_at: DateTime<Utc>,
    pub last_error: Option<String>,
}
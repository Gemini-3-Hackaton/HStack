use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::ticket::TicketPayload;
use crate::integration::{AuthIdentity, AuthProvider, ConnectionStatus, ExternalResourceKind, IntegrationProvider, BindingStatus, BindingSyncMode};

#[derive(Debug, Serialize, Deserialize)]
pub struct UserCreate {
    pub first_name: String,
    pub last_name: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserLogin {
    pub first_name: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthStartRequest {
    pub provider: AuthProvider,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthStartResponse {
    pub provider: AuthProvider,
    pub authorization_url: String,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDTO,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserDTO {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub auth_identities: Vec<AuthIdentity>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegrationConnectionDTO {
    pub id: String,
    pub provider: IntegrationProvider,
    pub account_label: String,
    pub status: ConnectionStatus,
    pub scopes: Vec<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TicketBindingDTO {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskPayload {
    pub r#type: String,
    pub status: String,
    pub payload: Option<TicketPayload>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncActionInput {
    pub action_id: Uuid,
    pub r#type: String, // CREATE, UPDATE, DELETE
    pub entity_id: Uuid,
    pub entity_type: String,
    pub payload: Option<TicketPayload>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncActionsMessage {
    pub r#type: String,
    pub actions: Vec<SyncActionInput>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncAck {
    pub r#type: String,
    pub ack_action_ids: Vec<Uuid>,
    pub server_hash: String,
}

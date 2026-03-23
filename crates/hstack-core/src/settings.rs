use crate::provider::{ProviderKind, RateLimitConfig};
use crate::ticket::TicketLocation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedProvider {
    pub id: String, // UUID
    pub name: String,
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model_name: String,
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SyncMode {
    #[default]
    LocalOnly,
    CloudOfficial,
    CloudCustom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedLocation {
    pub id: String,
    pub label: String,
    pub location: TicketLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    pub providers: Vec<SavedProvider>,
    pub default_provider_id: Option<String>,
    pub local_processing: bool,
    pub locale: Option<String>,
    pub hour12: Option<bool>,
    pub sync_mode: SyncMode,
    pub custom_server_url: Option<String>,
    pub sync_user_id: Option<i64>,
    pub sync_user_name: Option<String>,
    pub saved_locations: Vec<SavedLocation>,
    pub onboarding_complete: bool,
}

impl UserSettings {
    pub fn active_provider(&self) -> Option<&SavedProvider> {
        self.default_provider_id
            .as_ref()
            .and_then(|id| self.providers.iter().find(|p| &p.id == id))
    }
}

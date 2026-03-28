use serde::{Deserialize, Serialize};

pub const DEFAULT_VOICE_DIRECT_API_BASE_URL: &str = "https://api.mistral.ai";
pub const DEFAULT_VOICE_DIRECT_MODEL: &str = "voxtral-mini-transcribe-realtime-2602";
pub const MANAGED_VOICE_FEATURE_CODE: &str = "managed_voice_input";
pub const MANAGED_VOICE_WEBSOCKET_PATH: &str = "/ws/voice";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoiceMode {
    Disabled,
    Auto,
    DirectOnly,
}

impl Default for VoiceMode {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioEncoding {
    #[serde(rename = "pcm_s16le")]
    PcmS16Le,
}

impl Default for AudioEncoding {
    fn default() -> Self {
        Self::PcmS16Le
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioFormat {
    pub encoding: AudioEncoding,
    pub sample_rate: u32,
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self {
            encoding: AudioEncoding::PcmS16Le,
            sample_rate: 16_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub mode: VoiceMode,
    pub direct_api_base_url: String,
    pub direct_model_name: String,
    pub target_streaming_delay_ms: Option<u32>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            mode: VoiceMode::Disabled,
            direct_api_base_url: DEFAULT_VOICE_DIRECT_API_BASE_URL.to_string(),
            direct_model_name: DEFAULT_VOICE_DIRECT_MODEL.to_string(),
            target_streaming_delay_ms: Some(240),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCapabilityResponse {
    pub available: bool,
    pub feature_code: String,
    pub reason: Option<String>,
    pub remaining_count: Option<i64>,
    pub websocket_path: Option<String>,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedVoiceAuthMessage {
    pub token: String,
    pub audio_format: AudioFormat,
    pub target_streaming_delay_ms: Option<u32>,
}
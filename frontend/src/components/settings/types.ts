export const OPENAI_DEFAULT_ENDPOINT = "http://localhost:11434/v1";
export const OPENAI_DEFAULT_MODEL = "llama3";
export const VOICE_DEFAULT_ENDPOINT = "https://api.mistral.ai";
export const VOICE_DEFAULT_MODEL = "voxtral-mini-transcribe-realtime-2602";

export interface GeminiModelSummary {
    name: string;
    label: string;
}

export interface SavedProvider {
    id: string;
    name: string;
    kind: 'OpenAiCompatible' | 'Gemini';
    endpoint: string;
    model_name: string;
}

export interface SavedLocation {
    id: string;
    label: string;
    location: {
        location_type: 'address_text';
        address: string;
        label?: string | null;
    };
}

export interface VoiceSettings {
    mode: 'Disabled' | 'Auto' | 'DirectOnly';
    direct_api_base_url: string;
    direct_model_name: string;
    target_streaming_delay_ms: number | null;
}

export interface VoiceSecretStatus {
    direct_api_key_present: boolean;
}

export interface VoiceCapabilityResponse {
    available: boolean;
    feature_code: string;
    reason: string | null;
    remaining_count: number | null;
    websocket_path: string | null;
    model_name: string | null;
}

export interface UserSettings {
    providers: SavedProvider[];
    default_provider_id: string | null;
    voice: VoiceSettings;
    local_processing: boolean;
    locale: string | null;
    hour12: boolean | null;
    sync_mode: 'LocalOnly' | 'CloudOfficial' | 'CloudCustom';
    custom_server_url: string | null;
    sync_user_id?: number | null;
    sync_user_name?: string | null;
    saved_locations: SavedLocation[];
    onboarding_complete: boolean;
}

export interface ProviderDraft extends Partial<SavedProvider> {
    apiKey?: string;
}
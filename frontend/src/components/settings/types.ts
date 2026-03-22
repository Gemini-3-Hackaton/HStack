export const OPENAI_DEFAULT_ENDPOINT = "http://localhost:11434/v1";
export const OPENAI_DEFAULT_MODEL = "llama3";

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

export interface UserSettings {
    providers: SavedProvider[];
    default_provider_id: string | null;
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
import { useState, useEffect, type ChangeEvent, type FocusEvent, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Plus, Trash2, Database, Globe, Key, Edit2, HardDrive, LoaderCircle, LogOut, RefreshCw, Server } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./WebGLGrain";
import { authenticateRemote, clearRemoteSession, formatRemoteUserName, saveRemoteSession, type RemoteAuthMode, resolveAuthBaseUrl } from "../syncAuth";
import { isOfficialCloudConfigured, normalizeSyncBaseUrl, notifySyncConfigUpdated, type SyncSessionInfo } from "../syncConfig";
import { getSupportedLocale, SUPPORTED_LOCALE_OPTIONS, type TranslationKey, useI18n } from "../i18n";

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const THEMES = {
  default: {
    c1: [48, 48, 48], 
    c2: [34, 34, 34], 
    c3: [24, 24, 24], 
    c4: [20, 20, 20]
  },
  emerald: {
    c1: [42, 52, 48],
    c2: [32, 38, 35], 
    c3: [24, 26, 25], 
    c4: [20, 20, 20]
  }
};

const OPENAI_DEFAULT_ENDPOINT = "http://localhost:11434/v1";
const OPENAI_DEFAULT_MODEL = "llama3";

interface GeminiModelSummary {
    name: string;
    label: string;
}

const HStackMark = ({ size = 18 }: { size?: number }) => (
        <svg width={size} height={size} viewBox="0 0 210 210" fill="none" aria-hidden="true">
                <rect x="0" y="0" width="60" height="210" fill="currentColor" />
                <rect x="150" y="0" width="60" height="210" fill="currentColor" />
                <rect x="50" y="45" width="100" height="30" fill="currentColor" />
                <rect x="50" y="90" width="100" height="30" fill="currentColor" />
                <rect x="50" y="135" width="100" height="30" fill="currentColor" />
        </svg>
);

const PhysicalWrapper = ({ children, outerClass = '', innerClass = '', checked = false, shaderColors = THEMES.default }: {
  children: React.ReactNode;
  outerClass?: string;
  innerClass?: string;
  checked?: boolean;
  shaderColors?: any;
}) => (
  <div className={cn(
      "relative transition-all duration-300 bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem]",
      checked ? "opacity-50" : "opacity-100",
      outerClass
  )}>
    <div className={cn(
        "relative w-full h-full overflow-hidden shadow-[0_2px_5px_rgba(0,0,0,0.7)] rounded-[15px]",
        innerClass
    )}>
      <WebGLGrain colors={shaderColors} />
      <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
      <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />
      <div className="relative z-20 w-full h-full">
        {children}
      </div>
    </div>
  </div>
);

const InsetSurface = ({ children, className = '' }: { children: React.ReactNode; className?: string }) => (
    <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem]">
        <div className={cn("relative overflow-hidden shadow-[0_2px_5px_rgba(0,0,0,0.7)] rounded-[15px] bg-[#121212]", className)}>
            <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.03)_0%,rgba(255,255,255,0.01)_18%,rgba(0,0,0,0)_44%,rgba(0,0,0,0.18)_100%)]" />
            <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
            <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />
            <div className="relative z-20 w-full h-full">{children}</div>
        </div>
    </div>
);

const EngravedInput = ({ label, className, ...props }: any) => (
    <div className="flex flex-col gap-2">
        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">{label}</label>
        <InsetSurface>
                <input 
                    {...props}
                    className={cn(
                        "relative z-20 w-full bg-transparent px-4 py-3 text-[14px] text-[#D1D1D1] outline-none transition-colors placeholder:text-[#555]", 
                        className
                    )}
                />
        </InsetSurface>
    </div>
);

export interface SavedProvider {
    id: string;
    name: string;
    kind: 'OpenAiCompatible' | 'Gemini';
    endpoint: string;
    model_name: string;
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
    onboarding_complete: boolean;
}

const HOSTING_OPTIONS: Array<{
    mode: 'LocalOnly' | 'CloudOfficial' | 'CloudCustom';
    titleKey: TranslationKey;
    descriptionKey: TranslationKey;
    icon: React.ComponentType<{ size?: number }>;
}> = [
    {
        mode: 'LocalOnly' as const,
        titleKey: 'hostingLocalTitle',
        descriptionKey: 'hostingLocalDescription',
        icon: HardDrive,
    },
    {
        mode: 'CloudOfficial' as const,
        titleKey: 'hostingOfficialTitle',
        descriptionKey: 'hostingOfficialDescription',
        icon: HStackMark,
    },
    {
        mode: 'CloudCustom' as const,
        titleKey: 'hostingCustomTitle',
        descriptionKey: 'hostingCustomDescription',
        icon: Server,
    }
];

interface SettingsProps {
    isOpen: boolean;
    onClose: () => void;
}

export const Settings = ({ isOpen, onClose }: SettingsProps) => {
    const { t } = useI18n();
    const [settings, setSettings] = useState<UserSettings | null>(null);
    const [syncSession, setSyncSession] = useState<SyncSessionInfo | null>(null);
    const [isAdding, setIsAdding] = useState(false);
    const [syncAuthMode, setSyncAuthMode] = useState<RemoteAuthMode>('login');
    const [syncFirstName, setSyncFirstName] = useState('');
    const [syncLastName, setSyncLastName] = useState('');
    const [syncPassword, setSyncPassword] = useState('');
    const [syncPending, setSyncPending] = useState(false);
    const [syncError, setSyncError] = useState<string | null>(null);
    const [geminiModels, setGeminiModels] = useState<GeminiModelSummary[]>([]);
    const [isLoadingGeminiModels, setIsLoadingGeminiModels] = useState(false);
    const [geminiModelsError, setGeminiModelsError] = useState<string | null>(null);
    const [newProvider, setNewProvider] = useState<Partial<SavedProvider & { apiKey: string }>>({
        name: "",
        kind: "OpenAiCompatible",
        endpoint: OPENAI_DEFAULT_ENDPOINT,
        apiKey: "",
        model_name: OPENAI_DEFAULT_MODEL
    });

    useEffect(() => {
        if (isOpen) {
            loadSettings();
        }
    }, [isOpen]);

    const loadSettings = async () => {
        try {
            const [loadedSettings, loadedSession] = await Promise.all([
                invoke<UserSettings>("get_settings"),
                invoke<SyncSessionInfo>("get_sync_session"),
            ]);
            setSettings(loadedSettings);
            setSyncSession(loadedSession);
            setSyncError(null);
            setSyncPassword('');
        } catch (err) {
            console.error("Failed to load settings:", err);
        }
    };

    const persistSettings = async (updated: UserSettings) => {
        await invoke("save_settings", { settings: updated });
        setSettings(updated);
    };

    const handleEditProvider = (p: SavedProvider) => {
        setNewProvider({
            id: p.id,
            name: p.name,
            kind: p.kind,
            endpoint: p.endpoint,
            model_name: p.model_name,
            apiKey: "" // Explicitly blank, meaning 'keep existing' on backend
        });
        setIsAdding(true);
    };

    const handleAddNew = () => {
        setNewProvider({
            id: undefined, // Force new UUID on save
            name: "",
            kind: "OpenAiCompatible",
            endpoint: OPENAI_DEFAULT_ENDPOINT,
            apiKey: "",
            model_name: OPENAI_DEFAULT_MODEL
        });
        setGeminiModels([]);
        setGeminiModelsError(null);
        setIsAdding(true);
    };

    const fetchGeminiModels = async (apiKey: string) => {
        const trimmedKey = apiKey.trim();
        if (!trimmedKey) {
            setGeminiModels([]);
            setGeminiModelsError(null);
            return;
        }

        try {
            setIsLoadingGeminiModels(true);
            setGeminiModelsError(null);

            const collected = new Map<string, GeminiModelSummary>();
            let nextPageToken: string | null = null;

            do {
                const url = new URL('https://generativelanguage.googleapis.com/v1beta/models');
                url.searchParams.set('key', trimmedKey);
                url.searchParams.set('pageSize', '1000');
                if (nextPageToken) {
                    url.searchParams.set('pageToken', nextPageToken);
                }

                const response = await fetch(url.toString());
                if (!response.ok) {
                    throw new Error(`Gemini models request failed: ${response.status}`);
                }

                const payload = await response.json() as {
                    models?: Array<{
                        name?: string;
                        baseModelId?: string;
                        displayName?: string;
                        supportedGenerationMethods?: string[];
                    }>;
                    nextPageToken?: string;
                };

                for (const model of payload.models || []) {
                    const supportsGeneration = model.supportedGenerationMethods?.includes('generateContent');
                    const modelName = model.baseModelId || model.name?.replace(/^models\//, '') || '';
                    if (!supportsGeneration || !modelName.startsWith('gemini')) {
                        continue;
                    }

                    if (!collected.has(modelName)) {
                        collected.set(modelName, {
                            name: modelName,
                            label: model.displayName?.trim() || modelName,
                        });
                    }
                }

                nextPageToken = payload.nextPageToken || null;
            } while (nextPageToken);

            const sortedModels = Array.from(collected.values()).sort((left, right) => left.label.localeCompare(right.label));
            setGeminiModels(sortedModels);

            setNewProvider((current) => {
                if (current.kind !== 'Gemini') {
                    return current;
                }

                if (current.model_name && sortedModels.some((model) => model.name === current.model_name)) {
                    return current;
                }

                return {
                    ...current,
                    model_name: sortedModels[0]?.name || current.model_name || '',
                };
            });
        } catch (error) {
            console.error('Failed to load Gemini models:', error);
            setGeminiModels([]);
            setGeminiModelsError(error instanceof Error ? error.message : 'Failed to load Gemini models.');
        } finally {
            setIsLoadingGeminiModels(false);
        }
    };

    const handleProviderKindChange = (kind: SavedProvider["kind"]) => {
        setNewProvider((current) => {
            if (kind === 'Gemini') {
                return {
                    ...current,
                    kind,
                    endpoint: '',
                    model_name: current.kind === 'Gemini' ? (current.model_name || '') : '',
                };
            }

            return {
                ...current,
                kind,
                endpoint: current.endpoint?.trim() ? current.endpoint : OPENAI_DEFAULT_ENDPOINT,
                model_name:
                    current.kind === 'OpenAiCompatible' && current.model_name?.trim()
                        ? current.model_name
                        : OPENAI_DEFAULT_MODEL,
            };
        });

        if (kind !== 'Gemini') {
            setGeminiModels([]);
            setGeminiModelsError(null);
            setIsLoadingGeminiModels(false);
        }
    };

    useEffect(() => {
        if (!isAdding || newProvider.kind !== 'Gemini') {
            return;
        }

        const apiKey = newProvider.apiKey?.trim();
        if (!apiKey) {
            setGeminiModels([]);
            setGeminiModelsError(null);
            setIsLoadingGeminiModels(false);
            return;
        }

        const timer = window.setTimeout(() => {
            void fetchGeminiModels(apiKey);
        }, 300);

        return () => {
            window.clearTimeout(timer);
        };
    }, [isAdding, newProvider.kind, newProvider.apiKey]);

    const handleUpsertProvider = async () => {
        if (!settings || !newProvider.name || !newProvider.model_name) return;

        const provider: SavedProvider = {
            id: newProvider.id || crypto.randomUUID(),
            name: newProvider.name,
            kind: (newProvider.kind as any) || "OpenAiCompatible",
            endpoint: newProvider.kind === 'Gemini' ? "" : (newProvider.endpoint || ""),
            model_name: newProvider.model_name,
        };

        try {
            await invoke("upsert_provider", { 
                provider, 
                // Only send apiKey if they typed something. 
                // In Rust, api_key parameter is Option<String>.
                apiKey: newProvider.apiKey ? newProvider.apiKey : null 
            });
            await loadSettings();
            setIsAdding(false);
        } catch (err) {
            console.error("Failed to save provider:", err);
        }
    };

    const handleDeleteProvider = async (id: string) => {
        try {
            await invoke("delete_provider", { id });
            await loadSettings();
        } catch (err) {
            console.error("Failed to delete provider:", err);
        }
    };

    const setDefault = async (id: string) => {
        if (!settings) return;
        const updated = { ...settings, default_provider_id: id };
        try {
            await persistSettings(updated);
        } catch (err) {
            console.error("Failed to set default:", err);
        }
    };

    const handleSyncModeChange = async (syncMode: UserSettings["sync_mode"]) => {
        if (!settings || settings.sync_mode === syncMode) return;

        try {
            await clearRemoteSession();
            const updated = {
                ...settings,
                sync_mode: syncMode,
                sync_user_id: null,
                sync_user_name: null,
            };
            await persistSettings(updated);
            setSyncSession({ user_id: null, user_name: null, token: null });
            setSyncAuthMode('login');
            setSyncFirstName('');
            setSyncLastName('');
            setSyncPassword('');
            setSyncError(null);
            notifySyncConfigUpdated();
        } catch (err) {
            console.error("Failed to update hosting mode:", err);
        }
    };

    const handleRemoteAuth = async () => {
        if (!settings || syncPending) return;

        try {
            setSyncPending(true);
            setSyncError(null);

            const baseUrl = resolveAuthBaseUrl(settings.sync_mode, settings.custom_server_url);

            if (!baseUrl) {
                throw new Error(
                    settings.sync_mode === 'CloudOfficial'
                        ? t('officialCloudUnavailable')
                        : t('enterValidServerUrl')
                );
            }

            const authResult = await authenticateRemote({
                baseUrl,
                mode: syncAuthMode,
                firstName: syncFirstName,
                lastName: syncLastName,
                password: syncPassword,
            });

            await saveRemoteSession(authResult);

            const normalizedCustomUrl = settings.sync_mode === 'CloudCustom'
                ? normalizeSyncBaseUrl(settings.custom_server_url)
                : settings.custom_server_url;
            const userName = formatRemoteUserName(authResult.user);
            const updatedSettings = {
                ...settings,
                custom_server_url: normalizedCustomUrl,
                sync_user_id: authResult.user.id,
                sync_user_name: userName,
            };

            if (settings.sync_mode === 'CloudCustom' && normalizedCustomUrl !== settings.custom_server_url) {
                await persistSettings(updatedSettings);
            } else {
                setSettings(updatedSettings);
            }

            setSyncSession({
                user_id: authResult.user.id,
                user_name: userName,
                token: authResult.token,
            });
            setSyncLastName('');
            setSyncPassword('');
            notifySyncConfigUpdated();
        } catch (err) {
            console.error('Failed to authenticate sync account:', err);
            setSyncError(err instanceof Error ? err.message : t('remoteAuthFailed'));
        } finally {
            setSyncPending(false);
        }
    };

    const handleSignOut = async () => {
        if (!settings || syncPending) return;

        try {
            setSyncPending(true);
            await clearRemoteSession();
            setSettings({
                ...settings,
                sync_user_id: null,
                sync_user_name: null,
            });
            setSyncSession({ user_id: null, user_name: null, token: null });
            setSyncLastName('');
            setSyncPassword('');
            setSyncError(null);
            notifySyncConfigUpdated();
        } catch (err) {
            console.error('Failed to clear sync session:', err);
        } finally {
            setSyncPending(false);
        }
    };

    const remoteBaseUrl = settings ? resolveAuthBaseUrl(settings.sync_mode, settings.custom_server_url) : null;
    const hasRemoteMode = settings?.sync_mode === 'CloudOfficial' || settings?.sync_mode === 'CloudCustom';
    const hasRemoteSession = Boolean(syncSession?.token && syncSession.user_id);

    const isGeminiProvider = newProvider.kind === 'Gemini';
    const selectedLocale = getSupportedLocale(settings?.locale || 'en-GB');
    const geminiModelOptions = (() => {
        const options = [...geminiModels];
        const currentModel = newProvider.model_name?.trim();
        if (currentModel && !options.some((model) => model.name === currentModel)) {
            options.unshift({ name: currentModel, label: currentModel });
        }
        return options;
    })();

    return (
        <AnimatePresence>
            {isOpen && (
                <motion.div 
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 20 }}
                    className="absolute inset-0 z-[100] flex flex-col bg-[#080808] overflow-hidden"
                >
                    <WebGLGrain 
                        colors={{ c1: [20, 20, 20], c2: [15, 15, 15], c3: [10, 10, 10], c4: [5, 5, 5] }}
                        spreadX={0.35} spreadY={1.1} contrast={2.0} noiseFactor={0.7} opacity={1.0}
                    />

                    <div className="relative z-20 h-full flex flex-col p-6 pt-12">
                        {/* Header */}
                        <div className="flex items-center justify-between mb-8 shrink-0" data-tauri-drag-region>
                            <div className="flex items-center gap-3 pointer-events-none" data-tauri-drag-region>
                                <div data-tauri-drag-region>
                                    <h2 className="text-[20px] font-semibold tracking-tight text-[#D1D1D1]" data-tauri-drag-region>{t('settingsTitle')}</h2>
                                    <p className="text-[9px] text-[#777] uppercase tracking-widest font-bold" data-tauri-drag-region>{t('settingsSubtitle')}</p>
                                </div>
                            </div>
                            <button 
                                onClick={onClose}
                                className="w-10 h-10 rounded-full hover:bg-white/5 flex items-center justify-center text-[#777] hover:text-[#D1D1D1] transition-all shrink-0"
                            >
                                <X size={24} />
                            </button>
                        </div>

                        {/* Scrollable Content Area */}
                        <div className="flex-1 overflow-y-auto no-scrollbar pr-2 pb-2">
                            {/* Hosting & Sync Section */}
                            <section className="mb-8">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        {t('settingsHostingSync')}
                                    </h3>
                                </div>

                                <div className="flex flex-col gap-3">
                                    {settings && HOSTING_OPTIONS.map(option => {
                                        const isSelected = settings.sync_mode === option.mode;
                                        const Icon = option.icon;

                                        return (
                                            <button
                                                key={option.mode}
                                                type="button"
                                                onClick={() => handleSyncModeChange(option.mode)}
                                                className="text-left"
                                            >
                                                <PhysicalWrapper
                                                    outerClass="rounded-[1.15rem]"
                                                    innerClass="px-3.5 py-3 flex items-start gap-3 transition-colors"
                                                    shaderColors={isSelected ? THEMES.emerald : THEMES.default}
                                                >
                                                    <div className="relative z-20 min-w-0 flex-1">
                                                        <div className="flex items-start justify-between gap-3">
                                                            <div>
                                                                <div className="flex items-center gap-2.5">
                                                                    <div className={cn(
                                                                        "flex h-5 w-5 shrink-0 items-center justify-center transition-colors",
                                                                        isSelected ? "text-[#D9DDE4]" : "text-[#C8CDD6]"
                                                                    )}>
                                                                        <Icon size={16} />
                                                                    </div>
                                                                    <span className="text-[16px] font-medium tracking-[-0.02em] text-[#F1F1F1]">{t(option.titleKey)}</span>
                                                                </div>
                                                                <p className="mt-1 pr-1 text-[13px] leading-6 text-white/58">{t(option.descriptionKey)}</p>
                                                            </div>
                                                            <span className={cn(
                                                                "inline-flex shrink-0 items-center pt-0.5 text-[9px] font-bold uppercase tracking-[0.22em]",
                                                                isSelected
                                                                    ? "text-white/45"
                                                                    : "text-white/32"
                                                            )}>
                                                                {isSelected ? t('current') : t('select')}
                                                            </span>
                                                        </div>
                                                    </div>
                                                </PhysicalWrapper>
                                            </button>
                                        );
                                    })}

                                    {settings?.sync_mode === 'CloudCustom' && (
                                        <div className="pt-2">
                                            <EngravedInput
                                                label={t('customServerUrl')}
                                                value={settings.custom_server_url || ""}
                                                onChange={(e: ChangeEvent<HTMLInputElement>) => setSettings({ ...settings, custom_server_url: e.target.value })}
                                                onBlur={async (e: FocusEvent<HTMLInputElement>) => {
                                                    if (!settings) return;
                                                    const normalized = normalizeSyncBaseUrl(e.target.value);
                                                    const didChange = normalized !== normalizeSyncBaseUrl(settings.custom_server_url);
                                                    const updated = {
                                                        ...settings,
                                                        custom_server_url: normalized,
                                                        sync_user_id: didChange ? null : settings.sync_user_id,
                                                        sync_user_name: didChange ? null : settings.sync_user_name,
                                                    };

                                                    try {
                                                        if (didChange) {
                                                            await clearRemoteSession();
                                                            setSyncSession({ user_id: null, user_name: null, token: null });
                                                            setSyncPassword('');
                                                            setSyncError(null);
                                                        }
                                                        await persistSettings(updated);
                                                        if (didChange) {
                                                            notifySyncConfigUpdated();
                                                        }
                                                    } catch (err) {
                                                        console.error("Failed to save custom server URL:", err);
                                                    }
                                                }}
                                                onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => {
                                                    if (e.key === 'Enter') {
                                                        e.currentTarget.blur();
                                                    }
                                                }}
                                                placeholder={t('customServerPlaceholder')}
                                                className="font-mono"
                                            />
                                            <p className="mt-2 px-1 text-[10px] text-[#555]">{t('customServerHint')}</p>
                                        </div>
                                    )}

                                    {hasRemoteMode && settings && (
                                        <PhysicalWrapper
                                            outerClass="rounded-[1.15rem]"
                                            innerClass="p-4 flex flex-col gap-4"
                                            shaderColors={THEMES.default}
                                        >
                                            <div className="relative z-20 flex items-start justify-between gap-4">
                                                <div className="min-w-0">
                                                    <p className="text-[9px] font-bold uppercase tracking-[0.22em] text-white/32">{t('account')}</p>
                                                    <p className="mt-2 text-[14px] leading-6 text-[#E6E6E6]">
                                                        {hasRemoteSession
                                                            ? t('connectedAs', { name: syncSession?.user_name || settings.sync_user_name || t('yourAccount') })
                                                            : settings.sync_mode === 'CloudOfficial'
                                                                ? t('signInManagedCloud')
                                                                : t('signInSelfHosted')}
                                                    </p>
                                                </div>
                                                {remoteBaseUrl ? (
                                                    <span className="shrink-0 rounded-full border border-white/10 bg-black/15 px-3 py-1 text-[10px] font-medium text-white/34">
                                                        {remoteBaseUrl}
                                                    </span>
                                                ) : null}
                                            </div>

                                            {settings.sync_mode === 'CloudOfficial' && !isOfficialCloudConfigured() ? (
                                                <div className="relative z-20 rounded-[1rem] border border-amber-300/18 bg-amber-200/6 px-3.5 py-3 text-[12px] leading-5 text-amber-100/72">
                                                    {t('officialCloudUnavailable')}
                                                </div>
                                            ) : null}

                                            {settings.sync_mode === 'CloudCustom' && !remoteBaseUrl ? (
                                                <div className="relative z-20 rounded-[1rem] border border-white/8 bg-black/18 px-3.5 py-3 text-[12px] leading-5 text-white/48">
                                                    {t('enterServerBeforeSignIn')}
                                                </div>
                                            ) : null}

                                            {hasRemoteSession ? (
                                                <div className="relative z-20 flex items-center justify-between gap-4 rounded-[1rem] border border-white/8 bg-black/14 px-3.5 py-3.5">
                                                    <div className="min-w-0">
                                                        <p className="text-[15px] font-medium text-[#F1F1F1]">{syncSession?.user_name || settings.sync_user_name || t('connectedAccount')}</p>
                                                        <p className="mt-1 text-[12px] text-white/42">
                                                            {settings.sync_mode === 'CloudOfficial' ? t('managedCloudSession') : t('selfHostedSession')}
                                                        </p>
                                                    </div>
                                                    <button
                                                        type="button"
                                                        onClick={handleSignOut}
                                                        disabled={syncPending}
                                                        className="inline-flex shrink-0 items-center gap-2 rounded-[999px] border border-white/10 px-3 py-2 text-[10px] font-bold uppercase tracking-[0.18em] text-white/58 transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-45"
                                                    >
                                                        {syncPending ? <LoaderCircle size={13} className="animate-spin" /> : <LogOut size={13} />}
                                                        {t('signOut')}
                                                    </button>
                                                </div>
                                            ) : (
                                                <div className="relative z-20 flex flex-col gap-4">
                                                    <div className="flex rounded-[999px] border border-white/8 bg-black/15 p-1">
                                                        {(['login', 'register'] as const).map(mode => {
                                                            const isActive = syncAuthMode === mode;
                                                            return (
                                                                <button
                                                                    key={mode}
                                                                    type="button"
                                                                    onClick={() => setSyncAuthMode(mode)}
                                                                    className={cn(
                                                                        "flex-1 rounded-[999px] px-3 py-2 text-[10px] font-bold uppercase tracking-[0.18em] transition-colors",
                                                                        isActive ? "bg-white text-[#080808]" : "text-white/40 hover:text-white/64"
                                                                    )}
                                                                >
                                                                    {mode === 'login' ? t('login') : t('register')}
                                                                </button>
                                                            );
                                                        })}
                                                    </div>

                                                    <div className="grid gap-3 sm:grid-cols-2">
                                                        <EngravedInput
                                                            label={t('firstName')}
                                                            value={syncFirstName}
                                                            onChange={(e: ChangeEvent<HTMLInputElement>) => setSyncFirstName(e.target.value)}
                                                            placeholder={syncAuthMode === 'login' ? t('accountFirstName') : t('chooseFirstName')}
                                                        />
                                                        {syncAuthMode === 'register' ? (
                                                            <EngravedInput
                                                                label={t('lastName')}
                                                                value={syncLastName}
                                                                onChange={(e: ChangeEvent<HTMLInputElement>) => setSyncLastName(e.target.value)}
                                                                placeholder={t('optional')}
                                                            />
                                                        ) : (
                                                            <EngravedInput
                                                                label={t('password')}
                                                                type="password"
                                                                value={syncPassword}
                                                                onChange={(e: ChangeEvent<HTMLInputElement>) => setSyncPassword(e.target.value)}
                                                                placeholder={t('enterPassword')}
                                                            />
                                                        )}
                                                    </div>

                                                    {syncAuthMode === 'register' ? (
                                                        <EngravedInput
                                                            label={t('password')}
                                                            type="password"
                                                            value={syncPassword}
                                                            onChange={(e: ChangeEvent<HTMLInputElement>) => setSyncPassword(e.target.value)}
                                                            placeholder={t('createPassword')}
                                                        />
                                                    ) : null}

                                                    {syncError ? (
                                                        <div className="rounded-[1rem] border border-red-400/18 bg-red-500/8 px-3.5 py-3 text-[12px] leading-5 text-red-100/80">
                                                            {syncError}
                                                        </div>
                                                    ) : null}

                                                    <button
                                                        type="button"
                                                        onClick={handleRemoteAuth}
                                                        disabled={!syncFirstName.trim() || !syncPassword || !remoteBaseUrl || syncPending}
                                                        className="inline-flex items-center justify-center gap-2 rounded-[1rem] bg-[#EFEFEF] px-4 py-3 text-[11px] font-bold uppercase tracking-[0.2em] text-[#080808] transition-all hover:bg-white disabled:cursor-not-allowed disabled:opacity-45"
                                                    >
                                                        {syncPending ? <LoaderCircle size={14} className="animate-spin" /> : null}
                                                        {syncAuthMode === 'login' ? t('signIn') : t('createAccount')}
                                                    </button>

                                                    <p className="text-[10px] leading-5 text-white/26">
                                                        {t('syncKeychainHint')}
                                                    </p>
                                                </div>
                                            )}
                                        </PhysicalWrapper>
                                    )}
                                </div>
                            </section>

                            {/* Providers Section */}
                            <section className="mb-8">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        {t('llmProviders')}
                                    </h3>
                                    <button 
                                        onClick={handleAddNew}
                                        className="text-[9px] font-bold uppercase tracking-widest text-[#D1D1D1] hover:text-white transition-colors flex items-center gap-1.5 shrink-0"
                                    >
                                        <Plus size={12} />
                                        {t('addNew')}
                                    </button>
                                </div>

                                <div className="flex flex-col gap-3">
                                    {settings?.providers.map(p => {
                                        const isDefault = settings.default_provider_id === p.id;
                                        return (
                                            <PhysicalWrapper 
                                                key={p.id}
                                                innerClass="p-4 flex flex-col gap-3 group cursor-pointer transition-colors"
                                                shaderColors={isDefault ? THEMES.emerald : THEMES.default}
                                            >
                                                <div 
                                                    className="absolute inset-0 z-10" 
                                                    onClick={() => setDefault(p.id)} 
                                                />
                                                <div className="flex items-start justify-between relative z-20 pointer-events-none">
                                                    <div className="flex flex-col gap-1 min-w-0">
                                                        <div className="flex items-center gap-2">
                                                            <span className="text-[15px] font-medium text-[#D1D1D1] truncate">{p.name}</span>
                                                        </div>
                                                        <span className="text-[12px] text-[#777] font-mono tracking-tight break-all">{p.model_name}</span>
                                                    </div>
                                                    <div className="flex items-center gap-1 pointer-events-auto shrink-0">
                                                        <button 
                                                            onClick={(e) => { e.stopPropagation(); handleEditProvider(p); }}
                                                            className="p-2 rounded-lg hover:bg-white/5 text-[#777] hover:text-[#D1D1D1] transition-all opacity-0 group-hover:opacity-100"
                                                            title={t('editProvider')}
                                                        >
                                                            <Edit2 size={16} />
                                                        </button>
                                                        <button 
                                                            onClick={(e) => { e.stopPropagation(); handleDeleteProvider(p.id); }}
                                                            className="p-2 rounded-lg hover:bg-red-500/10 text-[#777] hover:text-red-400 transition-all opacity-0 group-hover:opacity-100"
                                                            title={t('deleteProvider')}
                                                        >
                                                            <Trash2 size={16} />
                                                        </button>
                                                    </div>
                                                </div>
                                                <div className="flex items-center gap-4 text-[11px] text-[#777] relative z-20 pointer-events-none">
                                                    <div className="flex items-center gap-1.5 shrink-0">
                                                        <Database size={12} />
                                                        {p.kind === 'OpenAiCompatible' ? t('openAiCompatible') : t('googleGemini')}
                                                    </div>
                                                    <div className="flex items-center gap-1.5 min-w-0">
                                                        <Globe size={12} className="shrink-0" />
                                                        <span className="truncate">{p.endpoint || t('cloudApi')}</span>
                                                    </div>
                                                </div>
                                            </PhysicalWrapper>
                                        );
                                    })}

                                    {settings?.providers.length === 0 && !isAdding && (
                                        <div className="p-8 rounded-[1.25rem] border border-dashed border-[#333] text-center">
                                            <p className="text-[13px] text-[#777]">{t('noProvidersConfigured')}</p>
                                        </div>
                                    )}
                                </div>
                            </section>

                            {/* Locale Settings - Positioned below providers in scroll area */}
                            <section className="mt-6 border-t border-white/5 pt-6">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        {t('localeDisplay')}
                                    </h3>
                                </div>

                                <div className="flex flex-col gap-5">
                                    <div className="flex flex-col gap-2">
                                        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">{t('languageRegion')}</label>
                                        <select 
                                            value={selectedLocale}
                                            onChange={async (e) => {
                                                if (!settings) return;
                                                const updated = { ...settings, locale: e.target.value };
                                                await invoke("save_settings", { settings: updated });
                                                setSettings(updated);
                                                setTimeout(() => {
                                                    window.dispatchEvent(new CustomEvent('localeUpdated'));
                                                }, 100);
                                            }}
                                            className="bg-[#141414] p-4 shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] text-[14px] text-[#D1D1D1] outline-none border border-transparent hover:border-white/5 transition-all"
                                        >
                                            {SUPPORTED_LOCALE_OPTIONS.map((option) => (
                                                <option key={option.value} value={option.value}>
                                                    {option.value === 'en-GB'
                                                        ? t('languageEnglishUk')
                                                        : option.value === 'fr-FR'
                                                            ? t('languageFrench')
                                                            : option.value === 'es-ES'
                                                                ? t('languageSpanish')
                                                                : t('languageChinese')}
                                                </option>
                                            ))}
                                        </select>
                                        <p className="text-[10px] text-[#555] px-1">{t('localeAppliesImmediately')}</p>
                                    </div>

                                    <div className="flex flex-col gap-2">
                                        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">{t('timeFormat')}</label>
                                        <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] flex gap-2 border border-transparent">
                                            <button
                                                onClick={async () => {
                                                    if (!settings) return;
                                                    const updated = { ...settings, hour12: true };
                                                    await invoke("save_settings", { settings: updated });
                                                    setSettings(updated);
                                                    setTimeout(() => {
                                                        window.dispatchEvent(new CustomEvent('localeUpdated'));
                                                    }, 100);
                                                }}
                                                className={cn(
                                                    "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                    settings?.hour12 !== false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                )}
                                            >
                                                {settings?.hour12 !== false && (
                                                    <div className="absolute inset-0 rounded-[13px] bg-[linear-gradient(180deg,#1c1c1c_0%,#141414_100%)]" />
                                                )}
                                                <span className="relative z-10">{t('timeFormat12h')}</span>
                                            </button>
                                            <button
                                                onClick={async () => {
                                                    if (!settings) return;
                                                    const updated = { ...settings, hour12: false };
                                                    await invoke("save_settings", { settings: updated });
                                                    setSettings(updated);
                                                    setTimeout(() => {
                                                        window.dispatchEvent(new CustomEvent('localeUpdated'));
                                                    }, 100);
                                                }}
                                                className={cn(
                                                    "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                    settings?.hour12 === false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                )}
                                            >
                                                {settings?.hour12 === false && (
                                                    <div className="absolute inset-0 rounded-[13px] bg-[linear-gradient(180deg,#1c1c1c_0%,#141414_100%)]" />
                                                )}
                                                <span className="relative z-10">{t('timeFormat24h')}</span>
                                            </button>
                                        </div>
                                        <p className="text-[10px] text-[#555] px-1">{t('localeAppliesImmediately')}</p>
                                    </div>
                                </div>
                            </section>
                        </div>

                        {/* Footer / Info - Fixed at bottom */}
                        <div className="mt-6 pt-6 border-t border-white/5 shrink-0">
                            <div className="flex items-center gap-3 p-4 rounded-[1.25rem] bg-[#141414] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] border border-transparent text-[11px] text-[#777] leading-relaxed relative overflow-hidden">
                                <Key size={16} className="shrink-0 text-[#D1D1D1] relative z-10" />
                                <p className="relative z-10">{t('apiKeysSecurity')}</p>
                                <div className="absolute inset-0 bg-[#121212] opacity-50 z-0"></div>
                            </div>
                        </div>

                        {/* Add Provider Modal */}
                        <AnimatePresence>
                            {isAdding && (
                                <motion.div 
                                    initial={{ opacity: 0 }}
                                    animate={{ opacity: 1 }}
                                    exit={{ opacity: 0 }}
                                    className="absolute inset-0 z-[110] bg-[#080808]/90 backdrop-blur-md p-6 flex flex-col pt-12"
                                >
                                    <div className="flex items-center justify-between mb-8">
                                        <h3 className="text-[18px] font-semibold text-[#D1D1D1]">
                                            {newProvider.id ? t('editProvider') : t('newProvider')}
                                        </h3>
                                        <button onClick={() => setIsAdding(false)} className="text-[#777] hover:text-[#D1D1D1] transition-colors">
                                            <X size={24} />
                                        </button>
                                    </div>

                                    <div className="flex-1 overflow-y-auto no-scrollbar flex flex-col gap-5 pb-8">
                                        <EngravedInput 
                                            label={t('friendlyName')}
                                            value={newProvider.name}
                                            onChange={(e: any) => setNewProvider({...newProvider, name: e.target.value})}
                                            placeholder={t('friendlyNamePlaceholder')}
                                        />

                                        <div className="flex flex-col gap-2">
                                            <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">{t('providerKind')}</label>
                                            <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] flex gap-2">
                                                {['OpenAiCompatible', 'Gemini'].map(kind => {
                                                    const isSelected = newProvider.kind === kind;
                                                    return (
                                                        <button
                                                            key={kind}
                                                            onClick={() => handleProviderKindChange(kind as SavedProvider['kind'])}
                                                            className={cn(
                                                                "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                                isSelected ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                            )}
                                                        >
                                                            {isSelected && (
                                                                <div className="absolute inset-0 rounded-[13px] bg-[linear-gradient(180deg,#1c1c1c_0%,#141414_100%)]" />
                                                            )}
                                                            <span className="relative z-10">{kind === 'OpenAiCompatible' ? t('openAiCompatible') : t('googleGemini')}</span>
                                                        </button>
                                                    );
                                                })}
                                            </div>
                                        </div>

                                        {isGeminiProvider ? (
                                            <div className="flex flex-col gap-2">
                                                <div className="flex items-center justify-between gap-3 px-1">
                                                    <label className="text-[9px] font-bold uppercase tracking-widest text-[#777]">{t('geminiModel')}</label>
                                                    <button
                                                        type="button"
                                                        onClick={() => void fetchGeminiModels(newProvider.apiKey || '')}
                                                        disabled={!newProvider.apiKey?.trim() || isLoadingGeminiModels}
                                                        className="inline-flex items-center gap-1 text-[9px] font-bold uppercase tracking-widest text-[#777] transition-colors hover:text-[#D1D1D1] disabled:cursor-not-allowed disabled:opacity-40"
                                                    >
                                                        <RefreshCw size={10} className={cn(isLoadingGeminiModels && 'animate-spin')} />
                                                        {t('refresh')}
                                                    </button>
                                                </div>
                                                <InsetSurface>
                                                    <select
                                                        value={newProvider.model_name || ''}
                                                        onChange={(e) => setNewProvider({ ...newProvider, model_name: e.target.value })}
                                                        disabled={geminiModelOptions.length === 0}
                                                        className="relative z-20 w-full bg-transparent px-4 py-3 text-[14px] text-[#D1D1D1] outline-none disabled:text-[#666]"
                                                    >
                                                        {geminiModelOptions.length === 0 ? (
                                                            <option value="">
                                                                {newProvider.apiKey?.trim() ? t('noGeminiModelsLoaded') : t('enterGeminiApiKeyFirst')}
                                                            </option>
                                                        ) : null}
                                                        {geminiModelOptions.map((model) => (
                                                            <option key={model.name} value={model.name}>
                                                                {model.label}
                                                            </option>
                                                        ))}
                                                    </select>
                                                </InsetSurface>
                                                <p className="text-[10px] text-[#555] px-1">{t('geminiEndpointHint')}</p>
                                                {geminiModelsError ? (
                                                    <p className="text-[10px] text-red-300/70 px-1">{geminiModelsError}</p>
                                                ) : null}
                                            </div>
                                        ) : (
                                            <>
                                                <EngravedInput 
                                                    label={t('endpointUrl')}
                                                    value={newProvider.endpoint}
                                                    onChange={(e: any) => setNewProvider({...newProvider, endpoint: e.target.value})}
                                                    placeholder={OPENAI_DEFAULT_ENDPOINT}
                                                    className="font-mono"
                                                />

                                                <EngravedInput 
                                                    label={t('modelName')}
                                                    value={newProvider.model_name}
                                                    onChange={(e: any) => setNewProvider({...newProvider, model_name: e.target.value})}
                                                    placeholder={t('modelNamePlaceholder')}
                                                    className="font-mono"
                                                />
                                            </>
                                        )}

                                        <EngravedInput 
                                            label={isGeminiProvider ? t('geminiApiKey') : t('apiKey')}
                                            type="password"
                                            value={newProvider.apiKey}
                                            onChange={(e: any) => setNewProvider({...newProvider, apiKey: e.target.value})}
                                            placeholder={newProvider.id ? t('apiKeyPlaceholderKeepExisting') : '••••••••'}
                                        />

                                        <button 
                                            onClick={handleUpsertProvider}
                                            className="mt-6 bg-[#D1D1D1] text-[#080808] font-bold tracking-widest uppercase text-[11px] py-4 rounded-[1.25rem] hover:scale-[1.02] active:scale-[0.98] transition-all shadow-[0_4px_10px_rgba(0,0,0,0.5)]"
                                        >
                                            {t('saveProvider')}
                                        </button>
                                    </div>
                                </motion.div>
                            )}
                        </AnimatePresence>
                    </div>
                </motion.div>
            )}
        </AnimatePresence>
    );
};

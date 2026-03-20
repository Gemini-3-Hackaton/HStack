import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Plus, Trash2, Database, Globe, Key, Edit2, Cloud, HardDrive, Server } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./WebGLGrain";

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

const EngravedInput = ({ label, className, ...props }: any) => (
    <div className="flex flex-col gap-2">
        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">{label}</label>
        <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem]">
            <div className="relative overflow-hidden shadow-[0_2px_5px_rgba(0,0,0,0.7)] rounded-[15px] bg-[#121212]">
                <WebGLGrain colors={THEMES.default} />
                <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
                <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />
                <input 
                    {...props}
                    className={cn(
                        "relative z-20 w-full bg-transparent px-4 py-3 text-[14px] text-[#D1D1D1] outline-none transition-colors placeholder:text-[#555]", 
                        className
                    )}
                />
            </div>
        </div>
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
    onboarding_complete: boolean;
}

const HOSTING_OPTIONS = [
    {
        mode: 'LocalOnly' as const,
        title: 'Local Only',
        description: 'Keep everything on this device with no sync dependency.',
        hint: 'Best for privacy-first personal use.',
        icon: HardDrive,
    },
    {
        mode: 'CloudOfficial' as const,
        title: 'Official Cloud',
        description: 'Use the managed HStack cloud for sync across devices.',
        hint: 'Fastest setup for multi-device access.',
        icon: Cloud,
    },
    {
        mode: 'CloudCustom' as const,
        title: 'Self-Hosted',
        description: 'Connect this app to your own HStack Lite deployment.',
        hint: 'Bring your own server endpoint.',
        icon: Server,
    }
];

interface SettingsProps {
    isOpen: boolean;
    onClose: () => void;
}

export const Settings = ({ isOpen, onClose }: SettingsProps) => {
    const [settings, setSettings] = useState<UserSettings | null>(null);
    const [isAdding, setIsAdding] = useState(false);
    const [newProvider, setNewProvider] = useState<Partial<SavedProvider & { apiKey: string }>>({
        name: "",
        kind: "OpenAiCompatible",
        endpoint: "http://localhost:11434/v1",
        apiKey: "",
        model_name: "llama3"
    });

    useEffect(() => {
        if (isOpen) {
            loadSettings();
        }
    }, [isOpen]);

    const loadSettings = async () => {
        try {
            const res = await invoke<UserSettings>("get_settings");
            setSettings(res);
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
            endpoint: "http://localhost:11434/v1",
            apiKey: "",
            model_name: "llama3"
        });
        setIsAdding(true);
    };

    const handleUpsertProvider = async () => {
        if (!settings || !newProvider.name || !newProvider.model_name) return;

        const provider: SavedProvider = {
            id: newProvider.id || crypto.randomUUID(),
            name: newProvider.name,
            kind: (newProvider.kind as any) || "OpenAiCompatible",
            endpoint: newProvider.endpoint || "",
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
        const updated = {
            ...settings,
            sync_mode: syncMode,
            custom_server_url: syncMode === 'CloudCustom' ? settings.custom_server_url : null,
        };

        try {
            await persistSettings(updated);
        } catch (err) {
            console.error("Failed to update hosting mode:", err);
        }
    };

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
                                    <h2 className="text-[20px] font-semibold tracking-tight text-[#D1D1D1]" data-tauri-drag-region>Settings</h2>
                                    <p className="text-[9px] text-[#777] uppercase tracking-widest font-bold" data-tauri-drag-region>Configuration & LLM</p>
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
                            {/* Providers Section */}
                            <section className="mb-8">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        Hosting & Sync
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
                                                    innerClass="p-4 flex items-start gap-4 transition-colors"
                                                    shaderColors={isSelected ? THEMES.emerald : THEMES.default}
                                                >
                                                    <div className={cn(
                                                        "relative z-20 w-11 h-11 rounded-2xl border flex items-center justify-center shrink-0 transition-colors",
                                                        isSelected ? "border-white/10 bg-white/10 text-[#D1D1D1]" : "border-white/5 bg-black/20 text-[#777]"
                                                    )}>
                                                        <Icon size={18} />
                                                    </div>
                                                    <div className="relative z-20 flex-1 min-w-0">
                                                        <div className="flex items-center justify-between gap-3">
                                                            <span className="text-[15px] font-medium text-[#D1D1D1]">{option.title}</span>
                                                            <span className={cn(
                                                                "text-[9px] font-bold uppercase tracking-[0.22em]",
                                                                isSelected ? "text-[#D1D1D1]" : "text-[#555]"
                                                            )}>
                                                                {isSelected ? 'Selected' : 'Available'}
                                                            </span>
                                                        </div>
                                                        <p className="mt-1 text-[12px] text-[#8A8A8A] leading-relaxed">{option.description}</p>
                                                        <p className="mt-2 text-[10px] uppercase tracking-[0.18em] text-[#555]">{option.hint}</p>
                                                    </div>
                                                </PhysicalWrapper>
                                            </button>
                                        );
                                    })}

                                    {settings?.sync_mode === 'CloudCustom' && (
                                        <div className="pt-2">
                                            <EngravedInput
                                                label="Custom Server URL"
                                                value={settings.custom_server_url || ""}
                                                onChange={(e: any) => setSettings({ ...settings, custom_server_url: e.target.value })}
                                                onBlur={async (e: any) => {
                                                    if (!settings) return;
                                                    const trimmed = e.target.value.trim();
                                                    const updated = {
                                                        ...settings,
                                                        custom_server_url: trimmed || null,
                                                    };

                                                    try {
                                                        await persistSettings(updated);
                                                    } catch (err) {
                                                        console.error("Failed to save custom server URL:", err);
                                                    }
                                                }}
                                                onKeyDown={(e: any) => {
                                                    if (e.key === 'Enter') {
                                                        e.currentTarget.blur();
                                                    }
                                                }}
                                                placeholder="https://your-hstack-lite.example.com"
                                                className="font-mono"
                                            />
                                            <p className="mt-2 px-1 text-[10px] text-[#555]">Saved when the field loses focus. Point this to your HStack Lite base URL.</p>
                                        </div>
                                    )}
                                </div>
                            </section>

                            {/* Providers Section */}
                            <section className="mb-8">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        LLM Providers
                                    </h3>
                                    <button 
                                        onClick={handleAddNew}
                                        className="text-[9px] font-bold uppercase tracking-widest text-[#D1D1D1] hover:text-white transition-colors flex items-center gap-1.5 shrink-0"
                                    >
                                        <Plus size={12} />
                                        Add New
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
                                                            title="Edit Provider"
                                                        >
                                                            <Edit2 size={16} />
                                                        </button>
                                                        <button 
                                                            onClick={(e) => { e.stopPropagation(); handleDeleteProvider(p.id); }}
                                                            className="p-2 rounded-lg hover:bg-red-500/10 text-[#777] hover:text-red-400 transition-all opacity-0 group-hover:opacity-100"
                                                            title="Delete Provider"
                                                        >
                                                            <Trash2 size={16} />
                                                        </button>
                                                    </div>
                                                </div>
                                                <div className="flex items-center gap-4 text-[11px] text-[#777] relative z-20 pointer-events-none">
                                                    <div className="flex items-center gap-1.5 shrink-0">
                                                        <Database size={12} />
                                                        {p.kind === 'OpenAiCompatible' ? 'OpenAI compatible' : 'Google Gemini'}
                                                    </div>
                                                    <div className="flex items-center gap-1.5 min-w-0">
                                                        <Globe size={12} className="shrink-0" />
                                                        <span className="truncate">{p.endpoint || 'Cloud API'}</span>
                                                    </div>
                                                </div>
                                            </PhysicalWrapper>
                                        );
                                    })}

                                    {settings?.providers.length === 0 && !isAdding && (
                                        <div className="p-8 rounded-[1.25rem] border border-dashed border-[#333] text-center">
                                            <p className="text-[13px] text-[#777]">No providers configured yet.</p>
                                        </div>
                                    )}
                                </div>
                            </section>

                            {/* Locale Settings - Positioned below providers in scroll area */}
                            <section className="mt-6 border-t border-white/5 pt-6">
                                <div className="flex items-center justify-between mb-4 px-1">
                                    <h3 className="text-[12px] uppercase tracking-widest font-bold text-[#777] flex items-center gap-2">
                                        Locale & Display
                                    </h3>
                                </div>

                                <div className="flex flex-col gap-5">
                                    <div className="flex flex-col gap-2">
                                        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">Language & Region</label>
                                        <select 
                                            value={settings?.locale || navigator.language || 'en-US'}
                                            onChange={async (e) => {
                                                if (!settings) return;
                                                const updated = { ...settings, locale: e.target.value };
                                                await invoke("save_settings", { settings: updated });
                                                setSettings(updated);
                                                // Show a subtle notification instead of full reload
                                                setTimeout(() => {
                                                    const event = new CustomEvent('localeUpdated');
                                                    window.dispatchEvent(event);
                                            }, 100);
                                            }}
                                            className="bg-[#141414] p-4 shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] text-[14px] text-[#D1D1D1] outline-none border border-transparent hover:border-white/5 transition-all"
                                        >
                                            <option value="en-US">English (US)</option>
                                            <option value="en-GB">English (UK)</option>
                                            <option value="fr-FR">Français (France)</option>
                                            <option value="de-DE">Deutsch (Deutschland)</option>
                                            <option value="es-ES">Español (España)</option>
                                            <option value="it-IT">Italiano (Italia)</option>
                                            <option value="ja-JP">日本語 (日本)</option>
                                            <option value="zh-CN">中文 (中国)</option>
                                        </select>
                                        <p className="text-[10px] text-[#555] px-1">Changes will apply on next app launch</p>
                                    </div>

                                    <div className="flex flex-col gap-2">
                                        <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">Time Format</label>
                                        <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] flex gap-2 border border-transparent">
                                            <button
                                                onClick={async () => {
                                                    if (!settings) return;
                                                    const updated = { ...settings, hour12: true };
                                                    await invoke("save_settings", { settings: updated });
                                                    setSettings(updated);
                                                }}
                                                className={cn(
                                                    "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                    settings?.hour12 !== false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                )}
                                            >
                                                {settings?.hour12 !== false && (
                                                    <>
                                                        <WebGLGrain colors={THEMES.default} />
                                                        <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03]" />
                                                        <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03]" />
                                                    </>
                                                )}
                                                <span className="relative z-10">12-hour (AM/PM)</span>
                                            </button>
                                            <button
                                                onClick={async () => {
                                                    if (!settings) return;
                                                    const updated = { ...settings, hour12: false };
                                                    await invoke("save_settings", { settings: updated });
                                                    setSettings(updated);
                                                }}
                                                className={cn(
                                                    "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                    settings?.hour12 === false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                )}
                                            >
                                                {settings?.hour12 === false && (
                                                    <>
                                                        <WebGLGrain colors={THEMES.default} />
                                                        <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03]" />
                                                        <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03]" />
                                                    </>
                                                )}
                                                <span className="relative z-10">24-hour</span>
                                            </button>
                                        </div>
                                        <p className="text-[10px] text-[#555] px-1">Changes will apply on next app launch</p>
                                    </div>
                                </div>
                            </section>
                        </div>

                        {/* Footer / Info - Fixed at bottom */}
                        <div className="mt-6 pt-6 border-t border-white/5 shrink-0">
                            <div className="flex items-center gap-3 p-4 rounded-[1.25rem] bg-[#141414] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] border border-transparent text-[11px] text-[#777] leading-relaxed relative overflow-hidden">
                                <Key size={16} className="shrink-0 text-[#D1D1D1] relative z-10" />
                                <p className="relative z-10">API keys are securely stored in your OS Keychain (Hardware Encrypted) and never written to disk in plaintext.</p>
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
                                            {newProvider.id ? 'Edit Provider' : 'New Provider'}
                                        </h3>
                                        <button onClick={() => setIsAdding(false)} className="text-[#777] hover:text-[#D1D1D1] transition-colors">
                                            <X size={24} />
                                        </button>
                                    </div>

                                    <div className="flex-1 overflow-y-auto no-scrollbar flex flex-col gap-5 pb-8">
                                        <EngravedInput 
                                            label="Friendly Name"
                                            value={newProvider.name}
                                            onChange={(e: any) => setNewProvider({...newProvider, name: e.target.value})}
                                            placeholder="e.g. Local Ollama"
                                        />

                                        <div className="flex flex-col gap-2">
                                            <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">Provider Kind</label>
                                            <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] flex gap-2">
                                                {['OpenAiCompatible', 'Gemini'].map(kind => {
                                                    const isSelected = newProvider.kind === kind;
                                                    return (
                                                        <button
                                                            key={kind}
                                                            onClick={() => setNewProvider({...newProvider, kind: kind as any})}
                                                            className={cn(
                                                                "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                                isSelected ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                            )}
                                                        >
                                                            {isSelected && (
                                                                <>
                                                                    <WebGLGrain colors={THEMES.default} />
                                                                    <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03]" />
                                                                    <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03]" />
                                                                </>
                                                            )}
                                                            <span className="relative z-10">{kind === 'OpenAiCompatible' ? 'OpenAI compatible' : 'Gemini'}</span>
                                                        </button>
                                                    );
                                                })}
                                            </div>
                                        </div>

                                        <EngravedInput 
                                            label="Endpoint URL"
                                            value={newProvider.endpoint}
                                            onChange={(e: any) => setNewProvider({...newProvider, endpoint: e.target.value})}
                                            placeholder="http://localhost:11434/v1"
                                            className="font-mono"
                                        />

                                        <EngravedInput 
                                            label="Model Name"
                                            value={newProvider.model_name}
                                            onChange={(e: any) => setNewProvider({...newProvider, model_name: e.target.value})}
                                            placeholder="llama3, gemini-1.5-flash..."
                                            className="font-mono"
                                        />

                                        <EngravedInput 
                                            label="API Key"
                                            type="password"
                                            value={newProvider.apiKey}
                                            onChange={(e: any) => setNewProvider({...newProvider, apiKey: e.target.value})}
                                            placeholder={newProvider.id ? "•••••••• (Leave blank to keep existing)" : "••••••••"}
                                        />

                                        {/* Locale Settings */}
                                        <div className="flex flex-col gap-2">
                                            <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">Locale</label>
                                            <select 
                                                value={settings?.locale || navigator.language || 'en-US'}
                                                onChange={async (e) => {
                                                    if (!settings) return;
                                                    const updated = { ...settings, locale: e.target.value };
                                                    await invoke("save_settings", { settings: updated });
                                                    setSettings(updated);
                                                    // Reload locale in App.tsx
                                                    await new Promise(resolve => setTimeout(resolve, 100));
                                                    window.location.reload();
                                                }}
                                                className="bg-[#141414] p-4 shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] text-[14px] text-[#D1D1D1] outline-none"
                                            >
                                                <option value="en-US">English (US)</option>
                                                <option value="en-GB">English (UK)</option>
                                                <option value="fr-FR">Français (France)</option>
                                                <option value="de-DE">Deutsch (Deutschland)</option>
                                                <option value="es-ES">Español (España)</option>
                                                <option value="it-IT">Italiano (Italia)</option>
                                                <option value="ja-JP">日本語 (日本)</option>
                                                <option value="zh-CN">中文 (中国)</option>
                                                <option value="auto">Auto-detect</option>
                                            </select>
                                        </div>

                                        <div className="flex flex-col gap-2">
                                            <label className="text-[9px] font-bold uppercase tracking-widest text-[#777] px-1">Time Format</label>
                                            <div className="bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem] flex gap-2">
                                                <button
                                                    onClick={async () => {
                                                        if (!settings) return;
                                                        const updated = { ...settings, hour12: true };
                                                        await invoke("save_settings", { settings: updated });
                                                        setSettings(updated);
                                                        // Reload locale in App.tsx
                                                        await new Promise(resolve => setTimeout(resolve, 100));
                                                        window.location.reload();
                                                    }}
                                                    className={cn(
                                                        "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                        settings?.hour12 !== false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                    )}
                                                >
                                                    {settings?.hour12 !== false && (
                                                        <>
                                                            <WebGLGrain colors={THEMES.default} />
                                                            <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03]" />
                                                            <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03]" />
                                                        </>
                                                    )}
                                                    <span className="relative z-10">12-hour (AM/PM)</span>
                                                </button>
                                                <button
                                                    onClick={async () => {
                                                        if (!settings) return;
                                                        const updated = { ...settings, hour12: false };
                                                        await invoke("save_settings", { settings: updated });
                                                        setSettings(updated);
                                                        // Reload locale in App.tsx
                                                        await new Promise(resolve => setTimeout(resolve, 100));
                                                        window.location.reload();
                                                    }}
                                                    className={cn(
                                                        "relative flex-1 py-3 rounded-[13px] text-[12px] font-medium transition-all overflow-hidden",
                                                        settings?.hour12 === false ? "text-[#D1D1D1] shadow-[0_2px_5px_rgba(0,0,0,0.7)]" : "text-[#777] hover:text-[#D1D1D1] bg-transparent"
                                                    )}
                                                >
                                                    {settings?.hour12 === false && (
                                                        <>
                                                            <WebGLGrain colors={THEMES.default} />
                                                            <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03]" />
                                                            <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03]" />
                                                        </>
                                                    )}
                                                    <span className="relative z-10">24-hour</span>
                                                </button>
                                            </div>
                                        </div>

                                        <button 
                                            onClick={handleUpsertProvider}
                                            className="mt-6 bg-[#D1D1D1] text-[#080808] font-bold tracking-widest uppercase text-[11px] py-4 rounded-[1.25rem] hover:scale-[1.02] active:scale-[0.98] transition-all shadow-[0_4px_10px_rgba(0,0,0,0.5)]"
                                        >
                                            Save Provider
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

import React from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SetupWizardProps {
  onComplete: () => void;
}

export const SetupWizard: React.FC<SetupWizardProps> = ({ onComplete }) => {
  const handleSelectMode = async (mode: string) => {
    // Note: in a real impl, we'd handle custom URL inputs etc.
    await invoke('complete_onboarding', { mode });
    onComplete();
  };

  return (
    <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-[1000] p-6 backdrop-blur-sm">
      <div className="bg-[#1a1a1a] border border-white/10 rounded-2xl p-8 max-w-2xl w-full shadow-2xl">
        <h1 className="text-3xl font-bold mb-2">Welcome to HStack</h1>
        <p className="text-white/60 mb-8 text-lg">Choose how you'd like to manage your time and tasks.</p>
        
        <div className="grid gap-4 md:grid-cols-3">
          <button 
            onClick={() => handleSelectMode('LocalOnly')}
            className="flex flex-col items-start p-6 rounded-xl border border-white/10 bg-white/5 hover:bg-white/10 transition-all text-left group"
          >
            <span className="text-2xl mb-3 group-hover:scale-110 transition-transform">🔒</span>
            <h3 className="font-bold mb-1">Local Only</h3>
            <p className="text-xs text-white/40">Purely on-device. No accounts, no internet required. Maximum privacy.</p>
          </button>

          <button 
            onClick={() => handleSelectMode('CloudOfficial')}
            className="flex flex-col items-start p-6 rounded-xl border border-white/10 bg-blue-500/10 hover:bg-blue-500/20 transition-all text-left group"
          >
            <span className="text-2xl mb-3 group-hover:scale-110 transition-transform">☁️</span>
            <h3 className="font-bold mb-1 text-blue-400">Official Cloud</h3>
            <p className="text-xs text-white/40">Sync across devices, AI directions enrichment, and premium support.</p>
          </button>

          <button 
            onClick={() => handleSelectMode('CloudCustom')}
            className="flex flex-col items-start p-6 rounded-xl border border-white/10 bg-purple-500/5 hover:bg-purple-500/10 transition-all text-left group"
          >
            <span className="text-2xl mb-3 group-hover:scale-110 transition-transform">🛠️</span>
            <h3 className="font-bold mb-1">Self-Hosted</h3>
            <p className="text-xs text-white/40">Connect to your own HStack Lite server instance.</p>
          </button>
        </div>

        <p className="mt-8 text-[10px] text-white/20 text-center uppercase tracking-widest">
          You can change this anytime in settings.
        </p>
      </div>
    </div>
  );
};

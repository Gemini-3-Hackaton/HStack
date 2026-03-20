import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Cloud, HardDrive, Server } from 'lucide-react';
import { WebGLGrain } from './WebGLGrain';

interface SetupWizardProps {
  onComplete: () => void;
}

export const SetupWizard: React.FC<SetupWizardProps> = ({ onComplete }) => {
  const [savingMode, setSavingMode] = useState<string | null>(null);

  const options = [
    {
      mode: 'LocalOnly',
      title: 'Local Only',
      description: 'Everything stays on this device. No account or internet required.',
      hint: 'Private, simple, and fully offline.',
      icon: HardDrive,
    },
    {
      mode: 'CloudOfficial',
      title: 'Official Cloud',
      description: 'Use the managed HStack cloud to keep your stack available everywhere.',
      hint: 'Best if you want multi-device sync right away.',
      icon: Cloud,
    },
    {
      mode: 'CloudCustom',
      title: 'Self-Hosted',
      description: 'Point HStack to your own HStack Lite server when you are ready.',
      hint: 'Great for teams or custom infrastructure.',
      icon: Server,
    },
  ] as const;

  const handleSelectMode = async (mode: string) => {
    if (savingMode) return;

    try {
      setSavingMode(mode);
      await invoke('complete_onboarding', { mode });
      onComplete();
    } catch (error) {
      console.error('Failed to save hosting mode:', error);
      setSavingMode(null);
    }
  };

  return (
    <div className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/80 p-6 backdrop-blur-md">
      <div className="relative w-full max-w-2xl overflow-hidden rounded-[2rem] border border-white/10 bg-[#111111] shadow-[0_35px_80px_rgba(0,0,0,0.7)]">
        <WebGLGrain
          colors={{ c1: [24, 24, 24], c2: [18, 18, 18], c3: [12, 12, 12], c4: [8, 8, 8] }}
          spreadX={0.4}
          spreadY={1.1}
          contrast={1.9}
          noiseFactor={0.55}
          opacity={0.95}
        />

        <div className="relative z-20 p-8 md:p-10">
          <div className="mb-8">
            <span className="inline-flex items-center rounded-full border border-white/10 bg-white/5 px-3 py-1 text-[10px] font-bold uppercase tracking-[0.24em] text-white/45">
              Hosting
            </span>
            <h1 className="mt-5 text-3xl font-semibold tracking-[-0.03em] text-[#F5F5F5] md:text-4xl">How should HStack host your data?</h1>
            <p className="mt-3 max-w-xl text-[15px] leading-relaxed text-white/55">
              Pick the mode that matches your setup today. You can switch it later from settings without leaving the app.
            </p>
          </div>

          <div className="flex flex-col gap-4">
            {options.map((option) => {
              const Icon = option.icon;
              const isSaving = savingMode === option.mode;

              return (
                <button
                  key={option.mode}
                  type="button"
                  disabled={Boolean(savingMode)}
                  onClick={() => handleSelectMode(option.mode)}
                  className="rounded-[1.5rem] bg-[#161616] p-[4px] text-left shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] transition-transform duration-200 hover:scale-[1.01] disabled:cursor-wait disabled:opacity-70"
                >
                  <div className="relative overflow-hidden rounded-[1.3rem] border border-white/6 bg-[#121212] p-5 shadow-[0_10px_30px_rgba(0,0,0,0.35)]">
                    <WebGLGrain colors={{ c1: [28, 28, 28], c2: [20, 20, 20], c3: [14, 14, 14], c4: [10, 10, 10] }} opacity={0.85} />
                    <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
                    <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />

                    <div className="relative z-20 flex items-start gap-4">
                      <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-white/8 bg-white/6 text-[#D1D1D1]">
                        <Icon size={20} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-3">
                          <h3 className="text-[17px] font-medium text-[#EDEDED]">{option.title}</h3>
                          <span className="text-[10px] font-bold uppercase tracking-[0.22em] text-white/28">
                            {isSaving ? 'Saving' : 'Select'}
                          </span>
                        </div>
                        <p className="mt-1 text-[13px] leading-relaxed text-white/55">{option.description}</p>
                        <p className="mt-3 text-[10px] uppercase tracking-[0.18em] text-white/28">{option.hint}</p>
                      </div>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>

          <p className="mt-8 text-center text-[10px] uppercase tracking-[0.24em] text-white/22">
            You can change this anytime in settings.
          </p>
        </div>
      </div>
    </div>
  );
};

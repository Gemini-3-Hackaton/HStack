import React, { createContext, useContext, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import enGb from './locales/en-GB.json';
import frFr from './locales/fr-FR.json';
import esEs from './locales/es-ES.json';
import zhCn from './locales/zh-CN.json';

export type SupportedLocale = 'en-GB' | 'fr-FR' | 'es-ES' | 'zh-CN';

type LocaleConfig = {
  locale: SupportedLocale;
  hour12: boolean;
};

type TranslationVars = Record<string, string | number>;

const SUPPORTED_LOCALE_SET = new Set<SupportedLocale>(['en-GB', 'fr-FR', 'es-ES', 'zh-CN']);

export const SUPPORTED_LOCALE_OPTIONS: Array<{ value: SupportedLocale; label: string }> = [
  { value: 'en-GB', label: 'English (UK)' },
  { value: 'fr-FR', label: 'Français' },
  { value: 'es-ES', label: 'Español' },
  { value: 'zh-CN', label: '中文' },
];

const normalizeSupportedLocale = (locale?: string | null): SupportedLocale => {
  if (!locale) return 'en-GB';

  const normalized = locale.trim();
  if (SUPPORTED_LOCALE_SET.has(normalized as SupportedLocale)) {
    return normalized as SupportedLocale;
  }

  const lower = normalized.toLowerCase();
  if (lower.startsWith('fr')) return 'fr-FR';
  if (lower.startsWith('es')) return 'es-ES';
  if (lower.startsWith('zh')) return 'zh-CN';
  return 'en-GB';
};

let cachedLocaleConfig: LocaleConfig = {
  locale: normalizeSupportedLocale(typeof navigator !== 'undefined' ? navigator.language : 'en-GB'),
  hour12: true,
};

const translations = {
  'en-GB': enGb,
  'fr-FR': frFr,
  'es-ES': esEs,
  'zh-CN': zhCn,
} as const;

export type TranslationKey = keyof typeof enGb;

const interpolate = (template: string, vars?: TranslationVars) => {
  if (!vars) return template;
  return template.replace(/\{\{(.*?)\}\}/g, (_, key) => String(vars[key.trim()] ?? ''));
};

export const getLocaleConfig = (): LocaleConfig => cachedLocaleConfig;

export const getSupportedLocale = normalizeSupportedLocale;

export const loadUserLocale = async (): Promise<LocaleConfig> => {
  try {
    const [locale, hour12] = await invoke<[string, boolean]>('get_user_locale');
    cachedLocaleConfig = {
      locale: normalizeSupportedLocale(locale),
      hour12,
    };
  } catch (error) {
    cachedLocaleConfig = {
      locale: normalizeSupportedLocale(typeof navigator !== 'undefined' ? navigator.language : 'en-GB'),
      hour12: true,
    };
    console.warn('Failed to load user locale, using browser default:', error);
  }

  return cachedLocaleConfig;
};

export const translate = (key: TranslationKey, vars?: TranslationVars): string => {
  const language = normalizeSupportedLocale(cachedLocaleConfig.locale);
  const template = translations[language][key] ?? translations['en-GB'][key] ?? key;
  return interpolate(template, vars);
};

type I18nContextValue = {
  locale: SupportedLocale;
  hour12: boolean;
  t: (key: TranslationKey, vars?: TranslationVars) => string;
  refreshLocale: () => Promise<void>;
};

const I18nContext = createContext<I18nContextValue | null>(null);

export const I18nProvider = ({ children }: { children: React.ReactNode }) => {
  const [localeConfig, setLocaleConfig] = useState<LocaleConfig>(getLocaleConfig());

  useEffect(() => {
    const refreshLocale = async () => {
      const next = await loadUserLocale();
      setLocaleConfig(next);
    };

    const handleLocaleUpdated = () => {
      void refreshLocale();
    };

    void refreshLocale();
    window.addEventListener('localeUpdated', handleLocaleUpdated);

    return () => {
      window.removeEventListener('localeUpdated', handleLocaleUpdated);
    };
  }, []);

  const value = useMemo<I18nContextValue>(() => ({
    locale: localeConfig.locale,
    hour12: localeConfig.hour12,
    t: translate,
    refreshLocale: async () => {
      const next = await loadUserLocale();
      setLocaleConfig(next);
    },
  }), [localeConfig]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
};

export const useI18n = (): I18nContextValue => {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error('useI18n must be used within an I18nProvider');
  }
  return context;
};
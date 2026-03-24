export type SyncMode = 'LocalOnly' | 'CloudOfficial' | 'CloudCustom';

export interface UserSettingsShape {
  sync_mode: SyncMode;
  custom_server_url: string | null;
  sync_user_id?: number | null;
  sync_user_name?: string | null;
}

export interface SyncSessionInfo {
  user_id: number | null;
  user_name: string | null;
  token: string | null;
}

export interface RemoteSyncConfig {
  baseUrl: string;
  token: string;
  userId: number;
  userName: string | null;
}

export const SYNC_CONFIG_UPDATED_EVENT = 'hstack:sync-config-updated';

const DEFAULT_OFFICIAL_CLOUD_URL = 'https://hstack-private-api.onrender.com';

const OFFICIAL_CLOUD_URL = (
  import.meta.env.VITE_OFFICIAL_CLOUD_URL || DEFAULT_OFFICIAL_CLOUD_URL
).trim();

export const normalizeSyncBaseUrl = (value: string | null | undefined) => {
  const trimmed = value?.trim();
  if (!trimmed) return null;
  return trimmed.replace(/\/+$/, '');
};

export const getOfficialCloudUrl = () => normalizeSyncBaseUrl(OFFICIAL_CLOUD_URL);

export const isOfficialCloudConfigured = () => Boolean(getOfficialCloudUrl());

export const notifySyncConfigUpdated = () => {
  window.dispatchEvent(new CustomEvent(SYNC_CONFIG_UPDATED_EVENT));
};

export const resolveRemoteBaseUrl = (settings: UserSettingsShape) => {
  if (settings.sync_mode === 'CloudOfficial') {
    return getOfficialCloudUrl();
  }

  if (settings.sync_mode === 'CloudCustom') {
    return normalizeSyncBaseUrl(settings.custom_server_url);
  }

  return null;
};

export const buildApiUrl = (baseUrl: string, path: string) => {
  const url = new URL(baseUrl);
  url.pathname = path;
  url.search = '';
  url.hash = '';
  return url.toString();
};

export const resolveRemoteSyncConfig = (
  settings: UserSettingsShape,
  session: SyncSessionInfo
): RemoteSyncConfig | null => {
  const baseUrl = resolveRemoteBaseUrl(settings);

  if (!baseUrl || !session.user_id || !session.token) {
    return null;
  }

  return {
    baseUrl,
    token: session.token,
    userId: session.user_id,
    userName: session.user_name,
  };
};
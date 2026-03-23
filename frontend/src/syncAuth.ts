import { invoke } from '@tauri-apps/api/core';
import { buildApiUrl, getOfficialCloudUrl, normalizeSyncBaseUrl, type SyncMode } from './syncConfig';

export type RemoteAuthMode = 'login' | 'register';

interface RemoteUser {
  id: number;
  first_name: string;
  last_name: string;
  email?: string | null;
}

interface RemoteAuthResponse {
  token: string;
  user: RemoteUser;
}

interface RemoteAuthRequest {
  baseUrl: string;
  mode: RemoteAuthMode;
  loginEmail: string;
  firstName: string;
  lastName?: string;
  email: string;
  password: string;
}

export const resolveAuthBaseUrl = (syncMode: SyncMode, customServerUrl?: string | null) => {
  if (syncMode === 'CloudOfficial') {
    return getOfficialCloudUrl();
  }

  if (syncMode === 'CloudCustom') {
    return normalizeSyncBaseUrl(customServerUrl);
  }

  return null;
};

export const formatRemoteUserName = (user: RemoteUser) => {
  const fullName = [user.first_name, user.last_name].filter(Boolean).join(' ').trim();
  return fullName || user.first_name || '';
};

const readErrorMessage = async (response: Response) => {
  try {
    const payload = await response.json();

    if (typeof payload === 'string') {
      return payload;
    }

    if (payload?.message && typeof payload.message === 'string') {
      return payload.message;
    }

    if (payload?.error && typeof payload.error === 'string') {
      return payload.error;
    }
  } catch {
    const message = await response.text().catch(() => '');
    if (message.trim()) {
      return message;
    }
  }

  return `Request failed with status ${response.status}`;
};

export const authenticateRemote = async ({
  baseUrl,
  mode,
  loginEmail,
  firstName,
  lastName,
  email,
  password,
}: RemoteAuthRequest): Promise<RemoteAuthResponse> => {
  const endpoint = mode === 'register' ? '/api/auth/register' : '/api/auth/login';
  const payload =
    mode === 'register'
      ? {
          first_name: firstName.trim(),
          last_name: lastName?.trim() || null,
          email: email.trim(),
          password,
        }
      : {
          email: loginEmail.trim(),
          password,
        };

  const response = await fetch(buildApiUrl(baseUrl, endpoint), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    throw new Error(await readErrorMessage(response));
  }

  return response.json();
};

export const saveRemoteSession = async (session: RemoteAuthResponse) => {
  await invoke('save_sync_session', {
    userId: session.user.id,
    userName: formatRemoteUserName(session.user),
    token: session.token,
  });
};

export const clearRemoteSession = async () => {
  await invoke('clear_sync_session');
};
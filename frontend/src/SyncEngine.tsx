import { useState, useEffect, useCallback, createContext, ReactNode, useRef } from 'react';
import { v4 as uuidv4 } from 'uuid';
import { invoke } from '@tauri-apps/api/core';
import {
  SYNC_CONFIG_UPDATED_EVENT,
  buildApiUrl,
  resolveRemoteSyncConfig,
  type SyncMode,
  type SyncSessionInfo,
  type UserSettingsShape,
} from './syncConfig';

export type TicketType = 'HABIT' | 'EVENT' | 'TASK' | 'COMMUTE' | 'COUNTDOWN';
export type TicketStatus = 'idle' | 'in_focus' | 'completed' | 'expired';

export interface TaskModel {
  id: string;
  title: string;
  type: TicketType;
  status: TicketStatus;
  payload: any;
  notes?: string;
  created_at?: string;
  updated_at?: string;
}

export type SyncActionType = 'CREATE' | 'UPDATE' | 'DELETE';

export interface SyncAction {
  action_id: string;
  type: SyncActionType;
  entity_id: string;
  entity_type: string;
  status?: TicketStatus;
  payload?: any;
  timestamp: string;
}

interface SyncContextType {
  tasks: TaskModel[];
  isConnected: boolean;
  createTask: (type: TicketType, payload: any, status?: TicketStatus) => Promise<string>;
  updateTask: (id: string, payload: any) => Promise<void>;
  updateTaskStatus: (id: string, status: TicketStatus) => Promise<void>;
  deleteTask: (id: string) => Promise<void>;
  syncNow: () => Promise<void>;
}

interface SyncSettings extends UserSettingsShape {
  sync_mode: SyncMode;
  custom_server_url: string | null;
}

export const SyncContext = createContext<SyncContextType | undefined>(undefined);

// Hash function replica (using Web Crypto API)
async function sha256(message: string): Promise<string> {
  const msgBuffer = new TextEncoder().encode(message);                    
  const hashBuffer = await crypto.subtle.digest('SHA-256', msgBuffer);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
}

async function calculateClientStateHash(tasks: TaskModel[]): Promise<string> {
  // Same sequence as backend
  const stateList = tasks.map(t => ({
      id: t.id,
      type: t.type,
      payload: t.payload,
      status: t.status
  }));
  stateList.sort((a, b) => {
      if (a.id < b.id) return -1;
      if (a.id > b.id) return 1;
      return 0;
  });
  const stateStr = JSON.stringify(stateList);
  return await sha256(stateStr);
}


export const SyncProvider = ({ children, userId = 1 }: { children: ReactNode, userId?: number }) => {
  const [tasks, setTasks] = useState<TaskModel[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [localHistory, setLocalHistory] = useState<SyncAction[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const tasksRef = useRef<TaskModel[]>([]);
  const historyRef = useRef<SyncAction[]>([]);
  const [syncSettings, setSyncSettings] = useState<SyncSettings | null>(null);
  const [syncSession, setSyncSession] = useState<SyncSessionInfo | null>(null);

  const loadSyncConfig = useCallback(async () => {
    try {
      const [settings, session] = await Promise.all([
        invoke<SyncSettings>('get_settings'),
        invoke<SyncSessionInfo>('get_sync_session'),
      ]);
      setSyncSettings(settings);
      setSyncSession(session);
    } catch (error) {
      console.error('Failed to load sync configuration:', error);
    }
  }, []);

  useEffect(() => { tasksRef.current = tasks; }, [tasks]);
  useEffect(() => { historyRef.current = localHistory; }, [localHistory]);

  useEffect(() => {
    const handleSyncConfigUpdated = () => {
      void loadSyncConfig();
    };

    void loadSyncConfig();
    window.addEventListener(SYNC_CONFIG_UPDATED_EVENT, handleSyncConfigUpdated);

    return () => {
      window.removeEventListener(SYNC_CONFIG_UPDATED_EVENT, handleSyncConfigUpdated);
    };
  }, [loadSyncConfig]);

  // Load from local storage initially
  useEffect(() => {
    const cachedTasks = localStorage.getItem(`hstack_tasks_${userId}`);
    if (cachedTasks) setTasks(JSON.parse(cachedTasks));

    const cachedHistory = localStorage.getItem(`hstack_history_${userId}`);
    if (cachedHistory) setLocalHistory(JSON.parse(cachedHistory));
  }, [userId]);

  // Persist State Helper
  const persistState = useCallback((newTasks: TaskModel[], newHistory: SyncAction[]) => {
    setTasks(newTasks);
    setLocalHistory(newHistory);
    localStorage.setItem(`hstack_tasks_${userId}`, JSON.stringify(newTasks));
    localStorage.setItem(`hstack_history_${userId}`, JSON.stringify(newHistory));
  }, [userId]);


  // Initialize WebSocket sync sequence
  useEffect(() => {
    if (!syncSettings || !syncSession) return;

    let reconnectTimer: any;
    const remoteConfig = resolveRemoteSyncConfig(syncSettings, syncSession);
    const websocketUrl = remoteConfig?.wsUrl;

    if (!websocketUrl) {
      setIsConnected(false);
      return;
    }

    const connect = async () => {
      const ws = new WebSocket(websocketUrl);
      wsRef.current = ws;

      ws.onopen = async () => {
        setIsConnected(true);
        // Step 1: Handshake
        const hash = await calculateClientStateHash(tasksRef.current);
        ws.send(JSON.stringify({ type: 'HELLO', client_hash: hash }));
      };

      ws.onmessage = async (event) => {
        try {
          const data = JSON.parse(event.data);
          
          if (data.type === 'ACK') {
            console.log("In Sync with Server");
            if (historyRef.current.length > 0) flushHistory();
          } 
          else if (data.type === 'OUT_OF_SYNC') {
            console.log("Out of Sync, fetching full state...");
            await fetchFullState();
            if (historyRef.current.length > 0) flushHistory();
          }
          else if (data.type === 'SYNC_ACK') {
             // Remove actions from local history that were acknowledged
             const ackIds = data.ack_action_ids || [];
             setLocalHistory(prev => {
                const updated = prev.filter(a => !ackIds.includes(a.action_id));
                localStorage.setItem(`hstack_history_${userId}`, JSON.stringify(updated));
                return updated;
             });
          }
          else if (data.type === 'STATE_UPDATED') {
              console.log("Remote State Updated, refreshing...");
              await fetchFullState();
          }

        } catch (err) {
            console.error(err);
        }
      };

      ws.onclose = () => {
        setIsConnected(false);
        reconnectTimer = setTimeout(connect, 3000);
      };
      
      ws.onerror = (e) => {
         console.error('WebSocket Error:', e);
         ws.close();
      };
    };

    connect();
    return () => {
      clearTimeout(reconnectTimer);
      if (wsRef.current) wsRef.current.close();
    };
  }, [syncSettings, syncSession, userId]);


  const flushHistory = () => {
      if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN && historyRef.current.length > 0) {
          wsRef.current.send(JSON.stringify({
              type: 'SYNC_ACTIONS',
          actions: historyRef.current
          }));
      }
  };

  const fetchFullState = async () => {
      const remoteConfig = syncSettings && syncSession ? resolveRemoteSyncConfig(syncSettings, syncSession) : null;

      try {
        if (!remoteConfig) {
          const fullTasks = await invoke<TaskModel[]>("get_tasks");
          persistState(fullTasks, localHistory);
          console.log("State refreshed from Rust backend");
          return;
        }

        const res = await fetch(buildApiUrl(remoteConfig.baseUrl, '/api/tasks'), {
          headers: {
            Authorization: `Bearer ${remoteConfig.token}`,
          },
        });

        if (!res.ok) {
          throw new Error(`Remote task fetch failed: ${res.status}`);
        }

        const fullTasks = await res.json();
        persistState(fullTasks, localHistory);
        console.log('State refreshed from remote server');
      } catch (err) {
        console.error("Remote task fetch failed", err);
      }
  };


  // Action Dispatchers
  const pushAction = async (action: Omit<SyncAction, 'action_id' | 'timestamp'>) => {
    const fullAction: SyncAction = {
        ...action,
        action_id: uuidv4(),
        timestamp: new Date().toISOString()
    };
    
    // Optimistic Update
    let newTasks = tasks.map(t => {
        if (t.id === action.entity_id) {
            // Clone the task and update only provided fields
            const updatedTask = { ...t };
            if (action.payload !== undefined) updatedTask.payload = action.payload;
            if (action.status !== undefined) updatedTask.status = action.status;
            return updatedTask as TaskModel;
        }
        return t;
    });

    if (action.type === 'CREATE') {
        newTasks.push({ 
            id: action.entity_id, 
        title: action.payload?.title || '',
            type: action.entity_type as TicketType, 
            payload: action.payload,
            status: action.status || 'idle'
        });
    } else if (action.type === 'DELETE') {
        newTasks = newTasks.filter(t => t.id !== action.entity_id);
    }
    
    const newHistory = [...localHistory, fullAction];
    persistState(newTasks, newHistory);

    // Send immediately if open
    if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({
           type: 'SYNC_ACTIONS',
           actions: [fullAction]
        }));
    }
  };

  const createTask = async (type: TicketType, payload: any, status: TicketStatus = 'idle') => {
      const entity_id = uuidv4();
      await pushAction({ type: 'CREATE', entity_id, entity_type: type, payload, status });
      return entity_id;
  };

  const updateTask = async (id: string, payload: any) => {
      await pushAction({ type: 'UPDATE', entity_id: id, entity_type: 'TASK', payload });
  };

  const updateTaskStatus = async (id: string, status: TicketStatus) => {
      await pushAction({ type: 'UPDATE', entity_id: id, entity_type: 'TASK', status });
  };

  const deleteTask = async (id: string) => {
      await pushAction({ type: 'DELETE', entity_id: id, entity_type: 'TASK' });
  };


  return (
    <SyncContext.Provider value={{ tasks, isConnected, createTask, updateTask, updateTaskStatus, deleteTask, syncNow: fetchFullState }}>
      {children}
    </SyncContext.Provider>
  );
};


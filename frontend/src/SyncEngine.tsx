import { useState, useEffect, useCallback, createContext, ReactNode, useContext, useRef } from 'react';
import { v4 as uuidv4 } from 'uuid';

export type TicketType = 'HABIT' | 'EVENT' | 'TASK' | 'COMMUTE' | 'AGENT_TASK' | 'COUNTDOWN';

export interface TaskModel {
  id: string;
  type: TicketType;
  payload: any;
  created_at?: string;
  updated_at?: string;
}

export type SyncActionType = 'CREATE' | 'UPDATE' | 'DELETE';

export interface SyncAction {
  action_id: string;
  type: SyncActionType;
  entity_id: string;
  entity_type: string;
  payload?: any;
  timestamp: string;
}

interface SyncContextType {
  tasks: TaskModel[];
  isConnected: boolean;
  createTask: (type: TicketType, payload: any) => Promise<string>;
  updateTask: (id: string, payload: any) => Promise<void>;
  deleteTask: (id: string) => Promise<void>;
  syncNow: () => Promise<void>;
}

const SyncContext = createContext<SyncContextType | undefined>(undefined);

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
      payload: t.payload
  }));
  stateList.sort((a, b) => {
      // Very rough approximate sort matching pg (if needed exact, usually id will suffice)
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

  useEffect(() => { tasksRef.current = tasks; }, [tasks]);
  useEffect(() => { historyRef.current = localHistory; }, [localHistory]);

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
    let reconnectTimer: any;
    const connect = async () => {
      const ws = new WebSocket(`ws://127.0.0.1:8000/ws/sync/${userId}`);
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
            if (localHistory.length > 0) flushHistory();
          } 
          else if (data.type === 'OUT_OF_SYNC') {
            console.log("Out of Sync, fetching full state...");
            await fetchFullState();
            if (localHistory.length > 0) flushHistory();
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
  }, [userId]);


  const flushHistory = () => {
      if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN && localHistory.length > 0) {
          wsRef.current.send(JSON.stringify({
              type: 'SYNC_ACTIONS',
              actions: localHistory
          }));
      }
  };

  const fetchFullState = async () => {
      try {
          const res = await fetch(`http://localhost:8000/api/tasks?userid=${userId}`);
          if (res.ok) {
              const fullTasks = await res.json();
              persistState(fullTasks, localHistory);
          }
      } catch (err) {
          console.error("Fetch full state failed", err);
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
    let newTasks = [...tasks];
    if (action.type === 'CREATE') {
        newTasks.push({ id: action.entity_id, type: action.entity_type as TicketType, payload: action.payload });
    } else if (action.type === 'UPDATE') {
        const idx = newTasks.findIndex(t => t.id === action.entity_id);
        if (idx > -1) newTasks[idx].payload = action.payload;
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

  const createTask = async (type: TicketType, payload: any) => {
      const entity_id = uuidv4();
      await pushAction({ type: 'CREATE', entity_id, entity_type: type, payload });
      return entity_id;
  };

  const updateTask = async (id: string, payload: any) => {
      await pushAction({ type: 'UPDATE', entity_id: id, entity_type: 'TASK', payload });
  };

  const deleteTask = async (id: string) => {
      await pushAction({ type: 'DELETE', entity_id: id, entity_type: 'TASK' });
  };


  return (
    <SyncContext.Provider value={{ tasks, isConnected, createTask, updateTask, deleteTask, syncNow: fetchFullState }}>
      {children}
    </SyncContext.Provider>
  );
};

export const useSync = () => {
  const ctx = useContext(SyncContext);
  if (!ctx) throw new Error("useSync must be used within SyncProvider");
  return ctx;
};

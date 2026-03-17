import { useState, useEffect, useRef } from "react";
import { SyncProvider, useSync, TaskModel } from "./SyncEngine";
import { Send, X, ChevronDown, Plus, ChevronRight, Wifi, WifiOff } from "lucide-react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./components/WebGLGrain";

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// --- Specialized Card Components ---

const formatDate = (dateString?: string) => {
    if (!dateString) return "";
    const date = new Date(dateString);
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
};

// --- Engraved Dark Themes ---

const THEMES = {
  habit: {
    c1: [42, 52, 48],
    c2: [32, 38, 35], 
    c3: [24, 26, 25], 
    c4: [20, 20, 20]
  },
  event: {
    c1: [54, 48, 40],
    c2: [36, 34, 31], 
    c3: [25, 24, 23], 
    c4: [20, 20, 20]
  },
  default: {
    c1: [48, 48, 48], 
    c2: [34, 34, 34], 
    c3: [24, 24, 24], 
    c4: [20, 20, 20]
  }
};

type ThemeColors = typeof THEMES.default;

// --- Physical Wrapper (Moat Architecture) ---

const PhysicalWrapper = ({ children, outerClass = '', innerClass = '', checked = false, shaderColors = THEMES.default }: {
  children: React.ReactNode;
  outerClass?: string;
  innerClass?: string;
  checked?: boolean;
  shaderColors?: ThemeColors;
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
      {/* Zero-Blur Creases */}
      <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
      <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />
      <div className="relative z-20 w-full h-full">
        {children}
      </div>
    </div>
  </div>
);

// --- Tag Component (Carved from material) ---

const Tag = ({ text, type, italic, cardTheme = THEMES.default }: {
  text: string;
  type: string;
  italic?: boolean;
  cardTheme?: ThemeColors;
}) => {
  const borderColor = `rgba(${cardTheme.c1[0]}, ${cardTheme.c1[1]}, ${cardTheme.c1[2]}, 0.25)`;
  const baseClasses = "text-[9px] font-bold tracking-widest px-1.5 py-1 rounded border transition-colors uppercase whitespace-nowrap";

  if (type === 'info') {
    return (
      <span 
        className={cn(baseClasses, "bg-[#252525] text-[#888]", italic && "italic")}
        style={{ borderColor }}
      >
        {text}
      </span>
    );
  }

  let colorClass = 'text-[#888] bg-[#222]';
  if (type === 'habit') colorClass = 'text-emerald-400/80 bg-emerald-950/40';
  if (type === 'event') colorClass = 'text-amber-400/80 bg-amber-950/40';
  
  return (
    <span 
      className={cn(baseClasses, colorClass)}
      style={{ borderColor }}
    >
      {text}
    </span>
  );
};

// --- Scope Components ---

interface ScopeBlockProps {
    label: string;
    type: 'day' | 'week';
    children: React.ReactNode;
}

const ScopeBlock = ({ label, type, children }: ScopeBlockProps) => {
    const isWeek = type === 'week';
    return (
        <div className="flex flex-col mb-4">
            {/* Horizontal Title for the Scope - Aligned with the bar */}
            <div className="pl-[2.5px] mb-1 flex items-center h-4">
                <span className={cn(
                    "text-[10px] font-bold uppercase tracking-[1.5px] whitespace-nowrap",
                    isWeek ? "text-[#3B82F6]" : "text-white/30"
                )}>
                    {label}
                </span>
            </div>
            
            <div className="flex gap-2">
                {/* Thin Side Bar */}
                <div className="shrink-0 pl-[4px]">
                    <div className={cn(
                        "w-[1.5px] h-full rounded-full transition-all duration-300",
                        isWeek ? "bg-[#3B82F6]" : "bg-white/10"
                    )} />
                </div>
                
                <div className="flex-1 flex flex-col gap-4">
                    {children}
                </div>
            </div>
        </div>
    );
};

const groupTasks = (tasks: TaskModel[]) => {
    const grouped: { week: string | null; days: { [key: string]: TaskModel[] }; unplanned: TaskModel[] } = {
        week: null,
        days: {},
        unplanned: []
    };

    // Helper to normalize time for sorting (e.g., "15:00" -> 1500)
    const timeToValue = (timeStr?: string) => {
        if (!timeStr) return 999999;
        // Match HH:MM or HH (AM/PM optional)
        const match = timeStr.match(/^(\d{1,2})(?::(\d{2}))?\s*(AM|PM)?/i);
        if (match) {
            let hours = parseInt(match[1]);
            const mins = match[2] ? parseInt(match[2]) : 0;
            const ampm = match[3]?.toUpperCase();
            if (ampm === 'PM' && hours < 12) hours += 12;
            if (ampm === 'AM' && hours === 12) hours = 0;
            return hours * 100 + mins;
        }
        return 999998;
    };

    // 1. Sort all tasks chronologically
    const sortedTasks = [...tasks].sort((a, b) => {
        const valA = timeToValue(a.payload?.scheduled_time);
        const valB = timeToValue(b.payload?.scheduled_time);
        return valA - valB;
    });

    sortedTasks.forEach(task => {
        const time = task.payload?.scheduled_time?.trim();
        if (!time) {
            grouped.unplanned.push(task);
            return;
        }

        const lowerTime = time.toLowerCase();
        let dayLabel = "Today";
        let weekLabel = "Weekly Intention";

        if (lowerTime.includes("tomorrow")) {
            dayLabel = "Tomorrow";
        } else if (lowerTime.includes("monday")) {
            dayLabel = "Monday";
        } else if (lowerTime.includes("tuesday")) {
            dayLabel = "Tuesday";
        } else if (lowerTime.includes("wednesday")) {
            dayLabel = "Wednesday";
        } else if (lowerTime.includes("thursday")) {
            dayLabel = "Thursday";
        } else if (lowerTime.includes("friday")) {
            dayLabel = "Friday";
        } else if (lowerTime.includes("saturday")) {
            dayLabel = "Saturday";
        } else if (lowerTime.includes("sunday")) {
            dayLabel = "Sunday";
        } else if (/^\d{4}-\d{2}-\d{2}/.test(time)) {
            const d = new Date(time);
            if (!isNaN(d.getTime())) {
                dayLabel = d.toLocaleDateString('en-US', { weekday: 'long' });
                const now = new Date();
                const diff = d.getTime() - now.getTime();
                weekLabel = diff > 7 * 24 * 60 * 60 * 1000 ? "Future Scope" : "This Week";
            }
        }

        grouped.week = weekLabel;
        if (!grouped.days[dayLabel]) grouped.days[dayLabel] = [];
        grouped.days[dayLabel].push(task);
    });

    // Re-sort day keys to ensure Today > Tomorrow > Specific Days
    const dayOrder = ["Today", "Tomorrow", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const orderedDays: { [key: string]: TaskModel[] } = {};
    
    dayOrder.forEach(label => {
        if (grouped.days[label]) {
            orderedDays[label] = grouped.days[label];
        }
    });

    // Add any remaining days (e.g. from dates)
    Object.keys(grouped.days).forEach(label => {
        if (!orderedDays[label]) {
            orderedDays[label] = grouped.days[label];
        }
    });

    grouped.days = orderedDays;
    return grouped;
};

const TicketCard = ({ task }: { task: TaskModel }) => {
    const { updateTask } = useSync();
    const payload = task.payload || {};
    const isCompleted = payload.completed === true;
    const title = payload.title || 'Untitled';
    const type = task.type || 'TASK';
    const typeLower = type.toLowerCase();
    
    const theme = typeLower === 'habit' ? THEMES.habit : typeLower === 'event' ? THEMES.event : THEMES.default;

    const handleToggle = (e: React.MouseEvent) => {
        e.stopPropagation();
        updateTask(task.id, { ...payload, completed: !isCompleted });
    };

    return (
        <div onClick={handleToggle} className="cursor-default animate-[slideUp_0.3s_ease-out]">
            <PhysicalWrapper 
                outerClass="mb-0" 
                innerClass="p-4 flex items-start gap-4" 
                checked={isCompleted} 
                shaderColors={theme}
            >
                <div className="flex-1 min-w-0">
                    <h3 className={cn(
                        "font-medium text-[16px] tracking-wide text-[#d1d1d1] truncate transition-colors duration-300",
                        isCompleted && "line-through text-[#555]"
                    )}>
                        {title}
                    </h3>
                    <div className="flex flex-wrap items-center gap-2 mt-2">
                        <Tag text={type.replace('_', ' ')} type={typeLower} cardTheme={theme} />
                        {payload.scheduled_time && <Tag text={payload.scheduled_time} type="info" cardTheme={theme} />}
                        {payload.recurrence && <Tag text={payload.recurrence} type="info" italic={true} cardTheme={theme} />}
                        {!payload.scheduled_time && task.created_at && (
                            <Tag text={formatDate(task.created_at)} type="info" cardTheme={theme} />
                        )}
                    </div>
                </div>
            </PhysicalWrapper>
        </div>
    );
};

const CommuteAlert = ({ message, type, onDismiss }: { message: string, type: string, onDismiss: () => void }) => {
    const isLiveTrip = type === 'live_trip' || type === 'live_trip_expired';
    const alertTheme = isLiveTrip ? { c1: [60, 30, 30], c2: [40, 24, 24], c3: [28, 20, 20], c4: [20, 20, 20] } : THEMES.default;

    return (
        <div className="animate-[alertSlideIn_0.4s_ease-out]">
            <PhysicalWrapper 
                outerClass="mb-0" 
                innerClass="p-4 flex flex-col gap-2 relative" 
                shaderColors={alertTheme}
            >
                <button 
                    onClick={onDismiss}
                    className="absolute top-3 right-3 text-[#777] hover:text-white transition-colors z-30"
                >
                    <X size={14} />
                </button>
                {isLiveTrip && (
                    <div className="text-[9px] font-bold tracking-widest text-red-500/80 uppercase mb-1 flex items-center gap-1.5">
                        <span className="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse" />
                        LIVE TRIP
                    </div>
                )}
                <div className="text-[14px] text-[#d1d1d1] leading-relaxed italic opacity-90">
                    {message}
                </div>
            </PhysicalWrapper>
        </div>
    );
};
// --- Main App Component ---

function App() {
    const { tasks, syncNow, isConnected } = useSync();
    const [inputValue, setInputValue] = useState("");
    const [isProcessing, setIsProcessing] = useState(false);
    const [placeholder, setPlaceholder] = useState("Tell AI to manage your stack...");
    const [alerts, setAlerts] = useState<any[]>([]);
    const [feedback, setFeedback] = useState<string | null>(null);
    const [integrations, setIntegrations] = useState<string[]>([]); // No integrations by default

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const tauriWindow = useRef(getCurrentWindow());

    const minimizeWindow = (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
        console.log("Minimizing window...");
        tauriWindow.current.minimize().catch(err => {
            console.error("Failed to minimize window:", err);
        });
    };

    // Initial load
    useEffect(() => {
        syncNow();
    }, []);

    // Auto-resize textarea
    useEffect(() => {
        if (inputRef.current) {
            inputRef.current.style.height = 'auto';
            inputRef.current.style.height = (inputRef.current.scrollHeight) + 'px';
            if (inputValue === '') {
                inputRef.current.style.height = '48px';
            }
        }
    }, [inputValue]);

    const handleAction = async (e?: React.FormEvent) => {
        if (e) e.preventDefault();
        const message = inputValue.trim();
        if (!message || isProcessing) return;

        setInputValue("");
        setIsProcessing(true);
        setPlaceholder("Processing your action...");

        try {
            const resp = await fetch('http://localhost:8000/api/chat', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ message: message, userid: 1 }) // Hardcoded user 1 for now
            });
            const data = await resp.json();

            if (resp.ok) {
                await syncNow();
                if (data.action === 'clear') {
                    setAlerts([]);
                }

                const isDirections = data.action === 'get_directions' || data.action === 'start_live_directions';
                if (data.response && isDirections) {
                    setAlerts(prev => [{
                        message: data.response,
                        type: data.action === 'start_live_directions' ? 'live_trip' : 'directions',
                        id: Date.now()
                    }, ...prev]);
                } else if (data.response) {
                    showFeedback(data.response);
                } else if (data.action && data.action !== 'message') {
                    setPlaceholder("Action completed.");
                    setTimeout(() => setPlaceholder("Tell AI to manage your stack..."), 3000);
                }
            } else {
                setPlaceholder("Error communicating with AI.");
                setTimeout(() => setPlaceholder("Tell AI to manage your stack..."), 3000);
            }
        } catch (err) {
            setPlaceholder("Network error reaching the server.");
            setTimeout(() => setPlaceholder("Tell AI to manage your stack..."), 3000);
        } finally {
            setIsProcessing(false);
            if (inputRef.current) inputRef.current.focus();
        }
    };

    const showFeedback = (msg: string) => {
        setFeedback(msg);
        setTimeout(() => setFeedback(null), 5000);
    };

    return (
        <main 
            className="app-container w-screen h-screen flex flex-col relative bg-[#080808] rounded-[24px] overflow-hidden border border-white/15 shadow-2xl transition-all duration-300 ease-out"
        >
            {/* Full-page dithered background */}
            <WebGLGrain 
                colors={{
                    c1: [16, 16, 16],
                    c2: [12, 12, 12], 
                    c3: [9, 9, 9], 
                    c4: [6, 6, 6] 
                }}
                spreadX={0.35}
                spreadY={1.1}
                contrast={2.0}
                noiseFactor={0.7}
                opacity={1.0}
            />
            {/* Header Area (Draggable) */}
            <header 
                onPointerDown={(e) => {
                    if (e.button === 0) {
                        tauriWindow.current.startDragging().catch(console.error);
                    }
                }}
                className="user-header pt-[80px] pb-[16px] px-6 shrink-0 relative bg-transparent cursor-default select-none z-10"
            >
                {/* Top Interaction Row - Perfectly Aligned */}
                <div className="absolute top-[22px] left-[22px] right-[22px] h-9 flex items-center justify-between pointer-events-none">
                    <button 
                        onClick={minimizeWindow}
                        className="w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all pointer-events-auto bg-white/5 rounded-full hover:bg-white/10"
                    >
                        <ChevronDown size={20} />
                    </button>

                    <div className="flex items-center gap-3 pointer-events-auto h-full">
                        {/* Connectivity Indicator */}
                        <div className={cn(
                            "w-9 h-9 rounded-full flex items-center justify-center border transition-all duration-300 bg-white/5",
                            isConnected 
                                ? "border-white/10 text-[var(--text-primary)]" 
                                : "border-white/5 text-[var(--text-secondary)] opacity-40 grayscale"
                        )} title={isConnected ? "Connected to Server" : "Disconnected"}>
                            {isConnected ? (
                                <Wifi size={18} strokeWidth={2} />
                            ) : (
                                <WifiOff size={18} strokeWidth={2} />
                            )}
                        </div>
                        
                        {/* Integrations Bar */}
                        <div className="flex items-center h-9 bg-white/5 rounded-full border border-white/10 px-1 gap-1">
                            <button className="w-7 h-7 rounded-full flex items-center justify-center hover:bg-white/10 transition-colors text-[var(--text-secondary)] hover:text-white" title="Add Integration">
                                <Plus size={16} strokeWidth={2.5} />
                            </button>
                            
                            {integrations.length > 0 && (
                                <>
                                    <div className="w-[1px] h-4 bg-white/10 mx-0.5" />
                                    <div className="flex gap-1.5">
                                        {integrations.map(int => (
                                            <div key={int} className="w-7 h-7 rounded-full bg-[#0B0C0E] border border-white/10 flex items-center justify-center text-[var(--text-secondary)]">
                                                {/* Actual connected integration icons would be rendered here */}
                                            </div>
                                        ))}
                                    </div>
                                    <div className="text-[var(--text-secondary)] ml-1 mr-1">
                                        <ChevronRight size={14} strokeWidth={2.5} />
                                    </div>
                                </>
                            )}
                        </div>
                    </div>
                </div>
                
                <h1 className="text-[28px] font-semibold tracking-[-0.5px]">Hi Antoine,</h1>
                <p className="subtitle text-[var(--text-secondary)] mt-1.5 text-[14px]">Here is your stack.</p>
            </header>

            {/* Scrollable Content */}
            <section className="stack-container flex-1 overflow-y-auto no-scrollbar pb-4 flex flex-col relative z-10">
                <div className="px-6 pt-4 flex flex-col">
                    <div className="commute-alerts-container flex flex-col gap-2.5 mb-6">
                        {alerts.map(alert => (
                            <CommuteAlert 
                                key={alert.id}
                                message={alert.message}
                                type={alert.type}
                                onDismiss={() => setAlerts(prev => prev.filter(a => a.id !== alert.id))}
                            />
                        ))}
                    </div>

                    <div className="scope-root flex flex-col pt-2">
                        {tasks.length === 0 ? (
                            <div className="text-[var(--text-secondary)] text-center py-5 text-[13px]">
                                Your stack is empty.
                            </div>
                        ) : (
                            (() => {
                                const grouped = groupTasks(tasks);
                                const dayKeys = Object.keys(grouped.days);
                                
                                return (
                                    <>
                                        {/* 1. Unscheduled Tasks (No Scope) */}
                                        {grouped.unplanned.length > 0 && (
                                            <div className={cn(
                                                "task-list flex flex-col gap-4 px-4 pb-8",
                                                dayKeys.length > 0 && "opacity-60 grayscale-[0.5]"
                                            )}>
                                                {grouped.unplanned.map(task => <TicketCard key={task.id} task={task} />)}
                                            </div>
                                        )}

                                        {/* 2. Hierarchical Scopes (Scheduled) */}
                                        {dayKeys.length > 0 && (
                                            <ScopeBlock label={grouped.week || "Scope"} type="week">
                                                {dayKeys.map(dayLabel => (
                                                    <ScopeBlock key={dayLabel} label={dayLabel} type="day">
                                                        <div className="task-list flex flex-col gap-4">
                                                            {grouped.days[dayLabel].map(task => <TicketCard key={task.id} task={task} />)}
                                                        </div>
                                                    </ScopeBlock>
                                                ))}
                                            </ScopeBlock>
                                        )}
                                    </>
                                );
                            })()
                        )}
                    </div>
                </div>
                
                {/* Fixed Spacer */}
                <div className="h-[20px] shrink-0"></div>
            </section>

            {/* Moat Transition Bar */}
            <div className="w-full h-[6px] bg-[#141414] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] z-20 shrink-0" />

            {/* Chat Area — Full width, flush to window */}
            <div className="chat-container w-full z-30 relative overflow-hidden shrink-0">
                {/* Grain background matching card tiles */}
                <WebGLGrain colors={{ c1: [30, 30, 30], c2: [22, 22, 22], c3: [16, 16, 16], c4: [12, 12, 12] }} />
                {/* Top crease */}
                <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
                
                <div className="relative z-20">
                    {feedback && (
                        <div className="ai-feedback-toast absolute bottom-full left-1/2 -translate-x-1/2 bg-[rgba(26,26,30,0.95)] border border-[rgba(138,180,248,0.3)] text-[#8AB4F8] p-[10px_20px] rounded-[20px] text-[13px] font-medium whitespace-nowrap mb-4 backdrop-blur-[10px] shadow-2xl animate-in fade-in slide-in-from-bottom-2">
                            {feedback}
                        </div>
                    )}
                    
                    <form 
                        onSubmit={handleAction} 
                        className={cn(
                            "chat-input-wrapper flex items-end bg-transparent p-[20px_24px_28px] transition-all duration-400 relative overflow-hidden",
                            isProcessing && "shadow-[inset_0_0_30px_rgba(99,102,241,0.1)]"
                        )}
                    >
                        {isProcessing && (
                            <div className="effect-container absolute inset-0 z-0 pointer-events-none overflow-hidden transition-opacity duration-500 opacity-100 scale-150">
                                <div className="pearl-gradient pearl-slow"></div>
                                <div className="pearl-gradient pearl-fast"></div>
                            </div>
                        )}

                        <textarea 
                            ref={inputRef}
                            value={inputValue}
                            onChange={(e) => setInputValue(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' && !e.shiftKey) {
                                    e.preventDefault();
                                    handleAction();
                                }
                            }}
                            placeholder={placeholder}
                            className="flex-1 bg-transparent border-none text-[#d1d1d1] text-[14px] outline-none resize-none min-h-[40px] max-h-[120px] leading-[1.6] relative z-10 placeholder:text-[#555]"
                        />
                        <button 
                            type="submit" 
                            disabled={isProcessing || !inputValue.trim()}
                            className="send-btn bg-white border-none w-10 h-10 rounded-full flex items-center justify-center cursor-pointer text-black transition-all shrink-0 ml-4 mb-0 relative z-10 hover:scale-105 active:scale-95 disabled:opacity-30"
                        >
                            <Send size={20} />
                        </button>
                    </form>
                </div>
            </div>
        </main>
    );
}

export default function AppWrapper() {
    return (
        <SyncProvider userId={1}>
            <App />
        </SyncProvider>
    );
}

import { useState, useEffect, useRef } from "react";
import { SyncProvider, useSync, TaskModel } from "./SyncEngine";
import { Send, X, ChevronDown, Plus, ChevronRight, Wifi, WifiOff } from "lucide-react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// --- Specialized Card Components ---

const formatDate = (dateString?: string) => {
    if (!dateString) return "";
    const date = new Date(dateString);
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
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
    const isAgentTask = type === 'AGENT_TASK';
    const isCountdown = type === 'COUNTDOWN';
    const expiresAt = payload.expires_at ? new Date(payload.expires_at).getTime() : null;

    const [timeLeft, setTimeLeft] = useState<string>("");
    const [expired, setExpired] = useState(false);

    useEffect(() => {
        if (!expiresAt) return;

        const updateTimer = () => {
            const remaining = expiresAt - Date.now();
            if (remaining <= 0) {
                setTimeLeft('DONE');
                setExpired(true);
                return true; // finished
            }
            const mins = Math.floor(remaining / 60000);
            const secs = Math.floor((remaining % 60000) / 1000);
            setTimeLeft(`${mins}:${secs.toString().padStart(2, '0')}`);
            return false;
        };

        const finished = updateTimer();
        if (finished) return;

        const interval = setInterval(() => {
            const done = updateTimer();
            if (done) clearInterval(interval);
        }, 1000);

        return () => clearInterval(interval);
    }, [expiresAt]);

    const handleToggle = (e: React.MouseEvent) => {
        e.stopPropagation();
        updateTask(task.id, { ...payload, completed: !isCompleted });
    };

    return (
        <div 
            className={cn(
                "ticket-card bg-[var(--bg-surface)] border border-[var(--border-color)] rounded-[12px] p-4 flex items-center gap-4 transition-all duration-200 cursor-default animate-[slideUp_0.3s_ease-out] hover:translate-y-[-2px] hover:shadow-[0_6px_16px_rgba(0,0,0,0.4)] hover:border-[rgba(255,255,255,0.15)]",
                isCompleted && "opacity-60",
                isAgentTask && "border-l-[3px] border-l-[#FBBF24]",
                isCountdown && "border-l-[3px] border-l-[#38BDF8]"
            )}
        >
            <div 
                onClick={handleToggle}
                className={cn(
                    "ticket-status w-5 h-5 border-2 border-[var(--text-secondary)] rounded-[6px] flex items-center justify-center cursor-pointer shrink-0 transition-all duration-200",
                    isCompleted && "bg-[var(--accent-blue)] border-[var(--accent-blue)] after:content-[''] after:w-[5px] after:height-[10px] after:border-solid after:border-white after:border-width-[0_2px_2px_0] after:rotate-45 after:mb-[3px]"
                )}
            />
            <div className="ticket-content flex-1 overflow-hidden flex items-center justify-between gap-4">
                <div>
                    <div className={cn(
                        "ticket-title text-[var(--text-primary)] font-medium text-[15px] line-height-[1.4]",
                        isCompleted && "text-[var(--text-secondary)] line-through"
                    )}>
                        {title}
                    </div>
                    <div className="ticket-meta flex items-center gap-2 mt-1.5 overflow-x-auto no-scrollbar">
                        <span className={cn(
                            "type-badge text-[10px] font-bold uppercase tracking-[1px] px-2 py-0.5 rounded-[5px] bg-[rgba(255,255,255,0.05)]",
                            type.toLowerCase() === 'habit' && "text-[var(--type-habit)]",
                            type.toLowerCase() === 'event' && "text-[var(--type-event)]",
                            type.toLowerCase() === 'task' && "text-[var(--type-task)]",
                            type.toLowerCase() === 'commute' && "text-[var(--type-commute)] bg-[rgba(167,139,250,0.15)]"
                        )}>
                            {type.replace('_', ' ')}
                        </span>
                        
                        {payload.scheduled_time && (
                            <span className="text-[10px] font-bold text-[var(--text-secondary)] uppercase tracking-[1px] px-2 py-0.5 rounded-[5px] bg-[rgba(255,255,255,0.05)] border border-white/5 whitespace-nowrap">
                                {payload.scheduled_time}
                            </span>
                        )}

                        {payload.recurrence && (
                            <span className="text-[10px] font-bold text-[var(--text-secondary)] opacity-40 uppercase tracking-[1.5px] whitespace-nowrap italic">
                                {payload.recurrence}
                            </span>
                        )}

                        {!payload.scheduled_time && task.created_at && (
                            <span className="ticket-date text-[11px] text-[var(--text-secondary)] opacity-40">
                                {formatDate(task.created_at)}
                            </span>
                        )}
                    </div>
                </div>
            </div>
        </div>
    );
};

const CommuteAlert = ({ message, type, onDismiss }: { message: string, type: string, onDismiss: () => void }) => {
    const isLiveTrip = type === 'live_trip' || type === 'live_trip_expired';
    const isExpired = type === 'live_trip_expired';

    return (
        <div className={cn(
            "commute-alert relative overflow-hidden p-[14px_18px] rounded-[12px] bg-gradient-to-br from-[rgba(167,139,250,0.12)] to-[rgba(94,106,210,0.08)] border border-[rgba(167,139,250,0.25)] animate-[alertSlideIn_0.4s_ease-out] before:content-[''] before:absolute before:top-0 before:left-0 before:w-[3px] before:h-full before:bg-[var(--type-commute)] before:rounded-[3px_0_0_3px]",
            isLiveTrip && "from-[rgba(239,68,68,0.12)] to-[rgba(249,115,22,0.08)] border-[rgba(239,68,68,0.3)] before:bg-[#EF4444]"
        )}>
            <button 
                onClick={onDismiss}
                className="commute-alert-dismiss absolute top-2 right-2.5 bg-none border-none text-[var(--text-secondary)] cursor-pointer text-[16px] p-[2px_6px] rounded-[4px] transition-colors hover:text-[var(--text-primary)]"
            >
                <X size={14} />
            </button>
            {isLiveTrip && !isExpired && (
                <div className="live-badge inline-block bg-[#EF4444] text-white text-[10px] font-bold tracking-[1px] px-2 py-0.5 rounded-[4px] mb-1.5 animate-[pulse_2s_infinite]">
                    LIVE
                </div>
            )}
            <div className="commute-alert-message text-[13px] text-[var(--text-primary)] leading-[1.6] whitespace-pre-line">
                {message}
            </div>
        </div>
    );
};
// --- Main App Component ---

function App() {
    const { tasks, syncNow, isConnected } = useSync();
    const [inputValue, setInputValue] = useState("");
    const [isProcessing, setIsProcessing] = useState(false);
    const [placeholder, setPlaceholder] = useState("Tell Gemini to manage tickets...");
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
        setPlaceholder("Gemini is processing your action...");

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
                    setPlaceholder("Action completed. Tell Gemini to manage tickets...");
                    setTimeout(() => setPlaceholder("Tell Gemini to manage tickets..."), 3000);
                }
            } else {
                setPlaceholder("Error communicating with AI.");
                setTimeout(() => setPlaceholder("Tell Gemini to manage tickets..."), 3000);
            }
        } catch (err) {
            setPlaceholder("Network error reaching the server.");
            setTimeout(() => setPlaceholder("Tell Gemini to manage tickets..."), 3000);
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
            className="app-container w-screen h-screen flex flex-col relative bg-[#0B0C0E] rounded-[24px] overflow-hidden border border-white/10 shadow-2xl transition-all duration-300 ease-out"
        >
            {/* Header Area (Draggable) */}
            <header 
                onPointerDown={(e) => {
                    if (e.button === 0) {
                        tauriWindow.current.startDragging().catch(console.error);
                    }
                }}
                className="user-header pt-[80px] pb-[16px] px-6 shrink-0 relative bg-[var(--bg-main)] cursor-default select-none z-10"
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
            <section className="stack-container flex-1 overflow-y-auto no-scrollbar pb-4 flex flex-col">
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

            {/* Chat Area (Integrated Footer) */}
            <div className="chat-container w-full bg-[#1A1A1E] border-t border-white/10 z-30">
                <div className="w-full relative">
                    {feedback && (
                        <div className="ai-feedback-toast absolute bottom-full left-1/2 -translate-x-1/2 bg-[rgba(26,26,30,0.95)] border border-[rgba(138,180,248,0.3)] text-[#8AB4F8] p-[10px_20px] rounded-[20px] text-[13px] font-medium whitespace-nowrap mb-4 backdrop-blur-[10px] shadow-2xl animate-in fade-in slide-in-from-bottom-2">
                            {feedback}
                        </div>
                    )}
                    
                    <form 
                        onSubmit={handleAction} 
                        className={cn(
                            "chat-input-wrapper flex items-end bg-transparent p-[16px_24px_24px] transition-all duration-400 relative overflow-hidden",
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
                            className="flex-1 bg-transparent border-none text-[var(--text-primary)] text-[15px] outline-none resize-none min-h-[40px] max-h-[120px] leading-[1.6] relative z-10 placeholder:text-[var(--text-secondary)]"
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

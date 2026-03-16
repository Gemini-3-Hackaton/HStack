import { useState, useEffect, useRef } from "react";
import { SyncProvider, useSync, TaskModel } from "./SyncEngine";
import { Send, X, ChevronDown } from "lucide-react";
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
            <div className="ticket-content flex-1 overflow-hidden">
                <div className={cn(
                    "ticket-title text-[var(--text-primary)] font-medium text-[15px] line-height-[1.4]",
                    isCompleted && "text-[var(--text-secondary)] line-through"
                )}>
                    {title}
                </div>
                <div className="ticket-meta flex items-center gap-2 mt-1.5">
                    <span className={cn(
                        "type-badge text-[11px] font-semibold uppercase tracking-[0.5px] px-1.5 py-0.5 rounded-[4px] bg-[rgba(255,255,255,0.05)]",
                        type.toLowerCase() === 'habit' && "text-[var(--type-habit)]",
                        type.toLowerCase() === 'event' && "text-[var(--type-event)]",
                        type.toLowerCase() === 'task' && "text-[var(--type-task)]",
                        type.toLowerCase() === 'commute' && "text-[var(--type-commute)] bg-[rgba(167,139,250,0.15)]",
                        type.toLowerCase() === 'agent_task' && "text-[#FBBF24] bg-[rgba(251,191,36,0.15)]",
                        type.toLowerCase() === 'countdown' && "text-[#38BDF8] bg-[rgba(56,189,248,0.15)]"
                    )}>
                        {type.replace('_', ' ')}
                    </span>
                    {timeLeft && (
                        <span className={cn(
                            "text-[12px] font-mono font-bold px-2 py-0.5 rounded-[4px] tracking-[0.5px] animate-[pulse_1s_infinite]",
                            isCountdown ? "text-[#38BDF8] bg-[rgba(56,189,248,0.1)]" : "text-[#FBBF24] bg-[rgba(251,191,36,0.1)]",
                            expired && "text-[#34D399] bg-[rgba(52,211,153,0.1)] animate-none"
                        )}>
                            {timeLeft}
                        </span>
                    )}
                    <span className="ticket-date text-[12px] text-[var(--text-secondary)]">
                        {formatDate(task.created_at)}
                    </span>
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
    const { tasks, syncNow } = useSync();
    const [inputValue, setInputValue] = useState("");
    const [isProcessing, setIsProcessing] = useState(false);
    const [placeholder, setPlaceholder] = useState("Tell Gemini to manage tickets...");
    const [alerts, setAlerts] = useState<any[]>([]);
    const [feedback, setFeedback] = useState<string | null>(null);
    const [isExpanded, setIsExpanded] = useState(false);
    const [isChangingSize, setIsChangingSize] = useState(false);

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const tauriWindow = useRef(getCurrentWindow());

    // Window size management with visual smoothing
    useEffect(() => {
        const updateWindowSize = async () => {
            setIsChangingSize(true);
            
            if (isExpanded) {
                // Expand window first
                await tauriWindow.current.setSize(new LogicalSize(400, 700));
                // Wait a tiny bit for layout
                await new Promise(r => setTimeout(r, 40));
            } else {
                // Wait for content fade
                await new Promise(r => setTimeout(r, 60));
                await tauriWindow.current.setSize(new LogicalSize(64, 64));
            }
            
            setIsChangingSize(false);
        };
        updateWindowSize();
    }, [isExpanded]);

    const toggleExpand = (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
        setIsExpanded(!isExpanded);
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

    if (!isExpanded) {
        return (
            <div 
                onPointerDown={(e) => {
                    if (e.button === 0) { 
                        tauriWindow.current.startDragging().catch(console.error);
                    }
                }}
                onClick={toggleExpand}
                className={cn(
                    "w-[64px] h-[64px] bg-[#0B0C0E] rounded-full flex items-center justify-center cursor-pointer hover:bg-[rgba(255,255,255,0.08)] overflow-hidden relative group border border-white/25 transition-all duration-300 ease-out",
                    isChangingSize ? "opacity-0 scale-90" : "opacity-100 scale-100"
                )}
            >
                <div className="w-[34px] h-[34px] text-white transition-opacity group-hover:opacity-80 pointer-events-none select-none flex items-center justify-center">
                    <svg viewBox="0 0 210 211" fill="currentColor" className="w-full h-full">
                        <rect x="0" y="10" width="60" height="191" />
                        <rect x="150" y="10" width="60" height="191" />
                        <rect x="50" y="50" width="100" height="31" />
                        <rect x="50" y="90" width="100" height="31" />
                        <rect x="50" y="130" width="100" height="31" />
                    </svg>
                </div>
            </div>
        );
    }

    return (
        <main 
            className={cn(
                "app-container w-screen h-screen flex flex-col relative bg-[#0B0C0E] rounded-[24px] overflow-hidden border border-white/10 shadow-2xl transition-all duration-300 ease-out",
                isChangingSize ? "opacity-0 scale-95" : "opacity-100 scale-100"
            )}
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
                <button 
                    onClick={toggleExpand}
                    className="absolute top-[22px] left-[22px] w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all z-20 bg-white/5 rounded-full hover:bg-white/10"
                >
                    <ChevronDown size={20} />
                </button>
                <h1 className="text-[28px] font-semibold tracking-[-0.5px]">Hi Antoine,</h1>
                <p className="subtitle text-[var(--text-secondary)] mt-1.5 text-[14px]">Here is your stack.</p>
            </header>

            {/* Scrollable Content */}
            <section className="stack-container flex-1 overflow-y-auto no-scrollbar px-6 pb-4 flex flex-col gap-4">
                <div className="commute-alerts-container flex flex-col gap-2.5">
                    {alerts.map(alert => (
                        <CommuteAlert 
                            key={alert.id}
                            message={alert.message}
                            type={alert.type}
                            onDismiss={() => setAlerts(prev => prev.filter(a => a.id !== alert.id))}
                        />
                    ))}
                </div>

                <div className="task-list flex flex-col gap-4">
                    {tasks.length === 0 ? (
                        <div className="text-[var(--text-secondary)] text-center py-5">
                            Your stack is empty! Ask Gemini below to add a Habit, Event, or Task.
                        </div>
                    ) : (
                        tasks.map(task => <TicketCard key={task.id} task={task} />)
                    )}
                </div>
                
                {/* Fixed Spacer to prevent chat overlap */}
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

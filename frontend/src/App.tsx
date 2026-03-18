import { useState, useEffect, useRef } from "react";
import { SyncProvider, useSync, TaskModel } from "./SyncEngine";
import { Send, X, ChevronDown, Plus, Trash2, Wifi, WifiOff } from "lucide-react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./components/WebGLGrain";
import { motion, AnimatePresence } from "framer-motion";

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

// --- Specialized Expanded Components ---

const CommuteSteps = ({ directions }: { directions: any }) => {
  if (!directions) return null;

  if (directions.error || directions.total_duration === 'Enriching...') {
    const isEnriching = directions.total_duration === 'Enriching...' && !directions.error;
    return (
      <div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-2 animate-in fade-in slide-in-from-top-2 duration-500">
        <span className="text-[10px] font-bold text-blue-400/60 uppercase tracking-widest">Directions Status</span>
        <p className="text-[13px] text-white/50 italic leading-relaxed">
          {isEnriching 
            ? "Fetching transit data..." 
            : `Service currently unreachable: ${directions.error?.includes('GOOGLE_MAPS_API_KEY') ? 'Configuration error (API Key)' : directions.error}`}
        </p>
      </div>
    );
  }

  if (!directions.steps || directions.steps.length === 0) {
    return (
      <div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-2 animate-in fade-in slide-in-from-top-2 duration-500">
        <span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Route Status</span>
        <p className="text-[13px] text-white/50 italic leading-relaxed">No transit routes found for this itinerary.</p>
      </div>
    );
  }

  return (
    <div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-4 animate-in fade-in slide-in-from-top-2 duration-500">
      <div className="flex items-center justify-between">
         <div className="flex flex-col">
            <span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Estimated Arrival</span>
            <span className="text-[14px] text-white/90 font-medium">{directions.arrival_time || 'Unknown'}</span>
         </div>
         <div className="flex flex-col items-end">
            <span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Total Duration</span>
            <span className="text-[14px] text-white/90 font-medium">{directions.total_duration || 'Unknown'}</span>
         </div>
      </div>

      <div className="flex flex-col gap-3">
        {directions.steps.map((step: any, idx: number) => (
          <div key={idx} className="flex gap-3 items-start">
            <div className="flex flex-col items-center shrink-0 mt-1">
              <div className="w-1.5 h-1.5 bg-white/20 rounded-sm" />
              {idx < directions.steps.length - 1 && <div className="w-0.5 h-full min-h-[16px] bg-white/5 my-1" />}
            </div>
            <div className="flex-1">
              <div className="text-[12px] text-white/80 leading-relaxed" dangerouslySetInnerHTML={{ __html: step.instruction }} />
              {step.travel_mode === 'TRANSIT' && (
                <div className="flex items-center gap-2 mt-1.5 grayscale opacity-70">
                    <span className="text-[10px] px-1.5 py-0.5 rounded border border-white/10 bg-white/5 text-white capitalize">
                        {step.vehicle_type?.toLowerCase() || 'Transit'}
                    </span>
                    <span className="text-[10px] font-bold text-white/40">{step.transit_line}</span>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

const CountdownTimer = ({ expiresAt }: { expiresAt: string }) => {
    const [timeLeft, setTimeLeft] = useState("");
    
    useEffect(() => {
        const target = new Date(expiresAt).getTime();
        const update = () => {
            const now = new Date().getTime();
            const diff = target - now;
            if (diff <= 0) {
                setTimeLeft("EXPIRED");
                return;
            }
            const mins = Math.floor(diff / 60000);
            const secs = Math.floor((diff % 60000) / 1000);
            setTimeLeft(`${mins}:${secs.toString().padStart(2, '0')}`);
        };
        update();
        const timer = setInterval(update, 1000);
        return () => clearInterval(timer);
    }, [expiresAt]);

    return (
        <div className="mt-4 flex flex-col items-center py-6 border-y border-white/5 bg-white/[0.02] rounded-lg">
            <span className="text-[10px] font-bold text-white/30 uppercase tracking-[0.2em] mb-2">Time Remaining</span>
            <span className="text-[42px] font-light tracking-widest text-white/90 tabular-nums font-mono">{timeLeft}</span>
        </div>
    );
};

const Tag = ({ text, type, italic, cardTheme = THEMES.default, glow = false }: {
  text: string;
  type: string;
  italic?: boolean;
  cardTheme?: ThemeColors;
  glow?: boolean;
}) => {
  const borderColor = `rgba(${cardTheme.c1[0]}, ${cardTheme.c1[1]}, ${cardTheme.c1[2]}, 0.25)`;
  const baseClasses = "text-[9px] font-bold tracking-widest px-1.5 py-1 rounded-[4px] border transition-colors uppercase whitespace-nowrap";

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
  if (type === 'commute') colorClass = 'text-blue-400/80 bg-blue-950/40';
  if (type === 'countdown') colorClass = 'text-red-400/80 bg-red-950/40';
  
  return (
    <span 
      className={cn(baseClasses, colorClass, glow && "ring-1 ring-white/10 shadow-[0_0_10px_rgba(255,255,255,0.05)]")}
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
                        "w-[1.5px] h-full transition-all duration-300",
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
    const grouped: { inFocus: TaskModel | null; days: { [key: string]: TaskModel[] }; unplanned: TaskModel[] } = {
        inFocus: null,
        days: {},
        unplanned: []
    };

    // Helper to normalize time for sorting
    const timeToValue = (timeStr?: string) => {
        if (!timeStr) return 999999;
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
        if (task.status === 'in_focus' && !grouped.inFocus) {
            grouped.inFocus = task;
            return;
        }

        const time = task.payload?.scheduled_time?.trim();
        if (!time) {
            grouped.unplanned.push(task);
            return;
        }

        const lowerTime = time.toLowerCase();
        let dayLabel = "Today";

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
            }
        }

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
    const { updateTask, deleteTask } = useSync();
    const [isExpanded, setIsExpanded] = useState(false);
    const [isDeleting, setIsDeleting] = useState(false);
    
    const payload = task.payload || {};
    const isCompleted = payload.completed === true;
    const title = payload.title || 'Untitled';
    const type = task.type || 'TASK';
    const status = task.status || 'idle';
    const isInFocus = status === 'in_focus';
    
    // Auto-expand if in focus
    const showExpanded = isExpanded || isInFocus;
    
    const theme = type === 'HABIT' ? THEMES.habit : type === 'EVENT' ? THEMES.event : THEMES.default;

    const handleToggleComplete = (e: React.MouseEvent) => {
        e.stopPropagation();
        updateTask(task.id, { ...payload, completed: !isCompleted });
    };

    const onDragEnd = (_: any, info: any) => {
        if (info.offset.x < -100) {
            // Delete on swipe left
            setIsDeleting(true);
            setTimeout(() => deleteTask(task.id), 200);
        } else if (info.offset.x > 100) {
            // Complete on swipe right
            updateTask(task.id, { ...payload, completed: !isCompleted });
        }
    };

    return (
        <motion.div 
            layout
            initial={{ opacity: 0, y: 20 }}
            animate={{ 
                opacity: isDeleting ? 0 : 1, 
                x: 0,
                scale: isDeleting ? 0.95 : 1
            }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ duration: 0.2 }}
            className="relative group mb-3"
        >
            {/* Background Actions */}
            <div className="absolute inset-0 rounded-[1rem] flex items-center justify-between px-6 overflow-hidden">
                <div className="flex items-center gap-2 text-emerald-500/30">
                     <div className="w-1 h-3 bg-emerald-500/20" />
                     <span className="text-[10px] font-bold uppercase tracking-widest">Complete</span>
                </div>
                <div className="flex items-center gap-2 text-red-500/30">
                     <span className="text-[10px] font-bold uppercase tracking-widest">Delete</span>
                     <Trash2 size={18} />
                </div>
            </div>

            <motion.div
                drag="x"
                dragConstraints={{ left: -120, right: 120 }}
                dragElastic={0.1}
                onDragEnd={onDragEnd}
                onClick={() => setIsExpanded(!isExpanded)}
                className={cn(
                    "relative cursor-pointer transition-transform duration-500",
                    isInFocus && !isExpanded && "scale-[1.01]"
                )}
            >
                <PhysicalWrapper 
                    outerClass={cn(
                        "transition-all duration-500", 
                        showExpanded && "rounded-[1.25rem] bg-[#1a1a1a]"
                    )}
                    innerClass={cn(
                        "p-4 flex flex-col items-stretch", 
                        showExpanded ? "min-h-[100px]" : "flex-row items-center gap-4 py-3"
                    )} 
                    checked={isCompleted} 
                    shaderColors={theme}
                >
                    <div className="flex flex-col w-full">
                        <div className="min-w-0">
                            <h3 className={cn(
                                "font-medium tracking-wide text-[#d1d1d1] truncate transition-all duration-300",
                                isCompleted && "line-through text-[#555]",
                                showExpanded ? "text-[18px] text-white" : "text-[15px]"
                            )}>
                                {title}
                            </h3>
                        </div>
                        
                        <div className="flex flex-wrap items-center gap-2 mt-2 opacity-80">
                            <Tag text={type} type={type.toLowerCase()} cardTheme={theme} glow={isInFocus} />
                            {payload.scheduled_time && <Tag text={payload.scheduled_time} type="info" cardTheme={theme} />}
                            {payload.recurrence && <Tag text={payload.recurrence} type="info" italic={true} cardTheme={theme} />}
                        </div>
                    </div>

                    <AnimatePresence>
                        {showExpanded && (
                            <motion.div 
                                initial={{ height: 0, opacity: 0 }}
                                animate={{ height: "auto", opacity: 1 }}
                                exit={{ height: 0, opacity: 0 }}
                                transition={{ duration: 0.3, ease: "easeOut" }}
                                className="overflow-hidden"
                            >
                                <div className="pt-2">
                                    {type === 'COMMUTE' && <CommuteSteps directions={payload.directions} />}
                                    {type === 'COUNTDOWN' && <CountdownTimer expiresAt={payload.expires_at} />}
                                    {payload.note && (
                                        <div className="mt-4 text-[13px] text-white/60 leading-relaxed font-light italic border-l-2 border-white/5 pl-4 ml-1">
                                            "{payload.note}"
                                        </div>
                                    )}
                                </div>
                            </motion.div>
                        )}
                    </AnimatePresence>
                </PhysicalWrapper>
            </motion.div>
        </motion.div>
    );
};
// --- Main App Component ---

function App() {
    const { tasks, syncNow, isConnected } = useSync();
    const [inputValue, setInputValue] = useState("");
    const [isProcessing, setIsProcessing] = useState(false);
    const [placeholder, setPlaceholder] = useState("Tell AI to manage your stack...");
    const [feedback, setFeedback] = useState<string | null>(null);
    const [integrations, setIntegrations] = useState<string[]>([]);

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const tauriWindow = useRef(getCurrentWindow());

    const minimizeWindow = (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
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
                body: JSON.stringify({ message: message, userid: 1 })
            });

            if (resp.ok) {
                const data = await resp.json();
                await syncNow();
                
                if (data.response) {
                    showFeedback(data.response);
                } else {
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
                                        {/* 1. In Focus Ticket */}
                                        {grouped.inFocus && (
                                            <div className="mb-8">
                                                <div className="pl-[2.5px] mb-2 flex items-center h-4">
                                                    <span className="text-[10px] font-bold uppercase tracking-[1.5px] text-[#3B82F6]">
                                                        Now in Focus
                                                    </span>
                                                </div>
                                                <TicketCard task={grouped.inFocus} />
                                            </div>
                                        )}

                                        {/* 2. Unscheduled Tasks (No Scope) */}
                                        {grouped.unplanned.length > 0 && (
                                            <div className={cn(
                                                "task-list flex flex-col gap-4 px-4 pb-8",
                                                dayKeys.length > 0 && "opacity-60 grayscale-[0.5]"
                                            )}>
                                                {grouped.unplanned.map(task => <TicketCard key={task.id} task={task} />)}
                                            </div>
                                        )}

                                        {/* 3. Hierarchical Scopes (Scheduled) */}
                                        {dayKeys.length > 0 && (
                                            <ScopeBlock label="Timeline" type="week">
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

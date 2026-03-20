import { useState, useEffect, useRef, useMemo } from "react";
import { SyncProvider, useSync, TaskModel } from "./SyncEngine";
import { Send, ChevronDown, Plus, Trash2, Wifi, WifiOff, Settings as SettingsIcon, ChevronRight, ChevronUp } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./components/WebGLGrain";
import { motion, AnimatePresence } from "framer-motion";
import { Settings } from "./components/Settings";
import { SetupWizard } from "./components/SetupWizard";

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// --- Interaction States & Colors ---
type InteractionState = 'IDLE' | 'PROCESSING' | 'AWAITING_REPLY' | 'SUCCESS' | 'ERROR';

// Linear interpolation helper: blends color toward dark gray (12, 12, 12)
const lerpColor = (color: [number, number, number], factor: number): [number, number, number] => {
  const target = [12, 12, 12];
  return [
    Math.round(color[0] + (target[0] - color[0]) * factor),
    Math.round(color[1] + (target[1] - color[1]) * factor),
    Math.round(color[2] + (target[2] - color[2]) * factor)
  ];
};

// Single base color per state, generates gradient toward dark gray
const BASE_COLORS = {
  IDLE: [30, 30, 30] as [number, number, number],
  PROCESSING: [40, 45, 80] as [number, number, number],
  AWAITING_REPLY: [30, 60, 75] as [number, number, number],
  SUCCESS: [25, 60, 40] as [number, number, number],
  ERROR: [70, 25, 25] as [number, number, number]
};

const makeTheme = (base: [number, number, number]) => ({
  c1: lerpColor(base, 0.2),   // 20% toward dark gray
  c2: lerpColor(base, 0.4),   // 40% toward dark gray
  c3: lerpColor(base, 0.6),   // 60% toward dark gray
  c4: lerpColor(base, 0.8)    // 80% toward dark gray
});

const INTERACTION_THEMES = {
  IDLE: makeTheme(BASE_COLORS.IDLE),
  PROCESSING: makeTheme(BASE_COLORS.PROCESSING),
  AWAITING_REPLY: makeTheme(BASE_COLORS.AWAITING_REPLY),
  SUCCESS: makeTheme(BASE_COLORS.SUCCESS),
  ERROR: makeTheme(BASE_COLORS.ERROR)
};

interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content?: string;
  name?: string;
}

// --- RRULE Types ---
type RRuleFreq = 'DAILY' | 'WEEKLY' | 'MONTHLY' | 'YEARLY';

interface ParsedRRule {
  freq: RRuleFreq | null;
  byDay: string[] | null;        // ['MO', 'TU', etc]
  byMonthDay: number[] | null;   // [2, 4, 15, etc]
  byMonth: number[] | null;      // [1, 2, 3, 4] for Jan-Apr
  until: string | null;
  count: number | null;
  interval: number;
}

// --- Locale Config Caching ---
let cachedLocale: { locale: string; hour12: boolean } | null = null;

const loadUserLocale = async (): Promise<void> => {
  try {
    // Try to load from Tauri backend
    const [locale, hour12] = await invoke<[string, boolean]>("get_user_locale");
    cachedLocale = { locale, hour12 };
  } catch (error) {
    // Fall back to browser detection
    console.warn("Failed to load user locale, using browser default:", error);
    const userLocale = navigator.language || 'en-US';
    cachedLocale = { locale: userLocale, hour12: true };
  }
};

const getLocaleConfig = (): { locale: string; hour12: boolean } => {
  // Return cached settings if available, otherwise default
  return cachedLocale || { locale: navigator.language || 'en-US', hour12: true };
};

// --- Parse RRULE component into structured object ---
const parseRRuleComponent = (rrulePart: string): ParsedRRule => {
  const result: ParsedRRule = {
    freq: null,
    byDay: null,
    byMonthDay: null,
    byMonth: null,
    until: null,
    count: null,
    interval: 1,
  };

  const parts = rrulePart.split(';');
  
  for (const part of parts) {
    const [key, value] = part.split('=');
    if (!key || !value) continue;
    
    switch (key) {
      case 'FREQ':
        if (['DAILY', 'WEEKLY', 'MONTHLY', 'YEARLY'].includes(value)) {
          result.freq = value as RRuleFreq;
        }
        break;
      case 'BYDAY':
        result.byDay = value.split(',');
        break;
      case 'BYMONTHDAY':
        result.byMonthDay = value.split(',').map(n => parseInt(n, 10));
        break;
      case 'BYMONTH':
        result.byMonth = value.split(',').map(n => parseInt(n, 10));
        break;
      case 'UNTIL':
        result.until = value;
        break;
      case 'COUNT':
        result.count = parseInt(value, 10);
        break;
      case 'INTERVAL':
        result.interval = parseInt(value, 10);
        break;
    }
  }
  
  return result;
};

// --- Day name mapping ---
const DAY_NAMES: Record<string, string> = {
  'MO': 'Monday',
  'TU': 'Tuesday', 
  'WE': 'Wednesday',
  'TH': 'Thursday',
  'FR': 'Friday',
  'SA': 'Saturday',
  'SU': 'Sunday'
};

// --- Format recurrence for display ---
const formatRecurrence = (parsed: ParsedRRule): string | undefined => {
  if (!parsed.freq) return undefined;
  
  switch (parsed.freq) {
    case 'DAILY': {
      if (parsed.byDay && parsed.byDay.length === 5 && 
          parsed.byDay.includes('MO') && parsed.byDay.includes('FR')) {
        return 'Weekdays';
      }
      if (parsed.interval > 1) {
        return `Every ${parsed.interval} days`;
      }
      return 'Daily';
    }
    
    case 'WEEKLY': {
      if (parsed.byDay) {
        // Check for weekdays pattern
        if (parsed.byDay.length === 5 && 
            ['MO', 'TU', 'WE', 'TH', 'FR'].every(d => parsed.byDay?.includes(d))) {
          return 'Weekdays';
        }
        // Single day
        if (parsed.byDay.length === 1) {
          const dayName = DAY_NAMES[parsed.byDay[0]] || parsed.byDay[0];
          return `Every ${dayName}`;
        }
        // Multiple specific days
        const dayNames = parsed.byDay.map(d => DAY_NAMES[d] || d).join(', ');
        return `Weekly (${dayNames})`;
      }
      if (parsed.interval > 1) {
        return `Every ${parsed.interval} weeks`;
      }
      return 'Weekly';
    }
    
    case 'MONTHLY': {
      if (parsed.byMonthDay) {
        const days = parsed.byMonthDay.join(', ');
        if (parsed.byMonth) {
          const months = parsed.byMonth.map(m => 
            new Date(2000, m - 1).toLocaleDateString(getLocaleConfig().locale, { month: 'short' })
          ).join(', ');
          return `Monthly ${days} (${months})`;
        }
        return `Monthly (${days})`;
      }
      if (parsed.interval > 1) {
        return `Every ${parsed.interval} months`;
      }
      return 'Monthly';
    }
    
    case 'YEARLY':
      return parsed.interval > 1 ? `Every ${parsed.interval} years` : 'Yearly';
      
    default:
      return undefined;
  }
};

// --- Main RRULE Parser for Display Tags ---
const parseRRule = (rruleStr: string): { dateTag: string; timeTag?: string; recurrenceTag?: string } | null => {
  if (!rruleStr) return null;
  
  // Extract DTSTART
  const dtstartMatch = rruleStr.match(/DTSTART:(\d{4})(\d{2})(\d{2})T?(\d{2})?(\d{2})?(\d{2})?/);
  if (!dtstartMatch) return null;
  
  const year = parseInt(dtstartMatch[1], 10);
  const month = parseInt(dtstartMatch[2], 10) - 1; // 0-indexed
  const day = parseInt(dtstartMatch[3], 10);
  const hour = dtstartMatch[4] ? parseInt(dtstartMatch[4], 10) : null;
  const minute = dtstartMatch[5] ? parseInt(dtstartMatch[5], 10) : 0;
  
  const dtstart = new Date(year, month, day, hour ?? 0, minute);
  const { locale, hour12 } = getLocaleConfig();
  
  // Calculate today/tomorrow for relative labels
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);
  
  // Determine date tag
  let dateTag: string;
  if (dtstart.toDateString() === today.toDateString()) {
    dateTag = "Today";
  } else if (dtstart.toDateString() === tomorrow.toDateString()) {
    dateTag = "Tomorrow";
  } else {
    const daysDiff = Math.floor((dtstart.getTime() - today.getTime()) / (1000 * 60 * 60 * 24));
    if (daysDiff >= 0 && daysDiff < 7) {
      dateTag = dtstart.toLocaleDateString(locale, { weekday: 'long' });
    } else {
      dateTag = dtstart.toLocaleDateString(locale, { month: 'short', day: 'numeric' });
    }
  }
  
  // Extract time if present
  let timeTag: string | undefined;
  if (hour !== null) {
    timeTag = dtstart.toLocaleTimeString(locale, { 
      hour: 'numeric', 
      minute: '2-digit', 
      hour12 
    });
  }
  
  // Parse and format recurrence if RRULE present
  let recurrenceTag: string | undefined;
  const rruleMatch = rruleStr.match(/RRULE:(.+)/);
  if (rruleMatch) {
    const parsed = parseRRuleComponent(rruleMatch[1]);
    recurrenceTag = formatRecurrence(parsed);
  }
  
  return { dateTag, timeTag, recurrenceTag };
};

// --- Engraved Dark Themes ---
const THEMES = {
  habit: { c1: [42, 52, 48], c2: [32, 38, 35], c3: [24, 26, 25], c4: [20, 20, 20] },
  event: { c1: [54, 48, 40], c2: [36, 34, 31], c3: [25, 24, 23], c4: [20, 20, 20] },
  default: { c1: [48, 48, 48], c2: [34, 34, 34], c3: [24, 24, 24], c4: [20, 20, 20] }
};

type ThemeColors = typeof THEMES.default;

// --- Physical Wrapper ---
const PhysicalWrapper = ({ children, outerClass = '', innerClass = '', checked = false, shaderColors = THEMES.default }: {
  children: React.ReactNode; outerClass?: string; innerClass?: string; checked?: boolean; shaderColors?: ThemeColors;
}) => (
  <div className={cn("relative transition-all duration-300 bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] rounded-[1.25rem]", checked ? "opacity-50" : "opacity-100", outerClass)}>
    <div className={cn("relative w-full h-full overflow-hidden shadow-[0_2px_5px_rgba(0,0,0,0.7)] rounded-[15px]", innerClass)}>
      <WebGLGrain colors={shaderColors} />
      <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
      <div className="absolute top-0 left-0 bottom-0 w-[1px] bg-white/[0.03] z-10" />
      <div className="relative z-20 w-full h-full">{children}</div>
    </div>
  </div>
);

// --- Tag Component ---
const Tag = ({ text, type, italic, cardTheme = THEMES.default, glow = false }: {
  text: string; type: string; italic?: boolean; cardTheme?: ThemeColors; glow?: boolean;
}) => {
  const borderColor = `rgba(${cardTheme.c1[0]}, ${cardTheme.c1[1]}, ${cardTheme.c1[2]}, 0.25)`;
  const baseClasses = "text-[9px] font-bold tracking-widest px-1.5 py-1 rounded-[4px] border transition-colors uppercase whitespace-nowrap";
  if (type === 'info') return (<span className={cn(baseClasses, "bg-[#252525] text-[#888]", italic && "italic")} style={{ borderColor }}>{text}</span>);
  let colorClass = 'text-[#888] bg-[#222]';
  if (type === 'habit') colorClass = 'text-emerald-400/80 bg-emerald-950/40';
  if (type === 'event') colorClass = 'text-amber-400/80 bg-amber-950/40';
  if (type === 'commute') colorClass = 'text-blue-400/80 bg-blue-950/40';
  if (type === 'countdown') colorClass = 'text-red-400/80 bg-red-950/40';
  return (<span className={cn(baseClasses, colorClass, glow && "ring-1 ring-white/10 shadow-[0_0_10px_rgba(255,255,255,0.05)]")} style={{ borderColor }}>{text}</span>);
};

// --- Specialized Content ---
const CommuteSteps = ({ directions }: { directions: any }) => {
  if (!directions) return null;
  if (directions.error || directions.total_duration === 'Enriching...') {
    const isEnriching = directions.total_duration === 'Enriching...' && !directions.error;
    return (<div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-2 animate-in fade-in slide-in-from-top-2 duration-500"><span className="text-[10px] font-bold text-blue-400/60 uppercase tracking-widest">Directions Status</span><p className="text-[13px] text-white/50 italic leading-relaxed">{isEnriching ? "Fetching transit data..." : `Service currently unreachable: ${directions.error?.includes('GOOGLE_MAPS_API_KEY') ? 'Configuration error (API Key)' : directions.error}`}</p></div>);
  }
  if (!directions.steps || directions.steps.length === 0) return (<div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-2 animate-in fade-in slide-in-from-top-2 duration-500"><span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Route Status</span><p className="text-[13px] text-white/50 italic leading-relaxed">No transit routes found for this itinerary.</p></div>);
  return (
    <div className="mt-4 border-t border-white/5 pt-4 flex flex-col gap-4 animate-in fade-in slide-in-from-top-2 duration-500">
      <div className="flex items-center justify-between"><div className="flex flex-col"><span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Estimated Arrival</span><span className="text-[14px] text-white/90 font-medium">{directions.arrival_time || 'Unknown'}</span></div><div className="flex flex-col items-end"><span className="text-[10px] font-bold text-white/30 uppercase tracking-widest">Total Duration</span><span className="text-[14px] text-white/90 font-medium">{directions.total_duration || 'Unknown'}</span></div></div>
      <div className="flex flex-col gap-3">{directions.steps.map((step: any, idx: number) => (<div key={idx} className="flex gap-3 items-start"><div className="flex flex-col items-center shrink-0 mt-1"><div className="w-1.5 h-1.5 bg-white/20 rounded-sm" />{idx < directions.steps.length - 1 && <div className="w-0.5 h-full min-h-[16px] bg-white/5 my-1" />}</div><div className="flex-1"><div className="text-[12px] text-white/80 leading-relaxed" dangerouslySetInnerHTML={{ __html: step.instruction }} />{step.travel_mode === 'TRANSIT' && (<div className="flex items-center gap-2 mt-1.5 grayscale opacity-70"><span className="text-[10px] px-1.5 py-0.5 rounded border border-white/10 bg-white/5 text-white capitalize">{step.vehicle_type?.toLowerCase() || 'Transit'}</span><span className="text-[10px] font-bold text-white/40">{step.transit_line}</span></div>)}</div></div>))}</div>
    </div>
  );
};

const CountdownTimer = ({ expiresAt }: { expiresAt: string }) => {
    const [timeLeft, setTimeLeft] = useState("");
    useEffect(() => {
        const target = new Date(expiresAt).getTime();
        const update = () => { const now = new Date().getTime(); const diff = target - now; if (diff <= 0) { setTimeLeft("EXPIRED"); return; } const mins = Math.floor(diff / 60000); const secs = Math.floor((diff % 60000) / 1000); setTimeLeft(`${mins}:${secs.toString().padStart(2, '0')}`); };
        update(); const timer = setInterval(update, 1000); return () => clearInterval(timer);
    }, [expiresAt]);
    return (<div className="mt-4 flex flex-col items-center py-6 border-y border-white/5 bg-white/[0.02] rounded-lg"><span className="text-[10px] font-bold text-white/30 uppercase tracking-[0.2em] mb-2">Time Remaining</span><span className="text-[42px] font-light tracking-widest text-white/90 tabular-nums font-mono">{timeLeft}</span></div>);
};

// --- Scope Components ---
interface ScopeBlockProps { label: string; type: 'day' | 'week'; children: React.ReactNode; }
const ScopeBlock = ({ label, type, children }: ScopeBlockProps) => {
    const isWeek = type === 'week';
    return (<div className="flex flex-col mb-4"><div className="pl-[2.5px] mb-1 flex items-center h-4"><span className={cn("text-[10px] font-bold uppercase tracking-[1.5px] whitespace-nowrap", isWeek ? "text-[#3B82F6]" : "text-white/30")}>{label}</span></div><div className="flex gap-2"><div className="shrink-0 pl-[4px]"><div className={cn("w-[1.5px] h-full transition-all duration-300", isWeek ? "bg-[#3B82F6]" : "bg-white/10")} /></div><div className="flex-1 flex flex-col gap-4">{children}</div></div></div>);
};

// Helper to get day label from date
const getDayLabelFromDate = (date: Date): string => {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);
  
  if (date.toDateString() === today.toDateString()) return "Today";
  if (date.toDateString() === tomorrow.toDateString()) return "Tomorrow";
  
  const daysDiff = Math.floor((date.getTime() - today.getTime()) / (1000 * 60 * 60 * 24));
  if (daysDiff >= 0 && daysDiff < 7) {
    return date.toLocaleDateString('en-US', { weekday: 'long' });
  }
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
};

// Updated groupTasks to work with RRULE data
const groupTasks = (tasks: TaskModel[]) => {
  const grouped: { 
    inFocus: TaskModel | null; 
    days: { [key: string]: TaskModel[] }; 
    unplanned: TaskModel[] 
  } = { inFocus: null, days: {}, unplanned: [] };
  
  // First, handle in_focus task
  for (const task of tasks) {
    if (task.status === 'in_focus' && !grouped.inFocus) {
      grouped.inFocus = task;
      break;
    }
  }
  
  // Process remaining tasks
  const regularTasks = tasks.filter(t => t.status !== 'in_focus');
  
  for (const task of regularTasks) {
    const payload = task.payload || {};
    
    // Check if task has RRULE scheduling
    if (payload.rrule) {
      const dtstartMatch = payload.rrule.match(/DTSTART:(\d{4})(\d{2})(\d{2})/);
      if (dtstartMatch) {
        const year = parseInt(dtstartMatch[1], 10);
        const month = parseInt(dtstartMatch[2], 10) - 1;
        const day = parseInt(dtstartMatch[3], 10);
        const scheduledDate = new Date(year, month, day);
        const dayLabel = getDayLabelFromDate(scheduledDate);
        
        if (!grouped.days[dayLabel]) grouped.days[dayLabel] = [];
        grouped.days[dayLabel].push(task);
        continue;
      }
    }
    
    // Fallback to old scheduled_time parsing for backward compatibility
    const time = payload.scheduled_time?.trim();
    if (!time) {
      grouped.unplanned.push(task);
      continue;
    }
    
    const lowerTime = time.toLowerCase();
    let dayLabel = "Today";
    
    if (lowerTime.includes("tomorrow")) dayLabel = "Tomorrow";
    else if (lowerTime.includes("monday")) dayLabel = "Monday";
    else if (lowerTime.includes("tuesday")) dayLabel = "Tuesday";
    else if (lowerTime.includes("wednesday")) dayLabel = "Wednesday";
    else if (lowerTime.includes("thursday")) dayLabel = "Thursday";
    else if (lowerTime.includes("friday")) dayLabel = "Friday";
    else if (lowerTime.includes("saturday")) dayLabel = "Saturday";
    else if (lowerTime.includes("sunday")) dayLabel = "Sunday";
    else if (/^\d{4}-\d{2}-\d{2}/.test(time)) {
      const d = new Date(time);
      if (!isNaN(d.getTime())) {
        dayLabel = d.toLocaleDateString('en-US', { weekday: 'long' });
      }
    }
    
    if (!grouped.days[dayLabel]) grouped.days[dayLabel] = [];
    grouped.days[dayLabel].push(task);
  }
  
  // Sort each day group by time
  const dayOrder = ["Today", "Tomorrow", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
  const orderedDays: { [key: string]: TaskModel[] } = {};
  
  dayOrder.forEach(label => { 
    if (grouped.days[label]) {
      // Sort by time within each day
      grouped.days[label].sort((a, b) => {
        const timeA = a.payload?.rrule || a.payload?.scheduled_time || '';
        const timeB = b.payload?.rrule || b.payload?.scheduled_time || '';
        return timeA.localeCompare(timeB);
      });
      orderedDays[label] = grouped.days[label];
    }
  });
  
  // Add any remaining days not in dayOrder
  Object.keys(grouped.days).forEach(label => {
    if (!orderedDays[label]) orderedDays[label] = grouped.days[label];
  });
  
  grouped.days = orderedDays;
  
  // Sort unplanned by creation/payload
  grouped.unplanned.sort((a, b) => {
    const titleA = a.payload?.title || '';
    const titleB = b.payload?.title || '';
    return titleA.localeCompare(titleB);
  });
  
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
    const showExpanded = isExpanded || isInFocus;
    const theme = type === 'HABIT' ? THEMES.habit : type === 'EVENT' ? THEMES.event : THEMES.default;
    
    const onDragEnd = (_: any, info: any) => {
        if (info.offset.x < -100) {
            setIsDeleting(true);
            setTimeout(() => deleteTask(task.id), 200);
        } else if (info.offset.x > 100) {
            updateTask(task.id, { ...payload, completed: !isCompleted });
        }
    };
    
    // Parse RRULE for display tags
    const rruleTags = useMemo(() => {
        return payload.rrule ? parseRRule(payload.rrule) : null;
    }, [payload.rrule]);
    
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
                            {rruleTags?.dateTag && <Tag text={rruleTags.dateTag} type="info" cardTheme={theme} />}
                            {rruleTags?.timeTag && <Tag text={rruleTags.timeTag} type="info" cardTheme={theme} />}
                            {rruleTags?.recurrenceTag && <Tag text={rruleTags.recurrenceTag} type="info" italic={true} cardTheme={theme} />}
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
    const placeholder = "Tell AI to manage your stack...";
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [integrations] = useState<string[]>([]);
    const [chatHistory, setChatHistory] = useState<ChatMessage[]>([]);
    const [isHistoryExpanded, setIsHistoryExpanded] = useState(false);
    const [interactionState, setInteractionState] = useState<InteractionState>('IDLE');
    const [showSetup, setShowSetup] = useState(false);

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const tauriWindow = useRef(getCurrentWindow());
    const drawerRef = useRef<HTMLDivElement>(null);

    const minimizeWindow = (e?: React.MouseEvent) => { if (e) e.stopPropagation(); tauriWindow.current.minimize().catch(err => console.error(err)); };
    useEffect(() => { 
        const checkOnboarding = async () => {
            const settings = await invoke<any>('get_settings');
            if (!settings.onboarding_complete) {
              setShowSetup(true);
            }
        };
        checkOnboarding();
        syncNow();
        // Load user locale settings on app start
        loadUserLocale().catch(err => console.warn("Failed to load user locale:", err));
    }, []);
    useEffect(() => { if (drawerRef.current && isHistoryExpanded) { drawerRef.current.scrollTop = drawerRef.current.scrollHeight; } }, [chatHistory, isHistoryExpanded]);
    useEffect(() => { if (inputRef.current) { inputRef.current.style.height = 'auto'; inputRef.current.style.height = (inputRef.current.scrollHeight) + 'px'; if (inputValue === '') { inputRef.current.style.height = '48px'; } } }, [inputValue]);

    const handleAction = async (e?: React.FormEvent) => {
        if (e) e.preventDefault(); const message = inputValue.trim(); if (!message || isProcessing) return;
        setInputValue(""); setIsProcessing(true); setInteractionState('PROCESSING');
        const userMsg: ChatMessage = { role: 'user', content: message }; const updatedHistory = [...chatHistory, userMsg]; setChatHistory(updatedHistory);
        try {
            const response = await invoke<ChatMessage[]>("chat_local", { message, history: updatedHistory });
            if (response && response.length > 0) {
                setChatHistory(prev => [...prev, ...response]);
                const lastMsg = response[response.length - 1];
                if (lastMsg.content) { if (lastMsg.content.trim().endsWith('?')) { setInteractionState('AWAITING_REPLY'); } else { setInteractionState('SUCCESS'); setTimeout(() => setInteractionState('IDLE'), 2000); } }
            }
            await syncNow();
        } catch (invokeErr) {
            console.warn("Local chat error, falling back to server:", invokeErr);
            try {
                const resp = await fetch('http://localhost:8000/api/chat', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ message: message, userid: 1 }) });
                if (resp.ok) { const data = await resp.json(); await syncNow(); if (data.response) { setChatHistory(prev => [...prev, { role: 'assistant', content: data.response }]); if (data.response.trim().endsWith('?')) { setInteractionState('AWAITING_REPLY'); } else { setInteractionState('SUCCESS'); setTimeout(() => setInteractionState('IDLE'), 2000); } } } else { setInteractionState('ERROR'); setTimeout(() => setInteractionState('IDLE'), 3000); }
            } catch (err) { setInteractionState('ERROR'); setTimeout(() => setInteractionState('IDLE'), 3000); }
        } finally { setIsProcessing(false); if (inputRef.current) inputRef.current.focus(); }
    };

    return (
        <main className="app-container w-screen h-screen flex flex-col relative bg-[#080808] rounded-[24px] overflow-hidden border border-white/15 shadow-2xl transition-all duration-300 ease-out">
            <WebGLGrain colors={{ c1: [30, 30, 30], c2: [12, 12, 12], c3: [9, 9, 9], c4: [6, 6, 6] }} spreadX={0.35} spreadY={1.1} contrast={2.0} noiseFactor={0.7} opacity={1.0} />
            {showSetup && <SetupWizard onComplete={() => setShowSetup(false)} />}
            <header onPointerDown={(e) => { if (e.button === 0) { tauriWindow.current.startDragging().catch(console.error); } }} className="user-header pt-[80px] pb-[16px] px-6 shrink-0 relative bg-transparent cursor-default select-none z-10">
                <div className="absolute top-[22px] left-[22px] right-[22px] h-9 flex items-center justify-between pointer-events-none">
                    <button onClick={minimizeWindow} className="w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all pointer-events-auto bg-white/5 rounded-full hover:bg-white/10"><ChevronDown size={20} /></button>
                    <div className="flex items-center gap-3 pointer-events-auto h-full">
                        <button onClick={() => setIsSettingsOpen(true)} className="w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all bg-white/5 rounded-full hover:bg-white/10"><SettingsIcon size={18} /></button>
                        <div className={cn("w-9 h-9 rounded-full flex items-center justify-center border transition-all duration-300 bg-white/5", isConnected ? "border-white/10 text-[var(--text-primary)]" : "border-white/5 text-[var(--text-secondary)] opacity-40 grayscale")}>{isConnected ? <Wifi size={18} strokeWidth={2} /> : <WifiOff size={18} strokeWidth={2} />}</div>
                        <div className="flex items-center h-9 bg-white/5 rounded-full border border-white/10 px-1 gap-1">
                            <button className="w-7 h-7 rounded-full flex items-center justify-center hover:bg-white/10 transition-colors text-[var(--text-secondary)] hover:text-white"><Plus size={16} strokeWidth={2.5} /></button>
                            {integrations.length > 0 && (<><div className="w-[1px] h-4 bg-white/10 mx-0.5" /><div className="flex gap-1.5">{integrations.map(int => (<div key={int} className="w-7 h-7 rounded-full bg-[#0B0C0E] border border-white/10 flex items-center justify-center text-[var(--text-secondary)]" />))}</div><div className="text-[var(--text-secondary)] ml-1 mr-1"><ChevronRight size={14} strokeWidth={2.5} /></div></>)}
                        </div>
                    </div>
                </div>
                <h1 className="text-[28px] font-semibold tracking-[-0.5px]">Hi Antoine,</h1>
                <p className="subtitle text-[var(--text-secondary)] mt-1.5 text-[14px]">Here is your stack.</p>
            </header>

            <section className="stack-container flex-1 overflow-y-auto no-scrollbar pb-4 flex flex-col relative z-10">
                <div className="px-6 pt-4 flex flex-col"><div className="scope-root flex flex-col pt-2">{tasks.length === 0 ? (<div className="text-[var(--text-secondary)] text-center py-5 text-[13px]">Your stack is empty.</div>) : (
                    (() => {
                        const grouped = groupTasks(tasks); const dayKeys = Object.keys(grouped.days);
                        return (<>{grouped.inFocus && (<div className="mb-8"><div className="pl-[2.5px] mb-2 flex items-center h-4"><span className="text-[10px] font-bold uppercase tracking-[1.5px] text-[#3B82F6]">Now in Focus</span></div><TicketCard task={grouped.inFocus} /></div>)}{grouped.unplanned.length > 0 && (<div className={cn("task-list flex flex-col gap-4 px-4 pb-8", dayKeys.length > 0 && "opacity-60 grayscale-[0.5]")}>{grouped.unplanned.map(task => <TicketCard key={task.id} task={task} />)}</div>)}{dayKeys.length > 0 && (<ScopeBlock label="Timeline" type="week">{dayKeys.map(dayLabel => (<ScopeBlock key={dayLabel} label={dayLabel} type="day"><div className="task-list flex flex-col gap-4">{grouped.days[dayLabel].map(task => <TicketCard key={task.id} task={task} />)}</div></ScopeBlock>))}</ScopeBlock>)}</>);
                    })()
                )}</div></div>
                <div className="h-[20px] shrink-0" />
            </section>

            {/* --- THE HISTORY NOTCH (Truly Full-Width Physical Pull-Up) --- */}
            <div className="w-full z-40 relative pointer-events-none">
                <AnimatePresence>
                    {(chatHistory.length > 0 || isProcessing) && (
                        <motion.div 
                            initial={{ height: 0 }} 
                            animate={{ height: isHistoryExpanded ? 'auto' : '24px' }} 
                            exit={{ height: 0 }}
                            transition={{ type: "spring", stiffness: 300, damping: 30 }}
                            className="w-screen pointer-events-auto bg-[#0B0C0E] border-t border-white/[0.06] rounded-t-[28px] overflow-hidden shadow-[0_-15px_35px_rgba(0,0,0,0.6)] relative left-1/2 -translate-x-1/2"
                        >
                            {/* Unified Grain Surface bound to interaction state */}
                            <div className="absolute inset-0 z-0">
                                <WebGLGrain 
                                    colors={INTERACTION_THEMES[interactionState]} 
                                    opacity={0.9} 
                                    contrast={1.4} 
                                />
                            </div>

                            {/* Physical Crease at top edge */}
                            <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.07] z-10" />

                            <div className="relative z-20 flex flex-col">
                                {/* Centered Handle (Always Visible, Full Width) */}
                                <div 
                                    onClick={() => setIsHistoryExpanded(!isHistoryExpanded)}
                                    className="w-full h-[24px] flex items-center justify-center cursor-pointer group"
                                >
                                    <div className="text-white/20 group-hover:text-white/40 transition-colors">
                                        {isHistoryExpanded ? <ChevronDown size={22} strokeWidth={2.5} /> : <ChevronUp size={22} strokeWidth={2.5} />}
                                    </div>
                                    
                                </div>

                                {/* Expanded Conversation Area */}
                                <AnimatePresence>
                                    {isHistoryExpanded && (
                                        <motion.div 
                                            initial={{ opacity: 0 }}
                                            animate={{ opacity: 1 }}
                                            exit={{ opacity: 0 }}
                                            className="px-8 pb-10 pt-2 border-t border-white/[0.03]"
                                        >
                                            <div ref={drawerRef} className="max-h-[280px] overflow-y-auto pr-4 custom-scrollbar flex flex-col gap-6">
                                                {chatHistory.filter(m => m.role !== 'system').map((msg, i) => (
                                                    <div key={i} className={cn(
                                                        "text-[14px] leading-relaxed transition-opacity duration-500",
                                                        msg.role === 'user' ? "text-white/70 pl-5 border-l border-white/[0.04]" : "text-white/30 italic font-light"
                                                    )}>
                                                        <div className="text-[9px] font-bold uppercase tracking-[0.2em] opacity-30 mb-1.5">
                                                            {msg.role === 'user' ? 'User' : 'Agent'}
                                                        </div>
                                                        {msg.content}
                                                    </div>
                                                ))}
                                            </div>
                                        </motion.div>
                                    )}
                                </AnimatePresence>
                            </div>
                        </motion.div>
                    )}
                </AnimatePresence>
            </div>

            {/* Moat Transition Bar */}
            <div className="w-full h-[6px] bg-[#141414] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] z-20 shrink-0" />

            {/* Chat Area — Full width, flush to window */}
            <div className="chat-container w-full z-30 relative overflow-hidden shrink-0">
                <WebGLGrain colors={{ c1: [30, 30, 30], c2: [22, 22, 22], c3: [16, 16, 16], c4: [12, 12, 12] }} />
                <div className="absolute top-0 left-0 right-0 h-[1px] bg-white/[0.03] z-10" />
                <div className="relative z-20">
                    <form onSubmit={handleAction} className={cn("chat-input-wrapper flex items-end bg-transparent p-[20px_24px_28px] transition-all duration-400 relative overflow-hidden", isProcessing && "shadow-[inset_0_0_30px_rgba(99,102,241,0.1)]")}>
                        {isProcessing && (<div className="effect-container absolute inset-0 z-0 pointer-events-none overflow-hidden transition-opacity duration-500 opacity-100 scale-150"><div className="pearl-gradient pearl-slow" /><div className="pearl-gradient pearl-fast" /></div>)}
                        <textarea ref={inputRef} value={inputValue} onChange={(e) => setInputValue(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleAction(); } }} placeholder={interactionState === 'AWAITING_REPLY' ? "Reply to the agent..." : placeholder} className={cn("flex-1 bg-transparent border-none text-[#d1d1d1] text-[14px] outline-none resize-none min-h-[40px] max-h-[120px] leading-[1.6] relative z-10 transition-colors", interactionState === 'AWAITING_REPLY' ? "text-white placeholder:text-white/30" : "placeholder:text-[#555]")} />
                        <button type="submit" disabled={isProcessing || !inputValue.trim()} className={cn("send-btn border-none w-10 h-10 rounded-full flex items-center justify-center cursor-pointer transition-all shrink-0 ml-4 mb-0 relative z-10 hover:scale-105 active:scale-95 disabled:opacity-30", interactionState === 'AWAITING_REPLY' ? "bg-white text-black shadow-[0_0_15px_rgba(255,255,255,0.2)]" : "bg-white text-black")}><Send size={20} /></button>
                    </form>
                </div>
            </div>
            <AnimatePresence>{isSettingsOpen && (<Settings isOpen={isSettingsOpen} onClose={() => setIsSettingsOpen(false)} />)}</AnimatePresence>
        </main>
    );
}

export default function AppWrapper() {
    return (<SyncProvider userId={1}><App /></SyncProvider>);
}

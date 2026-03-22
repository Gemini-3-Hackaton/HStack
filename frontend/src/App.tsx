import { useState, useEffect, useRef, useMemo } from "react";
import { SyncProvider, TicketModel } from "./SyncEngine";
import { useSync } from "./useSync";
import { Send, ChevronDown, Plus, Wifi, WifiOff, Settings as SettingsIcon, ChevronRight, ChevronUp, ExternalLink } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { WebGLGrain } from "./components/WebGLGrain";
import { motion, AnimatePresence } from "framer-motion";
import { Settings } from "./components/Settings";
import { SetupWizard } from "./components/SetupWizard";
import { translate, useI18n } from "./i18n";
import {
  type SavedLocationRecord,
  type SavedLocationIndex,
  buildGoogleMapsEmbedUrl,
  buildGoogleMapsUrl,
  formatAbsoluteSchedule,
  formatDurationMinutes,
  formatMetadataTag,
  getDisplayScheduleDate,
  getScheduleTags,
  getSharedSchedule,
  groupTickets,
  resolveStructuredLocation,
} from "./ticketPresentation";
import { canUseDesktopWindowControls, minimizeDesktopWindow, startDesktopWindowDrag } from "./platform";

const TASK_TYPE_LABELS = {
  TASK: 'taskTypeTask',
  HABIT: 'taskTypeHabit',
  EVENT: 'taskTypeEvent',
  COMMUTE: 'taskTypeCommute',
  COUNTDOWN: 'taskTypeCountdown',
} as const;

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

const SectionLabel = ({ children }: { children: React.ReactNode }) => (
  <div className="text-[10px] font-semibold uppercase tracking-[0.2em] text-white/30">{children}</div>
);

const InfoPanel = ({ children, className = '' }: { children: React.ReactNode; className?: string }) => (
  <div className={cn('flex flex-col gap-1.5', className)}>{children}</div>
);

const FactGrid = ({ items }: { items: Array<{ label: string; value?: string }> }) => {
  const visibleItems = items.filter((item) => item.value);
  if (visibleItems.length === 0) return null;

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
      {visibleItems.map((item) => (
        <div key={`${item.label}-${item.value}`} className="rounded-[14px] border border-white/7 bg-white/[0.03] px-3 py-2.5">
          <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-white/34">{item.label}</div>
          <div className="mt-1 text-[13px] leading-relaxed text-white/82">{item.value}</div>
        </div>
      ))}
    </div>
  );
};

const MapLinkRow = ({ googleUrl }: { googleUrl?: string }) => {
  if (!googleUrl) return null;

  const handleOpen = async (event: React.MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();

    try {
      await openUrl(googleUrl);
    } catch (error) {
      console.error('Failed to open Google Maps link:', error);
    }
  };

  return (
    <div className="mt-1 flex flex-wrap gap-2">
      <button
        type="button"
        onClick={handleOpen}
        className="inline-flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-white/74 transition-colors hover:bg-white/[0.08]"
      >
        <ExternalLink size={12} />
        {translate('openInGoogleMaps')}
      </button>
    </div>
  );
};

const MapEmbed = ({ embedUrl }: { embedUrl?: string }) => {
  if (!embedUrl) return null;

  return (
    <InfoPanel className="mt-1 gap-2">
      <SectionLabel>{translate('detailMap')}</SectionLabel>
      <div className="overflow-hidden rounded-[16px] border border-white/8 bg-black/20">
        <iframe
          title={translate('detailMap')}
          src={embedUrl}
          loading="lazy"
          referrerPolicy="no-referrer-when-downgrade"
          className="h-[220px] w-full border-0"
          onClick={(event) => event.stopPropagation()}
        />
      </div>
    </InfoPanel>
  );
};

// --- Specialized Content ---
const CommuteSteps = ({ directions }: { directions: any }) => {
  if (!directions) return null;
  if (directions.error || directions.total_duration === 'Enriching...') {
    const isEnriching = directions.total_duration === 'Enriching...' && !directions.error;
    return (
      <InfoPanel className="mt-1">
        <SectionLabel>{translate('detailDirections')}</SectionLabel>
        <p className="text-[13px] italic leading-relaxed text-white/55">
          {isEnriching
            ? translate('fetchingTransitData')
            : translate('serviceCurrentlyUnreachable', {
                error: directions.error?.includes('GOOGLE_MAPS_API_KEY')
                  ? translate('configurationErrorApiKey')
                  : directions.error,
              })}
        </p>
      </InfoPanel>
    );
  }
  if (!directions.steps || directions.steps.length === 0) {
    return (
      <InfoPanel className="mt-1">
        <SectionLabel>{translate('detailRoute')}</SectionLabel>
        <p className="text-[13px] italic leading-relaxed text-white/55">{translate('noTransitRoutes')}</p>
      </InfoPanel>
    );
  }

  return (
    <InfoPanel className="mt-1 gap-2">
      <SectionLabel>{translate('detailDirections')}</SectionLabel>
      <DetailRow label={translate('detailArrival')} value={directions.arrival_time || translate('unknown')} />
      <DetailRow label={translate('detailDuration')} value={directions.total_duration || translate('unknown')} />
      <div className="flex flex-col gap-3">
        {directions.steps.map((step: any, idx: number) => (
          <div key={idx} className="flex items-start gap-3 border-b border-white/6 pb-3 last:border-b-0 last:pb-0">
            <div className="w-4 shrink-0 pt-0.5 text-[11px] font-medium tabular-nums text-white/28">{idx + 1}.</div>
            <div className="min-w-0">
              <div className="text-[12px] leading-relaxed text-white/80" dangerouslySetInnerHTML={{ __html: step.instruction }} />
              {step.travel_mode === 'TRANSIT' && (
                <div className="mt-1.5 flex flex-wrap items-center gap-2 text-[10px] uppercase tracking-[0.16em] text-white/38">
                  <span>{step.vehicle_type?.toLowerCase() || translate('transit')}</span>
                  {step.transit_line && <span>{step.transit_line}</span>}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </InfoPanel>
  );
};

const CountdownTimer = ({ expiresAt }: { expiresAt: string }) => {
    const [timeLeft, setTimeLeft] = useState("");
    useEffect(() => {
        const target = new Date(expiresAt).getTime();
        const update = () => { const now = new Date().getTime(); const diff = target - now; if (diff <= 0) { setTimeLeft(translate('expired').toUpperCase()); return; } const mins = Math.floor(diff / 60000); const secs = Math.floor((diff % 60000) / 1000); setTimeLeft(`${mins}:${secs.toString().padStart(2, '0')}`); };
        update(); const timer = setInterval(update, 1000); return () => clearInterval(timer);
    }, [expiresAt]);
    return (
      <InfoPanel className="mt-1">
        <SectionLabel>{translate('detailTimeRemaining')}</SectionLabel>
        <span className="font-mono text-[22px] font-light tracking-[0.08em] text-white/88 tabular-nums">{timeLeft}</span>
      </InfoPanel>
    );
};

const DetailRow = ({ label, value }: { label: string; value: string }) => (
  <div className="flex items-start justify-between gap-4 border-b border-white/6 py-2 last:border-b-0 last:pb-0 first:pt-0">
    <span className="text-[10px] font-semibold uppercase tracking-[0.18em] text-white/34">{label}</span>
    <span className="text-[13px] text-right leading-relaxed text-white/76">{value}</span>
  </div>
);

const DetailGroup = ({ title, rows, tone }: { title: string; rows: Array<{ label: string; value?: string }>; tone?: string }) => {
  const visibleRows = rows.filter((row) => row.value);
  if (visibleRows.length === 0) return null;

  return (
    <InfoPanel className={cn('mt-1', tone)}>
      <div className="mb-1">
        <SectionLabel>{title}</SectionLabel>
      </div>
      {visibleRows.map((row) => (
        <DetailRow key={`${title}-${row.label}`} label={row.label} value={row.value!} />
      ))}
    </InfoPanel>
  );
};

// --- Scope Components ---
interface ScopeBlockProps { label: string; type: 'day' | 'week'; children: React.ReactNode; }
const ScopeBlock = ({ label, type, children }: ScopeBlockProps) => {
    const isWeek = type === 'week';
    return (<div className="flex flex-col mb-4"><div className="pl-[2.5px] mb-1 flex items-center h-4"><span className={cn("text-[10px] font-bold uppercase tracking-[1.5px] whitespace-nowrap", isWeek ? "text-[#3B82F6]" : "text-white/30")}>{label}</span></div><div className="flex gap-2"><div className="shrink-0 pl-[4px]"><div className={cn("w-[1.5px] h-full transition-all duration-300", isWeek ? "bg-[#3B82F6]" : "bg-white/10")} /></div><div className="flex-1 flex flex-col gap-4">{children}</div></div></div>);
};

const TicketCard = ({ ticket, savedLocations }: { ticket: TicketModel; savedLocations: SavedLocationIndex }) => {
    const [isExpanded, setIsExpanded] = useState(false);
  const payload = ticket.payload || {};
    const isCompleted = payload.completed === true;
    const title = payload.title || translate('untitled');
  const type = ticket.type || 'TASK';
  const status = ticket.status || 'idle';
  const notes = ticket.notes || payload.notes || payload.note;
    const isInFocus = status === 'in_focus';
    const showExpanded = isExpanded || isInFocus;
    const theme = type === 'HABIT' ? THEMES.habit : type === 'EVENT' ? THEMES.event : THEMES.default;
    
    // Parse RRULE for display tags
    const scheduleTags = useMemo(() => {
      return getScheduleTags(payload);
    }, [payload]);

    const displayScheduleDate = useMemo(() => getDisplayScheduleDate(payload), [payload]);
    const sharedSchedule = useMemo(() => getSharedSchedule(payload), [payload]);
    const durationText = typeof sharedSchedule.durationMinutes === 'number'
      ? formatDurationMinutes(sharedSchedule.durationMinutes)
      : undefined;
    const scheduleText = displayScheduleDate ? formatAbsoluteSchedule(displayScheduleDate) : undefined;
    const statusTag = typeof payload.status === 'string' ? formatMetadataTag(payload.status) : undefined;
    const priorityTag = typeof payload.priority === 'string' ? formatMetadataTag(payload.priority) : undefined;
    const commuteDays = typeof payload.days === 'string'
      ? payload.days.split(',').map((day: string) => day.trim()).filter(Boolean).join(', ')
      : undefined;
    const eventLocation = resolveStructuredLocation(payload.location, savedLocations);
    const commuteOrigin = resolveStructuredLocation(payload.origin_location, savedLocations);
    const commuteDestination = resolveStructuredLocation(payload.destination_location, savedLocations);
    const eventLocationText = eventLocation?.display;
    const commuteOriginText = commuteOrigin?.display ?? payload.origin;
    const commuteDestinationText = commuteDestination?.display ?? payload.destination;
    const googleMapsUrl = type === 'COMMUTE'
      ? buildGoogleMapsUrl(commuteOrigin?.query ?? payload.origin, commuteDestination?.query ?? payload.destination)
      : undefined;
    const commuteEmbedUrl = type === 'COMMUTE'
      ? buildGoogleMapsEmbedUrl(commuteOrigin?.query ?? payload.origin, commuteDestination?.query ?? payload.destination)
      : undefined;
    const eventEmbedUrl = type === 'EVENT' ? buildGoogleMapsEmbedUrl(undefined, eventLocation?.query ?? eventLocationText) : undefined;
    const scheduleRows = [
      { label: 'When', value: scheduleText },
      { label: 'Repeats', value: scheduleTags?.recurrenceTag },
      { label: 'Duration', value: durationText },
    ];
    const eventRows = [
      { label: translate('detailWhere'), value: eventLocationText },
    ];
    const commuteRows = [
      { label: translate('detailFrom'), value: commuteOriginText },
      { label: translate('detailTo'), value: commuteDestinationText },
      { label: translate('detailDeadline'), value: payload.deadline },
      { label: translate('detailDays'), value: commuteDays },
      { label: translate('detailRemaining'), value: typeof payload.minutes_remaining === 'number' ? `${payload.minutes_remaining} min` : undefined },
    ];
    const countdownRows = [
      { label: translate('detailExpires'), value: payload.expires_at ? formatAbsoluteSchedule(new Date(payload.expires_at)) : undefined },
      { label: translate('detailLength'), value: typeof payload.duration_minutes === 'number' ? formatDurationMinutes(payload.duration_minutes) : undefined },
    ];
    const highlightFacts = type === 'TASK'
      ? [
          { label: translate('detailWhen'), value: scheduleText },
          { label: translate('detailDuration'), value: durationText },
          { label: translate('detailStatus'), value: statusTag },
        ]
      : type === 'HABIT'
        ? [
            { label: translate('detailRepeats'), value: scheduleTags?.recurrenceTag },
            { label: translate('detailWhen'), value: scheduleText },
            { label: translate('detailStatus'), value: statusTag },
          ]
        : type === 'EVENT'
          ? [
              { label: translate('detailWhen'), value: scheduleText },
              { label: translate('detailWhere'), value: eventLocationText },
              { label: translate('detailDuration'), value: durationText },
            ]
          : type === 'COMMUTE'
            ? [
                { label: translate('detailFrom'), value: commuteOriginText },
                { label: translate('detailTo'), value: commuteDestinationText },
                { label: translate('detailDeadline'), value: payload.deadline },
              ]
            : [
                { label: translate('detailExpires'), value: payload.expires_at ? formatAbsoluteSchedule(new Date(payload.expires_at)) : undefined },
                { label: translate('detailLength'), value: typeof payload.duration_minutes === 'number' ? formatDurationMinutes(payload.duration_minutes) : undefined },
              ];
    
    return (
        <motion.div 
            layout
            initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ duration: 0.2 }}
            className="relative group mb-3"
        >
            <motion.div
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
                    showExpanded ? "min-h-[100px]" : "px-4 py-3 flex flex-row items-center gap-4"
                    )}
                    checked={isCompleted}
                    shaderColors={theme}
                >
                  <div className={cn("flex w-full flex-col", showExpanded && "px-4 pt-4 pb-0")}>
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
                      <Tag text={translate(TASK_TYPE_LABELS[type as keyof typeof TASK_TYPE_LABELS] ?? 'taskTypeTask')} type={type.toLowerCase()} cardTheme={theme} glow={isInFocus} />
                      {statusTag && <Tag text={statusTag} type="info" cardTheme={theme} />}
                      {priorityTag && <Tag text={priorityTag} type="info" cardTheme={theme} />}
                      {scheduleTags?.dateTag && <Tag text={scheduleTags.dateTag} type="info" cardTheme={theme} />}
                      {scheduleTags?.timeTag && <Tag text={scheduleTags.timeTag} type="info" cardTheme={theme} />}
                      {scheduleTags?.recurrenceTag && <Tag text={scheduleTags.recurrenceTag} type="info" italic={true} cardTheme={theme} />}
                    </div>
                  </div>

                    <AnimatePresence>
                        {showExpanded && (
                            <motion.div
                                initial={{ height: 0, opacity: 0 }}
                                animate={{ height: "auto", opacity: 1 }}
                                exit={{ height: 0, opacity: 0 }}
                                transition={{ duration: 0.24, ease: "easeOut" }}
                        className="mt-3 overflow-hidden border-t border-white/6 bg-[#181818]"
                            >
                        <div className="px-4 pt-3 pb-4 flex flex-col gap-2.5">
                                  <FactGrid items={highlightFacts} />
                                  <DetailGroup title={translate('detailSchedule')} rows={scheduleRows.map((row) => ({
                                    ...row,
                                    label: row.label === 'When'
                                      ? translate('detailWhen')
                                      : row.label === 'Repeats'
                                        ? translate('detailRepeats')
                                        : translate('detailDuration'),
                                  }))} />
                                  {type === 'EVENT' && <DetailGroup title={translate('detailEvent')} rows={eventRows} />}
                                  {type === 'EVENT' && <MapEmbed embedUrl={eventEmbedUrl} />}
                                  {type === 'COMMUTE' && <DetailGroup title={translate('detailCommute')} rows={commuteRows} />}
                                  {type === 'COMMUTE' && <MapLinkRow googleUrl={googleMapsUrl} />}
                                  {type === 'COMMUTE' && <MapEmbed embedUrl={commuteEmbedUrl} />}
                                  {type === 'COMMUTE' && <CommuteSteps directions={payload.directions} />}
                                  {type === 'COUNTDOWN' && <DetailGroup title={translate('detailCountdown')} rows={countdownRows} />}
                                  {type === 'COUNTDOWN' && payload.expires_at && <CountdownTimer expiresAt={payload.expires_at} />}
                                  {notes && (
                                    <InfoPanel className="mt-1">
                                      <div className="mb-1">
                                        <SectionLabel>{translate('detailNotes')}</SectionLabel>
                                      </div>
                                      <p className="text-[13px] leading-relaxed text-white/68">{notes}</p>
                                    </InfoPanel>
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
  const { tickets, syncNow, isConnected } = useSync();
  const { t } = useI18n();
    const [inputValue, setInputValue] = useState("");
    const [isProcessing, setIsProcessing] = useState(false);
  const placeholder = t('placeholderManageStack');
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [integrations] = useState<string[]>([]);
    const [chatHistory, setChatHistory] = useState<ChatMessage[]>([]);
    const [isHistoryExpanded, setIsHistoryExpanded] = useState(false);
    const [interactionState, setInteractionState] = useState<InteractionState>('IDLE');
    const [showSetup, setShowSetup] = useState(false);
    const [savedLocations, setSavedLocations] = useState<SavedLocationIndex>({});
    const supportsDesktopWindowControls = useMemo(() => canUseDesktopWindowControls(), []);

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const drawerRef = useRef<HTMLDivElement>(null);

    const minimizeWindow = (e?: React.MouseEvent) => {
      if (e) e.stopPropagation();
      minimizeDesktopWindow().catch(err => console.error(err));
    };
    const handleHeaderPointerDown = (e: React.PointerEvent<HTMLElement>) => {
      if (!supportsDesktopWindowControls || e.button !== 0) {
        return;
      }

      startDesktopWindowDrag().catch(console.error);
    };
    useEffect(() => { 
      const loadSavedLocations = async () => {
        const settings = await invoke<any>('get_settings');
        const locations = Array.isArray(settings.saved_locations) ? settings.saved_locations : [];
        setSavedLocations(Object.fromEntries(locations.map((location: SavedLocationRecord) => [location.id, location])));
      };

        const checkOnboarding = async () => {
            const settings = await invoke<any>('get_settings');
            if (!settings.onboarding_complete) {
              setShowSetup(true);
            }
        };

        checkOnboarding();
      loadSavedLocations();
        syncNow();
    }, []);
    useEffect(() => {
      if (!isSettingsOpen) {
        invoke<any>('get_settings')
          .then((settings) => {
            const locations = Array.isArray(settings.saved_locations) ? settings.saved_locations : [];
            setSavedLocations(Object.fromEntries(locations.map((location: SavedLocationRecord) => [location.id, location])));
          })
          .catch((error) => console.error('Failed to refresh saved locations:', error));
      }
    }, [isSettingsOpen]);
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
            console.error("Local chat failed:", invokeErr);
            setInteractionState('ERROR');
            setTimeout(() => setInteractionState('IDLE'), 3000);
        } finally { setIsProcessing(false); if (inputRef.current) inputRef.current.focus(); }
    };

    return (
          <main className="app-container w-screen h-screen flex flex-col relative bg-[#080808] rounded-[24px] overflow-hidden border border-white/15 shadow-2xl transition-all duration-300 ease-out" style={{ minHeight: '100dvh' }}>
            <WebGLGrain colors={{ c1: [30, 30, 30], c2: [12, 12, 12], c3: [9, 9, 9], c4: [6, 6, 6] }} spreadX={0.35} spreadY={1.1} contrast={2.0} noiseFactor={0.7} opacity={1.0} />
            {showSetup && <SetupWizard onComplete={() => setShowSetup(false)} />}
              <header onPointerDown={handleHeaderPointerDown} className="user-header pt-[80px] pb-[16px] px-6 shrink-0 relative bg-transparent cursor-default select-none z-10" style={{ paddingTop: 'calc(80px + env(safe-area-inset-top, 0px))' }}>
                <div className="absolute top-[22px] left-[22px] right-[22px] h-9 flex items-center justify-between pointer-events-none" style={{ top: 'calc(22px + env(safe-area-inset-top, 0px))' }}>
                {supportsDesktopWindowControls ? <button onClick={minimizeWindow} className="w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all pointer-events-auto bg-white/5 rounded-full hover:bg-white/10"><ChevronDown size={20} /></button> : <div className="w-9 h-9" />}
                    <div className="flex items-center gap-3 pointer-events-auto h-full">
                        <button onClick={() => setIsSettingsOpen(true)} className="w-9 h-9 flex items-center justify-center text-[var(--text-secondary)] hover:text-white transition-all bg-white/5 rounded-full hover:bg-white/10"><SettingsIcon size={18} /></button>
                        <div className={cn("w-9 h-9 rounded-full flex items-center justify-center border transition-all duration-300 bg-white/5", isConnected ? "border-white/10 text-[var(--text-primary)]" : "border-white/5 text-[var(--text-secondary)] opacity-40 grayscale")}>{isConnected ? <Wifi size={18} strokeWidth={2} /> : <WifiOff size={18} strokeWidth={2} />}</div>
                        <div className="flex items-center h-9 bg-white/5 rounded-full border border-white/10 px-1 gap-1">
                            <button className="w-7 h-7 rounded-full flex items-center justify-center hover:bg-white/10 transition-colors text-[var(--text-secondary)] hover:text-white"><Plus size={16} strokeWidth={2.5} /></button>
                            {integrations.length > 0 && (<><div className="w-[1px] h-4 bg-white/10 mx-0.5" /><div className="flex gap-1.5">{integrations.map(int => (<div key={int} className="w-7 h-7 rounded-full bg-[#0B0C0E] border border-white/10 flex items-center justify-center text-[var(--text-secondary)]" />))}</div><div className="text-[var(--text-secondary)] ml-1 mr-1"><ChevronRight size={14} strokeWidth={2.5} /></div></>)}
                        </div>
                    </div>
                </div>
                <h1 className="text-[28px] font-semibold tracking-[-0.5px]">{t('greeting', { name: 'Antoine' })}</h1>
                <p className="subtitle text-[var(--text-secondary)] mt-1.5 text-[14px]">{t('stackSubtitle')}</p>
            </header>

            <section className="stack-container flex-1 overflow-y-auto no-scrollbar pb-4 flex flex-col relative z-10">
                <div className="px-6 pt-4 flex flex-col"><div className="scope-root flex flex-col pt-2">{tickets.length === 0 ? (<div className="text-[var(--text-secondary)] text-center py-5 text-[13px]">{t('emptyStack')}</div>) : (
                    (() => {
                  const grouped = groupTickets(tickets); const dayKeys = Object.keys(grouped.days);
                  return (<>{grouped.inFocus && (<div className="mb-8"><div className="pl-[2.5px] mb-2 flex items-center h-4"><span className="text-[10px] font-bold uppercase tracking-[1.5px] text-[#3B82F6]">{t('nowInFocus')}</span></div><TicketCard ticket={grouped.inFocus} savedLocations={savedLocations} /></div>)}{grouped.unplanned.length > 0 && (<div className={cn("task-list flex flex-col gap-4 px-4 pb-8", dayKeys.length > 0 && "opacity-60 grayscale-[0.5]")}>{grouped.unplanned.map(ticket => <TicketCard key={ticket.id} ticket={ticket} savedLocations={savedLocations} />)}</div>)}{dayKeys.length > 0 && (<ScopeBlock label={t('timeline')} type="week">{dayKeys.map(dayLabel => (<ScopeBlock key={dayLabel} label={dayLabel} type="day"><div className="task-list flex flex-col gap-4">{grouped.days[dayLabel].map(ticket => <TicketCard key={ticket.id} ticket={ticket} savedLocations={savedLocations} />)}</div></ScopeBlock>))}</ScopeBlock>)}</>);
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
                                                          {msg.role === 'user' ? t('user') : t('agent')}
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
                <form onSubmit={handleAction} className={cn("chat-input-wrapper flex items-end bg-transparent p-[20px_24px_28px] transition-all duration-400 relative overflow-hidden", isProcessing && "shadow-[inset_0_0_30px_rgba(99,102,241,0.1)]")} style={{ paddingBottom: 'calc(28px + env(safe-area-inset-bottom, 0px))' }}>
                        {isProcessing && (<div className="effect-container absolute inset-0 z-0 pointer-events-none overflow-hidden transition-opacity duration-500 opacity-100 scale-150"><div className="pearl-gradient pearl-slow" /><div className="pearl-gradient pearl-fast" /></div>)}
                        <textarea ref={inputRef} value={inputValue} onChange={(e) => setInputValue(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleAction(); } }} placeholder={interactionState === 'AWAITING_REPLY' ? t('placeholderReplyAgent') : placeholder} className={cn("flex-1 bg-transparent border-none text-[#d1d1d1] text-[14px] outline-none resize-none min-h-[40px] max-h-[120px] leading-[1.6] relative z-10 transition-colors", interactionState === 'AWAITING_REPLY' ? "text-white placeholder:text-white/30" : "placeholder:text-[#555]")} />
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

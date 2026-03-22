// Public client entrypoint.
// Review docs/public-private-contract.md before coupling client behavior to private-only backend capabilities.
mod secure_store;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use serde_json::{json, Value};
use hstack_core::error::Error as CoreError;
use hstack_core::provider::gemini::generate_gemini_content;
use hstack_core::provider::openai_compat::generate_openai_content;
use hstack_core::provider::{Message, Role, ProviderConfig, Tool, ToolCall, ToolFunctionCall};
use hstack_core::chat::{chat_loop, ToolExecutor, ContextRefreshFn};
use hstack_core::settings::{SavedLocation, SavedProvider, SyncMode, UserSettings};
use hstack_core::ticket::{
    CommuteDepartureTime,
    tool_schemas,
    EventAttendanceStatus,
    HabitWorkflowStatus,
    TaskWorkflowStatus,
    TicketLocation,
    Ticket,
    TicketPayload,
    TicketPriority,
    TicketStatus,
};
use hstack_core::sync::{SyncAction, SyncActionType, project_state, reconcile_state};
use hstack_core::temporal_parser::parse_agent_rrule;
use secure_store::SecureStore;
use uuid::Uuid;
use chrono::{Utc, Local, Datelike};
use std::collections::{HashMap, HashSet};

const SYNC_TOKEN_KEY: &str = "hstack-sync-token";
const DEFAULT_COMMUTE_BUFFER_MINUTES: i64 = 10;

#[derive(serde::Serialize)]
struct SyncSessionInfo {
    user_id: Option<i64>,
    user_name: Option<String>,
    token: Option<String>,
}

fn parse_optional_deserialized_arg<T>(args: &Value, key: &str, label: &str) -> Result<Option<T>, String>
where
    T: for<'de> serde::Deserialize<'de>,
{
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| format!("invalid {} value", label)),
    }
}

fn parse_location_arg(args: &Value, key: &str, label: &str) -> Result<Option<TicketLocation>, String> {
    parse_optional_deserialized_arg::<TicketLocation>(args, key, label)
}

fn parse_departure_time_arg(args: &Value, key: &str, label: &str) -> Result<Option<CommuteDepartureTime>, String> {
    parse_optional_deserialized_arg::<CommuteDepartureTime>(args, key, label)
}

fn normalize_address_text_location(text: &str, label: &str) -> Result<TicketLocation, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(format!("{} must not be empty", label));
    }

    Ok(TicketLocation::AddressText {
        address: trimmed.to_string(),
        label: None,
    })
}

fn location_display_text(location: &TicketLocation) -> String {
    match location {
        TicketLocation::SavedLocation {
            location_id,
            label,
        } => label.clone().unwrap_or_else(|| location_id.clone()),
        TicketLocation::Coordinates {
            latitude,
            longitude,
            label,
        } => label.clone().unwrap_or_else(|| format!("{}, {}", latitude, longitude)),
        TicketLocation::AddressText { address, .. } => address.clone(),
        TicketLocation::PlaceId { label, place_id, .. } => label.clone().unwrap_or_else(|| place_id.clone()),
        TicketLocation::CurrentPosition { label } => label.clone().unwrap_or_else(|| "Current position".to_string()),
    }
}

fn normalize_location_key(text: &str) -> String {
    text.trim().to_lowercase()
}

fn find_saved_location_by_id<'a>(settings: &'a UserSettings, location_id: &str) -> Option<&'a SavedLocation> {
    settings.saved_locations.iter().find(|location| location.id == location_id)
}

fn find_saved_location_by_label<'a>(settings: &'a UserSettings, label: &str) -> Option<&'a SavedLocation> {
    let normalized = normalize_location_key(label);
    settings
        .saved_locations
        .iter()
        .find(|location| normalize_location_key(&location.label) == normalized)
}

fn is_ambiguous_location_text(text: &str) -> bool {
    matches!(
        normalize_location_key(text).as_str(),
        "home"
            | "my home"
            | "house"
            | "my house"
            | "my place"
            | "place"
            | "work"
            | "office"
            | "my office"
            | "gym"
            | "school"
            | "there"
            | "here"
    )
}

fn resolve_saved_location_reference(
    settings: &UserSettings,
    location_id: &str,
    label: Option<String>,
    field_label: &str,
) -> Result<(String, TicketLocation), String> {
    let saved_location = find_saved_location_by_id(settings, location_id)
        .ok_or_else(|| format!("unknown {} location_id '{}'", field_label, location_id))?;

    let resolved = match &saved_location.location {
        TicketLocation::SavedLocation { .. } => {
            return Err(format!("saved location '{}' must resolve to a concrete location", saved_location.label));
        }
        concrete => location_display_text(concrete),
    };

    Ok((
        resolved,
        TicketLocation::SavedLocation {
            location_id: location_id.to_string(),
            label: label.or_else(|| Some(saved_location.label.clone())),
        },
    ))
}

fn resolve_location_object(
    location: TicketLocation,
    settings: &UserSettings,
    field_label: &str,
) -> Result<(String, TicketLocation), String> {
    match location {
        TicketLocation::SavedLocation { location_id, label } => {
            resolve_saved_location_reference(settings, &location_id, label, field_label)
        }
        other => {
            let rendered = location_display_text(&other);
            if rendered.trim().is_empty() {
                return Err(format!("{} structured location must render to a non-empty value", field_label));
            }

            Ok((rendered, other))
        }
    }
}

fn format_saved_locations_for_prompt(saved_locations: &[SavedLocation]) -> String {
    if saved_locations.is_empty() {
        return "- None".to_string();
    }

    saved_locations
        .iter()
        .map(|saved_location| {
            let rendered = match &saved_location.location {
                TicketLocation::SavedLocation { location_id, .. } => location_id.clone(),
                concrete => location_display_text(concrete),
            };

            format!("- {} | {} | {}", saved_location.id, saved_location.label, rendered)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_event_location(args: &Value, settings: &UserSettings) -> Result<Option<TicketLocation>, String> {
    match parse_location_arg(args, "location", "event location")? {
        None => Ok(None),
        Some(location) => resolve_location_object(location, settings, "event location").map(|(_, location)| Some(location)),
    }
}

fn resolve_commute_location(
    args: &Value,
    object_key: &str,
    text_key: &str,
    label: &str,
    settings: &UserSettings,
) -> Result<(String, TicketLocation), String> {
    let text_value = args.get(text_key).and_then(Value::as_str).map(str::trim);
    let object_value = parse_location_arg(args, object_key, label)?;

    match (text_value, object_value) {
        (Some(text), Some(location)) => {
            if text.is_empty() {
                return Err(format!("{} text must not be empty", label));
            }

            let (rendered, normalized) = resolve_location_object(location, settings, label)?;
            let text_matches_saved_label = matches!(
                &normalized,
                TicketLocation::SavedLocation {
                    label: Some(saved_label),
                    ..
                } if saved_label == text
            );

            if rendered != text && !text_matches_saved_label {
                return Err(format!(
                    "{} text '{}' does not match structured location '{}'",
                    label,
                    text,
                    rendered
                ));
            }

            Ok((rendered, normalized))
        }
        (Some(text), None) => {
            if find_saved_location_by_label(settings, text).is_some() {
                return Err(format!(
                    "{} '{}' matches a saved location; use location_id instead of raw text",
                    label,
                    text
                ));
            }

            if is_ambiguous_location_text(text) {
                return Err(format!(
                    "{} '{}' is ambiguous; ask the user which saved place or concrete address they mean",
                    label,
                    text
                ));
            }

            let location = normalize_address_text_location(text, label)?;
            Ok((text.to_string(), location))
        }
        (None, Some(location)) => resolve_location_object(location, settings, label),
        (None, None) => Err(format!("missing {}", label)),
    }
}

fn extract_rrule_days(rrule: &str) -> Option<String> {
    let rule_line = rrule.lines().find(|line| line.starts_with("RRULE:"))?;
    let byday = rule_line
        .trim_start_matches("RRULE:")
        .split(';')
        .find_map(|segment| segment.strip_prefix("BYDAY="))?;

    let normalized = byday
        .split(',')
        .filter_map(|token| match token {
            "MO" => Some("monday"),
            "TU" => Some("tuesday"),
            "WE" => Some("wednesday"),
            "TH" => Some("thursday"),
            "FR" => Some("friday"),
            "SA" => Some("saturday"),
            "SU" => Some("sunday"),
            _ => None,
        })
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.join(","))
    }
}

fn deadline_from_scheduled_time(scheduled_time_iso: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(scheduled_time_iso)
        .ok()
        .map(|value| value.with_timezone(&Local).format("%H:%M").to_string())
}

fn infer_commute_payload_from_event(event_id: &str, payload: &TicketPayload) -> Option<TicketPayload> {
    let TicketPayload::Event {
        title,
        scheduled_time_iso,
        rrule,
        location,
        ..
    } = payload else {
        return None;
    };

    if scheduled_time_iso.is_none() && rrule.is_none() {
        return None;
    }

    let destination_location = location.clone()?;
    if matches!(destination_location, TicketLocation::CurrentPosition { .. }) {
        return None;
    }

    let destination = location_display_text(&destination_location);
    if destination.trim().is_empty() {
        return None;
    }

    Some(TicketPayload::Commute {
        title: format!("Commute to {}", title),
        label: Some("event_commute".to_string()),
        origin: "Current position".to_string(),
        origin_location: Some(TicketLocation::CurrentPosition {
            label: Some("Current position".to_string()),
        }),
        destination,
        destination_location: Some(destination_location),
        departure_time: Some(CommuteDepartureTime::RelativeToArrival {
            buffer_minutes: DEFAULT_COMMUTE_BUFFER_MINUTES,
        }),
        scheduled_time_iso: scheduled_time_iso.clone(),
        rrule: rrule.clone(),
        deadline: scheduled_time_iso
            .as_deref()
            .and_then(deadline_from_scheduled_time),
        days: rrule.as_deref().and_then(extract_rrule_days),
        related_event_id: Some(event_id.to_string()),
        live: None,
        minutes_remaining: None,
        directions: None,
        priority: None,
        completed: Some(false),
    })
}

fn normalize_legacy_commute_payload(payload: &mut TicketPayload) {
    let TicketPayload::Commute {
        departure_time,
        scheduled_time_iso,
        rrule,
        ..
    } = payload else {
        return;
    };

    if departure_time.is_some() {
        return;
    }

    if scheduled_time_iso.is_none() && rrule.is_none() {
        return;
    }

    *departure_time = Some(CommuteDepartureTime::RelativeToArrival {
        buffer_minutes: DEFAULT_COMMUTE_BUFFER_MINUTES,
    });
}

fn normalize_projected_tasks(mut tasks: Vec<Ticket>) -> Vec<Ticket> {
    for ticket in &mut tasks {
        normalize_legacy_commute_payload(&mut ticket.payload);
    }

    tasks
}

fn find_related_commute_id(tasks: &[Ticket], event_id: &str) -> Option<String> {
    tasks.iter().find_map(|ticket| match &ticket.payload {
        TicketPayload::Commute {
            related_event_id: Some(related_event_id),
            ..
        } if related_event_id == event_id => Some(ticket.id.clone()),
        _ => None,
    })
}

async fn sync_inferred_event_commute(
    app: &AppHandle,
    event_id: &str,
    event_payload: Option<&TicketPayload>,
) -> Result<(), String> {
    let tasks = get_tasks(app.clone()).await.unwrap_or_default();
    let existing_commute_id = find_related_commute_id(&tasks, event_id);
    let inferred_payload = event_payload.and_then(|payload| infer_commute_payload_from_event(event_id, payload));

    match (existing_commute_id, inferred_payload) {
        (Some(commute_id), Some(payload)) => append_pending_action(
            app,
            SyncActionType::Update,
            commute_id,
            "COMMUTE".to_string(),
            Some(payload),
            None,
            None,
        ).await,
        (None, Some(payload)) => append_pending_action(
            app,
            SyncActionType::Create,
            Uuid::new_v4().to_string(),
            "COMMUTE".to_string(),
            Some(payload),
            Some(TicketStatus::Idle),
            None,
        ).await,
        (Some(commute_id), None) => append_pending_action(
            app,
            SyncActionType::Delete,
            commute_id,
            "COMMUTE".to_string(),
            None,
            None,
            None,
        ).await,
        (None, None) => Ok(()),
    }
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<UserSettings, String> {
    let store = match app.store("settings.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Settings store failure: {}", e)),
    };

    let settings_val = match store.get("user_settings") {
        Some(val) => val,
        None => json!(UserSettings::default()),
    };

    match serde_json::from_value(settings_val) {
        Ok(s) => Ok(s),
        Err(e) => Err(format!("Settings parse failure: {}", e)),
    }
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: UserSettings) -> Result<(), String> {
    let store = match app.store("settings.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Settings store failure: {}", e)),
    };

    store.set("user_settings", json!(settings));
    match store.save() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Settings save failure: {}", e)),
    }
}

#[tauri::command]
async fn upsert_provider(app: AppHandle, provider: SavedProvider, api_key: Option<String>) -> Result<(), String> {
    // 1. If API key is provided, save it to the app's secure store (Keychain/Keystore)
    if let Some(key) = api_key {
        SecureStore::set_key(&provider.id, &key)?;
    }

    // 2. Update settings.json
    let mut settings = get_settings(app.clone()).await?;
    
    if let Some(pos) = settings.providers.iter().position(|p| p.id == provider.id) {
        settings.providers[pos] = provider.clone();
    } else {
        settings.providers.push(provider.clone());
    }

    if settings.default_provider_id.is_none() {
        settings.default_provider_id = Some(provider.id.clone());
    }

    save_settings(app, settings).await
}

#[tauri::command]
async fn delete_provider(app: AppHandle, id: String) -> Result<(), String> {
    // 1. Delete from secure store
    let _ = SecureStore::delete_key(&id);

    // 2. Update settings.json
    let mut settings = get_settings(app.clone()).await?;
    settings.providers.retain(|p| p.id != id);
    
    if settings.default_provider_id.as_deref() == Some(&id) {
        settings.default_provider_id = settings.providers.first().map(|p| p.id.clone());
    }

    save_settings(app, settings).await
}

async fn append_pending_action(
    app: &AppHandle,
    action_type: SyncActionType,
    entity_id: String,
    entity_type: String,
    payload: Option<TicketPayload>,
    status: Option<TicketStatus>,
    notes: Option<String>,
) -> Result<(), String> {
    let store = match app.store("pending_actions.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("History store failure: {}", e)),
    };

    let mut actions: Vec<SyncAction> = match store.get("pending") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };

    actions.push(SyncAction {
        action_id: Uuid::new_v4().to_string(),
        r#type: action_type,
        entity_id,
        entity_type,
        status,
        payload,
        notes,
        timestamp: Utc::now().to_rfc3339(),
    });

    // Limit to last 50 actions to keep file size reasonable
    if actions.len() > 50 {
        actions.drain(0..actions.len() - 50);
    }

    store.set("pending", json!(actions));
    match store.save() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to save pending action: {}", e)),
    }
}

#[tauri::command]
async fn get_tasks(app: AppHandle) -> Result<Vec<Ticket>, String> {
    let base_store = match app.store("base_state.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Base state store failure: {}", e)),
    };
    let base_tickets: Vec<Ticket> = match base_store.get("tickets") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };

    let pending_store = match app.store("pending_actions.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Pending actions store failure: {}", e)),
    };
    let pending_actions: Vec<SyncAction> = match pending_store.get("pending") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };

    Ok(normalize_projected_tasks(project_state(base_tickets, &pending_actions)))
}

// Extract system prompt building into a separate function that can be called multiple times
async fn build_system_prompt_with_fresh_context(app: AppHandle) -> Result<String, String> {
    // Fetch current tasks for context
    let current_tasks = get_tasks(app.clone()).await.unwrap_or_default();
    let context_str = serde_json::to_string_pretty(&current_tasks).unwrap_or_else(|_| "[]".to_string());
    
    // Fetch recent actions from pending store for action history
    let pending_store = match app.store("pending_actions.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Pending actions store failure: {}", e)),
    };
    let pending_actions: Vec<SyncAction> = match pending_store.get("pending") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };
    
    let recent_actions_str = pending_actions.iter()
        .rev()
        .take(5)
        .map(|a| {
            let title = a.payload.as_ref()
                .map(|p| p.get_title())
                .unwrap_or("Unknown");
            format!("- {:?}: {} ({}) at {}", a.r#type, title, a.entity_type, a.timestamp)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Get local user context
    let now = Local::now();
    let local_time = now.format("%H:%M").to_string();
    let local_date = now.format("%Y-%m-%d").to_string();
    let weekday = now.weekday().to_string();
    let offset = now.offset().to_string();
    let settings = get_settings(app.clone()).await.unwrap_or_default();
    let saved_locations_str = format_saved_locations_for_prompt(&settings.saved_locations);

    let full_prompt = format!(
"
RECENT ACTIONS:
{}

CURRENT STACK:
{}

USER CONTEXT:
- Local Time: {}
- Local Date: {}
- Today is {}
- UTC Offset: {}

SAVED LOCATIONS:
{}

You are a ticket management assistant for HStack.
You manage a 'stack' of tickets for the user.

CRITICAL: TEMPORAL EXTRACTION - RRULE FORMAT (RFC 5545)
The `rrule` field MUST be a valid RRULE string following iCalendar RFC 5545.

RRULE STRUCTURE:
- DTSTART: Start datetime (YYYYMMDDTHHMMSSZ) - ALWAYS append 'Z' and calculate the UTC time using the user's offset.
- RRULE: Recurrence rule (optional, for repeating events)

COMMON PATTERNS:
- One-time tomorrow 9am: DTSTART:20260320T090000Z
- Every Monday 9am: DTSTART:20260324T090000Z RRULE:FREQ=WEEKLY;BYDAY=MO
- Daily 9am: DTSTART:20260320T090000Z RRULE:FREQ=DAILY

RULES:
1. Calculate DTSTART in UTC based on the user's relative date and local timezone offset.
2. Any time-bearing ticket may use the `rrule` field. Use `DTSTART:...` for one-time scheduling, and add `RRULE:...` when the ticket repeats.
3. Separate DTSTART and RRULE with a space.

GROUNDING AND PROVENANCE RULES:
1. Only use names, dates, places, and facts that appear in the current user message, CURRENT STACK, RECENT ACTIONS, or earlier turns in this session.
2. Never invent missing specifics. If a ticket update requires a concrete value and the user did not provide it, ask a clarification question unless a planning default below clearly applies.
3. If the user asks where a fact came from, answer only with grounded provenance. If you inferred something incorrectly, say so plainly instead of claiming the user provided it.
4. If the place is one of SAVED LOCATIONS, use the saved location reference with its location_id.
5. If the place is not saved, only use a concrete non-ambiguous address or clear place name.
6. If the place is ambiguous, such as 'my place', 'home', or 'work', ask a clarification question instead of calling a tool.

PROACTIVE PLANNING RULES:
1. HStack should reduce the user's mental load. When the user asks you to \"be smart\", \"handle it\", or otherwise implies they want proactive planning, choose a sensible default instead of bouncing the decision back.
2. If the user wants a task done before a known dated event, prefer adding or editing the ticket's schedule instead of only writing that constraint in notes.
3. Default planning time for a one-time task with a date but no explicit time is 10:00 local time.
4. If the user asks for something before a known dated event and gives no time, schedule it for 10:00 local time on the previous day unless that would already be in the past; if it would be in the past, choose the nearest reasonable future time that still satisfies the request.
5. When you apply a planning default, mention the assumption briefly in your natural-language response.
6. If the user mentions a concrete future commitment that affects planning, such as a meeting, workout, yoga session, appointment, dinner, or trip, and it is not already in CURRENT STACK, add it as its own ticket as well as updating the original task when appropriate.
7. Use EVENT for a specific scheduled commitment like \"yoga from 10 to 12\". Include the date/time in `rrule` or `DTSTART`, and include duration when the user gave a time range.

CRITICAL: TOOL CALLING RULES
1. ALWAYS use tools for state changes - never just describe what you would do.
2. Extract parameters EXACTLY from user input - don't invent values.
3. When editing, only include fields that need to change.
4. If multiple actions needed, make multiple tool calls.
5. If a request implies a scheduling change, update the ticket's `rrule`/`DTSTART` rather than burying the timing in notes.
6. **IF A TOOL CALL FAILS**: You will see a ⚠️ error message. Read it, correct the arguments, and retry only if you can improve them.

TICKET TYPES:
 HABIT: Routines and recurring commitments. Can use `rrule`.
 TASK: Actions or reminders. Can use `rrule` for one-time or repeating scheduling when the user gives a date/time.
 EVENT: Time-specific appointments, gatherings, meetings, and calendar-like items. Can use `rrule` for one-time or repeating scheduling.

TOOL EXAMPLES:
- create_ticket: {{\"type\": \"TASK\", \"title\": \"Walk the dog\", \"rrule\": \"DTSTART:20260320T140000Z\"}}
- edit_ticket: {{\"task_id\": \"uuid\", \"rrule\": \"DTSTART:20260322T090000Z\"}}
- edit_ticket: {{\"task_id\": \"uuid\", \"title\": \"Walk the dog (Jimbo)\"}}
- edit_ticket: {{\"task_id\": \"uuid\", \"rrule\": \"DTSTART:20260326T100000Z\"}}
- create_ticket: {{\"type\": \"EVENT\", \"title\": \"Yoga\", \"rrule\": \"DTSTART:20260326T100000Z\", \"duration_minutes\": 120}}
- create_ticket: {{\"type\": \"HABIT\", \"title\": \"Morning workout\", \"rrule\": \"DTSTART:20260320T070000Z RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\"}}
- create_ticket: {{\"type\": \"EVENT\", \"title\": \"Jimbo birthday party\", \"rrule\": \"DTSTART:20260327T190000Z\"}}
- create_ticket: {{\"type\": \"EVENT\", \"title\": \"Weekly team standup\", \"rrule\": \"DTSTART:20260324T083000Z RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\"}}
- create_ticket: {{\"type\": \"EVENT\", \"title\": \"Dinner at home\", \"location\": {{\"location_type\": \"saved_location\", \"location_id\": \"loc-home\"}}}}

REMEMBER:
- NO EMOJIS in titles.
- Keep titles clean and concise.
- Date qualifiers and times belong in `rrule`/`DTSTART`, not in the title.",
        recent_actions_str,
        context_str,
        local_time,
        local_date,
        weekday,
        offset,
        saved_locations_str
    );

    Ok(full_prompt)
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PlannerCommitment {
    r#type: Option<String>,
    title: Option<String>,
    rrule: Option<String>,
    duration_minutes: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PlannerDependencyImpact {
    ticket_id: String,
    title: Option<String>,
    reason: String,
    action_required: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PlannerAction {
    tool: String,
    arguments: Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PlannerPlan {
    user_goal: String,
    grounded_facts: Vec<String>,
    time_constraints: Vec<String>,
    existing_tickets_relevant: Vec<String>,
    dependent_tickets_impacted: Vec<PlannerDependencyImpact>,
    new_commitments_detected: Vec<PlannerCommitment>,
    proactive_opportunities: Vec<String>,
    assumptions_to_apply: Vec<String>,
    tool_actions: Vec<PlannerAction>,
    user_reply_strategy: String,
}

async fn build_planner_prompt(app: AppHandle, tools: &[Tool]) -> Result<String, String> {
    let current_tasks = get_tasks(app.clone()).await.unwrap_or_default();
    let context_str = serde_json::to_string_pretty(&current_tasks).unwrap_or_else(|_| "[]".to_string());

    let pending_store = match app.store("pending_actions.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Pending actions store failure: {}", e)),
    };
    let pending_actions: Vec<SyncAction> = match pending_store.get("pending") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };

    let recent_actions_str = pending_actions.iter()
        .rev()
        .take(5)
        .map(|a| {
            let title = a.payload.as_ref()
                .map(|p| p.get_title())
                .unwrap_or("Unknown");
            format!("- {:?}: {} ({}) at {}", a.r#type, title, a.entity_type, a.timestamp)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let now = Local::now();
    let local_time = now.format("%H:%M").to_string();
    let local_date = now.format("%Y-%m-%d").to_string();
    let weekday = now.weekday().to_string();
    let offset = now.offset().to_string();
    let settings = get_settings(app.clone()).await.unwrap_or_default();
    let saved_locations_str = format_saved_locations_for_prompt(&settings.saved_locations);
    let tools_str = serde_json::to_string_pretty(tools).unwrap_or_else(|_| "[]".to_string());

    Ok(format!(
"You are the HStack planning engine. Output ONLY valid JSON. Do not output prose, markdown, or code fences.

USER CONTEXT:
- Local Time: {}
- Local Date: {}
- Today is {}
- UTC Offset: {}

SAVED LOCATIONS:
{}

CURRENT STACK:
{}

RECENT ACTIONS:
{}

AVAILABLE TOOLS:
{}

Return exactly this JSON shape:
{{
  \"user_goal\": \"string\",
  \"grounded_facts\": [\"string\"],
  \"time_constraints\": [\"string\"],
  \"existing_tickets_relevant\": [\"string\"],
    \"dependent_tickets_impacted\": [{{
        \"ticket_id\": \"string\",
        \"title\": \"string|null\",
        \"reason\": \"string\",
        \"action_required\": true
    }}],
  \"new_commitments_detected\": [{{
    \"type\": \"EVENT|TASK|HABIT|null\",
    \"title\": \"string|null\",
    \"rrule\": \"string|null\",
    \"duration_minutes\": 0
  }}],
  \"proactive_opportunities\": [\"string\"],
  \"assumptions_to_apply\": [\"string\"],
  \"tool_actions\": [{{
    \"tool\": \"valid tool name\",
    \"arguments\": {{}}
  }}],
  \"user_reply_strategy\": \"string\"
}}

Planning rules:
1. Use only grounded facts from the current user message, prior conversation turns, CURRENT STACK, or RECENT ACTIONS.
2. Never invent names, dates, or provenance.
3. Prefer proactive planning that reduces the user's mental load.
4. If a user mentions a concrete future commitment that affects planning and it is not already in CURRENT STACK, include a create_ticket action for it when appropriate.
5. If a user wants something done before a known dated event, prefer scheduling the task rather than writing the constraint in notes.
6. Default one-time planning time is 10:00 local when a date is known but no time is given.
7. If the user gives a blocking time range on that date, schedule around it and add the blocking commitment itself when it is concrete and future-facing.
8. When moving or rescheduling a dated ticket, inspect CURRENT STACK for dependent tickets whose purpose or timing is anchored to that ticket, person, or occasion, and record them in dependent_tickets_impacted.
9. If a dependent ticket would become misaligned after the change, set action_required to true and include the necessary edit_ticket action in tool_actions.
10. tool_actions must use only AVAILABLE TOOLS and arguments must match the tool schema exactly.
11. If no tool action is needed, return an empty tool_actions array.
12. If a place matches SAVED LOCATIONS, use its location_id instead of raw text.
13. If a place is ambiguous, do not emit a tool action for it; require a clarification question in user_reply_strategy.",
        local_time,
        local_date,
        weekday,
        offset,
        saved_locations_str,
        context_str,
        recent_actions_str,
        tools_str,
    ))
}

fn extract_first_json_value(content: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        return Some(value);
    }

    let trimmed = content.trim();
    if let Some(stripped) = trimmed.strip_prefix("```") {
        let without_lang = if let Some(newline_idx) = stripped.find('\n') {
            &stripped[newline_idx + 1..]
        } else {
            stripped
        };
        if let Some(end_idx) = without_lang.rfind("```") {
            let candidate = &without_lang[..end_idx].trim();
            if let Ok(value) = serde_json::from_str::<Value>(candidate) {
                return Some(value);
            }
        }
    }

    if let Some(start) = content.find('{') {
        if let Some(end) = content.rfind('}') {
            if end > start {
                let candidate = &content[start..=end];
                if let Ok(value) = serde_json::from_str::<Value>(candidate) {
                    return Some(value);
                }
            }
        }
    }

    None
}

fn has_matching_edit_action(plan: &PlannerPlan, ticket_id: &str) -> bool {
    plan.tool_actions.iter().any(|action| {
        action.tool == "edit_ticket"
            && action.arguments.get("task_id").and_then(Value::as_str) == Some(ticket_id)
    })
}

fn validate_plan(plan: PlannerPlan, tools: &[Tool]) -> Result<PlannerPlan, String> {
    if plan.user_goal.trim().is_empty() {
        return Err("planner returned an empty user_goal".to_string());
    }

    if plan.user_reply_strategy.trim().is_empty() {
        return Err("planner returned an empty user_reply_strategy".to_string());
    }

    if !plan.tool_actions.is_empty() && plan.grounded_facts.is_empty() {
        return Err("planner proposed tool actions without grounded facts".to_string());
    }

    if plan.grounded_facts.iter().any(|fact| fact.trim().is_empty()) {
        return Err("planner returned an empty grounded fact".to_string());
    }

    if plan.time_constraints.iter().any(|constraint| constraint.trim().is_empty()) {
        return Err("planner returned an empty time constraint".to_string());
    }

    if plan.existing_tickets_relevant.iter().any(|ticket| ticket.trim().is_empty()) {
        return Err("planner returned an empty relevant-ticket reference".to_string());
    }

    if plan.proactive_opportunities.iter().any(|opportunity| opportunity.trim().is_empty()) {
        return Err("planner returned an empty proactive opportunity".to_string());
    }

    if plan.assumptions_to_apply.iter().any(|assumption| assumption.trim().is_empty()) {
        return Err("planner returned an empty assumption".to_string());
    }

    if plan.tool_actions.len() > 8 {
        return Err("planner returned too many actions".to_string());
    }

    let mut seen_impacts = HashSet::new();
    for impact in &plan.dependent_tickets_impacted {
        if impact.ticket_id.trim().is_empty() {
            return Err("planner returned a dependent ticket with an empty ticket_id".to_string());
        }

        if impact.reason.trim().is_empty() {
            return Err(format!(
                "planner returned an empty dependency reason for ticket '{}'",
                impact.ticket_id
            ));
        }

        if let Some(title) = impact.title.as_deref() {
            if title.trim().is_empty() {
                return Err(format!(
                    "planner returned an empty dependency title for ticket '{}'",
                    impact.ticket_id
                ));
            }
        }

        if !seen_impacts.insert(impact.ticket_id.as_str()) {
            return Err(format!(
                "planner listed dependent ticket '{}' more than once",
                impact.ticket_id
            ));
        }
    }

    let mut seen_commitments = HashSet::new();
    for commitment in &plan.new_commitments_detected {
        let title = commitment.title.as_deref().map(str::trim);

        if title == Some("") {
            return Err("planner returned a commitment with an empty title".to_string());
        }

        if title.is_none()
            && (commitment.r#type.is_some() || commitment.rrule.is_some() || commitment.duration_minutes.is_some())
        {
            return Err("planner returned a commitment with scheduling details but no title".to_string());
        }

        if let Some(title) = title {
            let normalized_title = title.to_ascii_lowercase();
            if !seen_commitments.insert(normalized_title.clone()) {
                return Err(format!(
                    "planner listed commitment '{}' more than once",
                    title
                ));
            }
        }
    }

    let tool_map: HashMap<&str, &Tool> = tools.iter()
        .map(|tool| (tool.function.name.as_str(), tool))
        .collect();

    for action in &plan.tool_actions {
        let tool = tool_map
            .get(action.tool.as_str())
            .ok_or_else(|| format!("planner used unknown tool '{}'", action.tool))?;

        let args = action.arguments.as_object()
            .ok_or_else(|| format!("planner arguments for '{}' must be a JSON object", action.tool))?;

        let schema = tool.function.parameters.as_object()
            .ok_or_else(|| format!("tool '{}' has invalid schema", action.tool))?;

        let allowed_keys: Vec<String> = schema.get("properties")
            .and_then(|v| v.as_object())
            .map(|props| props.keys().cloned().collect())
            .unwrap_or_default();

        for key in args.keys() {
            if !allowed_keys.iter().any(|allowed| allowed == key) {
                return Err(format!("planner used unsupported argument '{}' for tool '{}'", key, action.tool));
            }
        }

        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            for required_key in required.iter().filter_map(|v| v.as_str()) {
                if !args.contains_key(required_key) {
                    return Err(format!("planner omitted required argument '{}' for tool '{}'", required_key, action.tool));
                }
            }
        }

        if action.tool == "create_ticket" {
            let ticket_type = args.get("type").and_then(Value::as_str).unwrap_or_default();
            let has_duration = args.get("duration_minutes").and_then(Value::as_i64).is_some();
            let has_schedule = args.get("rrule").and_then(Value::as_str).is_some();

            if ticket_type == "EVENT" && has_duration && !has_schedule {
                return Err("planner created a timed EVENT without an rrule/DTSTART schedule".to_string());
            }
        }
    }

    for commitment in &plan.new_commitments_detected {
        let Some(title) = commitment.title.as_deref() else {
            continue;
        };

        let matching_create = plan.tool_actions.iter().find(|action| {
            action.tool == "create_ticket"
                && action.arguments.get("title").and_then(Value::as_str)
                    .map(|candidate| candidate.eq_ignore_ascii_case(title))
                    .unwrap_or(false)
        });

        if commitment.rrule.is_some() || commitment.duration_minutes.is_some() || commitment.r#type.is_some() {
            let action = matching_create.ok_or_else(|| {
                format!("planner detected commitment '{}' but did not create a matching ticket action", title)
            })?;

            if let Some(expected_type) = commitment.r#type.as_deref() {
                let actual_type = action.arguments.get("type").and_then(Value::as_str);
                if actual_type != Some(expected_type) {
                    return Err(format!(
                        "planner commitment '{}' expected type '{}' but create_ticket used '{:?}'",
                        title,
                        expected_type,
                        actual_type
                    ));
                }
            }

            if let Some(expected_rrule) = commitment.rrule.as_deref() {
                let actual_rrule = action.arguments.get("rrule").and_then(Value::as_str);
                if actual_rrule != Some(expected_rrule) {
                    return Err(format!(
                        "planner commitment '{}' expected schedule '{}' but create_ticket used '{:?}'",
                        title,
                        expected_rrule,
                        actual_rrule
                    ));
                }
            }

            if let Some(expected_duration) = commitment.duration_minutes {
                let actual_duration = action.arguments.get("duration_minutes").and_then(Value::as_i64);
                if actual_duration != Some(expected_duration) {
                    return Err(format!(
                        "planner commitment '{}' expected duration '{}' but create_ticket used '{:?}'",
                        title,
                        expected_duration,
                        actual_duration
                    ));
                }
            }
        }
    }

    for impact in &plan.dependent_tickets_impacted {
        let matching_edit = has_matching_edit_action(&plan, &impact.ticket_id);

        if impact.action_required && !matching_edit {
            return Err(format!(
                "planner marked dependent ticket '{}' as requiring action but did not include a matching edit_ticket action",
                impact.ticket_id
            ));
        }

        if !impact.action_required && matching_edit {
            return Err(format!(
                "planner edited dependent ticket '{}' without marking action_required=true",
                impact.ticket_id
            ));
        }
    }

    Ok(plan)
}

async fn generate_model_message(
    config: &ProviderConfig,
    messages: &[Message],
    tools: Option<&[Tool]>,
) -> Result<Message, CoreError> {
    match config.kind {
        hstack_core::provider::ProviderKind::OpenAiCompatible => generate_openai_content(config, messages, tools).await,
        hstack_core::provider::ProviderKind::Gemini => generate_gemini_content(config, messages, tools).await,
    }
}

async fn plan_actions(
    app: AppHandle,
    config: &ProviderConfig,
    history: &[Message],
    user_message: &str,
    tools: &[Tool],
) -> Result<Option<PlannerPlan>, String> {
    let planner_prompt = build_planner_prompt(app, tools).await?;
    let mut planner_messages = vec![Message {
        role: Role::System,
        content: Some(planner_prompt),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];

    let prior_messages: Vec<Message> = history.iter()
        .filter(|m| m.role != Role::System)
        .rev()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    planner_messages.extend(prior_messages);
    planner_messages.push(Message {
        role: Role::User,
        content: Some(user_message.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let planner_response = generate_model_message(config, &planner_messages, None)
        .await
        .map_err(|e| format!("planner generation failed: {}", e))?;

    let planner_content = planner_response.content.unwrap_or_default();
    let Some(json_value) = extract_first_json_value(&planner_content) else {
        println!("--- PLANNER OUTPUT COULD NOT BE PARSED AS JSON ---\n{}", planner_content);
        return Ok(None);
    };

    let parsed: PlannerPlan = match serde_json::from_value(json_value) {
        Ok(plan) => plan,
        Err(e) => {
            println!("--- PLANNER OUTPUT FAILED SCHEMA PARSE: {} ---", e);
            return Ok(None);
        }
    };

    match validate_plan(parsed, tools) {
        Ok(plan) => Ok(Some(plan)),
        Err(e) => {
            println!("--- PLANNER OUTPUT FAILED VALIDATION: {} ---", e);
            Ok(None)
        }
    }
}

fn build_planner_execution_note(plan: &PlannerPlan, tool_results: &[(PlannerAction, String)]) -> String {
    let dependency_lines = plan.dependent_tickets_impacted.iter()
        .map(|impact| {
            let title = impact.title.as_deref().unwrap_or("Unknown");
            format!(
                "- {} ({}) => {} [{}]",
                title,
                impact.ticket_id,
                impact.reason,
                if impact.action_required { "action required" } else { "info only" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let commitments = plan.new_commitments_detected.iter()
        .filter_map(|commitment| {
            commitment.title.as_ref().map(|title| {
                let kind = commitment.r#type.as_deref().unwrap_or("UNKNOWN");
                let timing = commitment.rrule.as_deref().unwrap_or("no schedule");
                let duration = commitment.duration_minutes
                    .map(|value| format!(", duration {} min", value))
                    .unwrap_or_default();
                format!("- {} ({}) @ {}{}", title, kind, timing, duration)
            })
        })
        .collect::<Vec<_>>()
        .join("\n");

    let action_lines = tool_results.iter()
        .map(|(action, result)| format!("- {} {} => {}", action.tool, action.arguments, result))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "PLANNER SUMMARY:\nGoal: {}\nGrounded facts:\n{}\nTime constraints:\n{}\nRelevant tickets:\n{}\nDependent tickets impacted:\n{}\nDetected commitments:\n{}\nProactive opportunities:\n{}\nAssumptions applied:\n{}\nExecuted actions:\n{}\nReply strategy: {}\nUse this summary and the refreshed stack to answer the user naturally. Do not call tools again.",
        plan.user_goal,
        if plan.grounded_facts.is_empty() { "- none".to_string() } else { plan.grounded_facts.iter().map(|fact| format!("- {}", fact)).collect::<Vec<_>>().join("\n") },
        if plan.time_constraints.is_empty() { "- none".to_string() } else { plan.time_constraints.iter().map(|constraint| format!("- {}", constraint)).collect::<Vec<_>>().join("\n") },
        if plan.existing_tickets_relevant.is_empty() { "- none".to_string() } else { plan.existing_tickets_relevant.iter().map(|ticket| format!("- {}", ticket)).collect::<Vec<_>>().join("\n") },
        if dependency_lines.is_empty() { "- none".to_string() } else { dependency_lines },
        if commitments.is_empty() { "- none".to_string() } else { commitments },
        if plan.proactive_opportunities.is_empty() { "- none".to_string() } else { plan.proactive_opportunities.iter().map(|item| format!("- {}", item)).collect::<Vec<_>>().join("\n") },
        if plan.assumptions_to_apply.is_empty() { "- none".to_string() } else { plan.assumptions_to_apply.iter().map(|item| format!("- {}", item)).collect::<Vec<_>>().join("\n") },
        if action_lines.is_empty() { "- none".to_string() } else { action_lines },
        plan.user_reply_strategy,
    )
}

#[tauri::command]
async fn chat_local(app: AppHandle, message: String, history: Vec<Message>) -> Result<Vec<Message>, String> {
    println!("--- CHAT LOCAL RECEIVED MESSAGE: {} ---", message);
    let settings = match get_settings(app.clone()).await {
        Ok(s) => s,
        Err(e) => return Err(e),
    };

    let active_provider = match settings.active_provider() {
        Some(p) => p,
        None => return Err("No active provider configured".to_string()),
    };
    
    // Resolve full config by fetching key from the app's secure store
    let api_key = SecureStore::get_key(&active_provider.id)?;
    let config = ProviderConfig {
        name: active_provider.name.clone(),
        kind: active_provider.kind.clone(),
        endpoint: active_provider.endpoint.clone(),
        api_key,
        model_name: active_provider.model_name.clone(),
        rate_limit: active_provider.rate_limit.clone(),
    };

    let mut messages = history.clone();
    
    // Inject system instructions if not present
    if !messages.iter().any(|m| m.role == Role::System) {
        let full_prompt = build_system_prompt_with_fresh_context(app.clone()).await?;
        
        messages.insert(0, Message {
            role: Role::System,
            content: Some(full_prompt),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    let initial_len = messages.len();
    
    messages.push(Message {
        role: Role::User,
        content: Some(message.clone()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let tools = tool_schemas();

    let app_clone = app.clone();
    let executor: ToolExecutor = Box::new(move |name, args| {
        println!("EXECUTING TOOL: {} with args: {:?}", name, args);
        let app = app_clone.clone();

        Box::pin(async move {
            let settings = get_settings(app.clone()).await.unwrap_or_default();

            match name.as_str() {
                "create_ticket" => {
                    let ticket_type_str = args.get("type").and_then(|v| v.as_str()).unwrap_or("TASK").to_uppercase();
                    let notes = args.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
                    let duration_minutes = args.get("duration_minutes").and_then(|v| v.as_i64());
                    let priority = match parse_optional_deserialized_arg::<TicketPriority>(&args, "priority", "priority") {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let task_status = match parse_optional_deserialized_arg::<TaskWorkflowStatus>(&args, "status", "task status") {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let event_status = match parse_optional_deserialized_arg::<EventAttendanceStatus>(&args, "status", "event status") {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let habit_status = match parse_optional_deserialized_arg::<HabitWorkflowStatus>(&args, "status", "habit status") {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let event_location = match resolve_event_location(&args, &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    
                    let mut scheduled_time_iso = None;
                    let mut rrule_out = None;

                    // Strict parsing for rrule
                    if let Some(rrule_input) = args.get("rrule").and_then(|v| v.as_str()) {
                        match parse_agent_rrule(rrule_input) {
                            Ok((start_datetime, rrule_str)) => {
                                scheduled_time_iso = Some(start_datetime.to_rfc3339());
                                rrule_out = rrule_str;
                            },
                            Err(e) => {
                                return Ok(format!("⚠️ Tool failed: {}. You must output a valid RFC 5545 string. Try again.", e));
                            }
                        }
                    }

                    let payload = match ticket_type_str.as_str() {
                        "HABIT" => TicketPayload::Habit {
                            title,
                            scheduled_time_iso,
                            rrule: rrule_out,
                            status: habit_status,
                            priority,
                            completed: Some(false),
                        },
                        "EVENT" => TicketPayload::Event {
                            title,
                            scheduled_time_iso,
                            rrule: rrule_out,
                            duration_minutes,
                            location: event_location,
                            status: event_status,
                            priority,
                            completed: Some(false),
                        },
                        _ => TicketPayload::Task {
                            title,
                            scheduled_time_iso,
                            rrule: rrule_out,
                            duration_minutes,
                            status: task_status,
                            priority,
                            completed: Some(false),
                        },
                    };

                    let entity_id = Uuid::new_v4().to_string();
                    let inferred_created_event = if ticket_type_str == "EVENT" {
                        Some((entity_id.clone(), payload.clone()))
                    } else {
                        None
                    };

                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        entity_id,
                        ticket_type_str.clone(),
                        Some(payload),
                        Some(TicketStatus::Idle),
                        notes,
                    ).await {
                        Ok(_) => {
                            if let Some((event_id, event_payload)) = inferred_created_event.as_ref() {
                                if let Err(e) = sync_inferred_event_commute(&app, event_id, Some(event_payload)).await {
                                    return Ok(format!("⚠️ Tool failed: {}.", e));
                                }
                            }
                            Ok("Ticket created successfully.".to_string())
                        }
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. The agent should analyze this error and try again with corrected parameters or a different approach. Details: {}", name, e)),
                    }
                }
                "delete_ticket" => {
                    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                    if task_id.is_empty() {
                        return Ok("⚠️ Tool failed: task_id missing. The agent should identify the correct task_id from the conversation context and try again.".to_string());
                    }
                    
                    let tasks = get_tasks(app.clone()).await.unwrap_or_default();
                    let actual_entity_type = tasks.iter()
                        .find(|t| t.id == task_id)
                        .map(|t| format!("{:?}", t.r#type).to_uppercase())
                        .unwrap_or_else(|| "TASK".to_string());

                    match append_pending_action(
                        &app,
                        SyncActionType::Delete,
                        task_id.to_string(),
                        actual_entity_type, 
                        None,
                        None,
                        None,
                    ).await {
                        Ok(_) => Ok("Ticket deleted.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. Details: {}", name, e)),
                    }
                }
                "delete_all_tickets" => {
                    let tasks = match get_tasks(app.clone()).await {
                        Ok(t) => t,
                        Err(e) => return Ok(format!("⚠️ Tool failed: Could not retrieve tasks: {}", e)),
                    };
                    
                    if tasks.is_empty() {
                        return Ok("⚠️ Tool failed: no tickets to delete.".to_string());
                    }
                    
                    if let Ok(store) = app.store("pending_actions.json") {
                        let mut actions: Vec<SyncAction> = store.get("pending")
                            .and_then(|val| serde_json::from_value(val).ok())
                            .unwrap_or_default();
                            
                        for task in tasks {
                            actions.push(SyncAction {
                                action_id: Uuid::new_v4().to_string(),
                                r#type: SyncActionType::Delete,
                                entity_id: task.id,
                                entity_type: format!("{:?}", task.r#type).to_uppercase(),
                                status: None,
                                payload: None,
                                notes: None,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            });
                        }
                        
                        store.set("pending", serde_json::json!(actions));
                        let _ = store.save();
                    }
                    
                    Ok("All tickets deleted.".to_string())
                }
                "edit_ticket" => {
                    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                    if task_id.is_empty() {
                        return Ok("⚠️ Tool failed: task_id missing.".to_string());
                    }

                    let tasks = get_tasks(app.clone()).await.unwrap_or_default();
                    let existing_ticket = tasks.iter().find(|t| t.id == task_id);
                    
                    let updated_entity_type = if let Some(new_type) = args.get("type").and_then(|v| v.as_str()) {
                        new_type.to_uppercase()
                    } else {
                        existing_ticket
                            .map(|t| format!("{:?}", t.r#type).to_uppercase())
                            .unwrap_or_else(|| "TASK".to_string())
                    };

                    let notes = args.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                    
                    // To do a generic edit cleanly since payload variants are strictly typed, 
                    // we can wrap the payload_updates in a Generic payload and the reconcile logic
                    // knows how to shallow merge a Generic into a Generic if it was Generic.
                    // But if base is strongly typed, SyncActionType::Update merging using Generic is weak.
                    // Instead, let's construct a TicketPayload::Generic just for the update,
                    // or serialize it out and merge.
                    let mut payload_updates = serde_json::Map::new();
                    if let Some(title) = args.get("title") { payload_updates.insert("title".to_string(), title.clone()); }
                    if let Some(duration_minutes) = args.get("duration_minutes") { payload_updates.insert("duration_minutes".to_string(), duration_minutes.clone()); }
                    
                    // Strict parsing for rrule in edits
                    if let Some(rrule_input) = args.get("rrule").and_then(|v| v.as_str()) {
                        match parse_agent_rrule(rrule_input) {
                            Ok((start_datetime, rrule_str)) => {
                                payload_updates.insert("scheduled_time_iso".to_string(), serde_json::json!(start_datetime.to_rfc3339()));
                                if let Some(rrule) = rrule_str {
                                    payload_updates.insert("rrule".to_string(), serde_json::json!(rrule));
                                } else {
                                    payload_updates.insert("rrule".to_string(), serde_json::Value::Null);
                                }
                            },
                            Err(e) => {
                                return Ok(format!("⚠️ Tool failed: {}. You must output a valid RFC 5545 string. Try again.", e));
                            }
                        }
                    }

                    if args.get("priority").is_some() {
                        let priority = match parse_optional_deserialized_arg::<TicketPriority>(&args, "priority", "priority") {
                            Ok(value) => value,
                            Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                        };
                        payload_updates.insert(
                            "priority".to_string(),
                            serde_json::to_value(priority).unwrap_or(Value::Null),
                        );
                    }

                    if args.get("status").is_some() {
                        let status_value = match updated_entity_type.as_str() {
                            "TASK" => match parse_optional_deserialized_arg::<TaskWorkflowStatus>(&args, "status", "task status") {
                                Ok(value) => serde_json::to_value(value).unwrap_or(Value::Null),
                                Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                            },
                            "EVENT" => match parse_optional_deserialized_arg::<EventAttendanceStatus>(&args, "status", "event status") {
                                Ok(value) => serde_json::to_value(value).unwrap_or(Value::Null),
                                Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                            },
                            "HABIT" => match parse_optional_deserialized_arg::<HabitWorkflowStatus>(&args, "status", "habit status") {
                                Ok(value) => serde_json::to_value(value).unwrap_or(Value::Null),
                                Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                            },
                            _ => return Ok("⚠️ Tool failed: status is currently only supported for TASK, EVENT, and HABIT tickets.".to_string()),
                        };
                        payload_updates.insert("status".to_string(), status_value);
                    }

                    if args.get("location").is_some() {
                        if updated_entity_type != "EVENT" {
                            return Ok("⚠️ Tool failed: location is currently only supported for EVENT tickets.".to_string());
                        }

                        let location = match resolve_event_location(&args, &settings) {
                            Ok(value) => value,
                            Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                        };

                        payload_updates.insert(
                            "location".to_string(),
                            serde_json::to_value(location).unwrap_or(Value::Null),
                        );
                    }

                    if args.get("departure_time").is_some() {
                        if updated_entity_type != "COMMUTE" {
                            return Ok("⚠️ Tool failed: departure_time is currently only supported for COMMUTE tickets.".to_string());
                        }

                        let departure_time = match parse_departure_time_arg(&args, "departure_time", "departure time") {
                            Ok(value) => value,
                            Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                        };

                        payload_updates.insert(
                            "departure_time".to_string(),
                            serde_json::to_value(departure_time).unwrap_or(Value::Null),
                        );
                    }
                    
                    let inferred_event_payload = if updated_entity_type == "EVENT" {
                        existing_ticket.and_then(|ticket| {
                            let mut projected = ticket.payload.clone();
                            projected.apply_partial_update(&payload_updates);
                            Some(projected)
                        })
                    } else {
                        None
                    };

                    match append_pending_action(
                        &app,
                        SyncActionType::Update,
                        task_id.to_string(),
                        updated_entity_type, 
                        if payload_updates.is_empty() { None } else { Some(TicketPayload::Generic(serde_json::Value::Object(payload_updates))) },
                        None,
                        notes,
                    ).await {
                        Ok(_) => {
                            if let Some(projected) = inferred_event_payload.as_ref() {
                                if let Err(e) = sync_inferred_event_commute(&app, task_id, Some(projected)).await {
                                    return Ok(format!("⚠️ Tool failed: {}.", e));
                                }
                            } else if existing_ticket.map(|ticket| matches!(ticket.r#type, hstack_core::ticket::TicketType::Event)).unwrap_or(false) {
                                if let Err(e) = sync_inferred_event_commute(&app, task_id, None).await {
                                    return Ok(format!("⚠️ Tool failed: {}.", e));
                                }
                            }
                            Ok("Ticket edited.".to_string())
                        }
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. Details: {}", name, e)),
                    }
                }
                "add_commute" => {
                    let label = args.get("label").and_then(|v| v.as_str()).unwrap_or("commute");
                    let deadline = args.get("deadline").and_then(|v| v.as_str()).unwrap_or("09:00");
                    let days = args.get("days").and_then(|v| v.as_str()).unwrap_or("monday,tuesday,wednesday,thursday,friday");
                    let (origin, origin_location) = match resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let (destination, destination_location) = match resolve_commute_location(&args, "destination_location", "destination", "destination location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let departure_time = match parse_departure_time_arg(&args, "departure_time", "departure time") {
                        Ok(Some(value)) => value,
                        Ok(None) => CommuteDepartureTime::RelativeToArrival {
                            buffer_minutes: DEFAULT_COMMUTE_BUFFER_MINUTES,
                        },
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    
                    let max_origin = std::cmp::min(15, origin.len());
                    let max_dest = std::cmp::min(15, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("{}: {}... -> {}... @ {}", label, &origin[..max_origin], &destination[..max_dest], deadline),
                        label: Some(label.to_string()),
                        origin,
                        origin_location: Some(origin_location),
                        destination,
                        destination_location: Some(destination_location),
                        departure_time: Some(departure_time),
                        scheduled_time_iso: None,
                        rrule: None,
                        deadline: Some(deadline.to_string()),
                        days: Some(days.to_string()),
                        related_event_id: None,
                        live: None,
                        minutes_remaining: None,
                        directions: None,
                        priority: None,
                        completed: Some(false),
                    };
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        Uuid::new_v4().to_string(),
                        "COMMUTE".to_string(),
                        Some(payload),
                        Some(TicketStatus::Idle),
                        None,
                    ).await {
                        Ok(_) => Ok("Commute registered.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. The agent should analyze this error and try again with corrected parameters or a different approach. Details: {}", name, e)),
                    }
                }
                "get_directions" => {
                    let (origin, origin_location) = match resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let (destination, destination_location) = match resolve_commute_location(&args, "destination_location", "destination", "destination location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    
                    let max_origin = std::cmp::min(15, origin.len());
                    let max_dest = std::cmp::min(15, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("Directions: {}... -> {}...", &origin[..max_origin], &destination[..max_dest]),
                        label: None,
                        origin,
                        origin_location: Some(origin_location),
                        destination,
                        destination_location: Some(destination_location),
                        departure_time: None,
                        scheduled_time_iso: None,
                        rrule: None,
                        deadline: None,
                        days: None,
                        related_event_id: None,
                        live: None,
                        minutes_remaining: None,
                        directions: Some(serde_json::json!({"steps": [], "total_duration": "Enriching via Server...", "total_duration_minutes": serde_json::Value::Null, "error": serde_json::Value::Null})),
                        priority: None,
                        completed: None,
                    };
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        Uuid::new_v4().to_string(),
                        "COMMUTE".to_string(),
                        Some(payload),
                        Some(TicketStatus::InFocus),
                        None,
                    ).await {
                        Ok(_) => Ok("Directions ticket created. Enrichment pending server sync.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. The agent should analyze this error and try again with corrected parameters or a different approach. Details: {}", name, e)),
                    }
                }
                "start_live_directions" => {
                    let minutes = args.get("minutes_until_deadline").and_then(|v| v.as_i64()).unwrap_or(30);
                    let (origin, origin_location) = match resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    let (destination, destination_location) = match resolve_commute_location(&args, "destination_location", "destination", "destination location", &settings) {
                        Ok(value) => value,
                        Err(e) => return Ok(format!("⚠️ Tool failed: {}.", e)),
                    };
                    
                    let max_dest = std::cmp::min(40, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("Trip to {}", &destination[..max_dest]),
                        label: None,
                        origin,
                        origin_location: Some(origin_location),
                        destination,
                        destination_location: Some(destination_location),
                        departure_time: None,
                        scheduled_time_iso: None,
                        rrule: None,
                        deadline: None,
                        days: None,
                        related_event_id: None,
                        live: Some(true),
                        minutes_remaining: Some(minutes),
                        directions: Some(serde_json::json!({"steps": [], "total_duration": "Enriching via Server...", "total_duration_minutes": serde_json::Value::Null, "error": serde_json::Value::Null})),
                        priority: None,
                        completed: None,
                    };
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        Uuid::new_v4().to_string(),
                        "COMMUTE".to_string(),
                        Some(payload),
                        Some(TicketStatus::InFocus),
                        None,
                    ).await {
                        Ok(_) => Ok("Live tracking created. Waiting on server sync for polling.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. The agent should analyze this error and try again with corrected parameters or a different approach. Details: {}", name, e)),
                    }
                }
                "create_countdown" => {
                    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Countdown");
                    let duration = args.get("duration_minutes").and_then(|v| v.as_i64()).unwrap_or(30);
                    
                    let expires_at = Utc::now() + chrono::Duration::minutes(duration);
                    
                    let payload = TicketPayload::Countdown {
                        title: title.to_string(),
                        duration_minutes: duration,
                        expires_at: Some(expires_at.to_rfc3339()),
                        priority: None,
                    };
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        Uuid::new_v4().to_string(),
                        "COUNTDOWN".to_string(),
                        Some(payload),
                        Some(TicketStatus::Idle),
                        None,
                    ).await {
                        Ok(_) => Ok("Countdown created locally.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. The agent should analyze this error and try again with corrected parameters or a different approach. Details: {}", name, e)),
                    }
                }
                _ => Ok(format!("⚠️ Tool failed: unknown tool '{}'. The agent should use only valid tool names from the available tool list.", name)),
            }
        })
    });

    // Create context refresh callback
    let app_clone2 = app.clone();
    let context_refresh: ContextRefreshFn = Box::new(move || {
        let app = app_clone2.clone();
        Box::pin(async move {
            build_system_prompt_with_fresh_context(app).await
                .map_err(|e| hstack_core::error::Error::Internal(e))
        })
    });

    if let Some(plan) = plan_actions(app.clone(), &config, &history, &message, &tools).await? {
        if !plan.tool_actions.is_empty() {
            println!("--- EXECUTING VALIDATED PLANNER ACTIONS ---");

            let synthetic_calls: Vec<ToolCall> = plan.tool_actions.iter().map(|action| ToolCall {
                id: Uuid::new_v4().to_string(),
                r#type: "function".to_string(),
                function: ToolFunctionCall {
                    name: action.tool.clone(),
                    arguments: serde_json::to_string(&action.arguments).unwrap_or_else(|_| "{}".to_string()),
                },
            }).collect();

            messages.push(Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(synthetic_calls.clone()),
                tool_call_id: None,
                name: None,
            });

            let mut tool_results: Vec<(PlannerAction, String)> = Vec::new();

            for (action, call) in plan.tool_actions.iter().cloned().zip(synthetic_calls.iter()) {
                let result = executor(call.function.name.clone(), action.arguments.clone()).await;
                let content = match result {
                    Ok(s) => s,
                    Err(e) => format!("Error executing tool: {:?}", e),
                };

                messages.push(Message {
                    role: Role::Tool,
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                    name: Some(call.function.name.clone()),
                });

                tool_results.push((action, content));
            }

            let planner_note = build_planner_execution_note(&plan, &tool_results);
            let mut refreshed_prompt = build_system_prompt_with_fresh_context(app.clone()).await?;
            refreshed_prompt.push_str("\n\n");
            refreshed_prompt.push_str(&planner_note);

            if let Some(system_msg_idx) = messages.iter().position(|m| m.role == Role::System) {
                messages[system_msg_idx] = Message {
                    role: Role::System,
                    content: Some(refreshed_prompt),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                };
            } else {
                messages.insert(0, Message {
                    role: Role::System,
                    content: Some(refreshed_prompt),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }

            match generate_model_message(&config, &messages, None).await {
                Ok(response) => {
                    messages.push(response);
                    return Ok(messages.split_off(initial_len));
                }
                Err(e) => {
                    messages.push(Message {
                        role: Role::Assistant,
                        content: Some(format!(
                            "I updated your stack based on the plan, but I couldn't generate the final response cleanly: {}",
                            e
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    return Ok(messages.split_off(initial_len));
                }
            }
        }
    }

    match chat_loop(&config, &mut messages, &tools, &executor, Some(&context_refresh)).await {
        Ok(_) => Ok(messages.split_off(initial_len)),
        Err(e) => {
            let err_msg = format!("Chat processing failed: {}", e);
            println!("--- {} ---", err_msg);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn apply_sync_update(app: AppHandle, new_base_tickets: Vec<Ticket>) -> Result<(), String> {
    let base_store = match app.store("base_state.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Base state store failure: {}", e)),
    };
    base_store.set("tickets", json!(new_base_tickets));
    let _ = base_store.save();

    let pending_store = match app.store("pending_actions.json") {
        Ok(s) => s,
        Err(e) => return Err(format!("Pending actions store failure: {}", e)),
    };

    let pending_actions: Vec<SyncAction> = match pending_store.get("pending") {
        Some(val) => serde_json::from_value(val).unwrap_or_default(),
        None => Vec::new(),
    };

    let remaining_actions = reconcile_state(&new_base_tickets, pending_actions);
    
    pending_store.set("pending", json!(remaining_actions));
    match pending_store.save() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to update pending actions after sync: {}", e)),
    }
}

#[tauri::command]
async fn get_user_locale(app: AppHandle) -> Result<(String, bool), String> {
    let settings = get_settings(app).await?;
    let locale = settings.locale.unwrap_or_else(|| {
        // Default to en-US if not set
        "en-US".to_string()
    });
    let hour12 = settings.hour12.unwrap_or(true);
    Ok((locale, hour12))
}

#[tauri::command]
async fn get_sync_session(app: AppHandle) -> Result<SyncSessionInfo, String> {
    let settings = get_settings(app).await?;
    let token = SecureStore::get_key(SYNC_TOKEN_KEY)?;

    Ok(SyncSessionInfo {
        user_id: settings.sync_user_id,
        user_name: settings.sync_user_name,
        token: if token.is_empty() { None } else { Some(token) },
    })
}

#[tauri::command]
async fn save_sync_session(app: AppHandle, user_id: i64, user_name: String, token: String) -> Result<(), String> {
    SecureStore::set_key(SYNC_TOKEN_KEY, &token)?;

    let mut settings = get_settings(app.clone()).await?;
    settings.sync_user_id = Some(user_id);
    settings.sync_user_name = Some(user_name);
    save_settings(app, settings).await
}

#[tauri::command]
async fn clear_sync_session(app: AppHandle) -> Result<(), String> {
    let _ = SecureStore::delete_key(SYNC_TOKEN_KEY);

    let mut settings = get_settings(app.clone()).await?;
    settings.sync_user_id = None;
    settings.sync_user_name = None;
    save_settings(app, settings).await
}

#[tauri::command]
async fn complete_onboarding(app: AppHandle, mode: String) -> Result<(), String> {
    let mut settings = get_settings(app.clone()).await?;
    settings.onboarding_complete = true;
    settings.sync_mode = match mode.as_str() {
        "CloudOfficial" => SyncMode::CloudOfficial,
        "CloudCustom" => SyncMode::CloudCustom,
        _ => SyncMode::LocalOnly,
    };
    save_settings(app, settings).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            upsert_provider,
            delete_provider,
            chat_local,
            get_tasks,
            apply_sync_update,
            get_user_locale,
            get_sync_session,
            save_sync_session,
            clear_sync_session,
            complete_onboarding
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::Reopen { .. } => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::{extract_first_json_value, format_saved_locations_for_prompt, infer_commute_payload_from_event, normalize_legacy_commute_payload, resolve_commute_location, validate_plan, PlannerAction, PlannerCommitment, PlannerDependencyImpact, PlannerPlan};
    use hstack_core::settings::{SavedLocation, UserSettings};
    use hstack_core::ticket::{tool_schemas, TicketLocation, TicketPayload};
    use serde_json::json;

    fn settings_with_home() -> UserSettings {
        UserSettings {
            saved_locations: vec![SavedLocation {
                id: "loc-home".to_string(),
                label: "Home".to_string(),
                location: TicketLocation::AddressText {
                    address: "12 Rue de Rivoli, Paris".to_string(),
                    label: None,
                },
            }],
            ..UserSettings::default()
        }
    }

    fn sample_plan() -> PlannerPlan {
        PlannerPlan {
            user_goal: "Reschedule prep work before a birthday dinner".to_string(),
            grounded_facts: vec![
                "Birthday dinner is a dated event already in the stack".to_string(),
                "Buy flowers depends on that dinner happening on time".to_string(),
            ],
            time_constraints: vec!["Dinner is next Friday at 19:00".to_string()],
            existing_tickets_relevant: vec!["event-birthday-dinner".to_string(), "task-buy-flowers".to_string()],
            dependent_tickets_impacted: vec![PlannerDependencyImpact {
                ticket_id: "task-buy-flowers".to_string(),
                title: Some("Buy flowers".to_string()),
                reason: "It is anchored to the dinner date".to_string(),
                action_required: true,
            }],
            new_commitments_detected: vec![PlannerCommitment {
                r#type: Some("EVENT".to_string()),
                title: Some("Birthday dinner".to_string()),
                rrule: Some("DTSTART:20260320T190000Z".to_string()),
                duration_minutes: Some(120),
            }],
            proactive_opportunities: vec!["Move flower pickup earlier in the day".to_string()],
            assumptions_to_apply: vec!["Use the existing event as the anchor".to_string()],
            tool_actions: vec![
                PlannerAction {
                    tool: "create_ticket".to_string(),
                    arguments: json!({
                        "type": "EVENT",
                        "title": "Birthday dinner",
                        "rrule": "DTSTART:20260320T190000Z",
                        "duration_minutes": 120,
                    }),
                },
                PlannerAction {
                    tool: "edit_ticket".to_string(),
                    arguments: json!({
                        "task_id": "task-buy-flowers",
                        "rrule": "DTSTART:20260320T140000Z"
                    }),
                },
            ],
            user_reply_strategy: "Explain the reschedule and confirm the new sequence briefly.".to_string(),
        }
    }

    #[test]
    fn extracts_json_from_fenced_planner_output() {
        let parsed = extract_first_json_value("```json\n{\"user_goal\":\"Plan\"}\n```")
            .expect("expected fenced JSON to parse");

        assert_eq!(parsed.get("user_goal").and_then(|value| value.as_str()), Some("Plan"));
    }

    #[test]
    fn validates_dependency_aware_plan() {
        let plan = sample_plan();

        validate_plan(plan, &tool_schemas()).expect("expected valid planner plan");
    }

    #[test]
    fn rejects_tool_actions_without_grounded_facts() {
        let mut plan = sample_plan();
        plan.grounded_facts.clear();

        let error = validate_plan(plan, &tool_schemas()).expect_err("expected validation to fail");
        assert!(error.contains("grounded facts"));
    }

    #[test]
    fn rejects_commitment_details_without_title() {
        let mut plan = sample_plan();
        plan.new_commitments_detected[0].title = None;

        let error = validate_plan(plan, &tool_schemas()).expect_err("expected validation to fail");
        assert!(error.contains("no title"));
    }

    #[test]
    fn rejects_duplicate_dependent_ticket_entries() {
        let mut plan = sample_plan();
        plan.dependent_tickets_impacted.push(PlannerDependencyImpact {
            ticket_id: "task-buy-flowers".to_string(),
            title: Some("Buy flowers".to_string()),
            reason: "Duplicate reference".to_string(),
            action_required: false,
        });

        let error = validate_plan(plan, &tool_schemas()).expect_err("expected validation to fail");
        assert!(error.contains("more than once"));
    }

    #[test]
    fn rejects_edit_without_action_required_flag() {
        let mut plan = sample_plan();
        plan.dependent_tickets_impacted[0].action_required = false;

        let error = validate_plan(plan, &tool_schemas()).expect_err("expected validation to fail");
        assert!(error.contains("action_required=true"));
    }

    #[test]
    fn normalizes_text_commute_locations_to_address_text() {
        let settings = UserSettings::default();
        let args = json!({
            "origin": "221B Baker Street, London"
        });

        let (display, location) = resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings)
            .expect("expected strict location normalization to succeed");

        assert_eq!(display, "221B Baker Street, London");
        assert_eq!(location, TicketLocation::AddressText {
            address: "221B Baker Street, London".to_string(),
            label: None,
        });
    }

    #[test]
    fn rejects_mismatched_text_and_structured_commute_locations() {
        let settings = UserSettings::default();
        let args = json!({
            "origin": "Current position",
            "origin_location": {
                "location_type": "address_text",
                "address": "10 Downing Street, London"
            }
        });

        let error = resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings)
            .expect_err("expected mismatch to fail");

        assert!(error.contains("does not match structured location"));
    }

    #[test]
    fn rejects_saved_location_labels_as_raw_text() {
        let settings = settings_with_home();
        let args = json!({
            "origin": "Home"
        });

        let error = resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings)
            .expect_err("expected saved-location raw text to fail");

        assert!(error.contains("use location_id"));
    }

    #[test]
    fn rejects_ambiguous_raw_location_text() {
        let settings = UserSettings::default();
        let args = json!({
            "origin": "my place"
        });

        let error = resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings)
            .expect_err("expected ambiguous location text to fail");

        assert!(error.contains("ambiguous"));
    }

    #[test]
    fn formats_saved_locations_for_prompt_context() {
        let rendered = format_saved_locations_for_prompt(&settings_with_home().saved_locations);

        assert!(rendered.contains("loc-home"));
        assert!(rendered.contains("Home"));
        assert!(rendered.contains("12 Rue de Rivoli, Paris"));
    }

    #[test]
    fn infers_commute_from_scheduled_event_with_location() {
        let payload = TicketPayload::Event {
            title: "Team dinner".to_string(),
            scheduled_time_iso: Some("2026-03-28T19:30:00+00:00".to_string()),
            rrule: None,
            duration_minutes: Some(90),
            location: Some(TicketLocation::AddressText {
                address: "12 Rue de Rivoli, Paris".to_string(),
                label: Some("Restaurant".to_string()),
            }),
            status: None,
            priority: None,
            completed: Some(false),
        };

        let commute = infer_commute_payload_from_event("event-1", &payload)
            .expect("expected a commute to be inferred");

        match commute {
            TicketPayload::Commute {
                departure_time,
                destination,
                destination_location,
                related_event_id,
                scheduled_time_iso,
                ..
            } => {
                assert_eq!(destination, "12 Rue de Rivoli, Paris");
                assert_eq!(related_event_id.as_deref(), Some("event-1"));
                assert_eq!(scheduled_time_iso.as_deref(), Some("2026-03-28T19:30:00+00:00"));
                assert_eq!(serde_json::to_value(departure_time).unwrap(), json!({
                    "departure_type": "relative_to_arrival",
                    "buffer_minutes": 10
                }));
                assert!(matches!(destination_location, Some(TicketLocation::AddressText { .. })));
            }
            other => panic!("expected commute payload, got {:?}", other),
        }
    }

    #[test]
    fn does_not_infer_commute_without_structured_destination() {
        let payload = TicketPayload::Event {
            title: "Deep work".to_string(),
            scheduled_time_iso: Some("2026-03-28T09:00:00+00:00".to_string()),
            rrule: None,
            duration_minutes: Some(120),
            location: None,
            status: None,
            priority: None,
            completed: Some(false),
        };

        assert!(infer_commute_payload_from_event("event-2", &payload).is_none());
    }

    #[test]
    fn normalizes_legacy_scheduled_commute_to_relative_departure() {
        let mut payload = TicketPayload::Commute {
            title: "Commute to dinner".to_string(),
            label: Some("event_commute".to_string()),
            origin: "Current position".to_string(),
            origin_location: Some(TicketLocation::CurrentPosition {
                label: Some("Current position".to_string()),
            }),
            destination: "12 Rue de Rivoli, Paris".to_string(),
            destination_location: Some(TicketLocation::AddressText {
                address: "12 Rue de Rivoli, Paris".to_string(),
                label: None,
            }),
            departure_time: None,
            scheduled_time_iso: Some("2026-04-04T20:00:00+00:00".to_string()),
            rrule: None,
            deadline: Some("20:00".to_string()),
            days: None,
            related_event_id: Some("event-1".to_string()),
            live: None,
            minutes_remaining: None,
            directions: None,
            priority: None,
            completed: Some(false),
        };

        normalize_legacy_commute_payload(&mut payload);

        match payload {
            TicketPayload::Commute { departure_time, .. } => {
                assert_eq!(serde_json::to_value(departure_time).unwrap(), json!({
                    "departure_type": "relative_to_arrival",
                    "buffer_minutes": 10
                }));
            }
            other => panic!("expected commute payload, got {:?}", other),
        }
    }
}

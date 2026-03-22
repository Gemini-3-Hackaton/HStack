#![deny(clippy::unwrap_used, clippy::expect_used)]

// Public client entrypoint.
// Review docs/public-private-contract.md before coupling client behavior to private-only backend capabilities.
mod app_state;
mod location_utils;
mod planner_support;
mod secure_store;
mod sync_runtime;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use serde_json::Value;
use hstack_core::error::Error as CoreError;
use hstack_core::provider::gemini::generate_gemini_content;
use hstack_core::provider::openai_compat::generate_openai_content;
use hstack_core::provider::{Message, Role, ProviderConfig, Tool, ToolCall, ToolFunctionCall};
use hstack_core::chat::{chat_loop, ToolExecutor, ContextRefreshFn};
use hstack_core::ticket::{
    CommuteDepartureTime,
    tool_schemas,
    EventAttendanceStatus,
    HabitWorkflowStatus,
    TaskWorkflowStatus,
    TicketPayload,
    TicketPriority,
    TicketStatus,
};
use hstack_core::sync::{SyncAction, SyncActionType};
use hstack_core::temporal_parser::parse_agent_rrule;
use secure_store::SecureStore;
use sync_runtime::NativeSyncRuntimeState;
use uuid::Uuid;
use chrono::{Utc, Local, Datelike};
use crate::app_state::{get_settings, get_tickets};
pub(crate) use app_state::{append_pending_action, apply_sync_update_state, load_sync_session, load_tickets_state, SyncSessionInfo};
pub(crate) use location_utils::{
    DEFAULT_COMMUTE_BUFFER_MINUTES,
    find_related_commute_id,
    format_saved_locations_for_prompt,
    infer_commute_payload_from_event,
    parse_departure_time_arg,
    parse_optional_deserialized_arg,
    resolve_commute_location,
    resolve_event_location,
};
pub(crate) use planner_support::{
    build_planner_execution_note,
    extract_first_json_value,
    PlannerAction,
    PlannerPlan,
    validate_plan,
};

async fn sync_inferred_event_commute(
    app: &AppHandle,
    event_id: &str,
    event_payload: Option<&TicketPayload>,
) -> Result<(), String> {
    let tickets = get_tickets(app.clone()).await.unwrap_or_default();
    let existing_commute_id = find_related_commute_id(&tickets, event_id);
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

// Extract system prompt building into a separate function that can be called multiple times
async fn build_system_prompt_with_fresh_context(app: AppHandle) -> Result<String, String> {
    // Fetch current tasks for context
    let current_tickets = load_tickets_state(app.clone()).await.unwrap_or_default();
    let context_str = serde_json::to_string_pretty(&current_tickets).unwrap_or_else(|_| "[]".to_string());
    
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
- edit_ticket: {{\"ticket_id\": \"uuid\", \"rrule\": \"DTSTART:20260322T090000Z\"}}
- edit_ticket: {{\"ticket_id\": \"uuid\", \"title\": \"Walk the dog (Jimbo)\"}}
- edit_ticket: {{\"ticket_id\": \"uuid\", \"rrule\": \"DTSTART:20260326T100000Z\"}}
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

async fn build_planner_prompt(app: AppHandle, tools: &[Tool]) -> Result<String, String> {
    let current_tickets = load_tickets_state(app.clone()).await.unwrap_or_default();
    let context_str = serde_json::to_string_pretty(&current_tickets).unwrap_or_else(|_| "[]".to_string());

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
    let api_key = SecureStore::get_key(&app, &active_provider.id)?;
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
                    let ticket_id = args.get("ticket_id").and_then(|v| v.as_str()).unwrap_or("");
                    if ticket_id.is_empty() {
                        return Ok("⚠️ Tool failed: ticket_id missing. The agent should identify the correct ticket_id from the conversation context and try again.".to_string());
                    }
                    
                    let tickets = load_tickets_state(app.clone()).await.unwrap_or_default();
                    let actual_entity_type = tickets.iter()
                        .find(|t| t.id == ticket_id)
                        .map(|t| format!("{:?}", t.r#type).to_uppercase())
                        .unwrap_or_else(|| "TASK".to_string());

                    match append_pending_action(
                        &app,
                        SyncActionType::Delete,
                        ticket_id.to_string(),
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
                    let tickets = match load_tickets_state(app.clone()).await {
                        Ok(t) => t,
                        Err(e) => return Ok(format!("⚠️ Tool failed: Could not retrieve tasks: {}", e)),
                    };
                    
                    if tickets.is_empty() {
                        return Ok("⚠️ Tool failed: no tickets to delete.".to_string());
                    }
                    
                    if let Ok(store) = app.store("pending_actions.json") {
                        let mut actions: Vec<SyncAction> = store.get("pending")
                            .and_then(|val| serde_json::from_value(val).ok())
                            .unwrap_or_default();
                            
                        for ticket in tickets {
                            actions.push(SyncAction {
                                action_id: Uuid::new_v4().to_string(),
                                r#type: SyncActionType::Delete,
                                entity_id: ticket.id,
                                entity_type: format!("{:?}", ticket.r#type).to_uppercase(),
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
                    let ticket_id = args.get("ticket_id").and_then(|v| v.as_str()).unwrap_or("");
                    if ticket_id.is_empty() {
                        return Ok("⚠️ Tool failed: ticket_id missing.".to_string());
                    }

                    let tickets = load_tickets_state(app.clone()).await.unwrap_or_default();
                    let existing_ticket = tickets.iter().find(|t| t.id == ticket_id);
                    
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
                        ticket_id.to_string(),
                        updated_entity_type, 
                        if payload_updates.is_empty() { None } else { Some(TicketPayload::Generic(serde_json::Value::Object(payload_updates))) },
                        None,
                        notes,
                    ).await {
                        Ok(_) => {
                            if let Some(projected) = inferred_event_payload.as_ref() {
                                if let Err(e) = sync_inferred_event_commute(&app, ticket_id, Some(projected)).await {
                                    return Ok(format!("⚠️ Tool failed: {}.", e));
                                }
                            } else if existing_ticket.map(|ticket| matches!(ticket.r#type, hstack_core::ticket::TicketType::Event)).unwrap_or(false) {
                                if let Err(e) = sync_inferred_event_commute(&app, ticket_id, None).await {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = match tauri::Builder::default()
        .manage(NativeSyncRuntimeState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            app_state::get_settings,
            app_state::save_settings,
            app_state::upsert_provider,
            app_state::delete_provider,
            chat_local,
            app_state::get_tickets,
            app_state::apply_sync_update,
            app_state::get_user_locale,
            app_state::get_sync_session,
            app_state::save_sync_session,
            app_state::clear_sync_session,
            app_state::complete_onboarding,
            sync_runtime::start_native_sync,
            sync_runtime::stop_native_sync,
            sync_runtime::get_sync_connection_status,
            sync_runtime::queue_sync_action,
            sync_runtime::sync_refresh_now
        ])
        .build(tauri::generate_context!()) {
            Ok(app) => app,
            Err(error) => panic!("error while building tauri application: {error}"),
        };

    app.run(|app_handle, event| {
            #[cfg(desktop)]
            {
                if let tauri::RunEvent::Reopen { .. } = event {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }

            #[cfg(not(desktop))]
            {
                let _ = (&app_handle, &event);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{extract_first_json_value, format_saved_locations_for_prompt, infer_commute_payload_from_event, resolve_commute_location, validate_plan, PlannerAction, PlannerPlan};
    use crate::location_utils::normalize_legacy_commute_payload;
    use crate::planner_support::{PlannerCommitment, PlannerDependencyImpact};
    use hstack_core::settings::{SavedLocation, UserSettings};
    use hstack_core::ticket::{tool_schemas, TicketLocation, TicketPayload};
    use serde::Serialize;
    use serde_json::{json, Value};

    fn must_extract_json_value(input: &str) -> Value {
        match extract_first_json_value(input) {
            Some(value) => value,
            None => panic!("expected fenced JSON to parse"),
        }
    }

    fn assert_plan_is_valid(plan: PlannerPlan) {
        if let Err(error) = validate_plan(plan, &tool_schemas()) {
            panic!("expected valid planner plan: {error}");
        }
    }

    fn expect_plan_validation_error(plan: PlannerPlan) -> String {
        match validate_plan(plan, &tool_schemas()) {
            Ok(_) => panic!("expected validation to fail"),
            Err(error) => error,
        }
    }

    fn must_infer_commute_payload(event_id: &str, payload: &TicketPayload) -> TicketPayload {
        match infer_commute_payload_from_event(event_id, payload) {
            Some(commute) => commute,
            None => panic!("expected a commute to be inferred"),
        }
    }

    fn expect_commute_location_error(args: &Value, settings: &UserSettings) -> String {
        match resolve_commute_location(args, "origin_location", "origin", "origin location", settings) {
            Ok(_) => panic!("expected location resolution to fail"),
            Err(error) => error,
        }
    }

    fn must_json_value<T: Serialize>(value: T) -> Value {
        match serde_json::to_value(value) {
            Ok(json) => json,
            Err(error) => panic!("expected value to serialize in test: {error}"),
        }
    }

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
                        "ticket_id": "ticket-buy-flowers",
                        "rrule": "DTSTART:20260320T140000Z"
                    }),
                },
            ],
            user_reply_strategy: "Explain the reschedule and confirm the new sequence briefly.".to_string(),
        }
    }

    #[test]
    fn extracts_json_from_fenced_planner_output() {
        let parsed = must_extract_json_value("```json\n{\"user_goal\":\"Plan\"}\n```");

        assert_eq!(parsed.get("user_goal").and_then(|value| value.as_str()), Some("Plan"));
    }

    #[test]
    fn validates_dependency_aware_plan() {
        let plan = sample_plan();

        assert_plan_is_valid(plan);
    }

    #[test]
    fn rejects_tool_actions_without_grounded_facts() {
        let mut plan = sample_plan();
        plan.grounded_facts.clear();

        let error = expect_plan_validation_error(plan);
        assert!(error.contains("grounded facts"));
    }

    #[test]
    fn rejects_commitment_details_without_title() {
        let mut plan = sample_plan();
        plan.new_commitments_detected[0].title = None;

        let error = expect_plan_validation_error(plan);
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

        let error = expect_plan_validation_error(plan);
        assert!(error.contains("more than once"));
    }

    #[test]
    fn rejects_edit_without_action_required_flag() {
        let mut plan = sample_plan();
        plan.dependent_tickets_impacted[0].action_required = false;

        let error = expect_plan_validation_error(plan);
        assert!(error.contains("action_required=true"));
    }

    #[test]
    fn normalizes_text_commute_locations_to_address_text() {
        let settings = UserSettings::default();
        let args = json!({
            "origin": "221B Baker Street, London"
        });

        let (display, location) = match resolve_commute_location(&args, "origin_location", "origin", "origin location", &settings) {
            Ok(value) => value,
            Err(error) => panic!("expected strict location normalization to succeed: {error}"),
        };

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

        let error = expect_commute_location_error(&args, &settings);

        assert!(error.contains("does not match structured location"));
    }

    #[test]
    fn rejects_saved_location_labels_as_raw_text() {
        let settings = settings_with_home();
        let args = json!({
            "origin": "Home"
        });

        let error = expect_commute_location_error(&args, &settings);

        assert!(error.contains("use location_id"));
    }

    #[test]
    fn rejects_ambiguous_raw_location_text() {
        let settings = UserSettings::default();
        let args = json!({
            "origin": "my place"
        });

        let error = expect_commute_location_error(&args, &settings);

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

        let commute = must_infer_commute_payload("event-1", &payload);

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
                assert_eq!(must_json_value(departure_time), json!({
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
                assert_eq!(must_json_value(departure_time), json!({
                    "departure_type": "relative_to_arrival",
                    "buffer_minutes": 10
                }));
            }
            other => panic!("expected commute payload, got {:?}", other),
        }
    }
}

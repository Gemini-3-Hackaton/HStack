mod secure_store;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use serde_json::{json, Value};
use hstack_core::error::Error as CoreError;
use hstack_core::provider::gemini::generate_gemini_content;
use hstack_core::provider::openai_compat::generate_openai_content;
use hstack_core::provider::{Message, Role, ProviderConfig, Tool, ToolCall, ToolFunctionCall};
use hstack_core::chat::{chat_loop, ToolExecutor, ContextRefreshFn};
use hstack_core::settings::{UserSettings, SavedProvider, SyncMode};
use hstack_core::ticket::{tool_schemas, Ticket, TicketStatus, TicketPayload};
use hstack_core::sync::{SyncAction, SyncActionType, project_state, reconcile_state};
use hstack_core::temporal_parser::parse_agent_rrule;
use secure_store::SecureStore;
use uuid::Uuid;
use chrono::{Utc, Local, Datelike};
use std::collections::HashMap;

const SYNC_TOKEN_KEY: &str = "hstack-sync-token";

#[derive(serde::Serialize)]
struct SyncSessionInfo {
    user_id: Option<i64>,
    user_name: Option<String>,
    token: Option<String>,
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

    Ok(project_state(base_tickets, &pending_actions))
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

REMEMBER:
- NO EMOJIS in titles.
- Keep titles clean and concise.
- Date qualifiers and times belong in `rrule`/`DTSTART`, not in the title.",
        recent_actions_str,
        context_str,
        local_time,
        local_date,
        weekday,
        offset
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
    let tools_str = serde_json::to_string_pretty(tools).unwrap_or_else(|_| "[]".to_string());

    Ok(format!(
"You are the HStack planning engine. Output ONLY valid JSON. Do not output prose, markdown, or code fences.

USER CONTEXT:
- Local Time: {}
- Local Date: {}
- Today is {}
- UTC Offset: {}

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
11. If no tool action is needed, return an empty tool_actions array.",
        local_time,
        local_date,
        weekday,
        offset,
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

fn validate_plan(plan: PlannerPlan, tools: &[Tool]) -> Result<PlannerPlan, String> {
    if plan.tool_actions.len() > 8 {
        return Err("planner returned too many actions".to_string());
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
        if !impact.action_required {
            continue;
        }

        let matching_edit = plan.tool_actions.iter().find(|action| {
            action.tool == "edit_ticket"
                && action.arguments.get("task_id").and_then(Value::as_str) == Some(impact.ticket_id.as_str())
        });

        if matching_edit.is_none() {
            return Err(format!(
                "planner marked dependent ticket '{}' as requiring action but did not include a matching edit_ticket action",
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
            match name.as_str() {
                "create_ticket" => {
                    let ticket_type_str = args.get("type").and_then(|v| v.as_str()).unwrap_or("TASK").to_uppercase();
                    let notes = args.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
                    let duration_minutes = args.get("duration_minutes").and_then(|v| v.as_i64());
                    
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
                            completed: Some(false),
                        },
                        "EVENT" => TicketPayload::Event {
                            title,
                            scheduled_time_iso,
                            rrule: rrule_out,
                            duration_minutes,
                            completed: Some(false),
                        },
                        _ => TicketPayload::Task {
                            title,
                            scheduled_time_iso,
                            rrule: rrule_out,
                            duration_minutes,
                            completed: Some(false),
                        },
                    };

                    let entity_id = Uuid::new_v4().to_string();
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        entity_id,
                        ticket_type_str.clone(),
                        Some(payload),
                        Some(TicketStatus::Idle),
                        notes,
                    ).await {
                        Ok(_) => Ok("Ticket created successfully.".to_string()),
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

                    if let Some(dur) = args.get("duration_minutes") { payload_updates.insert("duration_minutes".to_string(), dur.clone()); }
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Update,
                        task_id.to_string(),
                        updated_entity_type, 
                        if payload_updates.is_empty() { None } else { Some(TicketPayload::Generic(serde_json::Value::Object(payload_updates))) },
                        None,
                        notes,
                    ).await {
                        Ok(_) => Ok("Ticket edited.".to_string()),
                        Err(e) => Ok(format!("⚠️ Tool failed: {}. Details: {}", name, e)),
                    }
                }
                "add_commute" => {
                    let label = args.get("label").and_then(|v| v.as_str()).unwrap_or("commute");
                    let origin = args.get("origin").and_then(|v| v.as_str()).unwrap_or("");
                    let destination = args.get("destination").and_then(|v| v.as_str()).unwrap_or("");
                    let deadline = args.get("deadline").and_then(|v| v.as_str()).unwrap_or("09:00");
                    let days = args.get("days").and_then(|v| v.as_str()).unwrap_or("monday,tuesday,wednesday,thursday,friday");
                    
                    let max_origin = std::cmp::min(15, origin.len());
                    let max_dest = std::cmp::min(15, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("{}: {}... -> {}... @ {}", label, &origin[..max_origin], &destination[..max_dest], deadline),
                        label: Some(label.to_string()),
                        origin: origin.to_string(),
                        destination: destination.to_string(),
                        deadline: Some(deadline.to_string()),
                        days: Some(days.to_string()),
                        live: None,
                        minutes_remaining: None,
                        directions: None,
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
                    let origin = args.get("origin").and_then(|v| v.as_str()).unwrap_or("");
                    let destination = args.get("destination").and_then(|v| v.as_str()).unwrap_or("");
                    
                    let max_origin = std::cmp::min(15, origin.len());
                    let max_dest = std::cmp::min(15, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("Directions: {}... -> {}...", &origin[..max_origin], &destination[..max_dest]),
                        label: None,
                        origin: origin.to_string(),
                        destination: destination.to_string(),
                        deadline: None,
                        days: None,
                        live: None,
                        minutes_remaining: None,
                        directions: Some(serde_json::json!({"steps": [], "total_duration": "Enriching via Server...", "error": serde_json::Value::Null})),
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
                    let origin = args.get("origin").and_then(|v| v.as_str()).unwrap_or("");
                    let destination = args.get("destination").and_then(|v| v.as_str()).unwrap_or("");
                    let minutes = args.get("minutes_until_deadline").and_then(|v| v.as_i64()).unwrap_or(30);
                    
                    let max_dest = std::cmp::min(40, destination.len());
                    
                    let payload = TicketPayload::Commute {
                        title: format!("Trip to {}", &destination[..max_dest]),
                        label: None,
                        origin: origin.to_string(),
                        destination: destination.to_string(),
                        deadline: None,
                        days: None,
                        live: Some(true),
                        minutes_remaining: Some(minutes),
                        directions: Some(serde_json::json!({"steps": [], "total_duration": "Enriching via Server...", "error": serde_json::Value::Null})),
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

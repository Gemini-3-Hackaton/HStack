mod secure_store;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use serde_json::{json, Value};
use hstack_core::provider::{Message, Role, ProviderConfig};
use hstack_core::chat::{chat_loop, ToolExecutor, ContextRefreshFn};
use hstack_core::settings::{UserSettings, SavedProvider};
use hstack_core::ticket::{tool_schemas, Ticket, TicketStatus};
use hstack_core::sync::{SyncAction, SyncActionType, project_state, reconcile_state};
use hstack_core::temporal_parser::parse_agent_rrule;
use secure_store::SecureStore;
use uuid::Uuid;
use chrono::{Utc, Local, Datelike};

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
    payload: Option<Value>,
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
                .and_then(|p| p.get("title"))
                .and_then(|v| v.as_str())
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
2. Include RRULE only for recurring events (HABIT type).
3. Separate DTSTART and RRULE with a space.

CRITICAL: TOOL CALLING RULES
1. ALWAYS use tools for state changes - never just describe what you would do.
2. Extract parameters EXACTLY from user input - don't invent values.
3. When editing, only include fields that need to change.
4. If multiple actions needed, make multiple tool calls.
4. **IF A TOOL CALL FAILS**: You will see a ⚠️ error message. Read it and try again.

TICKET TYPES:
- HABIT: Recurring routines (uses rrule field).
- TASK: One-off actions or things to do (most common).
- EVENT: Time-specific appointments with location/context.

TOOL EXAMPLES:
- create_ticket: {{\"type\": \"TASK\", \"title\": \"Walk the dog\", \"rrule\": \"DTSTART:20260320T140000Z\"}}
- edit_ticket: {{\"task_id\": \"uuid\", \"rrule\": \"DTSTART:20260322T090000Z\"}}
- create_ticket: {{\"type\": \"HABIT\", \"title\": \"Morning workout\", \"rrule\": \"DTSTART:20260320T070000Z RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\"}}

REMEMBER:
- NO EMOJIS in titles.
- Keep titles clean and concise.
- Date qualifiers (tomorrow, today, Monday) go in scheduled_time, not the title.",
        recent_actions_str,
        context_str,
        local_time,
        local_date,
        weekday,
        offset
    );

    Ok(full_prompt)
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
        content: Some(message),
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
                    let ticket_type_str = args.get("type").and_then(|v| v.as_str()).unwrap_or("TASK");
                    let notes = args.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let mut payload = serde_json::json!({ "completed": false });
                    
                    if let Some(obj) = payload.as_object_mut() {
                        if let Some(args_obj) = args.as_object() {
                            for (k, v) in args_obj {
                                if k != "type" && k != "notes" && k != "rrule" {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }

                    // Strict parsing for rrule
                    if let Some(rrule_input) = args.get("rrule").and_then(|v| v.as_str()) {
                        match parse_agent_rrule(rrule_input) {
                            Ok((start_datetime, rrule_str)) => {
                                if let Some(obj) = payload.as_object_mut() {
                                    obj.insert("scheduled_time_iso".to_string(), json!(start_datetime.to_rfc3339()));
                                    if let Some(rrule) = rrule_str {
                                        obj.insert("rrule".to_string(), json!(rrule));
                                    }
                                }
                            },
                            Err(e) => {
                                return Ok(format!("⚠️ Tool failed: {}. You must output a valid RFC 5545 string. Try again.", e));
                            }
                        }
                    }

                    let entity_id = Uuid::new_v4().to_string();
                    match append_pending_action(
                        &app,
                        SyncActionType::Create,
                        entity_id,
                        ticket_type_str.to_uppercase(),
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
                        if payload_updates.is_empty() { None } else { Some(serde_json::Value::Object(payload_updates)) },
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
                    
                    let payload = serde_json::json!({
                        "title": format!("{}: {}... -> {}... @ {}", label, &origin[..max_origin], &destination[..max_dest], deadline),
                        "label": label,
                        "origin": origin,
                        "destination": destination,
                        "deadline": deadline,
                        "days": days,
                        "completed": false
                    });
                    
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
                    
                    let payload = serde_json::json!({
                        "title": format!("Directions: {}... -> {}...", &origin[..max_origin], &destination[..max_dest]),
                        "origin": origin,
                        "destination": destination,
                        "directions": {"steps": [], "total_duration": "Enriching via Server...", "error": serde_json::Value::Null}
                    });
                    
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
                    
                    let payload = serde_json::json!({
                        "title": format!("Trip to {}", &destination[..max_dest]),
                        "origin": origin,
                        "destination": destination,
                        "live": true,
                        "minutes_remaining": minutes,
                        "directions": {"steps": [], "total_duration": "Enriching via Server...", "error": serde_json::Value::Null}
                    });
                    
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
                    
                    let payload = serde_json::json!({
                        "title": title,
                        "duration_minutes": duration,
                        "expires_at": expires_at.to_rfc3339()
                    });
                    
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
            get_user_locale
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

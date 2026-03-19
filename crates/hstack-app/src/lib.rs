mod secure_store;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use serde_json::{json, Value};
use hstack_core::provider::{Message, Role, ProviderConfig};
use hstack_core::chat::{chat_loop, ToolExecutor};
use hstack_core::settings::{UserSettings, SavedProvider};
use hstack_core::ticket::{tool_schemas, Ticket, TicketStatus};
use hstack_core::sync::{SyncAction, SyncActionType, project_state, reconcile_state};
use secure_store::SecureStore;
use uuid::Uuid;
use chrono::Utc;

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

    let mut messages = history.clone();
    
    // Inject system instructions if not present
    if !messages.iter().any(|m| m.role == Role::System) {
        let full_prompt = format!(
            "You are a ticket management assistant for HStack.
You manage a 'stack' of tickets for the user. 

CRITICAL: MEMORY & CONTEXT MANAGEMENT
1. SHORT-TERM CONTEXT: You have access to the recent conversation history. Use it to remember user clarifications.
2. REASONING & CLARIFICATION:
   - ALWAYS REASON BEFORE ACTING. Explain your intent briefly in text.
   - If a request is AMBIGUOUS or lacks details, ASK the user clarifying questions before calling tools.
   - Avoid repetitive tool calls. Check RECENT ACTIONS before performing an operation.
3. FAT TICKETS: Each ticket has a `notes` field. Use it for ticket-specific research or context.
4. SYSTEM PROFILE: Use the ticket with title 'SYSTEM_PROFILE' (type: TASK) for persistent global user memory.

CRITICAL: TEMPORAL EXTRACTION & NORMALIZATION
1. EXTRACT scheduled_time: Mandate extraction of any temporal data (e.g. 15:00).
2. NORMALIZE scheduled_time: Always convert to HH:MM (24-hour) e.g. \"08:00\".
3. SCHEDULING DSL for HABITs: Use `recurrence` (e.g., `WEEKDAYS`, `MON, WED, FRI`).

RECENT ACTIONS PERFORMED:
{}

CURRENT TICKET STACK (JSON):
{}

TICKET CATEGORIES:
- HABIT: Routines.
- TASK: One-off actions.
- EVENT: Time-specific appointments.

ACTION RULES:
1. ALWAYS use the provided tools for state changes.
2. NO EMOJIS in titles.
3. Respond with a brief confirmation of actions.

IMPORTANT: If your execution environment does not support native function calling, output your action as a raw JSON block.",
            if recent_actions_str.is_empty() { "No recent actions.".to_string() } else { recent_actions_str },
            context_str
        );
        
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
                                if k != "type" && k != "notes" {
                                    obj.insert(k.clone(), v.clone());
                                }
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
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
                    }
                }
                "delete_ticket" => {
                    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                    if task_id.is_empty() {
                        return Ok("Failed: task_id missing".to_string());
                    }
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Delete,
                        task_id.to_string(),
                        "TASK".to_string(), 
                        None,
                        None,
                        None,
                    ).await {
                        Ok(_) => Ok("Ticket deleted.".to_string()),
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
                    }
                }
                "delete_all_tickets" => {
                    let tasks = match get_tasks(app.clone()).await {
                        Ok(t) => t,
                        Err(e) => return Ok(format!("Failed to retrieve tasks: {}", e)),
                    };
                    
                    if tasks.is_empty() {
                        return Ok("Stack is already empty.".to_string());
                    }
                    
                    for task in tasks {
                        let _ = append_pending_action(
                            &app,
                            SyncActionType::Delete,
                            task.id,
                            "TASK".to_string(), 
                            None,
                            None,
                            None,
                        ).await;
                    }
                    Ok("All tickets deleted.".to_string())
                }
                "edit_ticket" => {
                    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                    if task_id.is_empty() {
                        return Ok("Failed: task_id missing".to_string());
                    }
                    
                    let notes = args.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let mut payload_updates = serde_json::Map::new();
                    if let Some(title) = args.get("title") { payload_updates.insert("title".to_string(), title.clone()); }
                    if let Some(time) = args.get("scheduled_time") { payload_updates.insert("scheduled_time".to_string(), time.clone()); }
                    if let Some(dur) = args.get("duration_minutes") { payload_updates.insert("duration_minutes".to_string(), dur.clone()); }
                    if let Some(rec) = args.get("recurrence") { payload_updates.insert("recurrence".to_string(), rec.clone()); }
                    
                    match append_pending_action(
                        &app,
                        SyncActionType::Update,
                        task_id.to_string(),
                        "TASK".to_string(), 
                        if payload_updates.is_empty() { None } else { Some(serde_json::Value::Object(payload_updates)) },
                        None,
                        notes,
                    ).await {
                        Ok(_) => Ok("Ticket edited.".to_string()),
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
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
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
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
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
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
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
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
                        Err(e) => Err(hstack_core::error::Error::Internal(e)),
                    }
                }
                _ => Ok(format!("Unknown tool: {}", name)),
            }
        })
    });

    match chat_loop(&config, &mut messages, &tools, &executor).await {
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
            apply_sync_update
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

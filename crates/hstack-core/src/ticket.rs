use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use crate::provider::{Tool, ToolFunction};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TicketType {
    Task,
    Habit,
    Event,
    Commute,
    Countdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    Idle,
    InFocus,
    Completed,
    Expired,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum TicketPayload {
    Commute {
        title: String,
        label: Option<String>,
        origin: String,
        destination: String,
        deadline: Option<String>,
        days: Option<String>,
        live: Option<bool>,
        minutes_remaining: Option<i64>,
        directions: Option<Value>,
        completed: Option<bool>,
    },
    Countdown {
        title: String,
        duration_minutes: i64,
        expires_at: Option<String>,
    },
    Event {
        title: String,
        scheduled_time_iso: Option<String>,
        rrule: Option<String>,
        duration_minutes: Option<i64>,
        completed: Option<bool>,
    },
    Habit {
        title: String,
        scheduled_time_iso: Option<String>,
        rrule: Option<String>,
        completed: Option<bool>,
    },
    Task {
        title: String,
        scheduled_time_iso: Option<String>,
        rrule: Option<String>,
        duration_minutes: Option<i64>,
        completed: Option<bool>,
    },
    Generic(Value), // Fallback for unknown payloads during migration
}

impl<'de> Deserialize<'de> for TicketPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(TicketPayload::Generic(Value::deserialize(deserializer)?))
    }
}

pub fn decode_ticket_payload_for_type(type_: &TicketType, value: Value) -> Result<TicketPayload, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "ticket payload must be a JSON object".to_string())?;

    match type_ {
        TicketType::Commute => Ok(TicketPayload::Commute {
            title: object.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            label: object.get("label").and_then(Value::as_str).map(str::to_string),
            origin: object.get("origin").and_then(Value::as_str).ok_or_else(|| "commute payload missing origin".to_string())?.to_string(),
            destination: object.get("destination").and_then(Value::as_str).ok_or_else(|| "commute payload missing destination".to_string())?.to_string(),
            deadline: object.get("deadline").and_then(Value::as_str).map(str::to_string),
            days: object.get("days").and_then(Value::as_str).map(str::to_string),
            live: object.get("live").and_then(Value::as_bool),
            minutes_remaining: object.get("minutes_remaining").and_then(Value::as_i64),
            directions: object.get("directions").cloned().filter(|item| !item.is_null()),
            completed: object.get("completed").and_then(Value::as_bool),
        }),
        TicketType::Countdown => Ok(TicketPayload::Countdown {
            title: object.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            duration_minutes: object.get("duration_minutes").and_then(Value::as_i64).ok_or_else(|| "countdown payload missing duration_minutes".to_string())?,
            expires_at: object.get("expires_at").and_then(Value::as_str).map(str::to_string),
        }),
        TicketType::Event => Ok(TicketPayload::Event {
            title: object.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            scheduled_time_iso: object.get("scheduled_time_iso").and_then(Value::as_str).map(str::to_string),
            rrule: object.get("rrule").and_then(Value::as_str).map(str::to_string),
            duration_minutes: object.get("duration_minutes").and_then(Value::as_i64),
            completed: object.get("completed").and_then(Value::as_bool),
        }),
        TicketType::Habit => Ok(TicketPayload::Habit {
            title: object.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            scheduled_time_iso: object.get("scheduled_time_iso").and_then(Value::as_str).map(str::to_string),
            rrule: object.get("rrule").and_then(Value::as_str).map(str::to_string),
            completed: object.get("completed").and_then(Value::as_bool),
        }),
        TicketType::Task => Ok(TicketPayload::Task {
            title: object.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
            scheduled_time_iso: object.get("scheduled_time_iso").and_then(Value::as_str).map(str::to_string),
            rrule: object.get("rrule").and_then(Value::as_str).map(str::to_string),
            duration_minutes: object.get("duration_minutes").and_then(Value::as_i64),
            completed: object.get("completed").and_then(Value::as_bool),
        }),
    }
}

#[derive(Deserialize)]
struct RawTicket {
    id: String,
    #[serde(rename = "type")]
    r#type: TicketType,
    status: TicketStatus,
    payload: Value,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    title: String,
}

impl TicketPayload {
    pub fn get_title(&self) -> &str {
        match self {
            TicketPayload::Commute { title, .. } => title,
            TicketPayload::Countdown { title, .. } => title,
            TicketPayload::Event { title, .. } => title,
            TicketPayload::Habit { title, .. } => title,
            TicketPayload::Task { title, .. } => title,
            TicketPayload::Generic(v) => v.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled"),
        }
    }

    pub fn set_title(&mut self, new_title: String) {
        match self {
            TicketPayload::Commute { title, .. } => *title = new_title,
            TicketPayload::Countdown { title, .. } => *title = new_title,
            TicketPayload::Event { title, .. } => *title = new_title,
            TicketPayload::Habit { title, .. } => *title = new_title,
            TicketPayload::Task { title, .. } => *title = new_title,
            TicketPayload::Generic(v) => {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("title".to_string(), serde_json::Value::String(new_title));
                }
            }
        }
    }

    pub fn apply_partial_update(&mut self, updates: &Map<String, Value>) {
        match self {
            TicketPayload::Commute {
                title,
                label,
                origin,
                destination,
                deadline,
                days,
                live,
                minutes_remaining,
                directions,
                completed,
            } => {
                apply_string_field(updates, "title", title);
                apply_option_string_field(updates, "label", label);
                apply_string_field(updates, "origin", origin);
                apply_string_field(updates, "destination", destination);
                apply_option_string_field(updates, "deadline", deadline);
                apply_option_string_field(updates, "days", days);
                apply_option_bool_field(updates, "live", live);
                apply_option_i64_field(updates, "minutes_remaining", minutes_remaining);
                apply_option_value_field(updates, "directions", directions);
                apply_option_bool_field(updates, "completed", completed);
            }
            TicketPayload::Countdown {
                title,
                duration_minutes,
                expires_at,
            } => {
                apply_string_field(updates, "title", title);
                apply_i64_field(updates, "duration_minutes", duration_minutes);
                apply_option_string_field(updates, "expires_at", expires_at);
            }
            TicketPayload::Event {
                title,
                scheduled_time_iso,
                rrule,
                duration_minutes,
                completed,
            } => {
                apply_string_field(updates, "title", title);
                apply_option_string_field(updates, "scheduled_time_iso", scheduled_time_iso);
                apply_option_string_field(updates, "rrule", rrule);
                apply_option_i64_field(updates, "duration_minutes", duration_minutes);
                apply_option_bool_field(updates, "completed", completed);
            }
            TicketPayload::Habit {
                title,
                scheduled_time_iso,
                rrule,
                completed,
            } => {
                apply_string_field(updates, "title", title);
                apply_option_string_field(updates, "scheduled_time_iso", scheduled_time_iso);
                apply_option_string_field(updates, "rrule", rrule);
                apply_option_bool_field(updates, "completed", completed);
            }
            TicketPayload::Task {
                title,
                scheduled_time_iso,
                rrule,
                duration_minutes,
                completed,
            } => {
                apply_string_field(updates, "title", title);
                apply_option_string_field(updates, "scheduled_time_iso", scheduled_time_iso);
                apply_option_string_field(updates, "rrule", rrule);
                apply_option_i64_field(updates, "duration_minutes", duration_minutes);
                apply_option_bool_field(updates, "completed", completed);
            }
            TicketPayload::Generic(value) => {
                if let Some(object) = value.as_object_mut() {
                    for (key, field_value) in updates {
                        object.insert(key.clone(), field_value.clone());
                    }
                }
            }
        }
    }

    pub fn matches_partial_update(&self, updates: &Map<String, Value>) -> bool {
        let mut projected = self.clone();
        projected.apply_partial_update(updates);
        &projected == self
    }
}

fn apply_string_field(updates: &Map<String, Value>, key: &str, target: &mut String) {
    if let Some(value) = updates.get(key).and_then(Value::as_str) {
        *target = value.to_string();
    }
}

fn apply_option_string_field(updates: &Map<String, Value>, key: &str, target: &mut Option<String>) {
    if let Some(value) = updates.get(key) {
        *target = value.as_str().map(|item| item.to_string());
    }
}

fn apply_i64_field(updates: &Map<String, Value>, key: &str, target: &mut i64) {
    if let Some(value) = updates.get(key).and_then(Value::as_i64) {
        *target = value;
    }
}

fn apply_option_i64_field(updates: &Map<String, Value>, key: &str, target: &mut Option<i64>) {
    if let Some(value) = updates.get(key) {
        *target = value.as_i64();
    }
}

fn apply_option_bool_field(updates: &Map<String, Value>, key: &str, target: &mut Option<bool>) {
    if let Some(value) = updates.get(key) {
        *target = value.as_bool();
    }
}

fn apply_option_value_field(updates: &Map<String, Value>, key: &str, target: &mut Option<Value>) {
    if let Some(value) = updates.get(key) {
        *target = if value.is_null() { None } else { Some(value.clone()) };
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub r#type: TicketType,
    pub status: TicketStatus,
    pub payload: TicketPayload,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for Ticket {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawTicket::deserialize(deserializer)?;
        let payload = decode_ticket_payload_for_type(&raw.r#type, raw.payload)
            .map_err(de::Error::custom)?;

        Ok(Ticket {
            id: raw.id,
            title: raw.title,
            r#type: raw.r#type,
            status: raw.status,
            payload,
            notes: raw.notes,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        })
    }
}

impl Ticket {
    pub fn new(title: String, type_: TicketType, payload: TicketPayload, notes: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            r#type: type_,
            status: TicketStatus::Idle,
            payload,
            notes,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_ticket_payload_for_type, Ticket, TicketPayload, TicketStatus, TicketType};

    #[test]
    fn decodes_event_payload_from_explicit_ticket_type() {
        let payload = decode_ticket_payload_for_type(&TicketType::Event, serde_json::json!({
            "title": "Yoga",
            "duration_minutes": 60
        }))
        .expect("payload should decode");

        match payload {
            TicketPayload::Event { title, duration_minutes, scheduled_time_iso, rrule, completed } => {
                assert_eq!(title, "Yoga");
                assert_eq!(duration_minutes, Some(60));
                assert_eq!(scheduled_time_iso, None);
                assert_eq!(rrule, None);
                assert_eq!(completed, None);
            }
            other => panic!("expected event payload, got {:?}", other),
        }
    }

    #[test]
    fn decodes_countdown_payload_from_explicit_ticket_type() {
        let payload = decode_ticket_payload_for_type(&TicketType::Countdown, serde_json::json!({
            "title": "Refactor auth",
            "duration_minutes": 45,
            "expires_at": "2026-03-20T22:00:00+00:00"
        }))
        .expect("countdown payload should decode");

        match payload {
            TicketPayload::Countdown {
                title,
                duration_minutes,
                expires_at,
            } => {
                assert_eq!(title, "Refactor auth");
                assert_eq!(duration_minutes, 45);
                assert_eq!(expires_at.as_deref(), Some("2026-03-20T22:00:00+00:00"));
            }
            other => panic!("expected countdown payload, got {:?}", other),
        }
    }

    #[test]
    fn deserializes_ticket_using_its_type_discriminator() {
        let ticket: Ticket = serde_json::from_value(serde_json::json!({
            "id": "ticket-1",
            "type": "EVENT",
            "status": "idle",
            "payload": {
                "title": "Yoga",
                "scheduled_time_iso": "2026-03-26T09:00:00+00:00",
                "duration_minutes": 60,
                "rrule": null,
                "completed": false
            },
            "notes": null,
            "created_at": "2026-03-20T22:00:00+00:00",
            "updated_at": "2026-03-20T22:00:00+00:00",
            "title": "Yoga"
        }))
        .expect("ticket should deserialize");

        assert_eq!(ticket.r#type, TicketType::Event);
        assert_eq!(ticket.status, TicketStatus::Idle);
        match ticket.payload {
            TicketPayload::Event { title, scheduled_time_iso, duration_minutes, .. } => {
                assert_eq!(title, "Yoga");
                assert_eq!(scheduled_time_iso.as_deref(), Some("2026-03-26T09:00:00+00:00"));
                assert_eq!(duration_minutes, Some(60));
            }
            other => panic!("expected event payload, got {:?}", other),
        }
    }
}

pub fn tool_schemas() -> Vec<Tool> {
    vec![
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "create_ticket".to_string(),
                description: "Create a new ticket in the user's stack. Must specify the type of ticket (HABIT, EVENT, or TASK) and the title payload. Any of these ticket types may include an RRULE/DTSTART schedule when the user gives timing information.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "description": "The type of the ticket. MUST be exactly one of: HABIT, EVENT, TASK"
                        },
                        "title": {
                            "type": "string",
                            "description": "The title or description of the ticket"
                        },
                        "notes": {
                            "type": "string",
                            "description": "Optional: Detailed context, research results, or user preferences for this specific ticket. Use Markdown formatting."
                        },
                        "rrule": {
                            "type": "string",
                            "description": "Optional: RFC 5545 scheduling string for any time-bearing ticket type. Use 'DTSTART:YYYYMMDDTHHMMSS' for a one-time scheduled ticket, or 'DTSTART:YYYYMMDDTHHMMSS RRULE:FREQ=WEEKLY;BYDAY=MO' for a recurring ticket. Examples: DTSTART:20260320T090000Z (tomorrow 9am), DTSTART:20260324T090000Z RRULE:FREQ=WEEKLY;BYDAY=MO (every Monday)"
                        },
                        "duration_minutes": {
                            "type": "integer",
                            "description": "Optional: Estimated duration in minutes."
                        }
                    },
                    "required": ["type", "title"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "delete_ticket".to_string(),
                description: "Delete a ticket from the user's stack given its ID string.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "The exact ID of the task/ticket to delete"
                        }
                    },
                    "required": ["task_id"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "delete_all_tickets".to_string(),
                description: "Deletes the entire stack of tickets for the user. Use this when the user wants to 'clear everything' or 'get rid of all tickets'.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "edit_ticket".to_string(),
                description: "Edit an existing ticket in the user's stack. You can change its type, title, notes, duration, or RRULE/DTSTART timing for any scheduled ticket type.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "The ID of the ticket to edit"
                        },
                        "type": {
                            "type": "string",
                            "description": "The new type (HABIT, EVENT, or TASK). Skip if no change."
                        },
                        "title": {
                            "type": "string",
                            "description": "The new title/description. Skip if no change."
                        },
                        "notes": {
                            "type": "string",
                            "description": "The new detailed notes for this ticket. Skip if no change."
                        },
                        "rrule": {
                            "type": "string",
                            "description": "The new RFC 5545 schedule for this ticket. Skip if no change. Format: 'DTSTART:YYYYMMDDTHHMMSSZ' for one-time scheduling or 'DTSTART:YYYYMMDDTHHMMSSZ RRULE:FREQ=WEEKLY;BYDAY=MO' for recurrence. Valid for HABIT, EVENT, and TASK tickets."
                        },
                        "duration_minutes": {
                            "type": "integer",
                            "description": "The new duration. Skip if no change."
                        }
                    },
                    "required": ["task_id"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "add_commute".to_string(),
                description: "Register a recurring commute for the user. Use this when the user says they regularly travel from one place to another at a specific time (e.g., 'I go from X to Y every morning at 9:30'). This will create a scheduled commute that automatically provides transit directions before the deadline.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "label": {
                            "type": "string",
                            "description": "A short label for the commute, e.g. 'morning_commute', 'evening_commute', 'work_commute'"
                        },
                        "origin": {
                            "type": "string",
                            "description": "The full starting address or place name"
                        },
                        "destination": {
                            "type": "string",
                            "description": "The full destination address or place name"
                        },
                        "deadline": {
                            "type": "string",
                            "description": "The time the user needs to arrive, in HH:MM 24-hour format (e.g. '09:30', '18:00')"
                        },
                        "days": {
                            "type": "string",
                            "description": "Comma-separated days of the week this commute applies, e.g. 'monday,tuesday,wednesday,thursday,friday'. Default is weekdays."
                        }
                    },
                    "required": ["label", "origin", "destination", "deadline"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "get_directions".to_string(),
                description: "Get real-time transit directions between two places. This creates a persistent COMMUTE ticket in the user's stack that renders expanded (in-focus) with step-by-step instructions.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "origin": {
                            "type": "string",
                            "description": "The starting address or place name"
                        },
                        "destination": {
                            "type": "string",
                            "description": "The destination address or place name"
                        }
                    },
                    "required": ["origin", "destination"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "remove_commute".to_string(),
                description: "Remove/delete a registered commute by its task ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "The ID of the commute task to remove"
                        }
                    },
                    "required": ["task_id"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "start_live_directions".to_string(),
                description: "Start a live directions tracker for an URGENT or ONE-TIME trip with a deadline. This creates a persistent COMMUTE ticket with `live: true` that stays in-focus and updates every 5 minutes until the deadline passes.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "origin": {
                            "type": "string",
                            "description": "The user's current location / starting address"
                        },
                        "destination": {
                            "type": "string",
                            "description": "Where the user needs to go"
                        },
                        "minutes_until_deadline": {
                            "type": "integer",
                            "description": "How many minutes from now the user needs to arrive. e.g. if they say 'in 30 mins' this is 30."
                        }
                    },
                    "required": ["origin", "destination", "minutes_until_deadline"]
                }),
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "create_countdown".to_string(),
                description: "Create a countdown timer (personal or agent-related). Use this for any task with a time limit (e.g., 'eat in 30 mins', 'IDE refactoring for 10 mins'). This creates a COUNTDOWN ticket with a live timer that auto-deletes when it expires.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "A short description of the task or timer, e.g., 'Refactoring code' or 'Time to leave'"
                        },
                        "duration_minutes": {
                            "type": "integer",
                            "description": "Number of minutes until the deadline."
                        }
                    },
                    "required": ["title", "duration_minutes"]
                }),
            },
        }
    ]
}
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub fn tool_schemas() -> Vec<Tool> {
    vec![
        Tool {
            r#type: "function".to_string(),
            function: ToolFunction {
                name: "create_ticket".to_string(),
                description: "Create a new ticket in the user's stack. Must specify the type of ticket (HABIT, EVENT, or TASK) and the title payload.".to_string(),
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
                            "description": "Optional: RRULE (iCalendar RFC 5545) for scheduling. Format: 'DTSTART:YYYYMMDDTHHMMSS' for one-time, or 'DTSTART:YYYYMMDDTHHMMSS RRULE:FREQ=WEEKLY;BYDAY=MO' for recurring. Examples: DTSTART:20260320T090000 (tomorrow 9am), DTSTART:20260324T090000 RRULE:FREQ=WEEKLY;BYDAY=MO (every Monday)"
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
                description: "Edit an existing ticket in the user's stack. You can change its type, title, or timing.".to_string(),
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
                            "description": "The new RRULE (iCalendar RFC 5545) for scheduling. Skip if no change. Format: 'DTSTART:YYYYMMDDTHHMMSS' or 'DTSTART:YYYYMMDDTHHMMSS RRULE:FREQ=WEEKLY;BYDAY=MO'"
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
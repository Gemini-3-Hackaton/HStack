use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use crate::ticket::{Ticket, TicketType, TicketStatus, TicketPayload};
use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncActionType {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAction {
    pub action_id: String,
    pub r#type: SyncActionType,
    pub entity_id: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TicketStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<TicketPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub timestamp: String,
}

#[derive(Serialize)]
struct HashStateItem {
    id: String,
    r#type: TicketType,
    payload: TicketPayload,
    status: TicketStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

/// Filters pending actions by removing those whose effects are already reflected in the base state.
/// This is the core of "Approach A": we keep local actions until the server state confirms them.
pub fn reconcile_state(base_tickets: &[Ticket], pending_actions: Vec<SyncAction>) -> Vec<SyncAction> {
    pending_actions
        .into_iter()
        .filter(|action| {
            match action.r#type {
                SyncActionType::Create => {
                    // Keep the action if the ticket is NOT yet in the base state
                    !base_tickets.iter().any(|t| t.id == action.entity_id)
                }
                SyncActionType::Update => {
                    // Find the ticket in the base state
                    match base_tickets.iter().find(|t| t.id == action.entity_id) {
                        // If it doesn't exist in base, it might be updating a pending creation, so keep it
                        None => true,
                        Some(t) => {
                            // Check if the base state matches the intended update
                            let status_matches = action.status.as_ref().map_or(true, |s| s == &t.status);
                            let notes_matches = action.notes.as_ref().map_or(true, |n| {
                                let norm_action = if n.is_empty() { None } else { Some(n.clone()) };
                                let norm_base = t.notes.as_ref().filter(|s| !s.is_empty()).cloned();
                                norm_action == norm_base
                            });
                            
                            let payload_matches = action.payload.as_ref().map_or(true, |p| {
                                // Since payload is fully typed, we just check equality.
                                // For merging (Update), we actually merge and then check equality
                                // of the relevant merged fields, but for now we expect the action's
                                // payload to be exactly what is requested (which may be a partial Generic 
                                // update, but that's handled gracefully if we fall back to generic eq).
                                // But if it's strongly typed, equality implies the update has fully propagated.
                                p == &t.payload
                            });

                            // Keep the action if either status or payload or notes doesn't match yet
                            !(status_matches && payload_matches && notes_matches)
                        }
                    }
                }
                SyncActionType::Delete => {
                    // Keep the action if the ticket IS still in the base state
                    base_tickets.iter().any(|t| t.id == action.entity_id)
                }
            }
        })
        .collect()
}

/// Projects the current state by applying a series of sync actions to a base set of tickets.
pub fn project_state(base_tickets: Vec<Ticket>, actions: &[SyncAction]) -> Vec<Ticket> {
    let mut effective_state = base_tickets;

    for action in actions {
        match action.r#type {
            SyncActionType::Create => {
                let type_ = match action.entity_type.to_lowercase().as_str() {
                    "habit" => TicketType::Habit,
                    "event" => TicketType::Event,
                    "commute" => TicketType::Commute,
                    "countdown" => TicketType::Countdown,
                    _ => TicketType::Task,
                };
                let payload = action.payload.clone().unwrap_or(TicketPayload::Generic(serde_json::json!({})));
                let title = payload.get_title().to_string();

                let ticket = Ticket {
                    id: action.entity_id.clone(),
                    r#type: type_,
                    status: action.status.clone().unwrap_or(TicketStatus::Idle),
                    payload,
                    notes: action.notes.clone(),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    title,
                };
                effective_state.push(ticket);
            }
            SyncActionType::Update => {
                if let Some(ticket) = effective_state.iter_mut().find(|t| t.id == action.entity_id) {
                    // Apply type morphing
                    ticket.r#type = match action.entity_type.to_uppercase().as_str() {
                        "HABIT" => TicketType::Habit,
                        "EVENT" => TicketType::Event,
                        "COMMUTE" => TicketType::Commute,
                        "COUNTDOWN" => TicketType::Countdown,
                        _ => TicketType::Task,
                    };

                    if let Some(status) = &action.status {
                        ticket.status = status.clone();
                    }
                    if let Some(notes) = &action.notes {
                        ticket.notes = Some(notes.clone());
                    }
                    if let Some(new_payload) = &action.payload {
                        match (&mut ticket.payload, new_payload) {
                            (TicketPayload::Generic(obj), TicketPayload::Generic(new_obj)) => {
                                if let (Some(o), Some(n)) = (obj.as_object_mut(), new_obj.as_object()) {
                                    for (k, v) in n {
                                        o.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            // Otherwise replace whole payload for simplicity
                            (_, p) => {
                                ticket.payload = p.clone();
                            }
                        }
                        ticket.title = ticket.payload.get_title().to_string();
                    }
                    ticket.updated_at = chrono::Utc::now();
                }
            }
            SyncActionType::Delete => {
                effective_state.retain(|t| t.id != action.entity_id);
            }
        }
    }
    effective_state
}

/// Calculate the SHA-256 hash of the entire state to check synchronization integrity.
pub fn calculate_state_hash(tasks: &[Ticket]) -> Result<String, Error> {
    // 1. Map to consistent structure for hashing
    let mut state_list: Vec<HashStateItem> = tasks
        .iter()
        .map(|t| HashStateItem {
            id: t.id.clone(),
            r#type: t.r#type.clone(),
            payload: t.payload.clone(),
            status: t.status.clone(),
            notes: t.notes.clone(),
        })
        .collect();

    // 2. Sort by ID for deterministic hashing (as per JS client logic)
    // Note: Python server sorts by created_at, id ASC. 
    // For local-first client, ID sort is usually most stable.
    state_list.sort_by(|a, b| a.id.cmp(&b.id));

    // 3. Serialize to JSON without whitespace (separators=(',', ':') in Python)
    let state_str = match serde_json::to_string(&state_list) {
        Ok(s) => s,
        Err(e) => return Err(Error::Serialization(e)),
    };

    // 4. Compute SHA-256
    let mut hasher = Sha256::new();
    hasher.update(state_str.as_bytes());
    let result = hasher.finalize();

    // 5. Hex encode
    Ok(format!("{:x}", result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::{TicketType, TicketStatus, TicketPayload};

    #[test]
    fn test_reconcile_create() {
        let base_tickets = vec![];
        let pending = vec![SyncAction {
            action_id: "a1".to_string(),
            r#type: SyncActionType::Create,
            entity_id: "t1".to_string(),
            entity_type: "TASK".to_string(),
            status: Some(TicketStatus::Idle),
            payload: Some(TicketPayload::Task {
                title: "Test".to_string(),
                scheduled_time_iso: None,
                rrule: None,
                duration_minutes: None,
                completed: Some(false),
            }),
            notes: None,
            timestamp: "now".to_string(),
        }];

        let reconciled = reconcile_state(&base_tickets, pending);
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].entity_id, "t1");
    }

    #[test]
    fn test_reconcile_already_created() {
        let payload = TicketPayload::Task {
            title: "Test".to_string(),
            scheduled_time_iso: None,
            rrule: None,
            duration_minutes: None,
            completed: Some(false),
        };
        let base_tickets = vec![Ticket {
            id: "t1".to_string(),
            title: "Test".to_string(),
            r#type: TicketType::Task,
            status: TicketStatus::Idle,
            payload: payload.clone(),
            notes: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }];
        let pending = vec![SyncAction {
            action_id: "a1".to_string(),
            r#type: SyncActionType::Create,
            entity_id: "t1".to_string(),
            entity_type: "TASK".to_string(),
            status: Some(TicketStatus::Idle),
            payload: Some(payload),
            notes: None,
            timestamp: "now".to_string(),
        }];

        let reconciled = reconcile_state(&base_tickets, pending);
        assert_eq!(reconciled.len(), 0);
    }
}

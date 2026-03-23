use chrono::Local;
use hstack_core::settings::{SavedLocation, UserSettings};
use hstack_core::ticket::{CommuteDepartureTime, Ticket, TicketLocation, TicketPayload};
use serde_json::Value;

pub(crate) const DEFAULT_COMMUTE_BUFFER_MINUTES: i64 = 10;

pub(crate) fn parse_optional_deserialized_arg<T>(args: &Value, key: &str, label: &str) -> Result<Option<T>, String>
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

pub(crate) fn parse_departure_time_arg(
    args: &Value,
    key: &str,
    label: &str,
) -> Result<Option<CommuteDepartureTime>, String> {
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

pub(crate) fn location_display_text(location: &TicketLocation) -> String {
    match location {
        TicketLocation::SavedLocation { location_id, label } => {
            label.clone().unwrap_or_else(|| location_id.clone())
        }
        TicketLocation::Coordinates {
            latitude,
            longitude,
            label,
        } => label
            .clone()
            .unwrap_or_else(|| format!("{}, {}", latitude, longitude)),
        TicketLocation::AddressText { address, .. } => address.clone(),
        TicketLocation::PlaceId {
            label,
            place_id,
            ..
        } => label.clone().unwrap_or_else(|| place_id.clone()),
        TicketLocation::CurrentPosition { label } => {
            label.clone().unwrap_or_else(|| "Current position".to_string())
        }
    }
}

fn normalize_location_key(text: &str) -> String {
    text.trim().to_lowercase()
}

fn find_saved_location_by_id<'a>(settings: &'a UserSettings, location_id: &str) -> Option<&'a SavedLocation> {
    settings
        .saved_locations
        .iter()
        .find(|location| location.id == location_id)
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
            return Err(format!(
                "saved location '{}' must resolve to a concrete location",
                saved_location.label
            ));
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
                return Err(format!(
                    "{} structured location must render to a non-empty value",
                    field_label
                ));
            }

            Ok((rendered, other))
        }
    }
}

pub(crate) fn format_saved_locations_for_prompt(saved_locations: &[SavedLocation]) -> String {
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

pub(crate) fn resolve_event_location(
    args: &Value,
    settings: &UserSettings,
) -> Result<Option<TicketLocation>, String> {
    match parse_location_arg(args, "location", "event location")? {
        None => Ok(None),
        Some(location) => {
            resolve_location_object(location, settings, "event location").map(|(_, location)| Some(location))
        }
    }
}

pub(crate) fn resolve_commute_location(
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
                    label, text, rendered
                ));
            }

            Ok((rendered, normalized))
        }
        (Some(text), None) => {
            if find_saved_location_by_label(settings, text).is_some() {
                return Err(format!(
                    "{} '{}' matches a saved location; use location_id instead of raw text",
                    label, text
                ));
            }

            if is_ambiguous_location_text(text) {
                return Err(format!(
                    "{} '{}' is ambiguous; ask the user which saved place or concrete address they mean",
                    label, text
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

pub(crate) fn infer_commute_payload_from_event(
    event_id: &str,
    payload: &TicketPayload,
) -> Option<TicketPayload> {
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

pub(crate) fn normalize_legacy_commute_payload(payload: &mut TicketPayload) {
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

pub(crate) fn normalize_projected_tickets(mut tickets: Vec<Ticket>) -> Vec<Ticket> {
    for ticket in &mut tickets {
        normalize_legacy_commute_payload(&mut ticket.payload);
    }

    tickets
}

pub(crate) fn find_related_commute_id(tickets: &[Ticket], event_id: &str) -> Option<String> {
    tickets.iter().find_map(|ticket| match &ticket.payload {
        TicketPayload::Commute {
            related_event_id: Some(related_event_id),
            ..
        } if related_event_id == event_id => Some(ticket.id.clone()),
        _ => None,
    })
}

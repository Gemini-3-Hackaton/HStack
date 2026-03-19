use chrono::{DateTime, Utc};
use rrule::RRuleSet;
use std::str::FromStr;

/// Parses an agent-generated RRULE/DTSTART string into an absolute DateTime and normalized RRULE.
/// Expects standard iCal format, e.g., "DTSTART:20260319T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=MO"
pub fn parse_agent_rrule(rrule_input: &str) -> Result<(DateTime<Utc>, Option<String>), String> {
    let normalized = rrule_input.replace(" RRULE:", "\nRRULE:");

    let rrule_set = RRuleSet::from_str(&normalized)
        .map_err(|e| format!("Agent generated invalid RFC 5545 string: {} (Input: {})", e, rrule_input))?;

    let start_time = rrule_set.get_dt_start().with_timezone(&Utc);

    let rrule_str = if rrule_set.get_rrule().is_empty() {
        None
    } else {
        Some(rrule_set.to_string())
    };

    Ok((start_time, rrule_str))
}
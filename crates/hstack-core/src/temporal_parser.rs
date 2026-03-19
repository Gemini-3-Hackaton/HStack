use chrono::{DateTime, Utc};
use rrule::RRuleSet;
use std::str::FromStr;

/// Parses an agent-generated RRULE/DTSTART string into an absolute DateTime and normalized RRULE.
/// Expects standard iCal format, e.g., "DTSTART:20260319T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=MO"
pub fn parse_agent_rrule(rrule_input: &str) -> Result<(DateTime<Utc>, Option<String>), String> {
    // LLM might forget the 'Z' on DTSTART despite the prompt, which makes rrule treat it as Local.
    // If it's missing 'Z', we aggressively add it so it parses as UTC explicitly.
    let mut input = rrule_input.to_string();
    if let Some(start_idx) = input.find("DTSTART:") {
        let content_start = start_idx + 8;
        if let Some(end_idx) = input[content_start..].find(|c: char| c.is_whitespace() || c == '\n') {
            let actual_end = content_start + end_idx;
            if !input[content_start..actual_end].ends_with('Z') {
                input.insert(actual_end, 'Z');
            }
        } else {
            // It goes to the end of the string
            if !input.ends_with('Z') {
                input.push('Z');
            }
        }
    }

    let normalized = input.replace(" RRULE:", "\nRRULE:");

    let rrule_set = RRuleSet::from_str(&normalized)
        .map_err(|e| format!("Agent generated invalid RFC 5545 string: {} (Input: {})", e, input))?;

    let start_time = rrule_set.get_dt_start().with_timezone(&Utc);

    let rrule_str = if rrule_set.get_rrule().is_empty() {
        None
    } else {
        Some(rrule_set.to_string())
    };

    Ok((start_time, rrule_str))
}
use chrono::{DateTime, NaiveDateTime, Utc};
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

    if !normalized.contains("RRULE:") {
        let dtstart_value = normalized
            .lines()
            .find_map(|line| line.strip_prefix("DTSTART:"))
            .map(str::trim)
            .ok_or_else(|| format!("Agent generated invalid RFC 5545 string: missing DTSTART (Input: {})", input))?;

        let start_time = parse_dtstart_value(dtstart_value)
            .map_err(|e| format!("Agent generated invalid RFC 5545 string: {} (Input: {})", e, input))?;

        return Ok((start_time, None));
    }

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

fn parse_dtstart_value(value: &str) -> Result<DateTime<Utc>, String> {
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%MZ"))
        .map_err(|e| format!("invalid DTSTART value: {}", e))?;

    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
mod tests {
    use super::parse_agent_rrule;
    use chrono::{TimeZone, Utc};

    #[test]
    fn parses_dtstart_only_as_one_time_schedule() {
        let (start, rrule) = parse_agent_rrule("DTSTART:20260326T100000Z").expect("dtstart-only schedule should parse");

        assert_eq!(start, Utc.with_ymd_and_hms(2026, 3, 26, 10, 0, 0).unwrap());
        assert_eq!(rrule, None);
    }

    #[test]
    fn normalizes_missing_z_for_dtstart_only_schedule() {
        let (start, rrule) = parse_agent_rrule("DTSTART:20260326T100000").expect("dtstart-only schedule without z should parse");

        assert_eq!(start, Utc.with_ymd_and_hms(2026, 3, 26, 10, 0, 0).unwrap());
        assert_eq!(rrule, None);
    }

    #[test]
    fn preserves_recurrence_when_rrule_is_present() {
        let (start, rrule) = parse_agent_rrule("DTSTART:20260324T083000Z RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR")
            .expect("recurring schedule should parse");

        assert_eq!(start, Utc.with_ymd_and_hms(2026, 3, 24, 8, 30, 0).unwrap());
        let normalized = rrule.expect("rrule should be preserved for recurring schedules");
        assert!(normalized.starts_with("DTSTART:20260324T083000Z\nRRULE:FREQ=WEEKLY"));
        assert!(normalized.contains("BYDAY=MO,WE,FR"));
    }
}
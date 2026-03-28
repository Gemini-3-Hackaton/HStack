use std::collections::{HashMap, HashSet};

use hstack_core::provider::Tool;
use serde_json::Value;

fn normalized_duration(duration: Option<i64>) -> Option<i64> {
    duration.filter(|value| *value > 0)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct PlannerCommitment {
    pub(crate) r#type: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) rrule: Option<String>,
    pub(crate) duration_minutes: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct PlannerDependencyImpact {
    pub(crate) ticket_id: String,
    pub(crate) title: Option<String>,
    pub(crate) reason: String,
    pub(crate) action_required: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct PlannerAction {
    pub(crate) tool: String,
    pub(crate) arguments: Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct PlannerPlan {
    pub(crate) user_goal: String,
    pub(crate) grounded_facts: Vec<String>,
    pub(crate) time_constraints: Vec<String>,
    pub(crate) existing_tickets_relevant: Vec<String>,
    pub(crate) dependent_tickets_impacted: Vec<PlannerDependencyImpact>,
    pub(crate) new_commitments_detected: Vec<PlannerCommitment>,
    pub(crate) proactive_opportunities: Vec<String>,
    pub(crate) assumptions_to_apply: Vec<String>,
    pub(crate) tool_actions: Vec<PlannerAction>,
    pub(crate) user_reply_strategy: String,
}

pub(crate) fn extract_first_json_value(content: &str) -> Option<Value> {
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

fn has_matching_edit_action(plan: &PlannerPlan, ticket_id: &str) -> bool {
    plan.tool_actions.iter().any(|action| {
        action.tool == "edit_ticket"
            && action.arguments.get("ticket_id").and_then(Value::as_str) == Some(ticket_id)
    })
}

pub(crate) fn validate_plan(plan: PlannerPlan, tools: &[Tool]) -> Result<PlannerPlan, String> {
    if plan.user_goal.trim().is_empty() {
        return Err("planner returned an empty user_goal".to_string());
    }

    if plan.user_reply_strategy.trim().is_empty() {
        return Err("planner returned an empty user_reply_strategy".to_string());
    }

    if !plan.tool_actions.is_empty() && plan.grounded_facts.is_empty() {
        return Err("planner proposed tool actions without grounded facts".to_string());
    }

    if plan.grounded_facts.iter().any(|fact| fact.trim().is_empty()) {
        return Err("planner returned an empty grounded fact".to_string());
    }

    if plan.time_constraints.iter().any(|constraint| constraint.trim().is_empty()) {
        return Err("planner returned an empty time constraint".to_string());
    }

    if plan
        .existing_tickets_relevant
        .iter()
        .any(|ticket| ticket.trim().is_empty())
    {
        return Err("planner returned an empty relevant-ticket reference".to_string());
    }

    if plan
        .proactive_opportunities
        .iter()
        .any(|opportunity| opportunity.trim().is_empty())
    {
        return Err("planner returned an empty proactive opportunity".to_string());
    }

    if plan
        .assumptions_to_apply
        .iter()
        .any(|assumption| assumption.trim().is_empty())
    {
        return Err("planner returned an empty assumption".to_string());
    }

    if plan.tool_actions.len() > 8 {
        return Err("planner returned too many actions".to_string());
    }

    let mut seen_impacts = HashSet::new();
    for impact in &plan.dependent_tickets_impacted {
        if impact.ticket_id.trim().is_empty() {
            return Err("planner returned a dependent ticket with an empty ticket_id".to_string());
        }

        if impact.reason.trim().is_empty() {
            return Err(format!(
                "planner returned an empty dependency reason for ticket '{}'",
                impact.ticket_id
            ));
        }

        if let Some(title) = impact.title.as_deref() {
            if title.trim().is_empty() {
                return Err(format!(
                    "planner returned an empty dependency title for ticket '{}'",
                    impact.ticket_id
                ));
            }
        }

        if !seen_impacts.insert(impact.ticket_id.as_str()) {
            return Err(format!(
                "planner listed dependent ticket '{}' more than once",
                impact.ticket_id
            ));
        }
    }

    let mut seen_commitments = HashSet::new();
    for commitment in &plan.new_commitments_detected {
        let title = commitment.title.as_deref().map(str::trim);

        if title == Some("") {
            return Err("planner returned a commitment with an empty title".to_string());
        }

        if title.is_none()
            && (commitment.r#type.is_some()
                || commitment.rrule.is_some()
                || commitment.duration_minutes.is_some())
        {
            return Err("planner returned a commitment with scheduling details but no title".to_string());
        }

        if let Some(title) = title {
            let normalized_title = title.to_ascii_lowercase();
            if !seen_commitments.insert(normalized_title.clone()) {
                return Err(format!("planner listed commitment '{}' more than once", title));
            }
        }
    }

    let tool_map: HashMap<&str, &Tool> = tools
        .iter()
        .map(|tool| (tool.function.name.as_str(), tool))
        .collect();

    for action in &plan.tool_actions {
        let tool = tool_map
            .get(action.tool.as_str())
            .ok_or_else(|| format!("planner used unknown tool '{}'", action.tool))?;

        let args = action
            .arguments
            .as_object()
            .ok_or_else(|| format!("planner arguments for '{}' must be a JSON object", action.tool))?;

        let schema = tool
            .function
            .parameters
            .as_object()
            .ok_or_else(|| format!("tool '{}' has invalid schema", action.tool))?;

        let allowed_keys: Vec<String> = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .map(|props| props.keys().cloned().collect())
            .unwrap_or_default();

        for key in args.keys() {
            if !allowed_keys.iter().any(|allowed| allowed == key) {
                return Err(format!(
                    "planner used unsupported argument '{}' for tool '{}'",
                    key, action.tool
                ));
            }
        }

        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            for required_key in required.iter().filter_map(|v| v.as_str()) {
                if !args.contains_key(required_key) {
                    return Err(format!(
                        "planner omitted required argument '{}' for tool '{}'",
                        required_key, action.tool
                    ));
                }
            }
        }

        if action.tool == "create_ticket" {
            let ticket_type = args.get("type").and_then(Value::as_str).unwrap_or_default();
            let has_duration = normalized_duration(
                args.get("duration_minutes").and_then(Value::as_i64),
            )
            .is_some();
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
                && action
                    .arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|candidate| candidate.eq_ignore_ascii_case(title))
                    .unwrap_or(false)
        });

        if commitment.rrule.is_some()
            || commitment.duration_minutes.is_some()
            || commitment.r#type.is_some()
        {
            let action = matching_create.ok_or_else(|| {
                format!(
                    "planner detected commitment '{}' but did not create a matching ticket action",
                    title
                )
            })?;

            if let Some(expected_type) = commitment.r#type.as_deref() {
                let actual_type = action.arguments.get("type").and_then(Value::as_str);
                if actual_type != Some(expected_type) {
                    return Err(format!(
                        "planner commitment '{}' expected type '{}' but create_ticket used '{:?}'",
                        title, expected_type, actual_type
                    ));
                }
            }

            if let Some(expected_rrule) = commitment.rrule.as_deref() {
                let actual_rrule = action.arguments.get("rrule").and_then(Value::as_str);
                if actual_rrule != Some(expected_rrule) {
                    return Err(format!(
                        "planner commitment '{}' expected schedule '{}' but create_ticket used '{:?}'",
                        title, expected_rrule, actual_rrule
                    ));
                }
            }

            let expected_duration = normalized_duration(commitment.duration_minutes);
            let actual_duration = normalized_duration(
                action.arguments.get("duration_minutes").and_then(Value::as_i64),
            );
            if let Some(expected_duration) = expected_duration {
                if actual_duration != Some(expected_duration) {
                    return Err(format!(
                        "planner commitment '{}' expected duration '{}' but create_ticket used '{:?}'",
                        title, expected_duration, actual_duration
                    ));
                }
            }
        }
    }

    for impact in &plan.dependent_tickets_impacted {
        let matching_edit = has_matching_edit_action(&plan, &impact.ticket_id);

        if impact.action_required && !matching_edit {
            return Err(format!(
                "planner marked dependent ticket '{}' as requiring action but did not include a matching edit_ticket action",
                impact.ticket_id
            ));
        }

        if !impact.action_required && matching_edit {
            return Err(format!(
                "planner edited dependent ticket '{}' without marking action_required=true",
                impact.ticket_id
            ));
        }
    }

    Ok(plan)
}

pub(crate) fn build_planner_execution_note(
    plan: &PlannerPlan,
    tool_results: &[(PlannerAction, String)],
) -> String {
    let dependency_lines = plan
        .dependent_tickets_impacted
        .iter()
        .map(|impact| {
            let title = impact.title.as_deref().unwrap_or("Unknown");
            format!(
                "- {} ({}) => {} [{}]",
                title,
                impact.ticket_id,
                impact.reason,
                if impact.action_required {
                    "action required"
                } else {
                    "info only"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let commitments = plan
        .new_commitments_detected
        .iter()
        .filter_map(|commitment| {
            commitment.title.as_ref().map(|title| {
                let kind = commitment.r#type.as_deref().unwrap_or("UNKNOWN");
                let timing = commitment.rrule.as_deref().unwrap_or("no schedule");
                let duration = commitment
                    .duration_minutes
                    .map(|value| format!(", duration {} min", value))
                    .unwrap_or_default();
                format!("- {} ({}) @ {}{}", title, kind, timing, duration)
            })
        })
        .collect::<Vec<_>>()
        .join("\n");

    let action_lines = tool_results
        .iter()
        .map(|(action, result)| format!("- {} {} => {}", action.tool, action.arguments, result))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "PLANNER SUMMARY:\nGoal: {}\nGrounded facts:\n{}\nTime constraints:\n{}\nRelevant tickets:\n{}\nDependent tickets impacted:\n{}\nDetected commitments:\n{}\nProactive opportunities:\n{}\nAssumptions applied:\n{}\nExecuted actions:\n{}\nReply strategy: {}\nUse this summary and the refreshed stack to answer the user naturally. Do not call tools again.",
        plan.user_goal,
        if plan.grounded_facts.is_empty() {
            "- none".to_string()
        } else {
            plan.grounded_facts
                .iter()
                .map(|fact| format!("- {}", fact))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if plan.time_constraints.is_empty() {
            "- none".to_string()
        } else {
            plan.time_constraints
                .iter()
                .map(|constraint| format!("- {}", constraint))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if plan.existing_tickets_relevant.is_empty() {
            "- none".to_string()
        } else {
            plan.existing_tickets_relevant
                .iter()
                .map(|ticket| format!("- {}", ticket))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if dependency_lines.is_empty() {
            "- none".to_string()
        } else {
            dependency_lines
        },
        if commitments.is_empty() {
            "- none".to_string()
        } else {
            commitments
        },
        if plan.proactive_opportunities.is_empty() {
            "- none".to_string()
        } else {
            plan.proactive_opportunities
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if plan.assumptions_to_apply.is_empty() {
            "- none".to_string()
        } else {
            plan.assumptions_to_apply
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if action_lines.is_empty() {
            "- none".to_string()
        } else {
            action_lines
        },
        plan.user_reply_strategy,
    )
}

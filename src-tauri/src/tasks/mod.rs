//! Task extraction and management helpers.

pub mod extract_from_memory;

pub use crate::storage::{Task, TaskType};

fn normalize_task_text(value: &str) -> String {
    value
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_list_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.trim();
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return rest.trim();
    }

    if let Some((prefix, rest)) = trimmed.split_once(' ') {
        let numbered = prefix.ends_with('.') || prefix.ends_with(')') || prefix.ends_with(':');
        let digits = prefix
            .trim_end_matches(['.', ')', ':'])
            .chars()
            .all(|ch| ch.is_ascii_digit());
        if numbered && digits {
            return rest.trim();
        }
    }

    trimmed
}

/// Best-effort classification for lines that did not carry an explicit
/// `TODO:` / `REMINDER:` / `FOLLOWUP:` prefix (for example markdown bullets).
/// Follow-up cues win over time-based reminder cues when both match.
pub fn infer_task_type_from_title(title: &str) -> TaskType {
    let lower = title.to_lowercase();
    if [
        "follow up",
        "follow-up",
        "reply to",
        "reach out",
        "check in with",
        "ping ",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
    {
        TaskType::Followup
    } else if [
        "tomorrow",
        "today",
        "tonight",
        "next week",
        "next month",
        "deadline",
        "due ",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
    {
        TaskType::Reminder
    } else {
        TaskType::Todo
    }
}

fn is_actionable_task_title(title: &str) -> bool {
    let normalized = normalize_task_text(title);
    if normalized.len() < 6 {
        return false;
    }
    if normalized.split_whitespace().count() < 2 {
        return false;
    }

    if matches!(
        normalized.as_str(),
        "todo"
            | "to do"
            | "task"
            | "follow up"
            | "followup"
            | "reminder"
            | "none"
            | "n a"
            | "no action items"
            | "no followups"
            | "no reminders"
    ) {
        return false;
    }

    true
}

/// Parse LLM response into task structs.
pub fn parse_tasks_from_llm_response(response: &str, source_app: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let now = chrono::Utc::now().timestamp_millis();

    for line in response.lines() {
        let line = line.trim().trim_matches('|');
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("none") {
            continue;
        }

        // Parse lines like "TODO: Send email", "REMINDER: ...", "FOLLOWUP: ..."
        let stripped = strip_list_prefix(line);
        let (mut task_type, title, type_from_prefix) =
            if let Some((prefix, rest)) = stripped.split_once(':') {
                let normalized_prefix = prefix
                    .trim()
                    .replace(['-', '_'], "")
                    .to_ascii_uppercase();
                let parsed_type = match normalized_prefix.as_str() {
                    "TODO" => Some(TaskType::Todo),
                    "REMINDER" => Some(TaskType::Reminder),
                    "FOLLOWUP" => Some(TaskType::Followup),
                    _ => None,
                };
                if let Some(parsed_type) = parsed_type {
                    (parsed_type, rest.trim(), true)
                } else if line.starts_with("- ") || line.starts_with("* ") {
                    (TaskType::Todo, stripped, false)
                } else {
                    continue;
                }
            } else if line.starts_with("- ") || line.starts_with("* ") {
                (TaskType::Todo, stripped, false)
            } else {
                continue;
            };

        let cleaned_title = title
            .trim()
            .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`')
            .trim_start_matches(['-', '*', ':', ' '])
            .trim();
        if !is_actionable_task_title(cleaned_title) {
            continue;
        }

        if !type_from_prefix {
            task_type = infer_task_type_from_title(cleaned_title);
        }

        let dedupe_key = (
            normalize_task_text(cleaned_title),
            match task_type {
                TaskType::Todo => "todo",
                TaskType::Reminder => "reminder",
                TaskType::Followup => "followup",
            },
        );
        if !seen.insert(dedupe_key) {
            continue;
        }

        if cleaned_title.len() > 5 {
            tasks.push(Task {
                id: uuid::Uuid::new_v4().to_string(),
                title: cleaned_title.to_string(),
                description: String::new(),
                source_app: source_app.to_string(),
                source_memory_id: None,
                created_at: now,
                due_date: None,
                is_completed: false,
                is_dismissed: false,
                task_type,
                linked_urls: Vec::new(),
                linked_memory_ids: Vec::new(),
            });
        }
    }

    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_followup_from_bullet_title() {
        let t = infer_task_type_from_title("Reply to Sarah about the contract draft");
        assert!(matches!(t, TaskType::Followup));
    }

    #[test]
    fn infer_reminder_from_time_cue() {
        let t = infer_task_type_from_title("Submit the form before Friday deadline");
        assert!(matches!(t, TaskType::Reminder));
    }

    #[test]
    fn parse_bullet_applies_infer_when_no_prefix() {
        let tasks = parse_tasks_from_llm_response(
            "- Reply to the team about API changes for next week",
            "TestApp",
        );
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].task_type, TaskType::Followup));
    }

    #[test]
    fn parse_explicit_prefix_respected() {
        let tasks = parse_tasks_from_llm_response(
            "TODO: Reply to the team about API changes for next week",
            "TestApp",
        );
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].task_type, TaskType::Todo));
    }
}

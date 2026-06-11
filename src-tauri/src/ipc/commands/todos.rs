//! Todo / task Tauri commands.

use super::common::is_internal_fndr_result;
use crate::storage::{MeetingSession, SearchResult, Task, TaskType};
use crate::AppState;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;

const TASK_LINK_SCAN_LIMIT: usize = 260;
const TASK_MEETING_LOOKBACK_DAYS: i64 = 14;
const TASK_MEMORY_BACKFILL_LIMIT: usize = 6;

// ========== Task Commands ==========

fn task_type_sort_key(task_type: &TaskType) -> u8 {
    match task_type {
        TaskType::Todo => 0,
        TaskType::Reminder => 1,
        TaskType::Followup => 2,
    }
}

fn parse_task_type(task_type: Option<&str>) -> TaskType {
    match task_type {
        Some("Reminder") => TaskType::Reminder,
        Some("Followup") => TaskType::Followup,
        _ => TaskType::Todo,
    }
}

fn normalize_task_text(value: &str) -> String {
    value
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_manual_task(task: &Task) -> bool {
    task.source_app.eq_ignore_ascii_case("manual")
}

fn is_meeting_task(task: &Task) -> bool {
    task.source_app.starts_with("Meeting:")
}

fn is_memory_task(task: &Task) -> bool {
    task.source_app.starts_with("Memory:") || task.source_app.eq_ignore_ascii_case("auto")
}

fn task_has_supporting_context(task: &Task) -> bool {
    task.source_memory_id.is_some()
        || !task.linked_memory_ids.is_empty()
        || !task.linked_urls.is_empty()
}

fn is_low_signal_task_title(title: &str) -> bool {
    let normalized = normalize_task_text(title);
    if normalized.len() < 6 {
        return true;
    }
    if normalized.split_whitespace().count() < 2 {
        return true;
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
            | "check this"
            | "look into this"
            | "work on this"
    ) {
        return true;
    }

    let generic_prefixes = [
        "complete ",
        "remember ",
        "follow up ",
        "followup ",
        "work on ",
        "check ",
        "look into ",
    ];
    generic_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix) && normalized.split_whitespace().count() <= 3)
}

fn task_priority_score(task: &Task, now_ms: i64) -> i64 {
    let age_hours = ((now_ms - task.created_at).max(0) / 3_600_000);
    let recency_bonus = (96 - age_hours).clamp(0, 96);
    let memory_bonus = (task.linked_memory_ids.len().min(10) as i64) * 4;
    let url_bonus = (task.linked_urls.len().min(6) as i64) * 2;
    let due_bonus = if task.due_date.is_some() { 12 } else { 0 };
    let source_bonus = if is_manual_task(task) {
        22
    } else if is_meeting_task(task) {
        16
    } else if is_memory_task(task) {
        11
    } else {
        6
    };
    let context_penalty =
        if !task_has_supporting_context(task) && !is_manual_task(task) && !is_meeting_task(task) {
            18
        } else {
            0
        };
    let title_penalty = if is_low_signal_task_title(&task.title) {
        24
    } else {
        0
    };
    recency_bonus + memory_bonus + url_bonus + due_bonus + source_bonus
        - context_penalty
        - title_penalty
}

/// Drop tasks whose primary memory row no longer exists, and prune dead
/// `linked_memory_ids`. Does not add new links from unrelated memories.
fn prune_tasks_with_deleted_memories(
    tasks: &mut [Task],
    valid_memory_ids: &HashSet<String>,
) -> bool {
    let mut changed = false;
    for task in tasks.iter_mut() {
        if task.is_completed {
            continue;
        }
        if let Some(ref sid) = task.source_memory_id {
            if !valid_memory_ids.contains(sid.as_str()) {
                if !task.is_dismissed {
                    task.is_dismissed = true;
                    changed = true;
                }
                continue;
            }
        }
        let before = task.linked_memory_ids.len();
        task.linked_memory_ids
            .retain(|id| valid_memory_ids.contains(id.as_str()));
        if task.linked_memory_ids.len() != before {
            changed = true;
        }
    }
    changed
}

fn backfill_tasks_from_meetings(tasks: &mut Vec<Task>, meetings: &[MeetingSession]) -> bool {
    let mut changed = false;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cutoff = now_ms - (TASK_MEETING_LOOKBACK_DAYS * 24 * 60 * 60 * 1000);
    let mut dedupe = tasks
        .iter()
        .map(|task| {
            (
                normalize_task_text(&task.title),
                task_type_sort_key(&task.task_type),
            )
        })
        .collect::<HashSet<_>>();

    let mut ordered = meetings.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .end_timestamp
            .unwrap_or(right.updated_at)
            .cmp(&left.end_timestamp.unwrap_or(left.updated_at))
    });

    for meeting in ordered {
        let event_ts = meeting.end_timestamp.unwrap_or(meeting.updated_at);
        if event_ts < cutoff {
            continue;
        }
        let Some(breakdown) = meeting.breakdown.as_ref() else {
            continue;
        };
        let source_app = format!("Meeting:{}", meeting.id);

        let mut add_items = |items: &[String], task_type: TaskType| {
            let type_key = task_type_sort_key(&task_type);
            for item in items {
                let title = item.trim();
                if title.is_empty() || is_low_signal_task_title(title) {
                    continue;
                }
                let dedupe_key = (normalize_task_text(title), type_key);
                if !dedupe.insert(dedupe_key) {
                    continue;
                }
                tasks.push(Task {
                    id: uuid::Uuid::new_v4().to_string(),
                    title: title.to_string(),
                    description: String::new(),
                    source_app: source_app.clone(),
                    source_memory_id: None,
                    created_at: event_ts,
                    due_date: None,
                    is_completed: false,
                    is_dismissed: false,
                    task_type: task_type.clone(),
                    linked_urls: Vec::new(),
                    linked_memory_ids: Vec::new(),
                });
                changed = true;
            }
        };

        add_items(&breakdown.todos, TaskType::Todo);
        add_items(&breakdown.reminders, TaskType::Reminder);
        add_items(&breakdown.followups, TaskType::Followup);
    }

    changed
}

#[derive(Debug, Clone)]
struct MemoryTaskCandidate {
    title: String,
    task_type: TaskType,
    score: i64,
    created_at: i64,
    source_app: String,
    source_memory_id: String,
    linked_urls: Vec<String>,
}

fn first_sentence(text: &str) -> String {
    text.split(['.', '!', '?'])
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .take(18)
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_memory_task_candidate(memory: &SearchResult) -> Option<MemoryTaskCandidate> {
    if is_internal_fndr_result(memory) {
        return None;
    }

    let mut text = memory.snippet.trim().to_string();
    if text.is_empty() {
        text = memory.clean_text.trim().to_string();
    }
    if text.is_empty() {
        text = memory.window_title.trim().to_string();
    }
    if text.is_empty() {
        return None;
    }

    let sentence = first_sentence(&text);
    if sentence.is_empty() {
        return None;
    }

    let cleaned = sentence
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`')
        .trim_start_matches("TODO:")
        .trim_start_matches("To do:")
        .trim_start_matches("to do:")
        .trim()
        .to_string();
    if cleaned.is_empty() || is_low_signal_task_title(&cleaned) {
        return None;
    }

    let lower = cleaned.to_lowercase();
    let action_cues = [
        "need to",
        "should",
        "must",
        "todo",
        "to do",
        "action item",
        "send",
        "reply",
        "schedule",
        "book",
        "finish",
        "complete",
        "prepare",
        "submit",
        "review",
        "update",
        "fix",
        "call",
        "email",
        "draft",
        "plan",
        "confirm",
        "deploy",
        "ship",
        "follow up",
    ];
    let action_hits = action_cues
        .iter()
        .filter(|cue| lower.contains(*cue))
        .count() as i64;
    if action_hits == 0 {
        return None;
    }

    let reminder_hits = [
        "tomorrow",
        "today",
        "tonight",
        "next week",
        "next month",
        "deadline",
        "due ",
    ]
    .iter()
    .filter(|cue| lower.contains(*cue))
    .count() as i64;
    let followup_hits = [
        "follow up",
        "follow-up",
        "reply to",
        "reach out",
        "check in with",
    ]
    .iter()
    .filter(|cue| lower.contains(*cue))
    .count() as i64;
    let score = action_hits * 4 + reminder_hits * 3 + followup_hits * 4;
    if score < 4 {
        return None;
    }

    Some(MemoryTaskCandidate {
        title: cleaned,
        task_type: crate::tasks::infer_task_type_from_title(&lower),
        score,
        created_at: memory.timestamp,
        source_app: format!("Memory:{}", memory.app_name),
        source_memory_id: memory.id.clone(),
        linked_urls: memory.url.clone().map(|url| vec![url]).unwrap_or_default(),
    })
}

fn backfill_tasks_from_memories(tasks: &mut Vec<Task>, recent_memories: &[SearchResult]) -> bool {
    let mut changed = false;
    let mut dedupe = tasks
        .iter()
        .map(|task| {
            (
                normalize_task_text(&task.title),
                task_type_sort_key(&task.task_type),
            )
        })
        .collect::<HashSet<_>>();

    let mut candidates = recent_memories
        .iter()
        .filter_map(build_memory_task_candidate)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.created_at.cmp(&left.created_at))
    });

    for candidate in candidates.into_iter().take(TASK_MEMORY_BACKFILL_LIMIT) {
        let type_key = task_type_sort_key(&candidate.task_type);
        let dedupe_key = (normalize_task_text(&candidate.title), type_key);
        if !dedupe.insert(dedupe_key) {
            continue;
        }

        tasks.push(Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: candidate.title,
            description: String::new(),
            source_app: candidate.source_app,
            source_memory_id: Some(candidate.source_memory_id.clone()),
            created_at: candidate.created_at,
            due_date: None,
            is_completed: false,
            is_dismissed: false,
            task_type: candidate.task_type.clone(),
            linked_urls: candidate.linked_urls,
            linked_memory_ids: vec![candidate.source_memory_id],
        });
        changed = true;
    }

    changed
}

fn dismiss_low_quality_auto_tasks(tasks: &mut [Task]) -> bool {
    let mut changed = false;

    for task in tasks
        .iter_mut()
        .filter(|task| !task.is_completed && !task.is_dismissed)
    {
        if is_manual_task(task) || is_meeting_task(task) {
            continue;
        }

        let stale_auto_seed = task.source_app.eq_ignore_ascii_case("auto");
        let weak_title = is_low_signal_task_title(&task.title);
        let missing_context = !task_has_supporting_context(task);
        if stale_auto_seed || weak_title || (is_memory_task(task) && missing_context) {
            task.is_dismissed = true;
            changed = true;
        }
    }

    changed
}

/// Add a new todo
#[tauri::command]
pub async fn add_todo(
    state: State<'_, Arc<AppState>>,
    title: String,
    task_type: Option<String>,
) -> Result<Task, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Task title cannot be empty.".to_string());
    }

    let parsed_task_type = parse_task_type(task_type.as_deref());

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        title: trimmed.to_string(),
        description: String::new(),
        source_app: "Manual".to_string(),
        source_memory_id: None,
        created_at: chrono::Utc::now().timestamp_millis(),
        due_date: None,
        is_completed: false,
        is_dismissed: false,
        task_type: parsed_task_type,
        linked_urls: Vec::new(),
        linked_memory_ids: Vec::new(),
    };

    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    tasks.push(task.clone());
    state
        .store
        .upsert_tasks(&tasks)
        .await
        .map_err(|e| e.to_string())?;

    Ok(task)
}

/// After a memory row is deleted, dismiss tasks that were grounded in it and
/// strip it from `linked_memory_ids` on other tasks.
pub(crate) async fn apply_memory_deletion_to_tasks(
    store: &crate::storage::Store,
    deleted_memory_id: &str,
) -> Result<(), String> {
    let mut tasks = store.list_tasks().await.map_err(|e| e.to_string())?;
    let mut changed = false;
    for task in tasks.iter_mut() {
        if task.is_completed {
            continue;
        }
        if task.source_memory_id.as_deref() == Some(deleted_memory_id) {
            if !task.is_dismissed {
                task.is_dismissed = true;
                changed = true;
            }
            continue;
        }
        let before = task.linked_memory_ids.len();
        task.linked_memory_ids.retain(|id| id != deleted_memory_id);
        if task.linked_memory_ids.len() != before {
            changed = true;
        }
    }
    if changed {
        store
            .upsert_tasks(&tasks)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Get all active todos
#[tauri::command]
pub async fn get_todos(state: State<'_, Arc<AppState>>) -> Result<Vec<Task>, String> {
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let valid_memory_ids = state
        .store
        .list_memory_ids()
        .await
        .map_err(|e| e.to_string())?;
    let recent_memories = state
        .store
        .list_recent_results(TASK_LINK_SCAN_LIMIT, None)
        .await
        .map_err(|e| e.to_string())?;
    let meetings = state
        .store
        .list_meetings()
        .await
        .map_err(|e| e.to_string())?;

    let prune_changed = prune_tasks_with_deleted_memories(&mut tasks, &valid_memory_ids);
    let memory_backfill_changed = backfill_tasks_from_memories(&mut tasks, &recent_memories);
    let meeting_backfill_changed = backfill_tasks_from_meetings(&mut tasks, &meetings);
    let cleanup_changed = dismiss_low_quality_auto_tasks(&mut tasks);
    if prune_changed || memory_backfill_changed || meeting_backfill_changed || cleanup_changed {
        state
            .store
            .upsert_tasks(&tasks)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut visible = tasks
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .filter(|task| !is_low_signal_task_title(&task.title))
        .filter(|task| {
            is_manual_task(task) || is_meeting_task(task) || task_has_supporting_context(task)
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    visible.retain(|task| {
        seen.insert((
            normalize_task_text(&task.title),
            task_type_sort_key(&task.task_type),
        ))
    });
    let now_ms = chrono::Utc::now().timestamp_millis();
    visible.sort_by(|left, right| {
        task_priority_score(right, now_ms)
            .cmp(&task_priority_score(left, now_ms))
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| {
                task_type_sort_key(&left.task_type).cmp(&task_type_sort_key(&right.task_type))
            })
    });
    Ok(visible)
}

/// Dismiss a task
#[tauri::command]
pub async fn dismiss_todo(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<bool, String> {
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
        task.is_dismissed = true;
        state
            .store
            .upsert_tasks(&tasks)
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update an existing task's title and/or type
#[tauri::command]
pub async fn update_todo(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    title: String,
    task_type: Option<String>,
) -> Result<Task, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Task title cannot be empty.".to_string());
    }

    let parsed_type = parse_task_type(task_type.as_deref());
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let task = tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "Task not found".to_string())?;

    task.title = trimmed.to_string();
    task.task_type = parsed_type;
    task.created_at = chrono::Utc::now().timestamp_millis();
    let updated = task.clone();

    state
        .store
        .upsert_tasks(&tasks)
        .await
        .map_err(|e| e.to_string())?;
    Ok(updated)
}

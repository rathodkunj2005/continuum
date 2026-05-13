//! Autofill overlay, shortcut, resolution, injection.

use super::common::{
    normalize_autofill_phrase, push_unique_case_insensitive, shared_embedder, truncate_chars,
};
use super::search::run_search_query;
use crate::config::AutofillConfig;
use crate::embedding::{Embedder, EmbeddingBackend};
use crate::storage::SearchResult;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

// ── Auto-fill commands ────────────────────────────────────────────────────────

const AUTOFILL_OVERLAY_LABEL: &str = "autofill-overlay";
const AUTOFILL_OVERLAY_WIDTH: f64 = 500.0;
const AUTOFILL_OVERLAY_HEIGHT: f64 = 430.0;
static AUTOFILL_OVERLAY_READY: once_cell::sync::Lazy<parking_lot::Mutex<bool>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(false));
static PENDING_AUTOFILL_PAYLOAD: once_cell::sync::Lazy<
    parking_lot::Mutex<Option<serde_json::Value>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Return the logical (x, y) bottom-right position for the autofill overlay on
/// the monitor the mouse cursor currently occupies. Falls back to primary monitor.
fn find_cursor_monitor<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    use enigo::{Enigo, Mouse, Settings};

    let cursor_pos = Enigo::new(&Settings::default())
        .ok()
        .and_then(|e| e.location().ok());

    let monitors = app.available_monitors().ok()?;

    let active = if let Some((cx, cy)) = cursor_pos {
        monitors
            .iter()
            .find(|m| {
                let pos = m.position();
                let size = m.size();
                cx >= pos.x
                    && cy >= pos.y
                    && cx < pos.x + size.width as i32
                    && cy < pos.y + size.height as i32
            })
            .or_else(|| monitors.first())
    } else {
        monitors.first()
    }?;

    let scale = active.scale_factor();
    let size = active.size();
    let pos = active.position();
    let w = size.width as f64 / scale;
    let h = size.height as f64 / scale;
    let lx = pos.x as f64 / scale;
    let ly = pos.y as f64 / scale;
    Some((
        lx + w - AUTOFILL_OVERLAY_WIDTH - 24.0,
        ly + h - AUTOFILL_OVERLAY_HEIGHT - 40.0,
    ))
}

/// Pre-create the autofill overlay window at startup (hidden) so it is fully
/// loaded and the React event listener is mounted before the first hotkey press.
/// Called once from main.rs setup.
pub fn create_autofill_overlay_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let url = tauri::WebviewUrl::App("autofill.html".into());

    let (x, y) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let size = m.size();
            let pos = m.position();
            let scale = m.scale_factor();
            // Both size and position are in physical pixels; divide by scale for logical coords.
            let w = size.width as f64 / scale;
            let h = size.height as f64 / scale;
            let lx = pos.x as f64 / scale;
            let ly = pos.y as f64 / scale;
            let x = lx + w - AUTOFILL_OVERLAY_WIDTH - 24.0;
            let y = ly + h - AUTOFILL_OVERLAY_HEIGHT - 40.0;
            tracing::info!(
                "autofill overlay: monitor {w}×{h} logical, scale={scale}, placing at ({x},{y})"
            );
            (x, y)
        })
        .unwrap_or((800.0, 400.0));

    match tauri::WebviewWindowBuilder::new(app, AUTOFILL_OVERLAY_LABEL, url)
        .title("FNDR Autofill")
        .inner_size(AUTOFILL_OVERLAY_WIDTH, AUTOFILL_OVERLAY_HEIGHT)
        .position(x, y)
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .build()
    {
        Ok(_) => tracing::info!("autofill overlay window pre-created (hidden)"),
        Err(err) => tracing::warn!("failed to pre-create autofill overlay window: {err}"),
    }
}

#[tauri::command]
pub async fn set_autofill_overlay_ready(ready: bool) -> Option<serde_json::Value> {
    *AUTOFILL_OVERLAY_READY.lock() = ready;
    if ready {
        PENDING_AUTOFILL_PAYLOAD.lock().take()
    } else {
        None
    }
}

#[tauri::command]
pub async fn take_pending_autofill_payload() -> Option<serde_json::Value> {
    PENDING_AUTOFILL_PAYLOAD.lock().take()
}

pub fn register_autofill_shortcut<R: tauri::Runtime>(
    app: &AppHandle<R>,
    config: &AutofillConfig,
) -> Result<(), String> {
    if let Err(err) = app.global_shortcut().unregister_all() {
        tracing::debug!("autofill: failed clearing existing shortcuts: {err}");
    }

    if !config.enabled {
        tracing::info!("autofill: shortcut disabled in settings");
        return Ok(());
    }

    let shortcut: Shortcut = config
        .shortcut
        .parse()
        .map_err(|err| format!("Invalid auto-fill shortcut '{}': {err}", config.shortcut))?;

    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }

            tracing::info!("Auto-fill hotkey fired");
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Capture the focused field before FNDR steals focus, otherwise we may
                // end up describing the overlay window instead of the target form input.
                let payload = match crate::accessibility::capture_focused_context() {
                    Ok(ctx) => {
                        tracing::info!(
                            "Auto-fill field context captured: label='{}' app='{}' window='{}'",
                            ctx.label,
                            ctx.app_name,
                            ctx.window_title
                        );
                        serde_json::to_value(&ctx).unwrap_or_default()
                    }
                    Err(err) => {
                        tracing::info!("Auto-fill context capture failed: {err}");
                        serde_json::json!({ "error": err })
                    }
                };

                *PENDING_AUTOFILL_PAYLOAD.lock() = Some(payload);

                // Reposition to the monitor the cursor is currently on before showing,
                // so the overlay appears near the user's active working context.
                let cursor_monitor = find_cursor_monitor(&handle);

                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                let h1 = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(win) = h1.get_webview_window(AUTOFILL_OVERLAY_LABEL) {
                        // Reposition to the active monitor's bottom-right corner.
                        if let Some((x, y)) = cursor_monitor {
                            let _ = win.set_position(tauri::LogicalPosition::new(x, y));
                        }
                        tracing::info!("autofill: showing overlay window");
                        let _ = win.show();
                        let _ = win.set_focus();
                    } else {
                        tracing::warn!("autofill: overlay window not found at hotkey time");
                    }
                    let _ = tx.send(());
                });

                // Wait for show() to complete, then give WKWebView a beat to resume
                // before delivering the payload. Emit the actual captured payload so
                // the frontend can start resolution immediately without polling.
                let _ = rx.await;
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                if *AUTOFILL_OVERLAY_READY.lock() {
                    // Emit the real payload directly — frontend handles FieldContext,
                    // error objects, and { scanning } objects the same way.
                    let payload_to_emit = PENDING_AUTOFILL_PAYLOAD.lock().clone();
                    if let Some(payload) = payload_to_emit {
                        let _ = handle.emit("autofill-triggered", payload);
                    } else {
                        let _ = handle.emit(
                            "autofill-triggered",
                            serde_json::json!({ "scanning": true, "message": "Preparing autofill" }),
                        );
                    }
                }
            });
        })
        .map_err(|err| err.to_string())
}

/// A single candidate memory value FNDR can inject into the active field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutofillCandidate {
    pub value: String,
    pub confidence: f32,
    pub match_reason: String,
    pub source_snippet: String,
    pub source_app: String,
    pub source_window_title: String,
    pub timestamp: i64,
    pub memory_id: String,
}

/// Result of resolving the active field against FNDR's memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutofillResolution {
    pub query: String,
    pub query_source: String,
    pub context_hint: String,
    pub candidates: Vec<AutofillCandidate>,
    pub auto_inject_threshold: f32,
    pub requires_confirmation: bool,
    pub used_ocr_fallback: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AutofillCandidateDraft {
    pub(crate) value: String,
    pub(crate) extraction_score: f32,
    pub(crate) match_reason: String,
    pub(crate) source_snippet: String,
    pub(crate) source_app: String,
    pub(crate) source_window_title: String,
    pub(crate) timestamp: i64,
    pub(crate) memory_id: String,
    pub(crate) search_score: f32,
    pub(crate) ocr_confidence: f32,
    pub(crate) noise_score: f32,
    pub(crate) context_alignment: f32,
}

fn normalized_tokens(input: &str) -> Vec<String> {
    normalize_autofill_phrase(input)
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn field_aliases(query: &str) -> Vec<String> {
    const GROUPS: &[&[&str]] = &[
        &[
            "tax id",
            "tax identification number",
            "employer identification number",
            "employer id number",
            "ein",
            "tin",
            "federal tax id",
        ],
        &["policy number", "policy no", "policy #"],
        &[
            "member id",
            "member number",
            "member #",
            "subscriber id",
            "subscriber number",
        ],
        &["group number", "group no", "group #"],
        &["claim number", "claim no", "claim #"],
        &["phone", "phone number", "telephone", "mobile"],
        &["email", "email address"],
        &["date of birth", "birth date", "dob"],
        &["zip", "zip code", "postal code"],
        &["routing number", "routing #", "aba routing number"],
        &["account number", "account #"],
    ];

    let normalized = normalize_autofill_phrase(query);
    let mut aliases = Vec::new();
    push_unique_case_insensitive(&mut aliases, query.trim());

    for group in GROUPS {
        let matches_group = group.iter().any(|alias| {
            let alias_normalized = normalize_autofill_phrase(alias);
            normalized == alias_normalized
                || normalized.contains(&alias_normalized)
                || alias_normalized.contains(&normalized)
        });
        if matches_group {
            for alias in *group {
                push_unique_case_insensitive(&mut aliases, *alias);
            }
        }
    }

    if normalized.ends_with(" number") {
        let stem = normalized.trim_end_matches(" number").trim();
        if !stem.is_empty() {
            push_unique_case_insensitive(&mut aliases, format!("{stem} no"));
            push_unique_case_insensitive(&mut aliases, format!("{stem} #"));
        }
    }

    aliases.truncate(6);
    aliases
}

fn is_generic_context_token(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "into"
            | "your"
            | "this"
            | "that"
            | "form"
            | "field"
            | "portal"
            | "screen"
            | "window"
            | "page"
            | "submit"
            | "cancel"
            | "save"
            | "continue"
            | "next"
            | "back"
            | "required"
            | "optional"
            | "section"
            | "information"
            | "details"
            | "value"
            | "google"
            | "chrome"
            | "safari"
            | "browser"
            | "preview"
            | "acrobat"
            | "adobe"
            | "microsoft"
            | "edge"
            | "firefox"
            | "brave"
    )
}

fn collect_context_terms(context: &crate::accessibility::FieldContext, query: &str) -> Vec<String> {
    let query_tokens = normalized_tokens(query).into_iter().collect::<HashSet<_>>();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for text in [
        &context.window_title,
        &context.screen_context,
        &context.app_name,
    ] {
        for token in normalized_tokens(text) {
            if token.len() < 3 || query_tokens.contains(&token) || is_generic_context_token(&token)
            {
                continue;
            }
            *counts.entry(token).or_insert(0) += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().map(|(term, _)| term).take(6).collect()
}

fn build_autofill_search_query(
    context: &crate::accessibility::FieldContext,
    query: &str,
) -> String {
    let context_terms = collect_context_terms(context, query);
    let normalized = normalize_autofill_phrase(query);
    let token_count = normalized.split_whitespace().count();

    if token_count <= 2 && !context_terms.is_empty() {
        let anchor = context_terms
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        return format!("{query} {anchor}");
    }

    query.trim().to_string()
}

fn build_autofill_query(
    context: &crate::accessibility::FieldContext,
    query_override: Option<&str>,
) -> (String, String) {
    if let Some(query) = query_override
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        return (query.to_string(), "manual".to_string());
    }

    if !context.label.trim().is_empty() {
        return (context.label.trim().to_string(), "label".to_string());
    }

    if !context.placeholder.trim().is_empty() {
        return (
            context.placeholder.trim().to_string(),
            "placeholder".to_string(),
        );
    }

    if !context.inferred_label.trim().is_empty() {
        return (context.inferred_label.trim().to_string(), "ocr".to_string());
    }

    (String::new(), "unknown".to_string())
}

fn context_hint(context: &crate::accessibility::FieldContext) -> String {
    if !context.screen_context.trim().is_empty() {
        return context.screen_context.clone();
    }

    if !context.window_title.trim().is_empty() {
        return context.window_title.trim().to_string();
    }

    context.app_name.trim().to_string()
}

fn split_tableish(line: &str) -> Vec<String> {
    let Some(regex) = regex::Regex::new(r"\s*\|\s*|\t+|\s{2,}").ok() else {
        return vec![line.trim().to_string()];
    };
    regex
        .split(line)
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn sanitize_autofill_value(raw: &str) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = compact
        .trim()
        .trim_matches(|ch: char| matches!(ch, ':' | ';' | ',' | '|' | '"' | '\'' | ' '))
        .to_string();

    if compact.contains("  ") {
        return compact
            .split("  ")
            .next()
            .unwrap_or(&compact)
            .trim()
            .to_string();
    }

    compact
}

fn looks_like_field_value(query: &str, raw: &str) -> bool {
    let value = sanitize_autofill_value(raw);
    let normalized_value = normalize_autofill_phrase(&value);
    let normalized_query = normalize_autofill_phrase(query);

    if value.is_empty() || value.len() > 160 || normalized_value == normalized_query {
        return false;
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        return false;
    }

    let word_count = value.split_whitespace().count();
    let has_digits = value.chars().any(|ch| ch.is_ascii_digit());
    let has_letters = value.chars().any(|ch| ch.is_ascii_alphabetic());

    if normalized_query.contains("address") {
        return word_count <= 12;
    }

    if normalized_query.contains("email") {
        return value.contains('@') && value.contains('.');
    }

    if normalized_query.contains("phone") {
        return has_digits && value.len() <= 32;
    }

    if normalized_query.contains("date") || normalized_query.contains("dob") {
        return has_digits && value.len() <= 24;
    }

    if normalized_query.contains("number")
        || normalized_query.ends_with(" id")
        || normalized_query.contains("ein")
        || normalized_query.contains("routing")
        || normalized_query.contains("account")
    {
        return has_digits || value.contains('-');
    }

    word_count <= 8 && (has_digits || has_letters)
}

fn alias_matches(label_cell: &str, aliases: &[String]) -> bool {
    let normalized = normalize_autofill_phrase(label_cell);
    if normalized.is_empty() {
        return false;
    }

    let normalized_tokens = normalized.split_whitespace().collect::<HashSet<_>>();
    aliases.iter().any(|alias| {
        let alias_normalized = normalize_autofill_phrase(alias);
        if alias_normalized.is_empty() {
            return false;
        }

        if normalized == alias_normalized
            || normalized.contains(&alias_normalized)
            || alias_normalized.contains(&normalized)
        {
            return true;
        }

        let alias_tokens = alias_normalized.split_whitespace().collect::<Vec<_>>();
        if alias_tokens.is_empty() {
            return false;
        }

        let matched = alias_tokens
            .iter()
            .filter(|token| normalized_tokens.contains(**token))
            .count();

        if alias_tokens.len() == 1 {
            matched == 1
        } else {
            matched == alias_tokens.len()
        }
    })
}

fn extract_inline_value(line: &str, alias: &str) -> Option<String> {
    let pattern = format!(
        r"(?i)\b{}\b\s*(?:[:=#-]\s*|\s+)([^\n\r]{{1,160}})",
        regex::escape(alias)
    );
    let regex = regex::Regex::new(&pattern).ok()?;
    let captures = regex.captures(line)?;
    let raw = captures.get(1)?.as_str();
    let cells = split_tableish(raw);
    let value = if cells.len() > 1 { &cells[0] } else { raw };
    Some(sanitize_autofill_value(value))
}

pub(crate) fn extract_candidates_from_result(
    query: &str,
    aliases: &[String],
    result: &SearchResult,
    context_alignment: f32,
) -> Vec<AutofillCandidateDraft> {
    let text = if !result.clean_text.trim().is_empty() {
        result.clean_text.as_str()
    } else {
        result.text.as_str()
    };

    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    let mut drafts = Vec::new();
    let mut seen = HashSet::new();

    let mut push_draft =
        |value: String, extraction_score: f32, reason: &str, source_snippet: String| {
            let normalized = normalize_autofill_phrase(&value);
            if normalized.is_empty() || !seen.insert(normalized) {
                return;
            }
            drafts.push(AutofillCandidateDraft {
                value,
                extraction_score,
                match_reason: reason.to_string(),
                source_snippet,
                source_app: result.app_name.clone(),
                source_window_title: result.window_title.clone(),
                timestamp: result.timestamp,
                memory_id: result.id.clone(),
                search_score: result.score,
                ocr_confidence: result.ocr_confidence,
                noise_score: result.noise_score,
                context_alignment,
            });
        };

    for (index, line) in lines.iter().enumerate() {
        if let Some((lhs, rhs)) = line.split_once(':').or_else(|| line.split_once('=')) {
            if alias_matches(lhs, aliases) && looks_like_field_value(query, rhs) {
                push_draft(
                    sanitize_autofill_value(rhs),
                    0.97,
                    "Matched a labeled value in a remembered document",
                    line.clone(),
                );
            }
        }

        let cells = split_tableish(line);
        if cells.len() >= 2 {
            for window in cells.windows(2) {
                if alias_matches(&window[0], aliases) && looks_like_field_value(query, &window[1]) {
                    push_draft(
                        sanitize_autofill_value(&window[1]),
                        0.95,
                        "Matched a label-value pair in a remembered document",
                        line.clone(),
                    );
                }
            }
        }

        for alias in aliases {
            if let Some(value) = extract_inline_value(line, alias) {
                if looks_like_field_value(query, &value) {
                    push_draft(
                        value,
                        0.93,
                        "Matched an inline field label in OCR text",
                        line.clone(),
                    );
                }
            }

            if alias_matches(line, std::slice::from_ref(alias)) {
                if let Some(next_line) = lines.iter().skip(index + 1).find(|next| !next.is_empty())
                {
                    if looks_like_field_value(query, next_line) {
                        push_draft(
                            sanitize_autofill_value(next_line),
                            0.88,
                            "Matched a stacked field label and value",
                            format!("{line} / {next_line}"),
                        );
                    }
                }
            }
        }
    }

    for pair in lines.windows(2) {
        let headers = split_tableish(&pair[0]);
        let values = split_tableish(&pair[1]);
        if headers.len() >= 2 && headers.len() == values.len() {
            for (idx, header) in headers.iter().enumerate() {
                if alias_matches(header, aliases) && looks_like_field_value(query, &values[idx]) {
                    push_draft(
                        sanitize_autofill_value(&values[idx]),
                        0.86,
                        "Matched a value from a remembered table",
                        format!("{} / {}", pair[0], pair[1]),
                    );
                }
            }
        }
    }

    // Prose / free-form fallback: structured patterns missed the value.
    // Look for any alias term appearing in a line and extract the trailing value,
    // or take short standalone lines that look like the right type (e.g. bare IDs).
    drop(push_draft);
    if drafts.is_empty() {
        let mut fallback: Vec<AutofillCandidateDraft> = Vec::new();
        let mut fb_seen: HashSet<String> = HashSet::new();
        let mut push_fb =
            |value: String, extraction_score: f32, reason: &str, source_snippet: String| {
                let normalized = normalize_autofill_phrase(&value);
                if normalized.is_empty() || !fb_seen.insert(normalized) {
                    return;
                }
                fallback.push(AutofillCandidateDraft {
                    value,
                    extraction_score,
                    match_reason: reason.to_string(),
                    source_snippet,
                    source_app: result.app_name.clone(),
                    source_window_title: result.window_title.clone(),
                    timestamp: result.timestamp,
                    memory_id: result.id.clone(),
                    search_score: result.score,
                    ocr_confidence: result.ocr_confidence,
                    noise_score: result.noise_score,
                    context_alignment,
                });
            };
        for line in &lines {
            let lower = line.to_ascii_lowercase();
            for alias in aliases {
                let alias_lower = alias.to_ascii_lowercase();
                if lower.contains(&alias_lower) {
                    let after_alias =
                        &line[lower.find(&alias_lower).unwrap_or(0) + alias_lower.len()..];
                    let value: String = after_alias
                        .trim_start_matches(|c: char| matches!(c, ':' | '=' | ' ' | '\t'))
                        .split_whitespace()
                        .take(10)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if looks_like_field_value(query, &value) {
                        push_fb(
                            sanitize_autofill_value(&value),
                            0.65,
                            "Found value near label in memory text",
                            line.clone(),
                        );
                    }
                }
            }

            // Bare structured value on its own line — e.g. "POL-88291-X" or "012-34-5678".
            let cells = split_tableish(line);
            if cells.len() == 1 && line.len() <= 64 && looks_like_field_value(query, line) {
                push_fb(
                    sanitize_autofill_value(line),
                    0.58,
                    "Remembered value matching this field type",
                    line.clone(),
                );
            }
        }
        drop(push_fb);
        drafts.extend(fallback);
    }

    drafts
}

fn context_alignment_score(
    context: &crate::accessibility::FieldContext,
    result: &SearchResult,
    query: &str,
) -> f32 {
    let context_terms = collect_context_terms(context, query);
    if context_terms.is_empty() {
        return 0.0;
    }

    let title_tokens = normalized_tokens(&format!(
        "{} {} {}",
        result.app_name,
        result.window_title,
        result.url.clone().unwrap_or_default()
    ))
    .into_iter()
    .collect::<HashSet<_>>();
    let body_tokens = normalized_tokens(&format!(
        "{} {}",
        truncate_chars(&result.clean_text, 500),
        truncate_chars(&result.snippet, 220)
    ))
    .into_iter()
    .collect::<HashSet<_>>();

    let title_hits = context_terms
        .iter()
        .filter(|term| title_tokens.contains(*term))
        .count() as f32;
    let body_hits = context_terms
        .iter()
        .filter(|term| body_tokens.contains(*term))
        .count() as f32;
    let total = context_terms.len() as f32;

    ((title_hits / total) * 0.68 + (body_hits / total) * 0.32).clamp(0.0, 1.0)
}

fn document_affinity(app_name: &str, window_title: &str) -> f32 {
    let app = app_name.to_ascii_lowercase();
    let window = window_title.to_ascii_lowercase();

    let mut score: f32 = 0.2;
    if [
        "preview", "acrobat", "excel", "numbers", "sheets", "word", "pages",
    ]
    .iter()
    .any(|needle| app.contains(needle))
    {
        score += 0.45;
    }
    if [
        ".pdf",
        ".xlsx",
        ".xls",
        ".csv",
        "statement",
        "invoice",
        "policy",
        "claim",
        "onboarding",
        "application",
        "tax",
        "form",
        "record",
        "spreadsheet",
        "sheet",
    ]
    .iter()
    .any(|needle| window.contains(needle))
    {
        score += 0.3;
    }
    score.clamp(0.0, 1.0)
}

fn value_shape_bonus(query: &str, value: &str) -> f32 {
    let normalized_query = normalize_autofill_phrase(query);
    if !(normalized_query.contains("number")
        || normalized_query.ends_with(" id")
        || normalized_query.contains("ein")
        || normalized_query.contains("routing")
        || normalized_query.contains("policy"))
    {
        return 0.0;
    }

    if value.chars().any(|ch| ch.is_ascii_digit())
        && value
            .chars()
            .any(|ch| ch.is_ascii_uppercase() || matches!(ch, '-' | '/'))
    {
        0.08
    } else if value.chars().any(|ch| ch.is_ascii_digit()) {
        0.04
    } else {
        0.0
    }
}

fn recency_score(timestamp: i64, lookback_days: u32) -> f32 {
    let age_ms = (chrono::Utc::now().timestamp_millis() - timestamp).max(0);
    let lookback_ms = (lookback_days.max(1) as i64) * 86_400_000;
    (1.0 - (age_ms as f32 / lookback_ms as f32)).clamp(0.0, 1.0)
}

fn rank_autofill_candidates(
    query: &str,
    drafts: Vec<AutofillCandidateDraft>,
    lookback_days: u32,
    max_candidates: usize,
) -> Vec<AutofillCandidate> {
    let mut best_by_value: HashMap<String, AutofillCandidate> = HashMap::new();

    for draft in drafts {
        let doc_score = document_affinity(&draft.source_app, &draft.source_window_title);
        let recency = recency_score(draft.timestamp, lookback_days);
        let shape_bonus = value_shape_bonus(query, &draft.value);
        let mut confidence = draft.search_score * 0.28
            + draft.extraction_score * 0.28
            + draft.context_alignment * 0.20
            + draft.ocr_confidence.clamp(0.0, 1.0) * 0.10
            + doc_score * 0.08
            + recency * 0.06
            + shape_bonus;
        confidence -= draft.noise_score.clamp(0.0, 1.0) * 0.08;
        confidence = confidence.clamp(0.0, 0.995);

        let candidate = AutofillCandidate {
            value: draft.value.clone(),
            confidence,
            match_reason: draft.match_reason,
            source_snippet: draft.source_snippet,
            source_app: draft.source_app,
            source_window_title: draft.source_window_title,
            timestamp: draft.timestamp,
            memory_id: draft.memory_id,
        };

        let key = normalize_autofill_phrase(&candidate.value);
        let should_replace = best_by_value
            .get(&key)
            .map(|existing| candidate.confidence > existing.confidence)
            .unwrap_or(true);
        if should_replace {
            best_by_value.insert(key, candidate);
        }
    }

    let mut candidates = best_by_value.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.timestamp.cmp(&left.timestamp))
    });
    candidates.truncate(max_candidates.max(1));
    candidates
}

pub(crate) fn needs_autofill_confirmation(
    candidates: &[AutofillCandidate],
    auto_threshold: f32,
) -> bool {
    let Some(top) = candidates.first() else {
        return false;
    };

    if top.confidence < auto_threshold {
        return true;
    }

    candidates.get(1).is_some_and(|next| {
        next.confidence >= auto_threshold - 0.03 || (top.confidence - next.confidence) <= 0.05
    })
}

#[tauri::command]
pub async fn get_autofill_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<AutofillConfig, String> {
    Ok(state.inner().config.read().autofill.clone().normalized())
}

#[tauri::command]
pub async fn set_autofill_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AutofillConfig,
) -> Result<AutofillConfig, String> {
    let mut normalized = settings.normalized();
    let shortcut: Shortcut = normalized.shortcut.parse().map_err(|err| {
        format!(
            "Invalid auto-fill shortcut '{}': {err}",
            normalized.shortcut
        )
    })?;
    normalized.shortcut = shortcut.into_string();

    {
        let mut config = state.inner().config.write();
        config.autofill = normalized.clone();
        config
            .save()
            .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    }

    register_autofill_shortcut(&app, &normalized)?;
    Ok(normalized)
}

#[tauri::command]
pub async fn resolve_autofill(
    state: State<'_, Arc<AppState>>,
    context: crate::accessibility::FieldContext,
    query_override: Option<String>,
) -> Result<AutofillResolution, String> {
    let settings = {
        let config = state.inner().config.read();
        config.autofill.clone().normalized()
    };
    let (query, query_source) = build_autofill_query(&context, query_override.as_deref());

    let mut resolution = AutofillResolution {
        query: query.clone(),
        query_source: query_source.clone(),
        context_hint: context_hint(&context),
        candidates: Vec::new(),
        auto_inject_threshold: settings.auto_inject_threshold,
        requires_confirmation: false,
        used_ocr_fallback: query_source == "ocr",
    };

    if query.trim().is_empty() {
        return Ok(resolution);
    }

    let time_filter = format!("{}d", settings.lookback_days);
    let aliases = field_aliases(&query);
    let search_query = build_autofill_search_query(&context, &query);
    let results = run_search_query(
        state.inner(),
        &search_query,
        Some(time_filter.as_str()),
        None,
        settings.max_candidates.max(4) * 3,
    )
    .await?;

    let mut drafts = Vec::new();
    for result in &results {
        let alignment = context_alignment_score(&context, result, &query);
        drafts.extend(extract_candidates_from_result(
            &query, &aliases, result, alignment,
        ));
    }

    resolution.candidates = rank_autofill_candidates(
        &query,
        drafts,
        settings.lookback_days,
        settings.max_candidates,
    );
    resolution.requires_confirmation =
        needs_autofill_confirmation(&resolution.candidates, settings.auto_inject_threshold);

    Ok(resolution)
}

#[tauri::command]
pub async fn inject_text(
    _app: AppHandle,
    state: State<'_, Arc<AppState>>,
    text: String,
) -> Result<(), String> {
    // Do NOT hide the overlay here. The frontend shows "injecting" → "done" / "error"
    // toast states that are only visible if the window stays open during injection.
    // The frontend calls dismissAutofill() after the SUCCESS_TOAST_MS / ERROR_TOAST_MS delay.
    //
    // Move all blocking work (osascript activation, enigo CGEvent posting) off the Tokio thread.
    // CGEvent APIs on macOS can crash or silently fail when called from async runtime threads.
    let prefer_typed = state.inner().config.read().autofill.prefer_typed_injection;
    tokio::task::spawn_blocking(move || {
        crate::accessibility::restore_target_app_focus();
        std::thread::sleep(std::time::Duration::from_millis(60));
        crate::accessibility::inject_text_into_field(&text, prefer_typed)
    })
    .await
    .map_err(|e| format!("Injection task failed to join: {e}"))?
}

/// Hide the autofill overlay window.
#[tauri::command]
pub async fn dismiss_autofill(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(AUTOFILL_OVERLAY_LABEL) {
        win.hide().map_err(|e| e.to_string())?;
    }
    PENDING_AUTOFILL_PAYLOAD.lock().take();
    crate::accessibility::restore_target_app_focus();
    Ok(())
}

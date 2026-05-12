//! Shared helpers for Tauri command handlers (`api::commands`).

use crate::embed::{embedding_runtime_status, Embedder, EmbeddingBackend};
use crate::privacy::Blocklist;
use crate::store::SearchResult;
use std::sync::OnceLock;

static SHARED_EMBEDDER: OnceLock<Result<Embedder, String>> = OnceLock::new();

pub(super) fn shared_embedder() -> Result<&'static Embedder, String> {
    match SHARED_EMBEDDER.get_or_init(Embedder::new) {
        Ok(embedder) => Ok(embedder),
        Err(err) => Err(err.clone()),
    }
}

pub(super) fn shared_real_embedder() -> Result<&'static Embedder, String> {
    let embedder = shared_embedder()?;
    if matches!(embedder.backend(), EmbeddingBackend::Real) {
        return Ok(embedder);
    }

    let status = embedding_runtime_status();
    Err(format!(
        "Real embeddings are required before running continuity repair or storage reclaim. Current backend: {}{}{}",
        status.backend,
        if status.degraded { " (degraded)" } else { "" },
        if status.detail.is_empty() {
            String::new()
        } else {
            format!(" - {}", status.detail)
        }
    ))
}

pub(super) fn is_internal_fndr_result(result: &SearchResult) -> bool {
    Blocklist::is_internal_app(&result.app_name, result.bundle_id.as_deref())
}

pub(super) fn strip_internal_fndr_results(mut results: Vec<SearchResult>) -> Vec<SearchResult> {
    results.retain(|result| !is_internal_fndr_result(result));
    results
}

pub(super) fn truncate_chars(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

pub(super) fn normalize_autofill_phrase(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '#' {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn push_unique_case_insensitive(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    let normalized = normalize_autofill_phrase(&value);
    if normalized.is_empty() {
        return;
    }
    if values
        .iter()
        .any(|existing| normalize_autofill_phrase(existing) == normalized)
    {
        return;
    }
    values.push(value);
}

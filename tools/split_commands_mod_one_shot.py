#!/usr/bin/env python3
"""One-shot split of src-tauri/src/api/commands/mod.rs into memory/quality/meeting/export/privacy/stats/autofill."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MOD = ROOT / "src-tauri" / "src" / "api" / "commands" / "mod.rs"
lines = MOD.read_text().splitlines(True)


def sl(a: int, b: int) -> str:
    """1-based inclusive line slice."""
    return "".join(lines[a - 1 : b])


def main() -> None:
    # --- memory.rs ---
    memory_hdr = '''//! Single-memory Tauri commands.

use crate::AppState;
use std::sync::Arc;
use tauri::State;

'''
    (ROOT / "src-tauri/src/api/commands/memory.rs").write_text(memory_hdr + sl(268, 303))

    # --- quality.rs (types + helpers + commands; delete_memory stays in memory.rs) ---
    quality_hdr = '''//! Memory quality, debug inspector, rebuild, retrieval eval.

use super::common::truncate_chars;
use super::search::run_search_query;
use crate::context_runtime;
use crate::memory_quality::classify_storage_outcome;
use crate::search::QueryContext;
use crate::store::MemoryRecord;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::State;

'''
    quality_body = sl(89, 237) + sl(305, 1181)
    (ROOT / "src-tauri/src/api/commands/quality.rs").write_text(quality_hdr + quality_body)

    # --- meeting.rs ---
    meeting_hdr = '''//! Meeting recorder Tauri commands.

use crate::meeting::{self, MeetingRecorderStatus, MeetingTranscript};
use crate::store::MeetingSession;
use std::sync::Arc;
use tauri::{AppHandle, State};

'''
    (ROOT / "src-tauri/src/api/commands/meeting.rs").write_text(meeting_hdr + sl(1263, 1308))

    # --- export.rs (PDF export) ---
    export_hdr = '''//! PDF export helpers and commands.

use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::AppHandle;

use genpdf::Element;

'''
    (ROOT / "src-tauri/src/api/commands/export.rs").write_text(export_hdr + sl(30, 49) + sl(1310, 1434))

    # --- privacy.rs ---
    privacy_hdr = '''//! Blocklist, wipe-all-data, proactive privacy alerts.

use super::common::push_unique_case_insensitive;
use crate::privacy::Blocklist;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

'''
    (ROOT / "src-tauri/src/api/commands/privacy.rs").write_text(privacy_hdr + sl(1467, 1697))

    # --- stats.rs (non-contiguous chunks) ---
    stats_hdr = '''//! Capture status, MCP/context, voice/capture toggles, stats, daily summary, time/focus.

use super::common::{
    shared_embedder, strip_internal_fndr_results, truncate_chars,
};
use super::search::{
    cache_is_fresh, card_domain, card_summary, is_low_signal_summary, is_low_signal_title,
    title_from_summary,
};
use crate::context_runtime;
use crate::embed::{embedding_runtime_status, Embedder, EmbeddingBackend};
use crate::mcp::{self, McpServerStatus};
use crate::privacy::Blocklist;
use crate::speech;
use crate::store::{SearchResult, Stats};
use crate::AppState;
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, State};

'''
    stats_body = sl(51, 67) + sl(77, 81) + sl(1183, 1262) + sl(1436, 1609) + sl(1699, 2178)
    (ROOT / "src-tauri/src/api/commands/stats.rs").write_text(stats_hdr + stats_body)

    # --- autofill.rs ---
    autof_hdr = '''//! Autofill overlay, shortcut, resolution, injection.

use super::common::{
    normalize_autofill_phrase, push_unique_case_insensitive, shared_embedder, truncate_chars,
};
use crate::config::AutofillConfig;
use crate::embed::{Embedder, EmbeddingBackend};
use crate::search::{run_search_query, SearchResult};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

'''
    (ROOT / "src-tauri/src/api/commands/autofill.rs").write_text(autof_hdr + sl(2180, 3232))

    # --- tests stay in mod (last part) ---
    tail = sl(3234, len(lines))

    new_mod = '''//! Tauri command handlers

mod common;
pub mod search;

pub use search::{
    list_memory_cards, search, search_memory_cards, search_raw_results, summarize_search,
};

mod memory;
pub use memory::*;

mod quality;
pub use quality::*;

mod meeting;
pub use meeting::*;

mod export;
pub use export::*;

mod privacy;
pub use privacy::*;

mod stats;
pub use stats::*;

mod autofill;
pub use autofill::*;

mod todos;
pub use todos::*;

mod maintenance;
pub use maintenance::*;

mod hermes_agent;
pub use hermes_agent::*;

''' + tail

    MOD.write_text(new_mod)
    print("Wrote split modules + thin mod.rs, lines:", new_mod.count("\n"))


if __name__ == "__main__":
    main()

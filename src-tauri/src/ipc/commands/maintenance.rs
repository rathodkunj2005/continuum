//! Memory repair, storage reclaim, and storage health Tauri commands.

use super::common::shared_real_embedder;
use crate::capture::{
    continuity_anchor_for_memory, eligible_for_story_merge, merge_memory_records_with_policy,
    passes_merge_threshold, score_memory_candidate,
};
use crate::memory_compaction::{
    best_embedding_text, best_snippet_embedding_text, best_support_embedding_texts,
    compact_memory_record_payload, is_low_signal_embedding, mean_pool_embeddings,
};
use crate::storage::MemoryRecord;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tokio::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRepairSummary {
    pub total_before: usize,
    pub total_after: usize,
    pub merged_count: usize,
    pub anchor_merges: usize,
    pub task_reference_updates: usize,
    pub screenshots_cleaned: usize,
    pub embeddings_refreshed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub chars_reclaimed: usize,
    pub app_merges: Vec<AppMergeCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReclaimSummary {
    pub records_scanned: usize,
    pub records_rewritten: usize,
    pub screenshot_paths_cleared: usize,
    pub screenshot_files_deleted: usize,
    pub embeddings_refreshed: usize,
    pub snippet_embeddings_refreshed: usize,
    #[serde(default)]
    pub support_embeddings_refreshed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub chars_reclaimed: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealth {
    pub memory_db_bytes: u64,
    pub frames_bytes: u64,
    pub models_bytes: u64,
    pub dev_build_cache_bytes: u64,
    pub runtime_total_bytes: u64,
    pub measured_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMergeCount {
    pub app_name: String,
    pub merged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRepairProgress {
    pub is_running: bool,
    pub phase: String,
    pub processed: usize,
    pub total: usize,
    pub merged_count: usize,
    pub anchor_merges: usize,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReclaimProgress {
    pub is_running: bool,
    pub phase: String,
    pub processed: usize,
    pub total: usize,
    pub records_rewritten: usize,
    pub screenshot_paths_cleared: usize,
    pub screenshot_files_deleted: usize,
    pub embeddings_refreshed: usize,
    pub snippet_embeddings_refreshed: usize,
    #[serde(default)]
    pub support_embeddings_refreshed: usize,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryRepairCheckpoint {
    version: u32,
    source_total: usize,
    source_fingerprint: u64,
    source_first_id: String,
    source_last_id: String,
    processed: usize,
    merged_memories: Vec<MemoryRecord>,
    id_redirect: HashMap<String, String>,
    merged_count: usize,
    anchor_merges: usize,
    app_merge_counts: HashMap<String, usize>,
}
const MEMORY_REPAIR_CHECKPOINT_VERSION: u32 = 5;
const MEMORY_REPAIR_SIMILARITY_SCAN_LIMIT: usize = 96;
const MEMORY_REPAIR_CHECKPOINT_ITEM_STEP: usize = 300;
const MEMORY_REPAIR_CHECKPOINT_MS: u64 = 12_000;
const STORAGE_RECLAIM_HEARTBEAT_ITEM_STEP: usize = 72;
const STORAGE_RECLAIM_HEARTBEAT_MS: u64 = 850;
const STORAGE_RECLAIM_EMBED_BATCH: usize = 48;
static MEMORY_REPAIR_RUNNING: AtomicBool = AtomicBool::new(false);
static STORAGE_RECLAIM_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn run_memory_repair_backfill(
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryRepairSummary, String> {
    run_memory_repair_backfill_for_state(state.inner().clone()).await
}

async fn run_memory_repair_backfill_for_state(
    state: Arc<AppState>,
) -> Result<MemoryRepairSummary, String> {
    let embedder = shared_real_embedder()?;
    if MEMORY_REPAIR_RUNNING.swap(true, Ordering::AcqRel) {
        return Err("Memory continuity repair is already running".to_string());
    }
    struct MemoryRepairRunGuard;
    impl Drop for MemoryRepairRunGuard {
        fn drop(&mut self) {
            MEMORY_REPAIR_RUNNING.store(false, Ordering::Release);
        }
    }
    let _run_guard = MemoryRepairRunGuard;

    let should_resume_capture = !state.is_paused.load(Ordering::SeqCst);
    if should_resume_capture {
        state.pause();
    }
    struct CaptureResumeGuard {
        state: Arc<AppState>,
        should_resume: bool,
    }
    impl Drop for CaptureResumeGuard {
        fn drop(&mut self) {
            if self.should_resume {
                self.state.resume();
            }
        }
    }
    let _capture_resume_guard = CaptureResumeGuard {
        state: state.clone(),
        should_resume: should_resume_capture,
    };

    let progress_path = memory_repair_progress_path(state.as_ref());
    let checkpoint_path = memory_repair_checkpoint_path(state.as_ref());
    let mut all_memories = state
        .store
        .list_all_memories()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    if all_memories.is_empty() {
        let _ = std::fs::remove_file(&checkpoint_path);
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "complete".to_string(),
                processed: 0,
                total: 0,
                merged_count: 0,
                anchor_merges: 0,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        return Ok(MemoryRepairSummary {
            total_before: 0,
            total_after: 0,
            merged_count: 0,
            anchor_merges: 0,
            task_reference_updates: 0,
            screenshots_cleaned: 0,
            embeddings_refreshed: 0,
            chars_before: 0,
            chars_after: 0,
            chars_reclaimed: 0,
            app_merges: Vec::new(),
        });
    }

    all_memories.sort_by_key(|memory| memory.timestamp);
    let before_count = all_memories.len();
    let chars_before = all_memories
        .iter()
        .map(|memory| memory.text.chars().count() + memory.clean_text.chars().count())
        .sum::<usize>();
    let source_fingerprint = memory_repair_source_fingerprint(&all_memories);
    let source_first_id = all_memories
        .first()
        .map(|memory| memory.id.clone())
        .unwrap_or_default();
    let source_last_id = all_memories
        .last()
        .map(|memory| memory.id.clone())
        .unwrap_or_default();

    let before_screenshots: HashSet<String> = all_memories
        .iter()
        .filter_map(|memory| memory.screenshot_path.clone())
        .collect();

    let backfill_engine: Option<&Arc<crate::inference::InferenceEngine>> = None;

    let mut merged_memories: Vec<MemoryRecord> = Vec::with_capacity(before_count);
    let mut anchor_index: HashMap<String, usize> = HashMap::new();
    let mut app_index: HashMap<String, Vec<usize>> = HashMap::new();
    let mut id_redirect: HashMap<String, String> = HashMap::new();
    let mut processed = 0usize;
    let mut resumed_from_checkpoint = false;

    let mut merged_count = 0usize;
    let mut anchor_merges = 0usize;
    let mut embeddings_refreshed = 0usize;
    let mut app_merge_counts: HashMap<String, usize> = HashMap::new();

    if let Some(checkpoint) = load_memory_repair_checkpoint(&checkpoint_path) {
        let checkpoint_valid = (checkpoint.version == MEMORY_REPAIR_CHECKPOINT_VERSION
            || checkpoint.version == 4
            || checkpoint.version == 1)
            && checkpoint.source_total == before_count
            && checkpoint.source_fingerprint == source_fingerprint
            && checkpoint.source_first_id == source_first_id
            && checkpoint.source_last_id == source_last_id
            && checkpoint.processed <= before_count
            && checkpoint.merged_memories.len() <= checkpoint.processed
            && checkpoint.id_redirect.len() <= checkpoint.processed;

        if checkpoint_valid {
            merged_memories = checkpoint.merged_memories;
            id_redirect = checkpoint.id_redirect;
            processed = checkpoint.processed;
            merged_count = checkpoint.merged_count;
            anchor_merges = checkpoint.anchor_merges;
            app_merge_counts = checkpoint.app_merge_counts;

            for (index, memory) in merged_memories.iter().enumerate() {
                if let Some(anchor) = continuity_anchor_for_memory(memory) {
                    anchor_index.insert(anchor, index);
                }
                app_index
                    .entry(memory.app_name.to_lowercase())
                    .or_default()
                    .push(index);
            }

            resumed_from_checkpoint = true;
            tracing::info!(
                "memory_repair_backfill: resumed from checkpoint at {}/{}",
                processed,
                before_count
            );
        } else {
            let _ = std::fs::remove_file(&checkpoint_path);
        }
    }

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: true,
            phase: if resumed_from_checkpoint {
                "resuming".to_string()
            } else {
                "scanning".to_string()
            },
            processed,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );

    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_secs(1);
    let heartbeat_count_step = 75usize;
    let checkpoint_interval = Duration::from_millis(MEMORY_REPAIR_CHECKPOINT_MS);
    let mut last_checkpoint = Instant::now();

    for incoming in all_memories.into_iter().skip(processed) {
        processed += 1;
        let incoming_id = incoming.id.clone();
        let normalized_app = incoming.app_name.to_lowercase();
        let incoming_anchor = continuity_anchor_for_memory(&incoming);
        let mut merged_into_idx: Option<usize> = None;

        if eligible_for_story_merge(&incoming) {
            if let Some(anchor) = incoming_anchor.as_ref() {
                if let Some(index) = anchor_index.get(anchor).copied() {
                    if merged_memories
                        .get(index)
                        .map(|existing| existing.app_name == incoming.app_name)
                        .unwrap_or(false)
                    {
                        merged_into_idx = Some(index);
                        anchor_merges += 1;
                    }
                }
            }

            if merged_into_idx.is_none() {
                if let Some(candidates) = app_index.get(&normalized_app) {
                    let mut best: Option<(usize, f32)> = None;
                    for candidate_index in candidates
                        .iter()
                        .rev()
                        .take(MEMORY_REPAIR_SIMILARITY_SCAN_LIMIT)
                    {
                        let existing = &merged_memories[*candidate_index];
                        let score = score_memory_candidate(&incoming, existing);
                        if !passes_merge_threshold(score) {
                            continue;
                        }
                        if best
                            .as_ref()
                            .map(|(_, best_score)| score.score > *best_score)
                            .unwrap_or(true)
                        {
                            best = Some((*candidate_index, score.score));
                        }
                    }
                    merged_into_idx = best.map(|(index, _)| index);
                }
            }
        }

        if let Some(target_index) = merged_into_idx {
            let existing_id = merged_memories[target_index].id.clone();
            let merged = merge_memory_records_with_policy(
                merged_memories[target_index].clone(),
                incoming.clone(),
                Some(embedder),
                backfill_engine,
                true,
                false,
            )
            .await;
            merged_memories[target_index] = merged.clone();
            id_redirect.insert(incoming_id, existing_id);
            merged_count += 1;

            *app_merge_counts
                .entry(incoming.app_name.clone())
                .or_insert(0) += 1;

            if let Some(anchor) = continuity_anchor_for_memory(&merged) {
                anchor_index.insert(anchor, target_index);
            }
            if processed % heartbeat_count_step == 0
                || last_heartbeat.elapsed() >= heartbeat_interval
            {
                tracing::info!(
                    "memory_repair_backfill:progress processed={} total={} merged={} anchor_merges={}",
                    processed,
                    before_count,
                    merged_count,
                    anchor_merges
                );
                persist_memory_repair_progress(
                    &progress_path,
                    &MemoryRepairProgress {
                        is_running: true,
                        phase: "scanning".to_string(),
                        processed,
                        total: before_count,
                        merged_count,
                        anchor_merges,
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    },
                );
                last_heartbeat = Instant::now();
            }

            if processed % MEMORY_REPAIR_CHECKPOINT_ITEM_STEP == 0
                || last_checkpoint.elapsed() >= checkpoint_interval
            {
                persist_memory_repair_checkpoint(
                    &checkpoint_path,
                    &MemoryRepairCheckpoint {
                        version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                        source_total: before_count,
                        source_fingerprint,
                        source_first_id: source_first_id.clone(),
                        source_last_id: source_last_id.clone(),
                        processed,
                        merged_memories: merged_memories.clone(),
                        id_redirect: id_redirect.clone(),
                        merged_count,
                        anchor_merges,
                        app_merge_counts: app_merge_counts.clone(),
                    },
                );
                last_checkpoint = Instant::now();
            }
            continue;
        }

        let index = merged_memories.len();
        if let Some(anchor) = incoming_anchor {
            anchor_index.insert(anchor, index);
        }
        app_index.entry(normalized_app).or_default().push(index);
        merged_memories.push(incoming);

        if processed % heartbeat_count_step == 0 || last_heartbeat.elapsed() >= heartbeat_interval {
            tracing::info!(
                "memory_repair_backfill:progress processed={} total={} merged={} anchor_merges={}",
                processed,
                before_count,
                merged_count,
                anchor_merges
            );
            persist_memory_repair_progress(
                &progress_path,
                &MemoryRepairProgress {
                    is_running: true,
                    phase: "scanning".to_string(),
                    processed,
                    total: before_count,
                    merged_count,
                    anchor_merges,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            last_heartbeat = Instant::now();
        }

        if processed % MEMORY_REPAIR_CHECKPOINT_ITEM_STEP == 0
            || last_checkpoint.elapsed() >= checkpoint_interval
        {
            persist_memory_repair_checkpoint(
                &checkpoint_path,
                &MemoryRepairCheckpoint {
                    version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                    source_total: before_count,
                    source_fingerprint,
                    source_first_id: source_first_id.clone(),
                    source_last_id: source_last_id.clone(),
                    processed,
                    merged_memories: merged_memories.clone(),
                    id_redirect: id_redirect.clone(),
                    merged_count,
                    anchor_merges,
                    app_merge_counts: app_merge_counts.clone(),
                },
            );
            last_checkpoint = Instant::now();
        }
    }

    for memory in &mut merged_memories {
        if is_low_signal_embedding(&memory.embedding) {
            let text_input = best_embedding_text(memory);
            if !text_input.is_empty() {
                if let Ok(mut vectors) = embedder.embed_batch_with_context(&[(
                    memory.app_name.clone(),
                    memory.window_title.clone(),
                    text_input,
                )]) {
                    if let Some(vector) = vectors.pop() {
                        memory.embedding = vector;
                        embeddings_refreshed += 1;
                    }
                }
            }
        }

        if is_low_signal_embedding(&memory.snippet_embedding) {
            let snippet_input = best_snippet_embedding_text(memory);
            if !snippet_input.is_empty() {
                if let Ok(mut vectors) = embedder.embed_batch_with_context(&[(
                    memory.app_name.clone(),
                    memory.window_title.clone(),
                    snippet_input,
                )]) {
                    if let Some(vector) = vectors.pop() {
                        memory.snippet_embedding = vector;
                        embeddings_refreshed += 1;
                    }
                }
            }
        }

        if is_low_signal_embedding(&memory.support_embedding) {
            let support_inputs = best_support_embedding_texts(memory);
            if !support_inputs.is_empty() {
                let contexts = support_inputs
                    .into_iter()
                    .map(|text| (memory.app_name.clone(), memory.window_title.clone(), text))
                    .collect::<Vec<_>>();
                if let Ok(vectors) = embedder.embed_batch_with_context(&contexts) {
                    memory.support_embedding = mean_pool_embeddings(&vectors);
                    embeddings_refreshed += 1;
                }
            }
        }
    }

    let chars_after = merged_memories
        .iter()
        .map(compact_memory_record_payload)
        .map(|memory| memory.text.chars().count() + memory.clean_text.chars().count())
        .sum::<usize>();

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: true,
            phase: "writing".to_string(),
            processed: before_count,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );
    persist_memory_repair_checkpoint(
        &checkpoint_path,
        &MemoryRepairCheckpoint {
            version: MEMORY_REPAIR_CHECKPOINT_VERSION,
            source_total: before_count,
            source_fingerprint,
            source_first_id: source_first_id.clone(),
            source_last_id: source_last_id.clone(),
            processed: before_count,
            merged_memories: merged_memories.clone(),
            id_redirect: id_redirect.clone(),
            merged_count,
            anchor_merges,
            app_merge_counts: app_merge_counts.clone(),
        },
    );

    if let Err(err) = state.store.delete_all().await {
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "error".to_string(),
                processed,
                total: before_count,
                merged_count,
                anchor_merges,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        persist_memory_repair_checkpoint(
            &checkpoint_path,
            &MemoryRepairCheckpoint {
                version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                source_total: before_count,
                source_fingerprint,
                source_first_id: source_first_id.clone(),
                source_last_id: source_last_id.clone(),
                processed,
                merged_memories,
                id_redirect,
                merged_count,
                anchor_merges,
                app_merge_counts,
            },
        );
        return Err(err.to_string());
    }
    state.invalidate_memory_derived_caches();
    if let Err(err) = state.store.add_batch_preserving_ids(&merged_memories).await {
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "error".to_string(),
                processed,
                total: before_count,
                merged_count,
                anchor_merges,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        persist_memory_repair_checkpoint(
            &checkpoint_path,
            &MemoryRepairCheckpoint {
                version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                source_total: before_count,
                source_fingerprint,
                source_first_id: source_first_id.clone(),
                source_last_id: source_last_id.clone(),
                processed,
                merged_memories,
                id_redirect,
                merged_count,
                anchor_merges,
                app_merge_counts,
            },
        );
        return Err(err.to_string());
    }
    state.invalidate_memory_derived_caches();

    let after_screenshots: HashSet<String> = merged_memories
        .iter()
        .map(compact_memory_record_payload)
        .filter_map(|memory| memory.screenshot_path)
        .collect();
    let screenshots_cleaned = before_screenshots
        .difference(&after_screenshots)
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count();

    let mut task_reference_updates = 0usize;
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    for task in &mut tasks {
        if let Some(source_id) = task.source_memory_id.clone() {
            if let Some(new_id) = id_redirect.get(&source_id) {
                if new_id != &source_id {
                    task.source_memory_id = Some(new_id.clone());
                    task_reference_updates += 1;
                }
            }
        }

        if !task.linked_memory_ids.is_empty() {
            let before = task.linked_memory_ids.clone();
            let mut seen = HashSet::new();
            let rewritten: Vec<String> = before
                .iter()
                .map(|memory_id| {
                    id_redirect
                        .get(memory_id)
                        .cloned()
                        .unwrap_or_else(|| memory_id.clone())
                })
                .filter(|memory_id| seen.insert(memory_id.clone()))
                .collect();
            if rewritten != before {
                task_reference_updates += before
                    .iter()
                    .zip(rewritten.iter())
                    .filter(|(left, right)| left != right)
                    .count()
                    + before.len().saturating_sub(rewritten.len());
                task.linked_memory_ids = rewritten;
            }
        }
    }

    if task_reference_updates > 0 {
        if let Err(err) = state.store.upsert_tasks(&tasks).await {
            persist_memory_repair_progress(
                &progress_path,
                &MemoryRepairProgress {
                    is_running: false,
                    phase: "error".to_string(),
                    processed: before_count,
                    total: before_count,
                    merged_count,
                    anchor_merges,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            persist_memory_repair_checkpoint(
                &checkpoint_path,
                &MemoryRepairCheckpoint {
                    version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                    source_total: before_count,
                    source_fingerprint,
                    source_first_id: source_first_id.clone(),
                    source_last_id: source_last_id.clone(),
                    processed: before_count,
                    merged_memories,
                    id_redirect,
                    merged_count,
                    anchor_merges,
                    app_merge_counts,
                },
            );
            return Err(err.to_string());
        }
    }

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: false,
            phase: "complete".to_string(),
            processed: before_count,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );
    let _ = std::fs::remove_file(&checkpoint_path);

    let mut app_merges: Vec<AppMergeCount> = app_merge_counts
        .into_iter()
        .map(|(app_name, merged)| AppMergeCount { app_name, merged })
        .collect();
    app_merges.sort_by(|left, right| right.merged.cmp(&left.merged));

    Ok(MemoryRepairSummary {
        total_before: before_count,
        total_after: merged_memories.len(),
        merged_count,
        anchor_merges,
        task_reference_updates,
        screenshots_cleaned,
        embeddings_refreshed,
        chars_before,
        chars_after,
        chars_reclaimed: chars_before.saturating_sub(chars_after),
        app_merges,
    })
}
fn memory_repair_source_fingerprint(memories: &[MemoryRecord]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for memory in memories {
        for byte in memory.id.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for byte in memory.timestamp.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn memory_repair_progress_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("memory_repair_progress.json")
}

fn memory_repair_checkpoint_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("memory_repair_checkpoint.json")
}

fn storage_reclaim_progress_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("storage_reclaim_progress.json")
}

fn persist_memory_repair_progress(path: &PathBuf, progress: &MemoryRepairProgress) {
    if let Ok(serialized) = serde_json::to_string_pretty(progress) {
        let _ = std::fs::write(path, serialized);
    }
}

fn persist_storage_reclaim_progress(path: &PathBuf, progress: &StorageReclaimProgress) {
    if let Ok(serialized) = serde_json::to_string_pretty(progress) {
        let _ = std::fs::write(path, serialized);
    }
}

fn persist_memory_repair_checkpoint(path: &PathBuf, checkpoint: &MemoryRepairCheckpoint) {
    let tmp = path.with_extension("json.tmp");
    if let Ok(serialized) = serde_json::to_string(checkpoint) {
        if std::fs::write(&tmp, serialized).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn load_memory_repair_checkpoint(path: &PathBuf) -> Option<MemoryRepairCheckpoint> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<MemoryRepairCheckpoint>(&content).ok()
}

fn recursive_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    }

    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn recursive_file_count(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return 1;
    }

    let mut total = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else {
                total = total.saturating_add(1);
            }
        }
    }
    total
}

fn memory_payload_bytes(state: &AppState) -> u64 {
    recursive_size(
        &state
            .store
            .data_dir()
            .join("lancedb")
            .join("memories.lance"),
    )
    .saturating_add(recursive_size(&state.store.frames_dir()))
}

fn dev_build_cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")
}

fn storage_health_for_state(state: &AppState) -> StorageHealth {
    let data_dir = state.store.data_dir();
    let memory_db_bytes = recursive_size(&data_dir.join("lancedb"));
    let frames_bytes = recursive_size(&state.store.frames_dir());
    let models_bytes = recursive_size(&data_dir.join("models"))
        .saturating_add(recursive_size(&data_dir.join("speech_models")));
    let runtime_total_bytes = recursive_size(&data_dir);
    let dev_build_cache_bytes = recursive_size(&dev_build_cache_dir());

    StorageHealth {
        memory_db_bytes,
        frames_bytes,
        models_bytes,
        dev_build_cache_bytes,
        runtime_total_bytes,
        measured_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}

#[tauri::command]
pub async fn get_storage_health(state: State<'_, Arc<AppState>>) -> Result<StorageHealth, String> {
    Ok(storage_health_for_state(state.inner()))
}

#[tauri::command]
pub async fn clean_dev_build_cache(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageHealth, String> {
    let target_dir = dev_build_cache_dir();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !target_dir.starts_with(&manifest_dir) || target_dir == manifest_dir {
        return Err("Refusing to clean an unexpected dev cache path.".to_string());
    }

    let before = recursive_size(&target_dir);
    let target_for_task = target_dir.clone();
    tokio::task::spawn_blocking(move || {
        if target_for_task.exists() {
            std::fs::remove_dir_all(&target_for_task)
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|err| format!("Dev cache cleanup task failed: {err}"))?
    .map_err(|err| format!("Failed to remove dev build cache: {err}"))?;

    tracing::info!(
        bytes_before = before,
        path = %target_dir.display(),
        "dev_build_cache:cleaned"
    );

    Ok(storage_health_for_state(state.inner()))
}

#[tauri::command]
pub async fn get_memory_repair_progress(
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryRepairProgress, String> {
    let path = memory_repair_progress_path(state.inner());
    if !path.exists() {
        return Ok(MemoryRepairProgress {
            is_running: false,
            phase: "idle".to_string(),
            processed: 0,
            total: 0,
            merged_count: 0,
            anchor_merges: 0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut progress: MemoryRepairProgress =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // If heartbeat is stale for over 2 minutes, mark as not running.
    if progress.is_running {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms.saturating_sub(progress.timestamp_ms) > 120_000 {
            progress.is_running = false;
            progress.phase = "stale".to_string();
            progress.timestamp_ms = now_ms;
            persist_memory_repair_progress(&path, &progress);
        }
    }

    Ok(progress)
}

#[tauri::command]
pub async fn get_storage_reclaim_progress(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageReclaimProgress, String> {
    let path = storage_reclaim_progress_path(state.inner());
    if !path.exists() {
        return Ok(StorageReclaimProgress {
            is_running: false,
            phase: "idle".to_string(),
            processed: 0,
            total: 0,
            records_rewritten: 0,
            screenshot_paths_cleared: 0,
            screenshot_files_deleted: 0,
            embeddings_refreshed: 0,
            snippet_embeddings_refreshed: 0,
            support_embeddings_refreshed: 0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut progress: StorageReclaimProgress =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;

    if progress.is_running {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms.saturating_sub(progress.timestamp_ms) > 120_000 {
            progress.is_running = false;
            progress.phase = "stale".to_string();
            progress.timestamp_ms = now_ms;
            persist_storage_reclaim_progress(&path, &progress);
        }
    }

    Ok(progress)
}

#[tauri::command]
pub async fn reclaim_memory_storage(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageReclaimSummary, String> {
    reclaim_memory_storage_for_state(state.inner().clone(), true).await
}

pub async fn reclaim_memory_storage_silent(
    state: Arc<AppState>,
) -> Result<StorageReclaimSummary, String> {
    reclaim_memory_storage_for_state(state, false).await
}

async fn reclaim_memory_storage_for_state(
    state: Arc<AppState>,
    publish_progress: bool,
) -> Result<StorageReclaimSummary, String> {
    shared_real_embedder()?;
    if STORAGE_RECLAIM_RUNNING.swap(true, Ordering::AcqRel) {
        return Err("Storage reclaim is already running".to_string());
    }
    struct StorageReclaimRunGuard;
    impl Drop for StorageReclaimRunGuard {
        fn drop(&mut self) {
            STORAGE_RECLAIM_RUNNING.store(false, Ordering::Release);
        }
    }
    let _run_guard = StorageReclaimRunGuard;

    let progress_path = storage_reclaim_progress_path(state.as_ref());
    let mut progress = StorageReclaimProgress {
        is_running: true,
        phase: "starting".to_string(),
        processed: 0,
        total: 0,
        records_rewritten: 0,
        screenshot_paths_cleared: 0,
        screenshot_files_deleted: 0,
        embeddings_refreshed: 0,
        snippet_embeddings_refreshed: 0,
        support_embeddings_refreshed: 0,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    };
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    progress.phase = "repairing_prerequisite".to_string();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }
    if let Err(err) = run_memory_repair_backfill_for_state(state.clone()).await {
        progress.is_running = false;
        progress.phase = "error".to_string();
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }
        return Err(err);
    }

    let should_resume_capture = !state.is_paused.load(Ordering::SeqCst);
    if should_resume_capture {
        state.pause();
    }
    struct CaptureResumeGuard {
        state: Arc<AppState>,
        should_resume: bool,
    }
    impl Drop for CaptureResumeGuard {
        fn drop(&mut self) {
            if self.should_resume {
                self.state.resume();
            }
        }
    }
    let _capture_resume_guard = CaptureResumeGuard {
        state: state.clone(),
        should_resume: should_resume_capture,
    };

    tracing::info!("storage_reclaim:start");
    progress.phase = "loading".to_string();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let reclaim_started = Instant::now();
    let bytes_before = memory_payload_bytes(state.as_ref());
    let embedder = shared_real_embedder()?;
    let memories = match state.store.list_all_memories().await {
        Ok(memories) => memories,
        Err(err) => {
            progress.is_running = false;
            progress.phase = "error".to_string();
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
            return Err(err.to_string());
        }
    };

    let mut rewritten_memories = Vec::with_capacity(memories.len());
    let mut changed_flags = Vec::with_capacity(memories.len());
    let mut records_rewritten = 0usize;
    let mut screenshot_paths_cleared = 0usize;
    let mut screenshot_files_deleted = 0usize;
    let mut embeddings_refreshed = 0usize;
    let mut snippet_embeddings_refreshed = 0usize;
    let mut support_embeddings_refreshed = 0usize;
    let mut chars_before = 0usize;
    let mut chars_after = 0usize;
    let mut text_embedding_jobs: Vec<(usize, (String, String, String))> = Vec::new();
    let mut snippet_embedding_jobs: Vec<(usize, (String, String, String))> = Vec::new();
    let mut support_embedding_jobs: Vec<(usize, String, String, Vec<String>)> = Vec::new();

    let frames_dir = state.store.frames_dir();
    let mut external_screenshot_paths: HashSet<PathBuf> = HashSet::new();

    progress.phase = "compacting".to_string();
    progress.total = memories.len();
    progress.processed = 0;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }
    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_millis(STORAGE_RECLAIM_HEARTBEAT_MS);

    for memory in memories {
        chars_before = chars_before
            .saturating_add(memory.text.chars().count() + memory.clean_text.chars().count());

        let compacted = compact_memory_record_payload(&memory);
        let mut changed = compacted.text != memory.text
            || compacted.clean_text != memory.clean_text
            || compacted.screenshot_path != memory.screenshot_path;

        if let Some(path) = memory.screenshot_path.as_deref() {
            screenshot_paths_cleared = screenshot_paths_cleared.saturating_add(1);
            let screenshot_path = PathBuf::from(path);
            if !screenshot_path.starts_with(&frames_dir) {
                external_screenshot_paths.insert(screenshot_path);
            }
            changed = true;
        }

        if is_low_signal_embedding(&compacted.embedding) {
            let embedding_text = best_embedding_text(&memory);
            if !embedding_text.is_empty() {
                text_embedding_jobs.push((
                    rewritten_memories.len(),
                    (
                        memory.app_name.clone(),
                        memory.window_title.clone(),
                        embedding_text,
                    ),
                ));
            }
        }

        let snippet_input = best_snippet_embedding_text(&compacted);
        if !snippet_input.is_empty() {
            snippet_embedding_jobs.push((
                rewritten_memories.len(),
                (
                    compacted.app_name.clone(),
                    compacted.window_title.clone(),
                    snippet_input,
                ),
            ));
        }

        if is_low_signal_embedding(&compacted.support_embedding) {
            let support_inputs = best_support_embedding_texts(&compacted);
            if !support_inputs.is_empty() {
                support_embedding_jobs.push((
                    rewritten_memories.len(),
                    compacted.app_name.clone(),
                    compacted.window_title.clone(),
                    support_inputs,
                ));
            }
        }

        chars_after = chars_after
            .saturating_add(compacted.text.chars().count() + compacted.clean_text.chars().count());
        rewritten_memories.push(compacted);

        changed_flags.push(changed);
        if changed {
            records_rewritten = records_rewritten.saturating_add(1);
        }

        progress.processed = rewritten_memories.len();
        progress.records_rewritten = records_rewritten;
        progress.screenshot_paths_cleared = screenshot_paths_cleared;
        if progress.processed % STORAGE_RECLAIM_HEARTBEAT_ITEM_STEP == 0
            || last_heartbeat.elapsed() >= heartbeat_interval
        {
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
            last_heartbeat = Instant::now();
        }
    }

    tracing::info!(
        "storage_reclaim:compacted records={} rewritten={} text_jobs={} snippet_jobs={} support_jobs={} elapsed_ms={}",
        rewritten_memories.len(),
        records_rewritten,
        text_embedding_jobs.len(),
        snippet_embedding_jobs.len(),
        support_embedding_jobs.len(),
        reclaim_started.elapsed().as_millis()
    );

    let total_embedding_jobs =
        text_embedding_jobs.len() + snippet_embedding_jobs.len() + support_embedding_jobs.len();
    if total_embedding_jobs > 0 {
        progress.phase = "embedding".to_string();
        progress.total = total_embedding_jobs;
        progress.processed = 0;
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }

        let mut embedded_jobs = 0usize;

        for chunk in text_embedding_jobs.chunks(STORAGE_RECLAIM_EMBED_BATCH) {
            let contexts = chunk
                .iter()
                .map(|(_, context)| context.clone())
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    let vector_count = vectors.len();
                    for ((record_index, _), vector) in chunk.iter().zip(vectors.into_iter()) {
                        rewritten_memories[*record_index].embedding = vector;
                        if !changed_flags[*record_index] {
                            changed_flags[*record_index] = true;
                            records_rewritten = records_rewritten.saturating_add(1);
                        }
                        embeddings_refreshed = embeddings_refreshed.saturating_add(1);
                    }
                    if vector_count != chunk.len() {
                        tracing::warn!(
                            "storage_reclaim:text embedding chunk mismatch vectors={} chunk={}",
                            vector_count,
                            chunk.len()
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:text embedding chunk failed ({} items): {}",
                        chunk.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(chunk.len());
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.embeddings_refreshed = embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }

        for chunk in snippet_embedding_jobs.chunks(STORAGE_RECLAIM_EMBED_BATCH) {
            let contexts = chunk
                .iter()
                .map(|(_, context)| context.clone())
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    let vector_count = vectors.len();
                    for ((record_index, _), vector) in chunk.iter().zip(vectors.into_iter()) {
                        rewritten_memories[*record_index].snippet_embedding = vector;
                        if !changed_flags[*record_index] {
                            changed_flags[*record_index] = true;
                            records_rewritten = records_rewritten.saturating_add(1);
                        }
                        snippet_embeddings_refreshed =
                            snippet_embeddings_refreshed.saturating_add(1);
                    }
                    if vector_count != chunk.len() {
                        tracing::warn!(
                            "storage_reclaim:snippet embedding chunk mismatch vectors={} chunk={}",
                            vector_count,
                            chunk.len()
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:snippet embedding chunk failed ({} items): {}",
                        chunk.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(chunk.len());
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }

        for (record_index, app_name, window_title, support_inputs) in support_embedding_jobs {
            let contexts = support_inputs
                .into_iter()
                .map(|text| (app_name.clone(), window_title.clone(), text))
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    rewritten_memories[record_index].support_embedding =
                        mean_pool_embeddings(&vectors);
                    if !changed_flags[record_index] {
                        changed_flags[record_index] = true;
                        records_rewritten = records_rewritten.saturating_add(1);
                    }
                    support_embeddings_refreshed = support_embeddings_refreshed.saturating_add(1);
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:support embedding failed for {} inputs: {}",
                        contexts.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(1);
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.support_embeddings_refreshed = support_embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }
    }

    if !external_screenshot_paths.is_empty() {
        progress.phase = "purging_external_files".to_string();
        progress.total = external_screenshot_paths.len();
        progress.processed = 0;
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }

        for path in external_screenshot_paths {
            if std::fs::remove_file(path).is_ok() {
                screenshot_files_deleted = screenshot_files_deleted.saturating_add(1);
            }
            progress.processed = progress.processed.saturating_add(1);
            progress.screenshot_files_deleted = screenshot_files_deleted;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }
    }

    progress.phase = "writing".to_string();
    progress.total = rewritten_memories.len();
    progress.processed = 0;
    progress.records_rewritten = records_rewritten;
    progress.embeddings_refreshed = embeddings_refreshed;
    progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
    progress.support_embeddings_refreshed = support_embeddings_refreshed;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let write_result = if records_rewritten > 0 {
        state
            .store
            .replace_all_memories_preserving_ids(&rewritten_memories)
            .await
    } else {
        Ok(())
    };
    if let Err(err) = write_result {
        progress.is_running = false;
        progress.phase = "error".to_string();
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }
        return Err(err);
    }

    progress.processed = rewritten_memories.len();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    progress.phase = "purging_frames".to_string();
    progress.total = 1;
    progress.processed = 0;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let frame_files_before = recursive_file_count(&frames_dir);
    if frames_dir.exists() && std::fs::remove_dir_all(&frames_dir).is_ok() {
        screenshot_files_deleted = screenshot_files_deleted.saturating_add(frame_files_before);
    }
    let _ = std::fs::create_dir_all(&frames_dir);

    progress.processed = 1;
    progress.screenshot_files_deleted = screenshot_files_deleted;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let bytes_after = memory_payload_bytes(state.as_ref());

    tracing::info!(
        "storage_reclaim:complete scanned={} rewritten={} screenshots_deleted={} bytes_reclaimed={} elapsed_ms={}",
        rewritten_memories.len(),
        records_rewritten,
        screenshot_files_deleted,
        bytes_before.saturating_sub(bytes_after),
        reclaim_started.elapsed().as_millis()
    );

    progress.is_running = false;
    progress.phase = "complete".to_string();
    progress.total = rewritten_memories.len();
    progress.processed = rewritten_memories.len();
    progress.records_rewritten = records_rewritten;
    progress.screenshot_paths_cleared = screenshot_paths_cleared;
    progress.screenshot_files_deleted = screenshot_files_deleted;
    progress.embeddings_refreshed = embeddings_refreshed;
    progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
    progress.support_embeddings_refreshed = support_embeddings_refreshed;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    Ok(StorageReclaimSummary {
        records_scanned: rewritten_memories.len(),
        records_rewritten,
        screenshot_paths_cleared,
        screenshot_files_deleted,
        embeddings_refreshed,
        snippet_embeddings_refreshed,
        support_embeddings_refreshed,
        chars_before,
        chars_after,
        chars_reclaimed: chars_before.saturating_sub(chars_after),
        bytes_before,
        bytes_after,
        bytes_reclaimed: bytes_before.saturating_sub(bytes_after),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleWikiCompileSummary {
    pub pages_upserted: usize,
}

/// Compile and persist wiki/knowledge pages during an explicit idle maintenance action.
#[tauri::command]
pub async fn run_idle_wiki_knowledge_compile(
    state: State<'_, Arc<AppState>>,
) -> Result<IdleWikiCompileSummary, String> {
    let pages = crate::context_runtime::compile_knowledge_pages(state.inner(), None).await?;
    if pages.is_empty() {
        return Ok(IdleWikiCompileSummary { pages_upserted: 0 });
    }
    let n = pages.len();
    state.inner().store.upsert_knowledge_pages(&pages).await?;
    Ok(IdleWikiCompileSummary { pages_upserted: n })
}

use crate::inference::model_config::CLEANUP_OLD_MODEL_DIRS;

#[tauri::command]
pub async fn models_cleanup_dry_run(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    use tauri::Manager;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let models_dir = app_data_dir.join("models");
    if !models_dir.exists() {
        return Ok(vec!["models/ directory does not exist".to_string()]);
    }
    let mut found = Vec::new();
    for name in CLEANUP_OLD_MODEL_DIRS {
        let dir = models_dir.join(name);
        if dir.exists() {
            let size = dir_size_bytes_cleanup(&dir).unwrap_or(0);
            found.push(format!("{} ({:.1} MB)", name, size as f64 / 1_000_000.0));
        }
    }
    for flat in &[
        "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
        "SmolVLM-500M-Instruct-Q4_K_M.gguf",
        "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        "bge-large-en-v1.5-quantized.onnx",
    ] {
        let path = models_dir.join(flat);
        if path.exists() {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            found.push(format!("{} ({:.1} MB)", flat, size as f64 / 1_000_000.0));
        }
    }
    if found.is_empty() {
        found.push("No old model files found.".to_string());
    }
    Ok(found)
}

#[tauri::command]
pub async fn models_cleanup_confirm(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    use tauri::Manager;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let models_dir = app_data_dir.join("models");
    if !models_dir.exists() {
        return Ok(vec!["models/ directory does not exist".to_string()]);
    }
    let mut removed = Vec::new();
    for name in CLEANUP_OLD_MODEL_DIRS {
        let dir = models_dir.join(name);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            removed.push(format!("Removed directory: {}", dir.display()));
        }
    }
    for flat in &[
        "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
        "SmolVLM-500M-Instruct-Q4_K_M.gguf",
        "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        "bge-large-en-v1.5-quantized.onnx",
    ] {
        let path = models_dir.join(flat);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            removed.push(format!("Removed file: {}", path.display()));
        }
    }
    if removed.is_empty() {
        removed.push("Nothing to remove.".to_string());
    }
    Ok(removed)
}

fn dir_size_bytes_cleanup(dir: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

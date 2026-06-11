//! Meeting recorder runtime and persistence.
//!
//! This module provides local-only meeting recording with segmented audio
//! capture and local transcription.

use crate::{
    config::DEFAULT_IMAGE_EMBEDDING_DIM,
    embedding::{Embedder, EMBEDDING_DIM},
    memory_compaction::{
        build_lexical_shadow, compact_summary_embedding_text, mean_pool_embeddings,
        support_embedding_texts,
    },
    speech,
    storage::{
        MeetingBreakdown, MeetingSegment, MeetingSession, MemoryRecord, Store, Task, TaskType,
    },
    AppState,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use uuid::Uuid;

const MEETINGS_DIR: &str = "meetings";
const SEGMENT_SECONDS: i64 = 20;
const STATUS_EVENT: &str = "meeting://status";
const FORCED_MODEL: &str = "whisper-large-v3-turbo-gguf";
static MEETING_EMBEDDER: OnceLock<Result<Embedder, String>> = OnceLock::new();

fn shared_meeting_embedder() -> Option<&'static Embedder> {
    match MEETING_EMBEDDER.get_or_init(Embedder::new) {
        Ok(embedder) => Some(embedder),
        Err(err) => {
            tracing::debug!("Meeting embedder unavailable: {}", err);
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingRecorderStatus {
    pub is_recording: bool,
    pub is_analyzing: bool,
    pub current_meeting_id: Option<String>,
    pub current_title: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<i64>,
    pub segment_count: usize,
    pub consent_state: String,
    pub consent_evidence: Option<String>,
    pub consent_checked_segments: usize,
    pub ffmpeg_available: bool,
    pub transcription_backend: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub meeting: MeetingSession,
    pub segments: Vec<MeetingSegment>,
    pub full_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSearchResult {
    pub meeting_id: String,
    pub meeting_title: String,
    pub segment_id: String,
    pub index: u32,
    pub text: String,
    pub score: f32,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
}

struct MeetingStore {
    root_dir: PathBuf,
    store: Arc<Store>,
}

impl MeetingStore {
    fn new(app_data_dir: PathBuf, store: Arc<Store>) -> Result<Self, String> {
        let root_dir = app_data_dir.join(MEETINGS_DIR);
        fs::create_dir_all(&root_dir).map_err(|e| format!("Failed to create meetings dir: {e}"))?;

        Ok(Self { root_dir, store })
    }

    async fn recover_unfinished(&self) -> Result<(), String> {
        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        let mut touched = false;
        for meeting in meetings.iter_mut() {
            if meeting.status == "recording" {
                meeting.status = "stopped".to_string();
                meeting.end_timestamp = Some(now_ms());
                meeting.updated_at = now_ms();
                touched = true;
            }
        }
        if touched {
            self.store
                .upsert_meetings(&meetings)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    async fn create_meeting(
        &self,
        title: String,
        participants: Vec<String>,
        model: String,
    ) -> Result<MeetingSession, String> {
        let now = now_ms();
        let meeting_id = Uuid::new_v4().to_string();
        let meeting_dir = self.root_dir.join(&meeting_id);
        let audio_dir = meeting_dir.join("audio");
        fs::create_dir_all(&audio_dir)
            .map_err(|e| format!("Failed to create meeting audio dir: {e}"))?;

        let rel_meeting_dir = MEETING_RELATIVE_PREFIX.to_string() + &meeting_id;
        let rel_audio_dir = rel_meeting_dir.clone() + "/audio";

        let meeting = MeetingSession {
            id: meeting_id,
            title,
            participants,
            model,
            status: "recording".to_string(),
            start_timestamp: now,
            end_timestamp: None,
            created_at: now,
            updated_at: now,
            segment_count: 0,
            duration_seconds: 0,
            meeting_dir: rel_meeting_dir,
            audio_dir: rel_audio_dir,
            transcript_path: None,
            breakdown: None,
        };

        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        meetings.push(meeting.clone());
        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())?;
        Ok(meeting)
    }

    async fn set_meeting_error(&self, meeting_id: &str, message: &str) -> Result<(), String> {
        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(meeting) = meetings.iter_mut().find(|m| m.id == meeting_id) {
            meeting.status = "error".to_string();
            meeting.updated_at = now_ms();
            meeting.end_timestamp = Some(now_ms());
            meeting.transcript_path = Some(message.to_string());
        }
        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())
    }

    async fn update_meeting_breakdown(
        &self,
        meeting_id: &str,
        breakdown: MeetingBreakdown,
        transcript_path: Option<String>,
    ) -> Result<(), String> {
        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(meeting) = meetings.iter_mut().find(|m| m.id == meeting_id) {
            meeting.status = "stopped".to_string();
            meeting.end_timestamp = Some(now_ms());
            meeting.updated_at = now_ms();
            meeting.transcript_path = transcript_path;
            meeting.breakdown = Some(breakdown);
            if let Some(end) = meeting.end_timestamp {
                meeting.duration_seconds = ((end - meeting.start_timestamp).max(0) / 1000) as u64;
            }
        }
        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())
    }

    async fn add_segments_batch(
        &self,
        meeting_id: &str,
        segments: &[MeetingSegment],
    ) -> Result<(), String> {
        if segments.is_empty() {
            return Ok(());
        }

        self.store
            .upsert_segments(segments)
            .await
            .map_err(|e| e.to_string())?;

        let total_segment_count = self.get_segments_for_meeting(meeting_id).await.len();
        let segment_end = segments
            .iter()
            .map(|segment| segment.end_timestamp)
            .max()
            .unwrap_or_else(now_ms);

        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(meeting) = meetings.iter_mut().find(|m| m.id == meeting_id) {
            meeting.segment_count = total_segment_count;
            meeting.duration_seconds =
                ((segment_end - meeting.start_timestamp).max(0) / 1000) as u64;
            meeting.updated_at = now_ms();
        }
        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_meetings(&self) -> Vec<MeetingSession> {
        let mut meetings = self.store.list_meetings().await.unwrap_or_default();
        meetings.sort_by_key(|m| std::cmp::Reverse(m.start_timestamp));
        meetings
    }

    async fn get_meeting(&self, meeting_id: &str) -> Option<MeetingSession> {
        let meetings = self.store.list_meetings().await.unwrap_or_default();
        meetings.into_iter().find(|m| m.id == meeting_id)
    }

    async fn delete_meeting(&self, meeting_id: &str) -> Result<bool, String> {
        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        let removed = meetings.iter().position(|m| m.id == meeting_id).map(|index| meetings.remove(index));

        let Some(meeting) = removed else {
            return Ok(false);
        };

        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())?;

        // Removal of segments
        let mut segments = self
            .store
            .list_segments()
            .await
            .map_err(|e| e.to_string())?;
        segments.retain(|s| s.meeting_id != meeting_id);
        self.store
            .upsert_segments_full(&segments)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(transcript_path) = meeting.transcript_path.as_ref() {
            let full_path = self.resolve_relative_path(transcript_path);
            if full_path.exists() {
                let _ = fs::remove_file(full_path);
            }
        }

        let meeting_dir = self.resolve_relative_path(&meeting.meeting_dir);
        if meeting_dir.exists() {
            fs::remove_dir_all(&meeting_dir)
                .map_err(|e| format!("Failed to remove meeting directory: {e}"))?;
        }

        Ok(true)
    }

    async fn get_segments_for_meeting(&self, meeting_id: &str) -> Vec<MeetingSegment> {
        let all = self.store.list_segments().await.unwrap_or_default();
        let mut segments: Vec<MeetingSegment> = all
            .into_iter()
            .filter(|s| s.meeting_id == meeting_id)
            .collect();
        segments.sort_by_key(|s| s.index);
        segments
    }

    async fn get_transcript(&self, meeting_id: &str) -> Result<MeetingTranscript, String> {
        let mut meeting = self
            .get_meeting(meeting_id)
            .await
            .ok_or_else(|| "Meeting not found".to_string())?;
        let segments = self.get_segments_for_meeting(meeting_id).await;
        let full_text = segments
            .iter()
            .map(|s| s.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if breakdown_needs_repair(meeting.breakdown.as_ref()) && !full_text.trim().is_empty() {
            meeting.breakdown = Some(build_quick_breakdown(
                &full_text,
                meeting.breakdown.as_ref(),
            ));
        }

        Ok(MeetingTranscript {
            meeting,
            segments,
            full_text,
        })
    }

    // search API removed globally as per simplified model

    async fn set_segment_texts_batch(
        &self,
        meeting_id: &str,
        updates: &HashMap<u32, String>,
    ) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }

        let mut segments = self
            .store
            .list_segments()
            .await
            .map_err(|e| e.to_string())?;
        let mut changed = false;
        for segment in segments
            .iter_mut()
            .filter(|segment| segment.meeting_id == meeting_id)
        {
            if let Some(text) = updates.get(&segment.index) {
                segment.text = text.clone();
                changed = true;
            }
        }

        if !changed {
            return Ok(());
        }

        self.store
            .upsert_segments_full(&segments)
            .await
            .map_err(|e| e.to_string())
    }

    async fn set_meeting_analyzing(&self, meeting_id: &str) -> Result<(), String> {
        let mut meetings = self
            .store
            .list_meetings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(meeting) = meetings.iter_mut().find(|m| m.id == meeting_id) {
            let end = now_ms();
            meeting.status = "analyzing".to_string();
            meeting.end_timestamp = Some(end);
            meeting.updated_at = end;
            meeting.duration_seconds = ((end - meeting.start_timestamp).max(0) / 1000) as u64;
        }
        self.store
            .upsert_meetings(&meetings)
            .await
            .map_err(|e| e.to_string())
    }

    fn resolve_relative_path(&self, rel: &str) -> PathBuf {
        if let Some(stripped) = rel.strip_prefix(MEETING_RELATIVE_PREFIX) {
            self.root_dir.join(stripped)
        } else {
            PathBuf::from(rel)
        }
    }

    async fn purge_audio_chunks(&self, meeting_id: &str) -> Result<(), String> {
        let Some(meeting) = self.get_meeting(meeting_id).await else {
            return Ok(());
        };
        let audio_dir = self.resolve_relative_path(&meeting.audio_dir);
        if !audio_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&audio_dir)
            .map_err(|e| format!("Failed reading audio dir for cleanup: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_wav = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("wav"))
                .unwrap_or(false);
            if is_wav {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

const MEETING_RELATIVE_PREFIX: &str = "rel://meetings/";

struct ActiveMeeting {
    meeting_id: String,
    title: String,
    model: String,
    started_at: i64,
    stop_flag: Arc<AtomicBool>,
    recorder: Child,
    worker: JoinHandle<()>,
}

#[derive(Debug, Clone)]
struct AnalyzingMeeting {
    meeting_id: String,
    title: String,
    model: String,
    started_at: i64,
}

#[derive(Default)]
struct MeetingRuntime {
    store: Option<Arc<MeetingStore>>,
    active: Option<ActiveMeeting>,
    analyzing: Option<AnalyzingMeeting>,
    app_handle: Option<AppHandle>,
    app_state: Option<Arc<AppState>>,
    last_error: Option<String>,
}


static RUNTIME: OnceLock<Mutex<MeetingRuntime>> = OnceLock::new();
static POSTPROCESS_IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn runtime() -> &'static Mutex<MeetingRuntime> {
    RUNTIME.get_or_init(|| Mutex::new(MeetingRuntime::default()))
}

fn postprocess_in_flight() -> &'static Mutex<HashSet<String>> {
    POSTPROCESS_IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

struct PostprocessGuard {
    meeting_id: String,
}

impl Drop for PostprocessGuard {
    fn drop(&mut self) {
        postprocess_in_flight().lock().remove(&self.meeting_id);
    }
}

async fn acquire_postprocess_guard(meeting_id: &str) -> PostprocessGuard {
    loop {
        let acquired = {
            let mut in_flight = postprocess_in_flight().lock();
            if in_flight.contains(meeting_id) {
                false
            } else {
                in_flight.insert(meeting_id.to_string());
                true
            }
        };

        if acquired {
            return PostprocessGuard {
                meeting_id: meeting_id.to_string(),
            };
        }

        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

pub async fn init(app_data_dir: PathBuf, store: Arc<Store>) -> Result<(), String> {
    let store = Arc::new(MeetingStore::new(app_data_dir, store)?);
    store.recover_unfinished().await?;

    let mut rt = runtime().lock();
    rt.store = Some(store);
    rt.last_error = None;
    Ok(())
}

pub fn bind_runtime(app_handle: AppHandle, app_state: Arc<AppState>) -> Result<(), String> {
    let mut rt = runtime().lock();
    rt.app_handle = Some(app_handle);
    rt.app_state = Some(app_state);
    Ok(())
}

pub async fn list_meetings() -> Result<Vec<MeetingSession>, String> {
    let store = get_store()?;
    Ok(store.list_meetings().await)
}

/// Return all segments for a given meeting, sorted by index.
pub async fn get_meeting_segments(meeting_id: &str) -> Vec<crate::storage::MeetingSegment> {
    match get_store() {
        Ok(store) => store.get_segments_for_meeting(meeting_id).await,
        Err(_) => Vec::new(),
    }
}

pub async fn delete_meeting(meeting_id: &str) -> Result<bool, String> {
    let should_stop_active = {
        let rt = runtime().lock();
        rt.active
            .as_ref()
            .map(|active| active.meeting_id == meeting_id)
            .unwrap_or(false)
    };

    if should_stop_active {
        stop_recording().await?;
    }

    let store = get_store()?;
    store.delete_meeting(meeting_id).await
}

pub async fn get_meeting_transcript(meeting_id: &str) -> Result<MeetingTranscript, String> {
    let store = get_store()?;
    store.get_transcript(meeting_id).await
}

pub async fn retranscribe_meeting(meeting_id: &str) -> Result<(), String> {
    let store = get_store()?;
    let model = FORCED_MODEL;

    // Re-ingest any WAV files not yet in the store, then transcribe all pending segments.
    if let Err(err) = ingest_discovered_segments(store.as_ref(), meeting_id, model).await {
        tracing::warn!(
            "retranscribe_meeting: ingest failed for {}: {}",
            meeting_id,
            err
        );
    }
    transcribe_meeting_postprocess(store.as_ref(), meeting_id, model).await
}

pub async fn search_meeting_transcripts(
    query: &str,
    limit: usize,
) -> Result<Vec<MeetingSearchResult>, String> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }

    let store = get_store()?;
    let meetings = store.list_meetings().await;
    let meeting_titles: HashMap<String, String> = meetings
        .into_iter()
        .map(|meeting| (meeting.id, meeting.title))
        .collect();

    let terms = transcript_search_terms(&normalized_query);
    let mut results = Vec::new();
    let all_segments = store
        .store
        .list_segments()
        .await
        .map_err(|e| e.to_string())?;
    for segment in all_segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        let score = transcript_match_score(text, &normalized_query, &terms);
        if score <= 0.0 {
            continue;
        }

        let meeting_title = meeting_titles
            .get(&segment.meeting_id)
            .cloned()
            .unwrap_or_else(|| "Meeting".to_string());

        results.push(MeetingSearchResult {
            meeting_id: segment.meeting_id.clone(),
            meeting_title,
            segment_id: segment.id.clone(),
            index: segment.index,
            text: text.to_string(),
            score,
            start_timestamp: segment.start_timestamp,
            end_timestamp: segment.end_timestamp,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.start_timestamp.cmp(&a.start_timestamp))
            .then_with(|| b.index.cmp(&a.index))
    });
    results.truncate(limit.max(1));
    Ok(results)
}

pub fn recorder_status() -> Result<MeetingRecorderStatus, String> {
    let rt = runtime().lock();
    let ffmpeg_available = resolve_ffmpeg_binary().is_some();
    let backend = detect_transcription_backend();

    if let Some(active) = rt.active.as_ref() {
        return Ok(MeetingRecorderStatus {
            is_recording: true,
            is_analyzing: rt.analyzing.is_some(),
            current_meeting_id: Some(active.meeting_id.clone()),
            current_title: Some(active.title.clone()),
            model: Some(active.model.clone()),
            started_at: Some(active.started_at),
            segment_count: 0,
            consent_state: "n/a".to_string(),
            consent_evidence: None,
            consent_checked_segments: 0,
            ffmpeg_available,
            transcription_backend: backend,
            last_error: rt.last_error.clone(),
        });
    }

    if let Some(analyzing) = rt.analyzing.as_ref() {
        return Ok(MeetingRecorderStatus {
            is_recording: false,
            is_analyzing: true,
            current_meeting_id: Some(analyzing.meeting_id.clone()),
            current_title: Some(analyzing.title.clone()),
            model: Some(analyzing.model.clone()),
            started_at: Some(analyzing.started_at),
            segment_count: 0,
            consent_state: "n/a".to_string(),
            consent_evidence: None,
            consent_checked_segments: 0,
            ffmpeg_available,
            transcription_backend: backend,
            last_error: rt.last_error.clone(),
        });
    }

    Ok(MeetingRecorderStatus {
        is_recording: false,
        is_analyzing: false,
        current_meeting_id: None,
        current_title: None,
        model: None,
        started_at: None,
        segment_count: 0,
        consent_state: "unknown".to_string(),
        consent_evidence: None,
        consent_checked_segments: 0,
        ffmpeg_available,
        transcription_backend: backend,
        last_error: rt.last_error.clone(),
    })
}

pub async fn start_recording(
    app_handle: Option<AppHandle>,
    title: String,
    participants: Vec<String>,
    _model: Option<String>,
) -> Result<MeetingRecorderStatus, String> {
    let clean_title = if title.trim().is_empty() {
        "Detected Meeting".to_string()
    } else {
        title.trim().to_string()
    };
    let clean_participants: Vec<String> = participants
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    let (store, app_for_worker) = {
        let mut rt = runtime().lock();
        let store = rt
            .store
            .as_ref()
            .cloned()
            .ok_or_else(|| "Meeting runtime is not initialized".to_string())?;

        if rt.active.is_some() {
            return Err("A meeting recording is already active".to_string());
        }

        if let Some(handle) = app_handle.clone() {
            rt.app_handle = Some(handle);
        }

        let app_for_worker = rt.app_handle.clone();
        (store, app_for_worker)
    };

    let meeting = store
        .create_meeting(clean_title, clean_participants, FORCED_MODEL.to_string())
        .await?;

    let active_exists_after_create = {
        let rt = runtime().lock();
        rt.active.is_some()
    };
    if active_exists_after_create {
        let _ = store
            .set_meeting_error(
                &meeting.id,
                "Another meeting recording became active before this one started.",
            )
            .await;
        return Err("A meeting recording is already active".to_string());
    }

    let segment_pattern = store
        .resolve_relative_path(&meeting.audio_dir)
        .join("segment_%05d.wav");
    let recorder = match spawn_ffmpeg_recorder(&segment_pattern) {
        Ok(child) => child,
        Err(err) => {
            let _ = store.set_meeting_error(&meeting.id, &err).await;
            runtime().lock().last_error = Some(err.clone());
            return Err(err);
        }
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let worker_stop_flag = stop_flag.clone();
    let worker = tokio::spawn(async move {
        // Keep ffmpeg as the only process touching segment files while recording.
        // Transcription resumes after stop, once files have stabilized on disk.
        while !worker_stop_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    let mut pending_active = Some(ActiveMeeting {
        meeting_id: meeting.id.clone(),
        title: meeting.title.clone(),
        model: meeting.model.clone(),
        started_at: meeting.start_timestamp,
        stop_flag,
        recorder,
        worker,
    });
    let active_already_present = {
        let mut rt = runtime().lock();
        if rt.active.is_some() {
            true
        } else {
            rt.active = pending_active.take();
            rt.last_error = None;
            false
        }
    };

    if active_already_present {
        if let Some(active) = pending_active {
            active.stop_flag.store(true, Ordering::SeqCst);
            let mut recorder = active.recorder;
            let _ = recorder.kill();
            let _ = recorder.wait();
            let _ = active.worker.await;
        }
        let _ = store
            .set_meeting_error(
                &meeting.id,
                "Another meeting recording became active before this one started.",
            )
            .await;
        return Err("A meeting recording is already active".to_string());
    }

    let status = recorder_status()?;
    if let Some(handle) = app_for_worker {
        let _ = handle.emit(STATUS_EVENT, &status);
    }
    Ok(status)
}

pub async fn stop_recording() -> Result<MeetingRecorderStatus, String> {
    let (store, app_handle, app_state, active) = {
        let mut rt = runtime().lock();
        let store = rt
            .store
            .as_ref()
            .cloned()
            .ok_or_else(|| "Meeting runtime is not initialized".to_string())?;
        let app_handle = rt.app_handle.clone();
        let app_state = rt.app_state.clone();
        let active = rt.active.take();
        (store, app_handle, app_state, active)
    };

    let Some(active) = active else {
        return recorder_status();
    };

    let ActiveMeeting {
        meeting_id,
        title,
        model,
        stop_flag,
        mut recorder,
        worker,
        ..
    } = active;

    stop_flag.store(true, Ordering::SeqCst);

    request_ffmpeg_stop(&mut recorder);
    let stopped = wait_for_process_exit(&mut recorder, Duration::from_secs(6)).await;
    if !stopped {
        if let Err(err) = recorder.kill() {
            tracing::warn!("Failed to terminate ffmpeg recorder cleanly: {}", err);
        }
    }
    let _ = recorder.wait();
    let _ = worker.await;

    if let Some(meeting) = store.get_meeting(&meeting_id).await {
        let audio_dir = store.resolve_relative_path(&meeting.audio_dir);
        wait_for_segment_stability(&audio_dir, Duration::from_millis(1200));
        if let Err(err) = ingest_discovered_segments(store.as_ref(), &meeting_id, &model).await {
            tracing::warn!(
                "Failed to ingest final discovered segments for {}: {}",
                meeting_id,
                err
            );
        }
    }

    if let Err(err) = transcribe_meeting_postprocess(store.as_ref(), &meeting_id, &model).await {
        tracing::warn!(
            "Post-meeting transcription pass failed for {}: {}",
            meeting_id,
            err
        );
    }

    if let Err(err) = store.set_meeting_analyzing(&meeting_id).await {
        tracing::warn!("Failed to set meeting analyzing state: {}", err);
    }

    {
        let mut rt = runtime().lock();
        rt.analyzing = Some(AnalyzingMeeting {
            meeting_id: meeting_id.clone(),
            title: title.clone(),
            model: model.clone(),
            started_at: now_ms(),
        });
        rt.last_error = None;
    }

    let app_for_background = app_handle.clone();
    let store_for_background = store.clone();
    let meeting_id_for_background = meeting_id.clone();
    let model_for_background = model.clone();
    tokio::spawn(async move {
        let result = finalize_meeting_analysis(
            store_for_background.as_ref(),
            &meeting_id_for_background,
            &model_for_background,
            app_state,
        )
        .await;

        {
            let mut rt = runtime().lock();
            if rt
                .analyzing
                .as_ref()
                .map(|analyzing| analyzing.meeting_id.as_str())
                == Some(meeting_id_for_background.as_str())
            {
                rt.analyzing = None;
            }

            match result {
                Ok(()) => {
                    rt.last_error = None;
                }
                Err(ref err) => {
                    rt.last_error = Some(err.clone());
                }
            }
        }

        if let Err(err) = result {
            tracing::warn!(
                "Meeting {} background analysis failed: {}",
                meeting_id_for_background,
                err
            );
            let _ = store_for_background
                .set_meeting_error(&meeting_id_for_background, &err)
                .await;
        }

        if let Some(handle) = app_for_background {
            if let Ok(status) = recorder_status() {
                let _ = handle.emit(STATUS_EVENT, &status);
            }
        }
    });

    let status = recorder_status()?;
    if let Some(handle) = app_handle.clone() {
        let _ = handle.emit(STATUS_EVENT, &status);
    }
    Ok(status)
}

async fn ingest_discovered_segments(
    store: &MeetingStore,
    meeting_id: &str,
    model: &str,
) -> Result<usize, String> {
    let Some(meeting) = store.get_meeting(meeting_id).await else {
        return Ok(0);
    };

    let audio_dir = store.resolve_relative_path(&meeting.audio_dir);
    let wav_files = collect_segment_files(&audio_dir);
    let mut existing_indices: HashSet<u32> = store
        .get_segments_for_meeting(meeting_id)
        .await
        .into_iter()
        .map(|segment| segment.index)
        .collect();

    let mut new_segments = Vec::new();
    for wav_path in &wav_files {
        let index = parse_segment_index(wav_path);
        if !existing_indices.insert(index) {
            continue;
        }

        let seg_start = meeting.start_timestamp + (index as i64 * SEGMENT_SECONDS * 1000);
        let seg_end = seg_start + (SEGMENT_SECONDS * 1000);
        new_segments.push(MeetingSegment {
            id: Uuid::new_v4().to_string(),
            meeting_id: meeting_id.to_string(),
            index,
            start_timestamp: seg_start,
            end_timestamp: seg_end,
            text: String::new(),
            audio_chunk_path: wav_path.to_string_lossy().to_string(),
            model: model.to_string(),
            created_at: now_ms(),
        });
    }

    let added_count = new_segments.len();
    if added_count > 0 {
        store.add_segments_batch(meeting_id, &new_segments).await?;
    }
    Ok(added_count)
}

async fn finalize_meeting_analysis(
    store: &MeetingStore,
    meeting_id: &str,
    model: &str,
    app_state: Option<Arc<AppState>>,
) -> Result<(), String> {
    if let Err(err) = ingest_discovered_segments(store, meeting_id, model).await {
        tracing::warn!(
            "Meeting {}: final discovered-segment ingest failed: {}",
            meeting_id,
            err
        );
    }

    if let Err(err) = transcribe_meeting_postprocess(store, meeting_id, model).await {
        tracing::warn!("Post-meeting transcription pass failed: {}", err);
    }

    let transcript = store.get_transcript(meeting_id).await?;
    let full_text = transcript.full_text.clone();

    let existing_breakdown = store
        .get_meeting(meeting_id)
        .await
        .and_then(|meeting| meeting.breakdown);
    let mut breakdown = build_quick_breakdown(&full_text, existing_breakdown.as_ref());

    // Fast result path: persist a deterministic breakdown immediately.
    if let Err(err) = persist_breakdown_tasks(store, meeting_id, &breakdown).await {
        tracing::warn!(
            "Failed to persist meeting breakdown tasks for {}: {}",
            meeting_id,
            err
        );
    }
    store
        .update_meeting_breakdown(meeting_id, breakdown.clone(), None)
        .await?;

    // Best-effort enrichment path: keep it bounded so finalization never stalls.
    if !full_text.trim().is_empty() {
        if let Some(engine) = app_state
            .as_ref()
            .and_then(|state| state.inference_engine())
        {
            tracing::info!("Running bounded AI meeting breakdown for {}", meeting_id);
            let structured = tokio::time::timeout(
                Duration::from_secs(12),
                engine.extract_meeting_breakdown(&full_text),
            )
            .await
            .ok()
            .flatten();

            let ai_breakdown = if let Some(structured) = structured {
                Some(MeetingBreakdown {
                    summary: structured.summary,
                    todos: structured.todos,
                    reminders: structured.reminders,
                    followups: structured.followups,
                })
            } else {
                let legacy_prompt = build_legacy_breakdown_prompt(&full_text);
                let legacy_raw = tokio::time::timeout(
                    Duration::from_secs(10),
                    engine.extract_todos(&legacy_prompt),
                )
                .await
                .ok();

                if let Some(parsed) =
                    legacy_raw.and_then(|raw| parse_legacy_breakdown_response(&raw))
                {
                    Some(parsed)
                } else {
                    let raw = tokio::time::timeout(
                        Duration::from_secs(6),
                        engine.extract_todos(&full_text),
                    )
                    .await
                    .ok();
                    raw.and_then(|raw| {
                        let parsed = crate::tasks::parse_tasks_from_llm_response(
                            &raw,
                            &format!("Meeting:{meeting_id}"),
                        );
                        let mut fallback = MeetingBreakdown::default();
                        for task in parsed {
                            match task.task_type {
                                TaskType::Todo => fallback.todos.push(task.title),
                                TaskType::Reminder => fallback.reminders.push(task.title),
                                TaskType::Followup => fallback.followups.push(task.title),
                            }
                        }
                        if fallback.todos.is_empty()
                            && fallback.reminders.is_empty()
                            && fallback.followups.is_empty()
                        {
                            None
                        } else {
                            Some(fallback)
                        }
                    })
                }
            };

            if let Some(ai_breakdown) = ai_breakdown {
                let merged = merge_breakdowns(Some(breakdown.clone()), ai_breakdown);
                breakdown = merged;
                if let Err(err) = persist_breakdown_tasks(store, meeting_id, &breakdown).await {
                    tracing::warn!(
                        "Failed to persist enriched meeting tasks for {}: {}",
                        meeting_id,
                        err
                    );
                }
                store
                    .update_meeting_breakdown(meeting_id, breakdown.clone(), None)
                    .await?;
            }
        }
    }

    if let Some(state) = app_state {
        let _ = ingest_transcript_into_fndr_memory(state, &transcript, None).await;
    }
    if let Err(err) = store.purge_audio_chunks(meeting_id).await {
        tracing::warn!("Failed to purge meeting audio chunks: {}", err);
    }

    Ok(())
}

fn summarize_transcript_fallback(transcript: &str) -> String {
    let sentences = split_transcript_sentences(transcript);
    if sentences.is_empty() {
        "Meeting captured with limited transcript detail.".to_string()
    } else {
        let mut prioritized = Vec::new();
        for sentence in &sentences {
            let lowered = sentence.to_lowercase();
            if has_any_keyword(
                &lowered,
                &[
                    "action item",
                    "follow up",
                    "followup",
                    "deadline",
                    "by ",
                    "must",
                    "need to",
                    "should",
                    "review",
                    "send",
                    "finish",
                    "complete",
                    "test",
                    "confirm",
                ],
            ) {
                prioritized.push(trim_words(sentence, 26));
            }
            if prioritized.len() >= 2 {
                break;
            }
        }

        if prioritized.is_empty() {
            prioritized.extend(
                sentences
                    .iter()
                    .take(2)
                    .map(|sentence| trim_words(sentence, 24)),
            );
        }

        format!("Discussion summary: {}", prioritized.join(" "))
    }
}

fn build_quick_breakdown(full_text: &str, existing: Option<&MeetingBreakdown>) -> MeetingBreakdown {
    if full_text.trim().is_empty() {
        let mut empty = existing.cloned().unwrap_or_default();
        if empty.summary.trim().is_empty() {
            empty.summary = "No audio was captured or transcription produced no text.".to_string();
        }
        return empty;
    }

    let mut breakdown = existing.cloned().unwrap_or_default();
    breakdown.summary = summarize_transcript_fallback(full_text);

    let mut todos = Vec::new();
    let mut reminders = Vec::new();
    let mut followups = Vec::new();
    for sentence in split_transcript_sentences(full_text) {
        let lowered = sentence.to_lowercase();
        if has_any_keyword(
            &lowered,
            &["todo", "to do", "need to", "should", "must", "action item"],
        ) {
            todos.push(sentence.clone());
        }
        if has_any_keyword(
            &lowered,
            &[
                "remember to",
                "reminder",
                "don't forget",
                "deadline",
                "by tomorrow",
                "by friday",
            ],
        ) {
            reminders.push(sentence.clone());
        }
        if has_any_keyword(
            &lowered,
            &[
                "follow up",
                "followup",
                "circle back",
                "check back",
                "send over",
                "share with",
            ],
        ) {
            followups.push(sentence);
        }
    }

    breakdown.todos = merge_string_lists(&breakdown.todos, &todos, 10);
    breakdown.reminders = merge_string_lists(&breakdown.reminders, &reminders, 10);
    breakdown.followups = merge_string_lists(&breakdown.followups, &followups, 10);
    breakdown
}

fn split_transcript_sentences(full_text: &str) -> Vec<String> {
    let normalized = full_text.replace('\r', "\n");
    let mut sentences = Vec::new();

    for chunk in normalized.split(['\n', '.', '?', '!', ';']) {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.len() <= 220 {
            if trimmed.len() >= 10 {
                sentences.push(trimmed.to_string());
            }
            continue;
        }

        let mut current = String::new();
        for fragment in trimmed.split(',') {
            let fragment = fragment.trim();
            if fragment.is_empty() {
                continue;
            }

            let candidate = if current.is_empty() {
                fragment.to_string()
            } else {
                format!("{current}, {fragment}")
            };

            if candidate.len() > 220 && !current.is_empty() {
                if current.len() >= 10 {
                    sentences.push(current.trim().to_string());
                }
                current = fragment.to_string();
            } else {
                current = candidate;
            }
        }

        if current.len() >= 10 {
            sentences.push(current.trim().to_string());
        }
    }

    sentences
}

fn has_any_keyword(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn trim_words(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .take(limit)
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_legacy_breakdown_prompt(full_text: &str) -> String {
    format!(
        "Review this meeting transcript and provide a structured breakdown.\n\nTRANSCRIPT:\n{}\n\n\
        Format your response exactly as:\n\
        SUMMARY: [one paragraph summary]\n\
        TODOS:\n- [task]\n\
        REMINDERS:\n- [reminder]\n\
        FOLLOWUPS:\n- [followup]",
        full_text.chars().take(4000).collect::<String>()
    )
}

fn parse_legacy_breakdown_response(raw: &str) -> Option<MeetingBreakdown> {
    let mut breakdown = MeetingBreakdown::default();
    let mut section = "";

    for line in raw.lines() {
        let line = line.trim().trim_matches('|');
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("SUMMARY:") {
            breakdown.summary = rest.trim().to_string();
            section = "summary";
            continue;
        }
        if line.eq_ignore_ascii_case("TODOS:") {
            section = "todos";
            continue;
        }
        if line.eq_ignore_ascii_case("REMINDERS:") {
            section = "reminders";
            continue;
        }
        if line.eq_ignore_ascii_case("FOLLOWUPS:")
            || line.eq_ignore_ascii_case("FOLLOW-UPS:")
            || line.eq_ignore_ascii_case("FOLLOW UPS:")
        {
            section = "followups";
            continue;
        }

        let item = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .map(str::trim)
            .unwrap_or(line);
        if item.len() < 4 {
            continue;
        }

        match section {
            "summary" if breakdown.summary.is_empty() => breakdown.summary = item.to_string(),
            "todos" => breakdown.todos.push(item.to_string()),
            "reminders" => breakdown.reminders.push(item.to_string()),
            "followups" => breakdown.followups.push(item.to_string()),
            _ => {}
        }
    }

    if breakdown.summary.trim().is_empty()
        && breakdown.todos.is_empty()
        && breakdown.reminders.is_empty()
        && breakdown.followups.is_empty()
    {
        None
    } else {
        Some(breakdown)
    }
}

fn merge_string_lists(existing: &[String], incoming: &[String], limit: usize) -> Vec<String> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();

    for item in existing.iter().chain(incoming.iter()) {
        let trimmed = item.trim();
        if trimmed.len() < 4 {
            continue;
        }
        let key = normalize_task_item_key(trimmed);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        merged.push(trimmed.to_string());
        if merged.len() >= limit {
            break;
        }
    }

    merged
}

fn merge_breakdowns(
    existing: Option<MeetingBreakdown>,
    incoming: MeetingBreakdown,
) -> MeetingBreakdown {
    if let Some(existing) = existing {
        MeetingBreakdown {
            summary: if incoming.summary.trim().is_empty() {
                existing.summary
            } else {
                incoming.summary
            },
            todos: merge_string_lists(&existing.todos, &incoming.todos, 10),
            reminders: merge_string_lists(&existing.reminders, &incoming.reminders, 10),
            followups: merge_string_lists(&existing.followups, &incoming.followups, 10),
        }
    } else {
        incoming
    }
}

fn breakdown_needs_repair(breakdown: Option<&MeetingBreakdown>) -> bool {
    breakdown
        .map(|breakdown| {
            breakdown.summary.trim().is_empty()
                && breakdown.todos.is_empty()
                && breakdown.reminders.is_empty()
                && breakdown.followups.is_empty()
        })
        .unwrap_or(true)
}

async fn persist_breakdown_tasks(
    store: &MeetingStore,
    meeting_id: &str,
    breakdown: &MeetingBreakdown,
) -> Result<(), String> {
    let mut existing = store.store.list_tasks().await.map_err(|e| e.to_string())?;
    let mut seen_active: HashSet<(String, &'static str)> = existing
        .iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .map(|task| {
            (
                normalize_task_item_key(task.title.trim()),
                task_type_key(&task.task_type),
            )
        })
        .collect();

    let created_at = now_ms();
    let source_app = format!("Meeting:{}", meeting_id);
    let mut added_any = false;

    let mut add_task = |items: &[String], task_type: TaskType| {
        let type_key = task_type_key(&task_type);
        for item in items {
            let title = item.trim();
            if title.len() < 3 || !is_actionable_task_item(title) {
                continue;
            }
            let dedupe_key = (normalize_task_item_key(title), type_key);
            if seen_active.contains(&dedupe_key) {
                continue;
            }
            seen_active.insert(dedupe_key);
            existing.push(Task {
                id: Uuid::new_v4().to_string(),
                title: title.to_string(),
                description: String::new(),
                source_app: source_app.clone(),
                source_memory_id: None,
                created_at,
                due_date: None,
                is_completed: false,
                is_dismissed: false,
                task_type: task_type.clone(),
                linked_urls: Vec::new(),
                linked_memory_ids: Vec::new(),
            });
            added_any = true;
        }
    };

    add_task(&breakdown.todos, TaskType::Todo);
    add_task(&breakdown.reminders, TaskType::Reminder);
    add_task(&breakdown.followups, TaskType::Followup);

    if !added_any {
        return Ok(());
    }

    store
        .store
        .upsert_tasks(&existing)
        .await
        .map_err(|e| e.to_string())
}

fn task_type_key(task_type: &TaskType) -> &'static str {
    match task_type {
        TaskType::Todo => "todo",
        TaskType::Reminder => "reminder",
        TaskType::Followup => "followup",
    }
}

fn normalize_task_item_key(value: &str) -> String {
    value
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_actionable_task_item(value: &str) -> bool {
    let cleaned = normalize_task_item_key(value);
    if cleaned.len() < 6 {
        return false;
    }
    if cleaned.split_whitespace().count() < 2 {
        return false;
    }

    if matches!(
        cleaned.as_str(),
        "todo"
            | "to do"
            | "task"
            | "follow up"
            | "reminder"
            | "none"
            | "n a"
            | "no action items"
            | "no reminders"
            | "no followups"
    ) {
        return false;
    }

    let boilerplate_prefixes = [
        "complete ",
        "remember ",
        "follow up ",
        "followup ",
        "do this ",
        "work on ",
    ];
    if boilerplate_prefixes
        .iter()
        .any(|prefix| cleaned.starts_with(prefix) && cleaned.split_whitespace().count() <= 3)
    {
        return false;
    }

    true
}

fn spawn_ffmpeg_recorder(segment_pattern: &Path) -> Result<Child, String> {
    let ffmpeg_path = resolve_ffmpeg_binary().ok_or_else(|| {
        "ffmpeg was not found. Install ffmpeg and restart FNDR to use meeting recording."
            .to_string()
    })?;

    if !ffmpeg_path.exists() && ffmpeg_path.as_os_str() != "ffmpeg" {
        return Err(
            "ffmpeg was not found. Install ffmpeg and restart FNDR to use meeting recording."
                .to_string(),
        );
    }

    let mut cmd = Command::new(ffmpeg_path);
    cmd.arg("-hide_banner").arg("-loglevel").arg("error");

    #[cfg(target_os = "macos")]
    {
        let capture = resolve_macos_audio_capture_plan();
        for input in &capture.inputs {
            cmd.args(["-f", "avfoundation", "-i", input.as_str()]);
        }
        if capture.mix_inputs {
            cmd.args([
                "-filter_complex",
                "[0:a][1:a]amix=inputs=2:duration=longest:dropout_transition=2[aout]",
                "-map",
                "[aout]",
            ]);
        }
        tracing::info!(
            "Meeting recorder using avfoundation inputs {:?} mix={}",
            capture.inputs,
            capture.mix_inputs
        );
    }
    #[cfg(target_os = "linux")]
    {
        cmd.args(["-f", "pulse", "-i", "default"]);
    }
    #[cfg(target_os = "windows")]
    {
        cmd.args(["-f", "dshow", "-i", "audio=default"]);
    }

    cmd.args([
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        "-f",
        "segment",
        "-segment_time",
        &SEGMENT_SECONDS.to_string(),
        "-reset_timestamps",
        "1",
    ]);
    cmd.arg(segment_pattern.to_string_lossy().to_string());
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    cmd.spawn()
        .map_err(|e| format!("Failed to start ffmpeg meeting recorder: {e}"))
}

fn request_ffmpeg_stop(recorder: &mut Child) {
    if let Some(stdin) = recorder.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }

    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-INT")
            .arg(recorder.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

async fn wait_for_process_exit(process: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match process.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }

        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MacAudioCapturePlan {
    inputs: Vec<String>,
    mix_inputs: bool,
}

#[cfg(target_os = "macos")]
fn resolve_macos_audio_capture_plan() -> MacAudioCapturePlan {
    if let Ok(explicit) = std::env::var("FNDR_MEETING_AUDIO_DEVICE") {
        let trimmed = explicit.trim().trim_start_matches(':');
        if !trimmed.is_empty() {
            return MacAudioCapturePlan {
                inputs: vec![format!(":{trimmed}")],
                mix_inputs: false,
            };
        }
    }

    let loopback_index = detect_macos_loopback_audio_device_index();
    let mic_index = detect_macos_preferred_microphone_index();

    if let (Some(loopback), Some(mic)) = (loopback_index.clone(), mic_index.clone()) {
        if loopback != mic {
            return MacAudioCapturePlan {
                inputs: vec![format!(":{loopback}"), format!(":{mic}")],
                mix_inputs: true,
            };
        }
    }

    if let Some(loopback) = loopback_index {
        return MacAudioCapturePlan {
            inputs: vec![format!(":{loopback}")],
            mix_inputs: false,
        };
    }

    if let Some(mic) = mic_index {
        return MacAudioCapturePlan {
            inputs: vec![format!(":{mic}")],
            mix_inputs: false,
        };
    }

    MacAudioCapturePlan {
        inputs: vec![":0".to_string()],
        mix_inputs: false,
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_loopback_audio_device_index() -> Option<String> {
    let listing = avfoundation_device_listing()?;
    detect_macos_loopback_audio_device_index_from_listing(&listing)
}

#[cfg(target_os = "macos")]
fn detect_macos_preferred_microphone_index() -> Option<String> {
    let listing = avfoundation_device_listing()?;
    detect_macos_preferred_microphone_index_from_listing(&listing)
}

#[cfg(target_os = "macos")]
fn avfoundation_device_listing() -> Option<String> {
    let ffmpeg_path = resolve_ffmpeg_binary()?;
    let output = Command::new(ffmpeg_path)
        .arg("-f")
        .arg("avfoundation")
        .arg("-list_devices")
        .arg("true")
        .arg("-i")
        .arg("")
        .output()
        .ok()?;

    let listing = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(listing)
}

#[cfg(target_os = "macos")]
fn detect_macos_loopback_audio_device_index_from_listing(listing: &str) -> Option<String> {
    let mut in_audio = false;
    for line in listing.lines() {
        let lowered = line.to_lowercase();
        if lowered.contains("avfoundation audio devices") {
            in_audio = true;
            continue;
        }
        if lowered.contains("avfoundation video devices") {
            in_audio = false;
            continue;
        }
        if !in_audio {
            continue;
        }

        let Some(index) = extract_avfoundation_index(line) else {
            continue;
        };

        let has_loopback_hint = [
            "blackhole",
            "loopback",
            "soundflower",
            "vb-cable",
            "background music",
            "virtual audio",
        ]
        .into_iter()
        .any(|needle| lowered.contains(needle));

        if has_loopback_hint {
            return Some(index);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn detect_macos_preferred_microphone_index_from_listing(listing: &str) -> Option<String> {
    let mut in_audio = false;
    let mut best_candidate: Option<(u8, String)> = None;

    for line in listing.lines() {
        let lowered = line.to_lowercase();
        if lowered.contains("avfoundation audio devices") {
            in_audio = true;
            continue;
        }
        if lowered.contains("avfoundation video devices") {
            in_audio = false;
            continue;
        }
        if !in_audio {
            continue;
        }

        let Some(index) = extract_avfoundation_index(line) else {
            continue;
        };
        let name = extract_avfoundation_name(line)
            .unwrap_or_default()
            .to_lowercase();

        let is_virtual = [
            "zoomaudiodevice",
            "blackhole",
            "loopback",
            "soundflower",
            "vb-cable",
            "virtual",
            "background music",
            "multi-output",
            "aggregate",
        ]
        .into_iter()
        .any(|needle| name.contains(needle));

        if is_virtual {
            continue;
        }

        // Prefer stable built-in mics over transient Continuity/remote devices.
        let score =
            if name.contains("macbook") || name.contains("built-in") || name.contains("internal") {
                0
            } else if name.contains("microphone") || name.ends_with(" mic") {
                1
            } else {
                2
            };

        match best_candidate {
            Some((best_score, _)) if best_score <= score => {}
            _ => best_candidate = Some((score, index)),
        }
    }

    best_candidate.map(|(_, index)| index)
}

#[cfg(target_os = "macos")]
fn extract_avfoundation_index(line: &str) -> Option<String> {
    for section in line.split('[').skip(1) {
        let candidate = section.split(']').next().unwrap_or("").trim();
        if !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_digit()) {
            return Some(candidate.to_string());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn extract_avfoundation_name(line: &str) -> Option<String> {
    let marker = "] ";
    let pos = line.rfind(marker)?;
    let name = line[(pos + marker.len())..].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

async fn transcribe_segment(
    segment_path: &Path,
    model: &str,
    app_data_dir: &Path,
) -> Result<String, String> {
    if let Ok(custom_cmd) = std::env::var("FNDR_MEETING_TRANSCRIBE_COMMAND")
        .or_else(|_| std::env::var("FNDR_PARAKEET_COMMAND"))
    {
        let audio = segment_path.to_path_buf();
        let model_name = model.to_string();
        let app_data = app_data_dir.to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            Command::new("sh")
                .arg("-c")
                .arg(custom_cmd)
                .env("FNDR_AUDIO_PATH", audio.to_string_lossy().to_string())
                .env("FNDR_TRANSCRIPT_MODEL", model_name)
                .env(
                    "FNDR_TRANSCRIPT_APP_DATA_DIR",
                    app_data.to_string_lossy().to_string(),
                )
                .output()
        })
        .await
        .map_err(|e| format!("Custom meeting transcription task failed: {e}"))?
        .map_err(|e| format!("Custom meeting transcription command failed to start: {e}"))?;

        if output.status.success() {
            let stdout = normalize_transcribed_text(&String::from_utf8_lossy(&output.stdout));
            if !stdout.is_empty() {
                return Ok(stdout);
            }
            return Err("Custom meeting transcription command returned empty output".to_string());
        }

        return Err(format!(
            "Custom meeting transcription command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let text = speech::transcribe_audio_file(app_data_dir, segment_path).await?;
    let text = normalize_transcribed_text(&text);
    if text.is_empty() {
        Err("Whisper GGUF runner returned empty transcript".to_string())
    } else {
        Ok(text)
    }
}

fn normalize_transcribed_text(raw: &str) -> String {
    let sanitized = raw
        .replace("[BLANK_AUDIO]", " ")
        .replace("[ Silence ]", " ")
        .replace("[SILENCE]", " ")
        .replace("[MUSIC]", " ")
        .replace("[NOISE]", " ");
    sanitized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn detect_transcription_backend() -> String {
    if std::env::var("FNDR_MEETING_TRANSCRIBE_COMMAND").is_ok()
        || std::env::var("FNDR_PARAKEET_COMMAND").is_ok()
    {
        return "custom-transcriber".to_string();
    }
    if speech::resolve_sidecar("whisper_gguf_runner.py").is_some() {
        return "whisper-large-v3-turbo-gguf (on-demand)".to_string();
    }
    "unavailable".to_string()
}

async fn transcribe_meeting_postprocess(
    store: &MeetingStore,
    meeting_id: &str,
    model: &str,
) -> Result<(), String> {
    let _guard = acquire_postprocess_guard(meeting_id).await;

    let app_data_dir = store
        .root_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| store.root_dir.clone());
    let segments = store.get_segments_for_meeting(meeting_id).await;
    let mut seen_indices = HashSet::new();
    let mut updates: HashMap<u32, String> = HashMap::new();
    for segment in segments {
        if !seen_indices.insert(segment.index) {
            continue;
        }
        if !should_retry_segment_text(&segment.text) {
            continue;
        }

        let audio_path = PathBuf::from(&segment.audio_chunk_path);
        let text = match transcribe_segment_with_retry(&audio_path, model, &app_data_dir).await {
            Ok(text) => text,
            Err(err) => {
                let size = fs::metadata(&audio_path).map(|m| m.len()).unwrap_or(0);
                // Ignore tiny/corrupt trailing chunks that can happen when a meeting stops mid-segment.
                if size < 1_500 {
                    String::new()
                } else {
                    format!(
                        "[Transcription unavailable for segment {}: {}]",
                        segment.index, err
                    )
                }
            }
        };

        updates.insert(segment.index, text);
    }

    if !updates.is_empty() {
        store.set_segment_texts_batch(meeting_id, &updates).await?;
    }
    Ok(())
}

async fn transcribe_segment_with_retry(
    segment_path: &Path,
    model: &str,
    app_data_dir: &Path,
) -> Result<String, String> {
    let first = transcribe_segment(segment_path, model, app_data_dir).await;
    if first.is_ok() {
        return first;
    }

    tokio::time::sleep(Duration::from_millis(140)).await;
    transcribe_segment(segment_path, model, app_data_dir).await
}

fn transcript_search_terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in query.split_whitespace() {
        if token.len() > 1 && !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
    }
    out
}

fn transcript_match_score(text: &str, normalized_query: &str, terms: &[String]) -> f32 {
    let normalized_text = text.to_lowercase();
    if normalized_text.contains(normalized_query) {
        return 1.0;
    }
    if terms.is_empty() {
        return 0.0;
    }

    let mut matched = 0usize;
    for term in terms {
        if normalized_text.contains(term) {
            matched += 1;
        }
    }

    matched as f32 / terms.len() as f32
}

fn transcript_embeddings_for_embedder(
    embedder: Option<&Embedder>,
    app_name: &str,
    title: &str,
    full_text: &str,
    compact_summary_text: &str,
    support_texts: &[String],
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let Some(embedder) = embedder else {
        return zero_transcript_embeddings();
    };

    let mut contexts = vec![
        (
            app_name.to_string(),
            title.to_string(),
            full_text.to_string(),
        ),
        (
            app_name.to_string(),
            title.to_string(),
            compact_summary_text.to_string(),
        ),
    ];
    contexts.extend(
        support_texts
            .iter()
            .cloned()
            .map(|value| (app_name.to_string(), title.to_string(), value)),
    );

    match embedder.embed_batch_with_context(&contexts) {
        Ok(vectors) => {
            let text_vec = vectors
                .first()
                .cloned()
                .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
            let snippet_vec = vectors
                .get(1)
                .cloned()
                .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
            let support_vec = if vectors.len() > 2 {
                mean_pool_embeddings(&vectors[2..])
            } else {
                vec![0.0; EMBEDDING_DIM]
            };
            (text_vec, snippet_vec, support_vec)
        }
        Err(err) => {
            tracing::warn!("Meeting transcript embedding failed: {}", err);
            zero_transcript_embeddings()
        }
    }
}

fn zero_transcript_embeddings() -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    (
        vec![0.0; EMBEDDING_DIM],
        vec![0.0; EMBEDDING_DIM],
        vec![0.0; EMBEDDING_DIM],
    )
}

fn should_retry_segment_text(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty()
        || trimmed.starts_with("[Transcription unavailable for segment")
        || trimmed.contains("backend unavailable")
        || trimmed.contains("Whisper GGUF runner returned empty transcript")
        || trimmed.contains("Custom meeting transcription command returned empty output")
}

async fn ingest_transcript_into_fndr_memory(
    app_state: Arc<AppState>,
    transcript: &MeetingTranscript,
    transcript_path: Option<&str>,
) -> Result<(), String> {
    let now = transcript.meeting.end_timestamp.unwrap_or_else(now_ms);

    let snippet: String = transcript.full_text.chars().take(260).collect::<String>();
    let full_text_for_embedding = transcript.full_text.chars().take(2200).collect::<String>();
    let snippet_for_embedding = if snippet.is_empty() {
        transcript.meeting.title.clone()
    } else {
        snippet.clone()
    };
    let lexical_shadow = build_lexical_shadow(
        &transcript.meeting.title,
        &snippet_for_embedding,
        &full_text_for_embedding,
        transcript_path,
    );
    let compact_summary_text = compact_summary_embedding_text(
        "fallback",
        &snippet_for_embedding,
        &full_text_for_embedding,
        &lexical_shadow,
    );
    let support_texts = support_embedding_texts(
        "FNDR Meetings",
        &transcript.meeting.title,
        &full_text_for_embedding,
        &lexical_shadow,
    );
    let (embedding, snippet_embedding, support_embedding) = transcript_embeddings_for_embedder(
        shared_meeting_embedder(),
        "FNDR Meetings",
        &transcript.meeting.title,
        &full_text_for_embedding,
        &compact_summary_text,
        &support_texts,
    );

    let record = MemoryRecord {
        id: Uuid::new_v4().to_string(),
        timestamp: now,
        day_bucket: chrono::Local::now().format("%Y-%m-%d").to_string(),
        app_name: "FNDR Meetings".to_string(),
        bundle_id: Some("com.fndr.meetings".to_string()),
        window_title: transcript.meeting.title.clone(),
        session_id: format!("meeting-{}", transcript.meeting.id),
        text: String::new(),
        clean_text: transcript.full_text.clone(),
        ocr_confidence: 1.0,
        ocr_block_count: transcript.segments.len() as u32,
        snippet: if snippet.is_empty() {
            "Meeting transcript captured".to_string()
        } else {
            snippet
        },
        summary_source: "fallback".to_string(),
        noise_score: 0.0,
        session_key: format!("meeting:{}", transcript.meeting.id),
        lexical_shadow,
        embedding,
        image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        screenshot_path: None,
        url: transcript_path.map(|p| p.to_string()),
        snippet_embedding,
        support_embedding,
        decay_score: 1.0,
        last_accessed_at: 0,
        ..Default::default()
    };

    app_state
        .store
        .add_batch(&[record.clone()])
        .await
        .map_err(|e| format!("Store add failed: {e}"))?;
    app_state.invalidate_memory_derived_caches();

    if let Err(err) =
        crate::context_runtime::sync_memory_record(app_state.as_ref(), &record, Some("audio")).await
    {
        tracing::warn!(
            "Context runtime sync failed for meeting transcript: {}",
            err
        );
    }

    Ok(())
}

fn collect_segment_files(audio_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(audio_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
        })
        .collect();

    files.sort();
    files
}

fn wait_for_segment_stability(audio_dir: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let unstable = collect_segment_files(audio_dir)
            .iter()
            .any(|path| is_recently_modified(path, 300));
        if !unstable || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

fn parse_segment_index(path: &Path) -> u32 {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return 0;
    };
    stem.rsplit('_')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

fn is_recently_modified(path: &Path, threshold_ms: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    elapsed.as_millis() < threshold_ms as u128
}

fn command_exists(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn resolve_ffmpeg_binary() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("FNDR_FFMPEG_PATH") {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }

    if command_exists("ffmpeg") {
        return Some(PathBuf::from("ffmpeg"));
    }

    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/opt/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for candidate in ["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for candidate in [
            "C:\\ffmpeg\\bin\\ffmpeg.exe",
            "C:\\Program Files\\ffmpeg\\bin\\ffmpeg.exe",
            "C:\\Program Files (x86)\\ffmpeg\\bin\\ffmpeg.exe",
        ] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}

fn get_store() -> Result<Arc<MeetingStore>, String> {
    runtime()
        .lock()
        .store
        .as_ref()
        .cloned()
        .ok_or_else(|| "Meeting runtime is not initialized".to_string())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingBackend;
    use std::io::Write;

    #[test]
    fn parse_segment_index_extracts_suffix() {
        assert_eq!(parse_segment_index(Path::new("segment_00000.wav")), 0);
        assert_eq!(parse_segment_index(Path::new("segment_00042.wav")), 42);
        assert_eq!(parse_segment_index(Path::new("not-a-segment.wav")), 0);
    }

    #[test]
    fn collect_segment_files_filters_and_sorts_wavs() {
        let root = std::env::temp_dir().join(format!("fndr-meeting-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp dir");

        let mut a = fs::File::create(root.join("segment_00010.wav")).expect("create wav a");
        a.write_all(b"wav-a").expect("write wav a");
        let mut b = fs::File::create(root.join("segment_00002.wav")).expect("create wav b");
        b.write_all(b"wav-b").expect("write wav b");
        let mut c = fs::File::create(root.join("notes.txt")).expect("create txt");
        c.write_all(b"ignore").expect("write txt");

        let files = collect_segment_files(&root);
        let names = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["segment_00002.wav", "segment_00010.wav"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_legacy_breakdown_response_sections() {
        let raw = "\
SUMMARY: Felipe owns homepage copy and Carlos owns meeting testing with clear deadlines.\n\
TODOS:\n\
- Finalize homepage copy and follow-up invite.\n\
REMINDERS:\n\
- Anna needs to send five UX issues by 2pm tomorrow.\n\
FOLLOWUPS:\n\
- Follow up with Carlos about the documented meeting test results.\n";

        let breakdown =
            parse_legacy_breakdown_response(raw).expect("legacy breakdown should parse");

        assert!(breakdown.summary.contains("Felipe owns homepage copy"));
        assert_eq!(breakdown.todos.len(), 1);
        assert_eq!(breakdown.reminders.len(), 1);
        assert_eq!(breakdown.followups.len(), 1);
    }

    #[test]
    fn quick_breakdown_extracts_summary_and_action_buckets() {
        let transcript = "\
Alright everyone, let's go through action items clearly so we can track to-dos, reminders, and follow-ups. \
Felipe must finalize homepage copy and the follow-up invite by 2pm tomorrow. \
Follow up with Anna to confirm the copy wording and review the onboarding screens. \
Anna needs to send five UX issues by 2pm tomorrow. \
Carlos must test the meeting feature in three scenarios and send results by Friday end of day.";

        let breakdown = build_quick_breakdown(transcript, None);

        assert!(breakdown.summary.contains("Discussion summary:"));
        assert!(breakdown
            .todos
            .iter()
            .any(|item| item.to_lowercase().contains("finalize homepage copy")));
        assert!(breakdown.reminders.iter().any(|item| {
            let lowered = item.to_lowercase();
            lowered.contains("tomorrow") || lowered.contains("friday")
        }));
        assert!(breakdown
            .followups
            .iter()
            .any(|item| item.to_lowercase().contains("follow up with anna")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prefers_real_microphone_over_virtual_audio_device() {
        let listing = r#"
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] FaceTime HD Camera
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] ZoomAudioDevice
[AVFoundation indev @ 0x1] [1] Anurup’s iPhone Microphone
[AVFoundation indev @ 0x1] [2] MacBook Pro Microphone
"#;

        assert_eq!(
            detect_macos_preferred_microphone_index_from_listing(listing),
            Some("2".to_string())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_loopback_device_when_available() {
        let listing = r#"
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] MacBook Pro Microphone
[AVFoundation indev @ 0x1] [3] BlackHole 2ch
"#;

        assert_eq!(
            detect_macos_loopback_audio_device_index_from_listing(listing),
            Some("3".to_string())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wait_for_process_exit_reports_timeout_for_long_running_process() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("sleep 2")
            .spawn()
            .expect("spawn sleep");
        let exited = wait_for_process_exit(&mut child, Duration::from_millis(120)).await;
        assert!(!exited);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wait_for_process_exit_detects_completion() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("sleep 0.1")
            .spawn()
            .expect("spawn short sleep");
        let exited = wait_for_process_exit(&mut child, Duration::from_secs(2)).await;
        assert!(exited);
    }

    #[test]
    fn transcript_embeddings_stay_zero_without_semantic_backend() {
        let support_texts = support_embedding_texts(
            "FNDR Meetings",
            "Weekly sync",
            "Reviewed roadmap updates and owners.",
            "Reviewed roadmap updates",
        );
        let (embedding, snippet_embedding, support_embedding) = transcript_embeddings_for_embedder(
            None,
            "FNDR Meetings",
            "Weekly sync",
            "Reviewed roadmap updates and owners.",
            "Reviewed roadmap updates",
            &support_texts,
        );

        assert!(embedding.iter().all(|value| *value == 0.0));
        assert!(snippet_embedding.iter().all(|value| *value == 0.0));
        assert!(support_embedding.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn transcript_embeddings_are_non_zero_for_real_backend_when_available() {
        let Ok(embedder) = Embedder::new() else {
            return;
        };
        if !matches!(embedder.backend(), EmbeddingBackend::Real) {
            return;
        }

        let support_texts = support_embedding_texts(
            "FNDR Meetings",
            "Weekly sync",
            "Reviewed roadmap updates and owners.",
            "Reviewed roadmap updates",
        );
        let (embedding, snippet_embedding, support_embedding) = transcript_embeddings_for_embedder(
            Some(&embedder),
            "FNDR Meetings",
            "Weekly sync",
            "Reviewed roadmap updates and owners.",
            "Reviewed roadmap updates",
            &support_texts,
        );

        assert!(embedding.iter().any(|value| *value != 0.0));
        assert!(snippet_embedding.iter().any(|value| *value != 0.0));
        assert!(support_embedding.iter().any(|value| *value != 0.0));
    }
}

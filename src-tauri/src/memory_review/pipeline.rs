//! Per-memory review pipeline: load → review → validate → write-back.
//!
//! The pipeline is intentionally pure with respect to the LLM call: it talks
//! to the model through the [`ReviewProvider`] trait so tests can substitute a
//! deterministic stub. Pressure-gating, queue draining, and worker scheduling
//! live in `worker.rs` and `mod.rs`; this file only describes the single-pass
//! transformation of one `MemoryRecord`.
//!
//! ## Validation contract
//! 1. **Grounding** — every entity / project / file-path / URL surfaced by
//!    the reviewer must appear in the bounded evidence we passed in, or in
//!    the same-day candidate id set. Hallucinated identifiers are rejected.
//! 2. **Meta-narration** — `memory_context` and `display_summary` are routed
//!    through [`narration_filter::clean_or_fallback_display_summary`]. If
//!    scrubbing fails we fall back to a deterministic non-narrated summary.
//! 3. **Insight** — `derive_insight_for_record` is re-run on the merged
//!    record so insight columns track the upgraded structured fields.
//! 4. **Embeddings** — `compose_embedding_text` regenerates the canonical
//!    embedding text and the v4 MiniLM 384 vectors are recomputed from it.
//!    V5 BGE 1024 chunks are not touched here; that surface is owned by the
//!    explicit `reindex_memories_v5` path.

use crate::embedding::Embedder;
use crate::memory_insight::derive_insight_for_record;
use crate::storage::{compose_embedding_text, MemoryRecord, Store};
use crate::summariser::narration_filter::clean_or_fallback_display_summary;
use futures::future::BoxFuture;

use super::queue::MemoryReviewJob;
use super::{
    review_skip_reason, MAX_RELATED_MEMORY_IDS, MAX_SAME_DAY_CANDIDATES, STATUS_PENDING,
    STATUS_PENDING_VISUAL_SEMANTICS, STATUS_REVIEWED_DAILY, STATUS_REVIEWED_LOCAL,
    STATUS_REVIEW_FAILED, SYNTHESIS_BRANCH_REVIEWED_DAILY, SYNTHESIS_BRANCH_REVIEWED_LOCAL,
};

const MAX_CLEAN_TEXT_CHARS: usize = 4000;

/// Bounded evidence handed to the reviewer.
#[derive(Debug, Clone)]
pub struct ReviewInput {
    pub memory_id: String,
    pub app_name: String,
    pub window_title: String,
    pub url: Option<String>,
    pub clean_text: String,
    pub current_memory_context: String,
    pub current_display_summary: String,
    pub synthesis_branch: String,
    pub same_day_candidates: Vec<SameDayCandidate>,
}

/// Slim representation of a same-day memory used so the reviewer can suggest
/// `related_memory_ids` grounded in records that actually exist.
#[derive(Debug, Clone)]
pub struct SameDayCandidate {
    pub id: String,
    pub display_title: String,
}

impl ReviewInput {
    pub fn from_record(record: &MemoryRecord, mut candidates: Vec<SameDayCandidate>) -> Self {
        candidates.retain(|c| c.id != record.id);
        if candidates.len() > MAX_SAME_DAY_CANDIDATES {
            candidates.truncate(MAX_SAME_DAY_CANDIDATES);
        }
        let clean_text = clip_chars(&record.clean_text, MAX_CLEAN_TEXT_CHARS);
        Self {
            memory_id: record.id.clone(),
            app_name: record.app_name.clone(),
            window_title: record.window_title.clone(),
            url: record.url.clone(),
            clean_text,
            current_memory_context: record.memory_context.clone(),
            current_display_summary: record.display_summary.clone(),
            synthesis_branch: record.synthesis_branch.clone(),
            same_day_candidates: candidates,
        }
    }

    pub fn candidate_ids(&self) -> Vec<String> {
        self.same_day_candidates
            .iter()
            .map(|c| c.id.clone())
            .collect()
    }
}

/// Reviewer output. All fields are advisory; the pipeline merges them into
/// the persisted record only after grounding + narration validation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewedMemory {
    pub memory_context: String,
    pub display_summary: String,
    pub topic: String,
    pub user_intent: String,
    pub activity_type: String,
    pub related_memory_ids: Vec<String>,
    pub confidence: f32,
}

/// LLM-free trait so tests can substitute a stub.
pub trait ReviewProvider: Send + Sync {
    fn review<'a>(
        &'a self,
        input: &'a ReviewInput,
    ) -> BoxFuture<'a, Result<ReviewedMemory, String>>;
}

/// How a successful review pass should be persisted. Selecting `ReviewedLocal`
/// keeps the original Subagent 9 behavior; `ReviewedDaily` is used by the
/// daily batch driver; `DryRun` computes the patched record but skips the
/// write-back (used by both manual + scheduled daily commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewWriteMode {
    /// Persist as `reviewed_local` / `synthesis_branch=reviewed_local`.
    ReviewedLocal,
    /// Persist as `reviewed_daily` / `synthesis_branch=reviewed_daily`.
    ReviewedDaily,
    /// Validate + compute the patched record, but skip the LanceDB write.
    DryRun,
}

impl ReviewWriteMode {
    fn success_status(self) -> &'static str {
        match self {
            ReviewWriteMode::ReviewedLocal => STATUS_REVIEWED_LOCAL,
            ReviewWriteMode::ReviewedDaily => STATUS_REVIEWED_DAILY,
            ReviewWriteMode::DryRun => STATUS_REVIEWED_LOCAL,
        }
    }

    fn success_branch(self) -> &'static str {
        match self {
            ReviewWriteMode::ReviewedLocal => SYNTHESIS_BRANCH_REVIEWED_LOCAL,
            ReviewWriteMode::ReviewedDaily => SYNTHESIS_BRANCH_REVIEWED_DAILY,
            ReviewWriteMode::DryRun => SYNTHESIS_BRANCH_REVIEWED_LOCAL,
        }
    }

    fn persists(self) -> bool {
        !matches!(self, ReviewWriteMode::DryRun)
    }
}

/// Outcome of a single review pass.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryReviewOutcome {
    /// The record was upgraded and persisted as `reviewed_local`.
    Reviewed {
        memory_id: String,
        reviewer_generation: u32,
    },
    /// The reviewer ran but the output was rejected by validation; the row
    /// is persisted with `enrichment_status = review_failed` and the original
    /// fields are preserved.
    Failed {
        memory_id: String,
        reason: ReviewError,
    },
    /// The job was skipped because the record no longer exists (e.g. it was
    /// deleted before the worker reached it). No write-back happens.
    Skipped { memory_id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewError {
    /// The LLM call itself failed.
    ProviderError(String),
    /// Reviewer surfaced an identifier not present in the evidence.
    GroundingViolation(String),
    /// Both the raw and scrubbed display_summary tripped the narration
    /// filter; the fallback was used and persisted.
    NarrationFallback,
    /// The reviewer returned both an empty memory_context and an empty
    /// display_summary — nothing to write back.
    EmptyOutput,
}

impl ReviewError {
    pub fn label(&self) -> &'static str {
        match self {
            ReviewError::ProviderError(_) => "provider_error",
            ReviewError::GroundingViolation(_) => "grounding_violation",
            ReviewError::NarrationFallback => "narration_fallback",
            ReviewError::EmptyOutput => "empty_output",
        }
    }
}

/// Single-record review pipeline.
///
/// `embedder` is optional; when omitted the embedding vector is left
/// untouched and only `embedding_text` is regenerated. Production callers
/// supply `Some(&Embedder::new()?)`. Tests can pass `None` to keep the unit
/// surface pure.
pub async fn review_one_memory(
    store: &Store,
    provider: &dyn ReviewProvider,
    embedder: Option<&Embedder>,
    job: &MemoryReviewJob,
    now_ms: i64,
) -> Result<MemoryReviewOutcome, String> {
    review_one_memory_with_mode(
        store,
        provider,
        embedder,
        job,
        now_ms,
        ReviewWriteMode::ReviewedLocal,
    )
    .await
}

/// Same as [`review_one_memory`] but parameterized over the target lifecycle
/// status. Used by the daily-review driver and by dry-run inspections.
pub async fn review_one_memory_with_mode(
    store: &Store,
    provider: &dyn ReviewProvider,
    embedder: Option<&Embedder>,
    job: &MemoryReviewJob,
    now_ms: i64,
    mode: ReviewWriteMode,
) -> Result<MemoryReviewOutcome, String> {
    let Some(mut record) = store
        .get_memory_by_id(&job.memory_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(MemoryReviewOutcome::Skipped {
            memory_id: job.memory_id.clone(),
            reason: "memory_not_found".to_string(),
        });
    };

    if let Some(reason) = review_skip_reason(&record) {
        if mode.persists() && repair_skipped_visual_fallback(&mut record, reason) {
            store
                .replace_memory_preserving_chunks(&record)
                .await
                .map_err(|err| err.to_string())?;
        }
        return Ok(MemoryReviewOutcome::Skipped {
            memory_id: job.memory_id.clone(),
            reason: reason.to_string(),
        });
    }

    let candidates = same_day_candidates(store, &record).await;
    let input = ReviewInput::from_record(&record, candidates);

    let reviewed = match provider.review(&input).await {
        Ok(reviewed) => reviewed,
        Err(err) => {
            if mode.persists() {
                mark_failed(&mut record, now_ms);
                persist_failure(store, &record).await?;
            }
            return Ok(MemoryReviewOutcome::Failed {
                memory_id: job.memory_id.clone(),
                reason: ReviewError::ProviderError(err),
            });
        }
    };

    let validated = match validate_review(&reviewed, &input) {
        Ok(validated) => validated,
        Err(err) => {
            if mode.persists() {
                mark_failed(&mut record, now_ms);
                persist_failure(store, &record).await?;
            }
            return Ok(MemoryReviewOutcome::Failed {
                memory_id: job.memory_id.clone(),
                reason: err,
            });
        }
    };

    let (merged_display_summary, narration_fallback_used) = {
        let url_ref = record.url.as_deref();
        let candidate = if !validated.display_summary.trim().is_empty() {
            validated.display_summary.clone()
        } else {
            record.display_summary.clone()
        };
        clean_or_fallback_display_summary(
            &candidate,
            &record.window_title,
            url_ref,
            record.timestamp,
        )
    };

    apply_reviewed_to_record(
        &mut record,
        &validated,
        &merged_display_summary,
        now_ms,
        mode.success_status(),
        mode.success_branch(),
    );

    derive_insight_for_record(&mut record);

    record.embedding_text = compose_embedding_text(&record);
    if let Some(embedder) = embedder {
        match embedder.embed_batch(&[record.embedding_text.clone()]) {
            Ok(vectors) => {
                if let Some(vector) = vectors.into_iter().next() {
                    if vector.len() == record.embedding.len() {
                        record.embedding = vector;
                    } else {
                        tracing::warn!(
                            memory_id = %record.id,
                            actual_dim = vector.len(),
                            expected_dim = record.embedding.len(),
                            "memory_review: embedder returned wrong-dim vector; keeping prior embedding"
                        );
                    }
                }
            }
            Err(err) => {
                tracing::warn!(
                    memory_id = %record.id,
                    err = %err,
                    "memory_review: embedder failed; keeping prior embedding"
                );
            }
        }
    }

    if mode.persists() {
        store
            .replace_memory_preserving_chunks(&record)
            .await
            .map_err(|err| err.to_string())?;
    }

    if narration_fallback_used {
        tracing::info!(
            memory_id = %record.id,
            reviewer_generation = record.reviewer_generation,
            mode = ?mode,
            "memory_review: narration fallback applied during review"
        );
    }

    Ok(MemoryReviewOutcome::Reviewed {
        memory_id: record.id.clone(),
        reviewer_generation: record.reviewer_generation,
    })
}

fn apply_reviewed_to_record(
    record: &mut MemoryRecord,
    reviewed: &ReviewedMemory,
    cleaned_display_summary: &str,
    now_ms: i64,
    success_status: &str,
    success_branch: &str,
) {
    if !reviewed.memory_context.trim().is_empty() {
        record.memory_context = reviewed.memory_context.trim().to_string();
    }
    if !cleaned_display_summary.trim().is_empty() {
        record.display_summary = cleaned_display_summary.trim().to_string();
    }
    if !reviewed.topic.trim().is_empty() {
        record.topic = reviewed.topic.trim().to_string();
    }
    if !reviewed.user_intent.trim().is_empty() {
        record.user_intent = reviewed.user_intent.trim().to_string();
    }
    if !reviewed.activity_type.trim().is_empty() {
        record.activity_type = reviewed.activity_type.trim().to_string();
    }
    if !reviewed.related_memory_ids.is_empty() {
        let mut combined = record.related_memory_ids.clone();
        for id in reviewed
            .related_memory_ids
            .iter()
            .take(MAX_RELATED_MEMORY_IDS)
        {
            if !combined.iter().any(|existing| existing == id) {
                combined.push(id.clone());
            }
        }
        record.related_memory_ids = combined;
    }
    record.enrichment_status = success_status.to_string();
    record.reviewed_at_ms = now_ms;
    record.reviewer_generation = record.reviewer_generation.saturating_add(1);
    record.synthesis_branch = success_branch.to_string();
    if reviewed.confidence > 0.0 {
        record.evidence_confidence = record.evidence_confidence.max(reviewed.confidence);
    }
}

fn mark_failed(record: &mut MemoryRecord, now_ms: i64) {
    record.enrichment_status = STATUS_REVIEW_FAILED.to_string();
    // Preserve every other field intact — the row's content is whatever
    // capture wrote. We do not bump `reviewer_generation` on failure; that
    // counter is reserved for successful upgrades.
    let _ = now_ms;
}

async fn persist_failure(store: &Store, record: &MemoryRecord) -> Result<(), String> {
    store
        .replace_memory_preserving_chunks(record)
        .await
        .map_err(|err| err.to_string())
}

fn repair_skipped_visual_fallback(record: &mut MemoryRecord, reason: &str) -> bool {
    let mut changed = false;
    if matches!(
        reason,
        "low_evidence_visual_fallback" | "visual_metadata_fallback"
    ) {
        if record.storage_outcome != "low_quality_evidence" {
            record.storage_outcome = "low_quality_evidence".to_string();
            changed = true;
        }
        let gate_reason = crate::memory_quality::quality_gate_reason(record);
        if record.quality_gate_reason != gate_reason {
            record.quality_gate_reason = gate_reason;
            changed = true;
        }
    }
    if matches!(
        record.enrichment_status.as_str(),
        "" | STATUS_PENDING | STATUS_REVIEW_FAILED
    ) {
        record.enrichment_status = STATUS_PENDING_VISUAL_SEMANTICS.to_string();
        changed = true;
    }
    changed
}

fn validate_review(
    reviewed: &ReviewedMemory,
    input: &ReviewInput,
) -> Result<ReviewedMemory, ReviewError> {
    if reviewed.memory_context.trim().is_empty() && reviewed.display_summary.trim().is_empty() {
        return Err(ReviewError::EmptyOutput);
    }

    // Narration filter — runs on both context + display_summary. The
    // pipeline downstream re-checks display_summary via
    // `clean_or_fallback_display_summary`; here we only catch the
    // memory_context narration (display_summary will fall back regardless).
    if narration_hits(&reviewed.memory_context) {
        return Err(ReviewError::NarrationFallback);
    }

    // Grounding: every related_memory_id must be a same-day candidate id.
    let candidate_ids = input.candidate_ids();
    for id in &reviewed.related_memory_ids {
        if !candidate_ids.iter().any(|known| known == id) {
            return Err(ReviewError::GroundingViolation(format!(
                "related_memory_id {id} is not a same-day candidate"
            )));
        }
    }

    // Grounding: free-text fields must not invent quoted identifiers that
    // aren't present in the evidence. We check a small set of structural
    // tokens (file paths, URLs, code identifiers with `::`) — natural-language
    // restatement is allowed.
    let evidence = build_evidence_blob(input);
    for field in [
        reviewed.memory_context.as_str(),
        reviewed.display_summary.as_str(),
    ] {
        if let Some(token) = first_ungrounded_structural_token(field, &evidence) {
            return Err(ReviewError::GroundingViolation(format!(
                "structural token {token:?} not present in evidence"
            )));
        }
    }

    let mut sanitized = reviewed.clone();
    sanitized
        .related_memory_ids
        .truncate(MAX_RELATED_MEMORY_IDS);
    sanitized.activity_type = crate::inference::normalize_activity_type(&sanitized.activity_type);
    Ok(sanitized)
}

fn build_evidence_blob(input: &ReviewInput) -> String {
    let mut blob = String::new();
    blob.push_str(&input.clean_text);
    blob.push('\n');
    blob.push_str(&input.window_title);
    blob.push('\n');
    if let Some(url) = &input.url {
        blob.push_str(url);
        blob.push('\n');
    }
    blob.push_str(&input.current_memory_context);
    blob.push('\n');
    blob.push_str(&input.current_display_summary);
    blob.push('\n');
    blob.push_str(&input.app_name);
    blob
}

fn first_ungrounded_structural_token(field: &str, evidence: &str) -> Option<String> {
    for token in field.split_whitespace() {
        let trimmed = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != ':');
        if trimmed.is_empty() {
            continue;
        }
        let is_structural = trimmed.contains("://")
            || trimmed.contains('/') && trimmed.len() > 4
            || trimmed.contains("::") && trimmed.len() > 4;
        if !is_structural {
            continue;
        }
        if !evidence.contains(trimmed) {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn narration_hits(value: &str) -> bool {
    crate::summariser::narration_filter::narration_filter_hits(value)
}

async fn same_day_candidates(store: &Store, record: &MemoryRecord) -> Vec<SameDayCandidate> {
    let day_start = if record.timestamp > 0 {
        record.timestamp - 24 * 60 * 60 * 1000
    } else {
        0
    };
    let day_end = if record.timestamp > 0 {
        record.timestamp + 24 * 60 * 60 * 1000
    } else {
        i64::MAX
    };
    match store.get_memories_in_range(day_start, day_end).await {
        Ok(records) => records
            .into_iter()
            .filter(|r| r.id != record.id)
            .take(MAX_SAME_DAY_CANDIDATES * 2)
            .map(|r| SameDayCandidate {
                display_title: pick_candidate_title(&r),
                id: r.id,
            })
            .collect(),
        Err(err) => {
            tracing::warn!(
                memory_id = %record.id,
                err = %err,
                "memory_review: could not load same-day candidates"
            );
            Vec::new()
        }
    }
}

fn pick_candidate_title(record: &MemoryRecord) -> String {
    if !record.display_summary.trim().is_empty() {
        record.display_summary.trim().chars().take(80).collect()
    } else if !record.window_title.trim().is_empty() {
        record.window_title.trim().chars().take(80).collect()
    } else {
        record.snippet.trim().chars().take(80).collect()
    }
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

// Compile-time check that the lifecycle constants stay stable across the
// crate. `STATUS_PENDING` is consumed by `capture/mod.rs` on the write path
// and by the worker on the read path.
const _: &str = STATUS_PENDING;

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt;
    use std::sync::Arc;

    struct StubProvider {
        result: Result<ReviewedMemory, String>,
    }

    impl ReviewProvider for StubProvider {
        fn review<'a>(
            &'a self,
            _input: &'a ReviewInput,
        ) -> BoxFuture<'a, Result<ReviewedMemory, String>> {
            let r = self.result.clone();
            async move { r }.boxed()
        }
    }

    struct PanicProvider;

    impl ReviewProvider for PanicProvider {
        fn review<'a>(
            &'a self,
            _input: &'a ReviewInput,
        ) -> BoxFuture<'a, Result<ReviewedMemory, String>> {
            async move { panic!("low-evidence visual fallback should not reach review provider") }
                .boxed()
        }
    }

    fn input(memory_id: &str, clean_text: &str, candidates: Vec<SameDayCandidate>) -> ReviewInput {
        ReviewInput {
            memory_id: memory_id.to_string(),
            app_name: "Visual Studio Code".to_string(),
            window_title: "continuum/chunk_route.rs".to_string(),
            url: None,
            clean_text: clean_text.to_string(),
            current_memory_context: String::new(),
            current_display_summary: String::new(),
            synthesis_branch: "llm".to_string(),
            same_day_candidates: candidates,
        }
    }

    #[test]
    fn empty_output_is_rejected() {
        let i = input("mem-1", "Working on the chunk route", Vec::new());
        let r = ReviewedMemory::default();
        let err = validate_review(&r, &i).unwrap_err();
        assert_eq!(err, ReviewError::EmptyOutput);
    }

    #[test]
    fn meta_narration_in_memory_context_is_rejected() {
        let i = input("mem-1", "Working on the chunk route", Vec::new());
        let r = ReviewedMemory {
            memory_context: "You reviewed memory_compaction.rs and the chunk route.".to_string(),
            display_summary: "Reviewed the chunk route".to_string(),
            ..ReviewedMemory::default()
        };
        let err = validate_review(&r, &i).unwrap_err();
        assert_eq!(err, ReviewError::NarrationFallback);
    }

    #[test]
    fn ungrounded_related_memory_id_is_rejected() {
        let i = input(
            "mem-1",
            "Working on the chunk route",
            vec![SameDayCandidate {
                id: "mem-real".to_string(),
                display_title: "Real candidate".to_string(),
            }],
        );
        let r = ReviewedMemory {
            memory_context: "Captured the chunk-first retrieval design.".to_string(),
            display_summary: "Chunk-first design".to_string(),
            related_memory_ids: vec!["mem-hallucinated".to_string()],
            ..ReviewedMemory::default()
        };
        match validate_review(&r, &i).unwrap_err() {
            ReviewError::GroundingViolation(msg) => {
                assert!(msg.contains("mem-hallucinated"), "msg={msg}");
            }
            other => panic!("expected grounding violation, got {other:?}"),
        }
    }

    #[test]
    fn ungrounded_url_path_in_display_summary_is_rejected() {
        let i = input("mem-1", "Just plain prose without URLs", Vec::new());
        let r = ReviewedMemory {
            memory_context: "Captured plain prose.".to_string(),
            display_summary: "Read https://hallucinated.example.com/path".to_string(),
            ..ReviewedMemory::default()
        };
        let err = validate_review(&r, &i).unwrap_err();
        match err {
            ReviewError::GroundingViolation(msg) => {
                assert!(msg.contains("hallucinated"), "msg={msg}");
            }
            other => panic!("expected grounding violation, got {other:?}"),
        }
    }

    #[test]
    fn grounded_url_in_evidence_passes() {
        let mut i = input(
            "mem-1",
            "Reviewed the docs at the LanceDB site.",
            Vec::new(),
        );
        i.url = Some("https://lancedb.com/docs".to_string());
        let r = ReviewedMemory {
            memory_context: "Reviewed the LanceDB query patterns at https://lancedb.com/docs"
                .to_string(),
            display_summary: "Reviewed LanceDB query docs".to_string(),
            ..ReviewedMemory::default()
        };
        let validated = validate_review(&r, &i).expect("grounded URL must pass");
        assert_eq!(validated.memory_context, r.memory_context);
    }

    #[test]
    fn related_memory_ids_are_truncated_after_cap() {
        let candidates = (0..10)
            .map(|i| SameDayCandidate {
                id: format!("mem-{i}"),
                display_title: format!("Title {i}"),
            })
            .collect::<Vec<_>>();
        let mut i = input("mem-x", "Some captured prose", candidates);
        i.same_day_candidates.truncate(10);
        let r = ReviewedMemory {
            memory_context: "Captured prose with related work.".to_string(),
            display_summary: "Prose with related work".to_string(),
            related_memory_ids: (0..6).map(|i| format!("mem-{i}")).collect(),
            ..ReviewedMemory::default()
        };
        let validated = validate_review(&r, &i).expect("valid");
        assert_eq!(validated.related_memory_ids.len(), MAX_RELATED_MEMORY_IDS);
    }

    #[test]
    fn review_validation_normalizes_invalid_activity_type() {
        let i = input(
            "mem-1",
            "Reviewed chunk-first retrieval results",
            Vec::new(),
        );
        let r = ReviewedMemory {
            memory_context: "Reviewed chunk-first retrieval results.".to_string(),
            display_summary: "Reviewed retrieval results".to_string(),
            activity_type: "coding|debugging|reviewing_agent_output|researching|unknown"
                .to_string(),
            ..ReviewedMemory::default()
        };

        let validated = validate_review(&r, &i).expect("valid review should pass");

        assert_eq!(validated.activity_type, "unknown");
    }

    #[tokio::test]
    async fn review_one_memory_marks_record_reviewed_local_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = Arc::new(
            tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
                .await
                .unwrap(),
        );

        let mut record = MemoryRecord::default();
        record.id = "mem-success".to_string();
        record.timestamp = 1_700_000_000_000;
        record.app_name = "Chrome".to_string();
        record.window_title = "Continuum architecture - Notion".to_string();
        record.clean_text =
            "The chunk-first route runs ANN against memory_chunks_v1_bge_1024.".to_string();
        record.snippet = "Chunk-first ANN route".to_string();
        record.enrichment_status = STATUS_PENDING.to_string();
        record.embedding = vec![0.0; 384];
        record.image_embedding = vec![0.0; 768];
        record.snippet_embedding = vec![0.0; 384];
        record.support_embedding = vec![0.0; 384];

        store
            .add_batch_preserving_ids(&[record.clone()])
            .await
            .unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Captured the chunk-first route ANN behavior.".to_string(),
                display_summary: "Chunk-first ANN route".to_string(),
                topic: "rag".to_string(),
                user_intent: "design".to_string(),
                activity_type: "browsing".to_string(),
                related_memory_ids: Vec::new(),
                confidence: 0.85,
            }),
        };

        let outcome = review_one_memory(
            &store,
            &provider,
            None,
            &MemoryReviewJob {
                memory_id: "mem-success".to_string(),
                day_bucket: record.day_bucket.clone(),
                enqueued_at_ms: 1_700_000_001_000,
            },
            1_700_000_002_000,
        )
        .await
        .unwrap();

        assert!(
            matches!(outcome, MemoryReviewOutcome::Reviewed { ref memory_id, reviewer_generation } if memory_id == "mem-success" && reviewer_generation == 1),
            "got {outcome:?}"
        );

        let written = store
            .get_memory_by_id("mem-success")
            .await
            .unwrap()
            .expect("record persists");
        assert_eq!(written.enrichment_status, STATUS_REVIEWED_LOCAL);
        assert_eq!(written.reviewed_at_ms, 1_700_000_002_000);
        assert_eq!(written.reviewer_generation, 1);
        assert_eq!(written.synthesis_branch, SYNTHESIS_BRANCH_REVIEWED_LOCAL);
        assert!(written.memory_context.contains("chunk-first"));
        assert!(!written.embedding_text.is_empty());
    }

    #[tokio::test]
    async fn review_one_memory_marks_failed_and_preserves_original_on_provider_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = Arc::new(
            tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
                .await
                .unwrap(),
        );

        let mut record = MemoryRecord::default();
        record.id = "mem-fail".to_string();
        record.timestamp = 1_700_000_000_000;
        record.app_name = "Chrome".to_string();
        record.window_title = "Original title".to_string();
        record.display_summary = "Original display summary".to_string();
        record.memory_context = "Original memory context".to_string();
        record.clean_text = "Captured page about something.".to_string();
        record.enrichment_status = STATUS_PENDING.to_string();
        record.embedding = vec![0.0; 384];
        record.image_embedding = vec![0.0; 768];
        record.snippet_embedding = vec![0.0; 384];
        record.support_embedding = vec![0.0; 384];

        store
            .add_batch_preserving_ids(&[record.clone()])
            .await
            .unwrap();

        let provider = StubProvider {
            result: Err("LLM timed out".to_string()),
        };
        let outcome = review_one_memory(
            &store,
            &provider,
            None,
            &MemoryReviewJob {
                memory_id: "mem-fail".to_string(),
                day_bucket: record.day_bucket.clone(),
                enqueued_at_ms: 1_700_000_001_000,
            },
            1_700_000_002_000,
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            MemoryReviewOutcome::Failed {
                reason: ReviewError::ProviderError(_),
                ..
            }
        ));

        let written = store
            .get_memory_by_id("mem-fail")
            .await
            .unwrap()
            .expect("record persists");
        assert_eq!(written.enrichment_status, STATUS_REVIEW_FAILED);
        assert_eq!(written.reviewer_generation, 0);
        assert_eq!(written.memory_context, "Original memory context");
        assert_eq!(written.display_summary, "Original display summary");
    }

    #[tokio::test]
    async fn review_one_memory_marks_failed_and_preserves_original_on_meta_narration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = Arc::new(
            tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
                .await
                .unwrap(),
        );

        let mut record = MemoryRecord::default();
        record.id = "mem-narration".to_string();
        record.timestamp = 1_700_000_000_000;
        record.app_name = "Chrome".to_string();
        record.window_title = "Original title".to_string();
        record.display_summary = "Original display summary".to_string();
        record.memory_context = "Original memory context".to_string();
        record.clean_text = "Captured page about debounce ticks.".to_string();
        record.enrichment_status = STATUS_PENDING.to_string();
        record.embedding = vec![0.0; 384];
        record.image_embedding = vec![0.0; 768];
        record.snippet_embedding = vec![0.0; 384];
        record.support_embedding = vec![0.0; 384];

        store
            .add_batch_preserving_ids(&[record.clone()])
            .await
            .unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "You reviewed memory_compaction.rs and the debounce ticks code."
                    .to_string(),
                display_summary: "Reviewed the chunk route".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let outcome = review_one_memory(
            &store,
            &provider,
            None,
            &MemoryReviewJob {
                memory_id: "mem-narration".to_string(),
                day_bucket: record.day_bucket.clone(),
                enqueued_at_ms: 1_700_000_001_000,
            },
            1_700_000_002_000,
        )
        .await
        .unwrap();

        match outcome {
            MemoryReviewOutcome::Failed { reason, .. } => {
                assert_eq!(reason, ReviewError::NarrationFallback);
            }
            other => panic!("expected Failed(NarrationFallback), got {other:?}"),
        }

        let written = store
            .get_memory_by_id("mem-narration")
            .await
            .unwrap()
            .expect("record persists");
        assert_eq!(written.enrichment_status, STATUS_REVIEW_FAILED);
        assert_eq!(written.memory_context, "Original memory context");
    }

    #[tokio::test]
    async fn review_one_memory_skips_empty_ocr_visual_fallback_without_marking_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = Arc::new(
            tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
                .await
                .unwrap(),
        );

        let mut record = MemoryRecord::default();
        record.id = "mem-visual-fallback".to_string();
        record.timestamp = 1_700_000_000_000;
        record.app_name = "Codex".to_string();
        record.window_title = "Codex".to_string();
        record.clean_text = "Screen capture (visual): Codex_1700000000000.png. Codex".to_string();
        record.memory_context = record.clean_text.clone();
        record.display_summary = record.clean_text.clone();
        record.synthesis_branch = "llm_ocr_grounded_visual_fallback".to_string();
        record.raw_evidence = r#"{"source_kind":"visual_capture","synthesis_branch":"llm_ocr_grounded_visual_fallback","visual_understanding":{"status":"clip_image_embedding","raw_pixels_persisted":false},"vlm_route":"fallback_ocr_only","vlm_runtime_status":"deferred_low_ram","vlm_capability":"model_missing"}"#.to_string();
        record.ocr_confidence = 0.0;
        record.ocr_block_count = 0;
        record.enrichment_status = STATUS_REVIEW_FAILED.to_string();
        record.storage_outcome = "enriched_memory_card".to_string();
        record.embedding = vec![0.0; 384];
        record.image_embedding = vec![0.0; 768];
        record.snippet_embedding = vec![0.0; 384];
        record.support_embedding = vec![0.0; 384];

        store
            .add_batch_preserving_ids(&[record.clone()])
            .await
            .unwrap();

        let outcome = review_one_memory(
            &store,
            &PanicProvider,
            None,
            &MemoryReviewJob {
                memory_id: "mem-visual-fallback".to_string(),
                day_bucket: record.day_bucket.clone(),
                enqueued_at_ms: 1_700_000_001_000,
            },
            1_700_000_002_000,
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            MemoryReviewOutcome::Skipped {
                ref memory_id,
                ref reason
            } if memory_id == "mem-visual-fallback" && reason == "low_evidence_visual_fallback"
        ));

        let written = store
            .get_memory_by_id("mem-visual-fallback")
            .await
            .unwrap()
            .expect("record persists");
        assert_eq!(written.enrichment_status, STATUS_PENDING_VISUAL_SEMANTICS);
        assert_eq!(written.storage_outcome, "low_quality_evidence");
        assert_eq!(written.reviewer_generation, 0);
    }
}

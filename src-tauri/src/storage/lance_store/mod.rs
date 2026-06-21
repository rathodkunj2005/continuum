//! LanceDB-backed storage for Continuum memory records.
//!
//! Replaces the JSON-based simple_store with a proper vector database.
//! All methods that touch LanceDB are async.

use super::schema::{
    ActivityEvent, AppCount, ContextDelta, ContextPack, DayCount, DaypartCount,
    DecisionLedgerEntry, DomainCount, EdgeType, EntityAliasRecord, GraphEdge, GraphNode, HourCount,
    KnowledgePage, MeetingSegment, MeetingSession, MemoryChunkRecord, MemoryChunkSearchResult,
    MemoryRecord, NodeType, ProjectContext, SearchResult, Stats, Task, WeekdayCount,
};
use crate::config::{
    DEFAULT_IMAGE_EMBEDDING_DIM, DEFAULT_STORE_KEYWORD_QUERY_MULTIPLIER,
    DEFAULT_STORE_MAX_KEYWORD_SCAN, DEFAULT_STORE_VECTOR_QUERY_MULTIPLIER,
    DEFAULT_TEXT_EMBEDDING_DIM,
};
use crate::inference::model_config::{BGE_V5_DIMENSIONS, MEMORIES_V5_TABLE};
// Re-exported so `lance_store::tests` can call the shared quality helpers via
// `use super::*;`. These are not used directly in this file.
#[allow(unused_imports)]
use crate::memory_quality::{
    classify_storage_outcome, default_memory_quality_config, deterministic_dedup_fingerprint,
    is_supported_dedup_fingerprint, quality_gate_reason as shared_quality_gate_reason,
};
use arrow_array::{
    Array, Float32Array, Int64Array, RecordBatch,
    RecordBatchIterator, RecordBatchReader, StringArray,
};
use chrono::{Datelike, Local, TimeZone, Timelike};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::AddDataMode;
use lancedb::Table;
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Active LanceDB table for memory records.
///
/// **Current durable write path:** v4 = all-MiniLM-L6-v2, 384-d vectors.
/// This re-exports `inference::model_config::MEMORIES_V4_TABLE` so the
/// embedding contract has exactly one source of truth (see `model_config.rs`).
pub const MEMORIES_TABLE: &str = crate::inference::model_config::MEMORIES_V4_TABLE;
pub const MEMORIES_V5_PARENT_TABLE: &str = MEMORIES_V5_TABLE;
pub const MEMORY_CHUNKS_TABLE: &str = "memory_chunks_v1_bge_1024";
pub const TASKS_TABLE: &str = "tasks";
pub const MEETINGS_TABLE: &str = "meetings";
pub const SEGMENTS_TABLE: &str = "segments";
pub const NODES_TABLE: &str = "knowledge_nodes";
pub const EDGES_TABLE: &str = "knowledge_edges";
pub const ACTIVITY_EVENTS_TABLE: &str = "activity_events";
pub const PROJECT_CONTEXTS_TABLE: &str = "project_contexts";
pub const DECISION_LEDGER_TABLE: &str = "decision_ledger";
pub const CONTEXT_PACKS_TABLE: &str = "context_packs";
pub const CONTEXT_DELTAS_TABLE: &str = "context_deltas";
pub const ENTITY_ALIASES_TABLE: &str = "entity_aliases";
pub const KNOWLEDGE_PAGES_TABLE: &str = "knowledge_pages";
/// Insight graph v2 (ADR-style types); distinct from `knowledge_nodes` / `knowledge_edges`.
pub const GRAPH_NODES_TABLE: &str = "graph_nodes";
pub const GRAPH_EDGES_TABLE: &str = "graph_edges";
const SEARCH_RESULT_COLUMNS: &[&str] = &[
    "id",
    "timestamp",
    "app_name",
    "bundle_id",
    "window_title",
    "session_id",
    "text",
    "clean_text",
    "ocr_confidence",
    "ocr_block_count",
    "snippet",
    "display_summary",
    "internal_context",
    "summary_source",
    "noise_score",
    "session_key",
    "lexical_shadow",
    "memory_context",
    "reopen_kind",
    "reopen_url",
    "reopen_file_path",
    "reopen_app_bundle_id",
    "reopen_app_name",
    "reopen_app_deep_link",
    "reopen_captured_at_ms",
    "reopen_confidence",
    "reopen_validation_status",
    "user_intent",
    "topic",
    "workflow",
    "search_aliases",
    "related_memory_ids",
    "evidence_confidence",
    "confidence_score",
    "importance_score",
    "specificity_score",
    "intent_score",
    "entity_score",
    "agent_usefulness_score",
    "ocr_noise_score",
    "screenshot_path",
    "url",
    "decay_score",
    "entities",
    "anchor_coverage_score",
    "content_hash",
    "insight_what_happened",
    "insight_why_mattered",
    "insight_what_changed",
    "insight_context_thread",
    "insight_spans_json",
    "insight_card_confidence",
    "synthesis_branch",
    "topic_categories",
    "enrichment_status",
    "reviewed_at_ms",
    "reviewer_generation",
    "storage_outcome",
    "raw_evidence",
];
const TEXT_EMBED_DIM: i32 = DEFAULT_TEXT_EMBEDDING_DIM as i32;
const IMAGE_EMBED_DIM: i32 = DEFAULT_IMAGE_EMBEDDING_DIM as i32;
const VECTOR_QUERY_MULTIPLIER: usize = DEFAULT_STORE_VECTOR_QUERY_MULTIPLIER;
const KEYWORD_QUERY_MULTIPLIER: usize = DEFAULT_STORE_KEYWORD_QUERY_MULTIPLIER;
const MAX_KEYWORD_SCAN: usize = DEFAULT_STORE_MAX_KEYWORD_SCAN;
const INDEX_NOISE_HOSTS: &[&str] = &[
    "accounts.google.com",
    "auth.openai.com",
    "idmsa.apple.com",
    "login.live.com",
    "login.microsoftonline.com",
];

/// LanceDB-backed store for memory records.
pub struct Store {
    data_dir: PathBuf,
    table: Table,
    memories_v5_table: Table,
    memory_chunks_table: Table,
    tasks_table: Table,
    meetings_table: Table,
    segments_table: Table,
    nodes_table: Table,
    edges_table: Table,
    activity_events_table: Table,
    project_contexts_table: Table,
    decision_ledger_table: Table,
    context_packs_table: Table,
    context_deltas_table: Table,
    entity_aliases_table: Table,
    knowledge_pages_table: Table,
    pub(crate) graph_nodes_table: Table,
    pub(crate) graph_edges_table: Table,
}

mod arrow_and_filters;
mod normalize_embed_migrate;
mod schemas;
mod text_kw;

use arrow_and_filters::*;
use normalize_embed_migrate::*;
use schemas::*;
use text_kw::*;

pub use normalize_embed_migrate::{
    compose_embedding_text, generate_search_aliases_public, normalize_record_for_index,
    pollution_ratio_score, salience_concentration_score, topic_clarity_score,
};

impl Store {
    /// Open (or create) the LanceDB store at `data_dir`.
    ///
    /// This is synchronous — it spins up a temporary Tokio runtime for
    /// initialization so it can be called from non-async contexts (e.g.
    /// the Tauri `setup()` callback).
    pub fn new(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = data_dir.to_path_buf();
        let db_path = data_dir.join("lancedb");
        std::fs::create_dir_all(&db_path)?;

        // Temporary single-threaded runtime for initialization only.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let (
            table,
            _legacy_table,
            memories_v5_table,
            memory_chunks_table,
            tasks_table,
            meetings_table,
            segments_table,
            nodes_table,
            edges_table,
            activity_events_table,
            project_contexts_table,
            decision_ledger_table,
            context_packs_table,
            context_deltas_table,
            entity_aliases_table,
            knowledge_pages_table,
            graph_nodes_table,
            graph_edges_table,
        ) = rt.block_on(open_all_tables(&db_path))?;

        // Migrate legacy storages if present.
        let memories_json = data_dir.join("memories.json");
        if memories_json.exists() {
            rt.block_on(migrate_from_json(&table, &memories_json));
        }

        let tasks_json = data_dir.join("tasks.json");
        if tasks_json.exists() {
            rt.block_on(migrate_tasks_from_json(&tasks_table, &tasks_json));
        }

        let meetings_json = data_dir.join("meetings/meetings.json");
        if meetings_json.exists() {
            rt.block_on(migrate_meetings_from_json(&meetings_table, &meetings_json));
        }

        let segments_json = data_dir.join("meetings/segments.json");
        if segments_json.exists() {
            rt.block_on(migrate_segments_from_json(&segments_table, &segments_json));
        }

        let graph_json = data_dir.join("memory_graph.json");
        if graph_json.exists() {
            rt.block_on(migrate_graph_from_json(
                &nodes_table,
                &edges_table,
                &graph_json,
            ));
        }

        Ok(Self {
            data_dir,
            table,
            memories_v5_table,
            memory_chunks_table,
            tasks_table,
            meetings_table,
            segments_table,
            nodes_table,
            edges_table,
            activity_events_table,
            project_contexts_table,
            decision_ledger_table,
            context_packs_table,
            context_deltas_table,
            entity_aliases_table,
            knowledge_pages_table,
            graph_nodes_table,
            graph_edges_table,
        })
    }

    pub async fn upsert_tasks(&self, tasks: &[Task]) -> Result<(), Box<dyn std::error::Error>> {
        let batch = task_to_batch(tasks)?;
        let schema = Arc::new(task_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.tasks_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>, Box<dyn std::error::Error>> {
        let batches = self
            .tasks_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for b in batches {
            results.extend(batch_to_tasks(&b));
        }
        Ok(results)
    }

    pub async fn upsert_activity_events(
        &self,
        events: &[ActivityEvent],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if events.is_empty() {
            return Ok(());
        }

        let mut by_id: HashMap<String, ActivityEvent> = HashMap::with_capacity(events.len());
        for event in events {
            by_id.insert(event.id.clone(), event.clone());
        }
        let deduped = by_id.into_values().collect::<Vec<_>>();
        if let Some(filter) = build_string_match_filter(
            "id",
            &deduped
                .iter()
                .map(|event| event.id.clone())
                .collect::<Vec<_>>(),
        ) {
            self.activity_events_table.delete(&filter).await?;
        }

        let batch = activity_events_to_batch(&deduped)?;
        let schema = Arc::new(activity_event_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.activity_events_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn get_activity_event_by_id(
        &self,
        event_id: &str,
    ) -> Result<Option<ActivityEvent>, Box<dyn std::error::Error>> {
        let filter = format!("id = '{}'", sql_escape(event_id));
        let batches = self
            .activity_events_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(event) = batch_to_activity_events(&batch).into_iter().next() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    pub async fn get_activity_event_by_memory_id(
        &self,
        memory_id: &str,
    ) -> Result<Option<ActivityEvent>, Box<dyn std::error::Error>> {
        let filter = format!("memory_id = '{}'", sql_escape(memory_id));
        let batches = self
            .activity_events_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(event) = batch_to_activity_events(&batch).into_iter().next() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    pub async fn list_activity_events(
        &self,
        limit: usize,
        project: Option<&str>,
    ) -> Result<Vec<ActivityEvent>, Box<dyn std::error::Error>> {
        let mut query = self.activity_events_table.query();
        if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
            query = query.only_if(format!("project = '{}'", sql_escape(project)));
        }
        let batches = query.execute().await?.try_collect::<Vec<_>>().await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_activity_events(&batch));
        }
        results.sort_by_key(|event| std::cmp::Reverse(event.end_time));
        results.truncate(limit.max(1));
        Ok(results)
    }

    pub async fn upsert_project_contexts(
        &self,
        contexts: &[ProjectContext],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if contexts.is_empty() {
            return Ok(());
        }

        let mut by_project: HashMap<String, ProjectContext> =
            HashMap::with_capacity(contexts.len());
        for context in contexts {
            by_project.insert(context.project.clone(), context.clone());
        }
        let deduped = by_project.into_values().collect::<Vec<_>>();
        if let Some(filter) = build_string_match_filter(
            "project",
            &deduped
                .iter()
                .map(|context| context.project.clone())
                .collect::<Vec<_>>(),
        ) {
            self.project_contexts_table.delete(&filter).await?;
        }

        let batch = project_contexts_to_batch(&deduped)?;
        let schema = Arc::new(project_context_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.project_contexts_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn get_project_context(
        &self,
        project: &str,
    ) -> Result<Option<ProjectContext>, Box<dyn std::error::Error>> {
        let filter = format!("project = '{}'", sql_escape(project));
        let batches = self
            .project_contexts_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(context) = batch_to_project_contexts(&batch).into_iter().next() {
                return Ok(Some(context));
            }
        }
        Ok(None)
    }

    pub async fn upsert_knowledge_pages(&self, pages: &[KnowledgePage]) -> Result<(), String> {
        if pages.is_empty() {
            return Ok(());
        }

        let mut by_id: HashMap<String, KnowledgePage> = HashMap::with_capacity(pages.len());
        for page in pages {
            by_id.insert(page.page_id.clone(), page.clone());
        }
        let deduped = by_id.into_values().collect::<Vec<_>>();
        if let Some(filter) = build_string_match_filter(
            "page_id",
            &deduped
                .iter()
                .map(|page| page.page_id.clone())
                .collect::<Vec<_>>(),
        ) {
            self.knowledge_pages_table
                .delete(&filter)
                .await
                .map_err(|e| e.to_string())?;
        }

        let batch = knowledge_pages_to_batch(&deduped).map_err(|e| e.to_string())?;
        let schema = Arc::new(knowledge_page_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.knowledge_pages_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn list_knowledge_pages(
        &self,
        limit: usize,
        page_type: Option<&str>,
        project: Option<&str>,
    ) -> Result<Vec<KnowledgePage>, Box<dyn std::error::Error>> {
        let mut query = self.knowledge_pages_table.query();
        let mut clauses = Vec::new();
        if let Some(page_type) = page_type.filter(|value| !value.trim().is_empty()) {
            clauses.push(format!("page_type = '{}'", sql_escape(page_type)));
        }
        if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
            clauses.push(format!("project = '{}'", sql_escape(project)));
        }
        if !clauses.is_empty() {
            query = query.only_if(clauses.join(" AND "));
        }
        let batches = query.execute().await?.try_collect::<Vec<_>>().await?;
        let mut pages = Vec::new();
        for batch in batches {
            pages.extend(batch_to_knowledge_pages(&batch));
        }
        pages.sort_by_key(|page| std::cmp::Reverse(page.last_updated));
        pages.truncate(limit.max(1));
        Ok(pages)
    }

    pub async fn get_knowledge_page(
        &self,
        page_id: &str,
    ) -> Result<Option<KnowledgePage>, Box<dyn std::error::Error>> {
        let filter = format!("page_id = '{}'", sql_escape(page_id));
        let batches = self
            .knowledge_pages_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(page) = batch_to_knowledge_pages(&batch).into_iter().next() {
                return Ok(Some(page));
            }
        }
        Ok(None)
    }

    pub async fn append_decision_ledger_entries(
        &self,
        entries: &[DecisionLedgerEntry],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if entries.is_empty() {
            return Ok(());
        }

        let batch = decision_ledger_to_batch(entries)?;
        let schema = Arc::new(decision_ledger_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.decision_ledger_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn list_decision_ledger_entries(
        &self,
        limit: usize,
        project: Option<&str>,
    ) -> Result<Vec<DecisionLedgerEntry>, Box<dyn std::error::Error>> {
        let mut query = self.decision_ledger_table.query();
        if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
            query = query.only_if(format!("project = '{}'", sql_escape(project)));
        }
        let batches = query.execute().await?.try_collect::<Vec<_>>().await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_decision_ledger_entries(&batch));
        }
        results.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
        results.truncate(limit.max(1));
        Ok(results)
    }

    pub async fn append_context_packs(
        &self,
        packs: &[ContextPack],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if packs.is_empty() {
            return Ok(());
        }

        let batch = context_packs_to_batch(packs)?;
        let schema = Arc::new(context_pack_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.context_packs_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn list_context_packs(
        &self,
        limit: usize,
        session_id: Option<&str>,
    ) -> Result<Vec<ContextPack>, Box<dyn std::error::Error>> {
        let mut query = self.context_packs_table.query();
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            query = query.only_if(format!("session_id = '{}'", sql_escape(session_id)));
        }
        let batches = query.execute().await?.try_collect::<Vec<_>>().await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_context_packs(&batch));
        }
        results.sort_by_key(|pack| std::cmp::Reverse(pack.generated_at));
        results.truncate(limit.max(1));
        Ok(results)
    }

    pub async fn get_context_pack_by_id(
        &self,
        pack_id: &str,
    ) -> Result<Option<ContextPack>, Box<dyn std::error::Error>> {
        let filter = format!("id = '{}'", sql_escape(pack_id));
        let batches = self
            .context_packs_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(pack) = batch_to_context_packs(&batch).into_iter().next() {
                return Ok(Some(pack));
            }
        }
        Ok(None)
    }

    pub async fn append_context_deltas(
        &self,
        deltas: &[ContextDelta],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if deltas.is_empty() {
            return Ok(());
        }

        let batch = context_deltas_to_batch(deltas)?;
        let schema = Arc::new(context_delta_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.context_deltas_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn list_context_deltas(
        &self,
        limit: usize,
        session_id: Option<&str>,
    ) -> Result<Vec<ContextDelta>, Box<dyn std::error::Error>> {
        let mut query = self.context_deltas_table.query();
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            query = query.only_if(format!("session_id = '{}'", sql_escape(session_id)));
        }
        let batches = query.execute().await?.try_collect::<Vec<_>>().await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_context_deltas(&batch));
        }
        results.sort_by_key(|delta| std::cmp::Reverse(delta.generated_at));
        results.truncate(limit.max(1));
        Ok(results)
    }

    pub async fn upsert_entity_aliases(
        &self,
        aliases: &[EntityAliasRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if aliases.is_empty() {
            return Ok(());
        }

        let mut by_alias: HashMap<String, EntityAliasRecord> =
            HashMap::with_capacity(aliases.len());
        for alias in aliases {
            by_alias.insert(alias.alias_key.clone(), alias.clone());
        }
        let deduped = by_alias.into_values().collect::<Vec<_>>();
        if let Some(filter) = build_string_match_filter(
            "alias_key",
            &deduped
                .iter()
                .map(|alias| alias.alias_key.clone())
                .collect::<Vec<_>>(),
        ) {
            self.entity_aliases_table.delete(&filter).await?;
        }

        let batch = entity_aliases_to_batch(&deduped)?;
        let schema = Arc::new(entity_alias_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.entity_aliases_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn resolve_entity_alias(
        &self,
        alias_key: &str,
        project: Option<&str>,
    ) -> Result<Option<EntityAliasRecord>, Box<dyn std::error::Error>> {
        let mut filters = vec![format!("alias_key = '{}'", sql_escape(alias_key))];
        if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
            filters.push(format!("project = '{}'", sql_escape(project)));
        }
        let filter = filters.join(" AND ");
        let batches = self
            .entity_aliases_table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for batch in batches {
            if let Some(alias) = batch_to_entity_aliases(&batch).into_iter().next() {
                return Ok(Some(alias));
            }
        }
        Ok(None)
    }

    pub async fn count_activity_events(&self) -> Result<usize, Box<dyn std::error::Error>> {
        count_table_rows(&self.activity_events_table).await
    }

    pub async fn count_decision_entries(&self) -> Result<usize, Box<dyn std::error::Error>> {
        count_table_rows(&self.decision_ledger_table).await
    }

    pub async fn count_context_packs(&self) -> Result<usize, Box<dyn std::error::Error>> {
        count_table_rows(&self.context_packs_table).await
    }

    pub async fn upsert_meetings(
        &self,
        meetings: &[MeetingSession],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let batch = meeting_to_batch(meetings)?;
        let schema = Arc::new(meeting_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.meetings_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn upsert_segments(
        &self,
        segments: &[MeetingSegment],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let batch = segment_to_batch(segments)?;
        let schema = Arc::new(segment_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.segments_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn get_all_nodes(&self) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
        let batches = self
            .nodes_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for b in batches {
            results.extend(batch_to_nodes(&b));
        }
        Ok(results)
    }

    /// Fetch graph nodes of a single type, newest first.
    pub async fn get_nodes_by_type(
        &self,
        node_type: NodeType,
        limit: usize,
    ) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
        let batches = self
            .nodes_table
            .query()
            .only_if(format!("node_type = '{}'", node_type_literal(node_type)))
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for b in batches {
            results.extend(batch_to_nodes(&b));
        }
        results.sort_by_key(|node| std::cmp::Reverse(node.created_at));
        results.truncate(limit);
        Ok(results)
    }

    pub async fn get_all_edges(&self) -> Result<Vec<GraphEdge>, Box<dyn std::error::Error>> {
        let batches = self
            .edges_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for b in batches {
            results.extend(batch_to_edges(&b));
        }
        Ok(results)
    }

    pub async fn get_nodes_by_ids(
        &self,
        node_ids: &[String],
    ) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
        let filter = match build_string_match_filter("id", node_ids) {
            Some(filter) => filter,
            None => return Ok(Vec::new()),
        };
        let batches = self
            .nodes_table
            .query()
            .only_if(filter)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_nodes(&batch));
        }
        Ok(results)
    }

    pub async fn get_task_reference_edges_for_targets(
        &self,
        target_node_ids: &[String],
    ) -> Result<Vec<GraphEdge>, Box<dyn std::error::Error>> {
        let target_filter = match build_string_match_filter("target", target_node_ids) {
            Some(filter) => filter,
            None => return Ok(Vec::new()),
        };
        let filter = format!(
            "edge_type = '{}' AND ({})",
            edge_type_literal(EdgeType::ReferenceForTask),
            target_filter
        );
        let batches = self
            .edges_table
            .query()
            .only_if(filter)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut results = Vec::new();
        for batch in batches {
            results.extend(batch_to_edges(&batch));
        }
        Ok(results)
    }

    pub async fn upsert_nodes(
        &self,
        nodes: &[GraphNode],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if nodes.is_empty() {
            return Ok(());
        }

        let mut by_id: HashMap<String, GraphNode> = HashMap::with_capacity(nodes.len());
        for node in nodes {
            by_id.insert(node.id.clone(), node.clone());
        }
        let deduped = by_id.into_values().collect::<Vec<_>>();
        if let Some(filter) = build_string_match_filter(
            "id",
            &deduped
                .iter()
                .map(|node| node.id.clone())
                .collect::<Vec<_>>(),
        ) {
            self.nodes_table.delete(&filter).await?;
        }

        let batch = nodes_to_batch(&deduped)?;
        let schema = Arc::new(node_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.nodes_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn upsert_segments_full(
        &self,
        segments: &[MeetingSegment],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.segments_table.delete("id IS NOT NULL").await?;
        if segments.is_empty() {
            return Ok(());
        }
        let batch = segment_to_batch(segments)?;
        let schema = Arc::new(segment_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.segments_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn list_meetings(&self) -> Result<Vec<MeetingSession>, Box<dyn std::error::Error>> {
        let batches = self
            .meetings_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut meetings = Vec::new();
        for batch in batches {
            meetings.extend(batch_to_meetings(&batch));
        }
        Ok(meetings)
    }

    pub async fn list_segments(&self) -> Result<Vec<MeetingSegment>, Box<dyn std::error::Error>> {
        let batches = self
            .segments_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut segments = Vec::new();
        for batch in batches {
            segments.extend(batch_to_segments(&batch));
        }
        Ok(segments)
    }

    pub async fn upsert_edges(
        &self,
        edges: &[GraphEdge],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if edges.is_empty() {
            return Ok(());
        }

        let mut by_rel: HashMap<(String, String, &'static str), GraphEdge> =
            HashMap::with_capacity(edges.len());
        for edge in edges {
            by_rel.insert(
                (
                    edge.source.clone(),
                    edge.target.clone(),
                    edge_type_literal(edge.edge_type),
                ),
                edge.clone(),
            );
        }
        let deduped = by_rel.into_values().collect::<Vec<_>>();

        for chunk in deduped.chunks(128) {
            if let Some(filter) = build_edge_identity_filter(chunk) {
                self.edges_table.delete(&filter).await?;
            }
        }

        let batch = edges_to_batch(&deduped)?;
        let schema = Arc::new(edge_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.edges_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    /// Return the data directory (sync — no DB access).
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    /// Insert a batch of records into LanceDB.
    pub async fn add_batch(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.add_batch_and_get_count(records).await?;
        Ok(())
    }

    /// Add a batch of records and return the count of records actually stored.
    /// This accounts for records filtered out due to low signal or deduplication.
    pub async fn add_batch_and_get_count(
        &self,
        records: &[MemoryRecord],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(0);
        }
        let incoming_count = records.len();
        let normalized = records
            .iter()
            .map(normalize_record_for_index)
            .filter(is_indexable_memory_record)
            .collect::<Vec<_>>();
        let compacted = dedup_records_for_insert(&normalized);
        let skipped_count = incoming_count.saturating_sub(normalized.len());
        let deduped_count = normalized.len().saturating_sub(compacted.len());
        let inserted_count = compacted.len();
        tracing::info!(
            incoming_count,
            inserted_count,
            skipped_count,
            deduped_count,
            "lancedb:add_batch"
        );
        if compacted.is_empty() {
            return Ok(0);
        }
        self.insert_memory_batch(&compacted).await?;
        Ok(inserted_count)
    }

    /// Product-named wrapper for writing one memory chunk to the stable index.
    pub async fn insert_memory_chunk(
        &self,
        record: &MemoryRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.add_batch(std::slice::from_ref(record)).await
    }

    /// Insert a batch without content-based deduping, preserving caller-provided ids.
    pub async fn add_batch_preserving_ids(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(());
        }
        let normalized = records
            .iter()
            .map(normalize_record_for_index)
            .collect::<Vec<_>>();
        self.insert_memory_batch(&normalized).await
    }

    /// Replace a single memory row in the v4 parent table, preserving its id
    /// and any linked memory chunks. Used by the memory_review worker after
    /// validation succeeds; callers must not pass partial records — the full
    /// `MemoryRecord` is required because the underlying table replace pattern
    /// is delete-then-insert.
    pub async fn replace_memory_preserving_chunks(
        &self,
        record: &MemoryRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if record.id.trim().is_empty() {
            return Err("Refusing to replace a memory with empty id".into());
        }
        let id = sql_escape(&record.id);
        self.table.delete(&format!("id = '{id}'")).await?;
        // Note: deliberately not calling delete_chunks_for_memory — children
        // outlive a parent review pass.
        let normalized = normalize_record_for_index(record);
        self.insert_memory_batch(&[normalized]).await
    }

    pub async fn add_v5_batch_preserving_ids(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(());
        }
        validate_v5_record_vectors(records)?;
        let batch = records_to_batch_with_text_dim(records, BGE_V5_DIMENSIONS as i32)?;
        let schema = Arc::new(memory_v5_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.memories_v5_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn upsert_memory_chunks(
        &self,
        chunks: &[MemoryChunkRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if chunks.is_empty() {
            return Ok(());
        }
        validate_memory_chunk_vectors(chunks)?;

        let ids = chunks
            .iter()
            .map(|chunk| chunk.id.clone())
            .collect::<Vec<_>>();
        for id_chunk in ids.chunks(128) {
            if let Some(filter) = build_string_match_filter("id", id_chunk) {
                self.memory_chunks_table.delete(&filter).await?;
            }
        }

        let batch = memory_chunks_to_batch(chunks, BGE_V5_DIMENSIONS as i32)?;
        let schema = Arc::new(memory_chunk_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.memory_chunks_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn delete_chunks_for_memory(
        &self,
        memory_id: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let before = self.memory_chunks_table.count_rows(None).await?;
        let escaped = sql_escape(memory_id);
        self.memory_chunks_table
            .delete(&format!("memory_id = '{escaped}'"))
            .await?;
        let after = self.memory_chunks_table.count_rows(None).await?;
        Ok(before.saturating_sub(after))
    }

    pub async fn list_chunks_for_memory(
        &self,
        memory_id: &str,
    ) -> Result<Vec<MemoryChunkRecord>, Box<dyn std::error::Error>> {
        let escaped = sql_escape(memory_id);
        let batches: Vec<RecordBatch> = self
            .memory_chunks_table
            .query()
            .only_if(format!("memory_id = '{escaped}'"))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut chunks = Vec::new();
        for batch in &batches {
            chunks.extend(batch_to_memory_chunks(batch));
        }
        chunks.sort_by_key(|chunk| chunk.chunk_index);
        Ok(chunks)
    }

    pub async fn chunk_vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryChunkSearchResult>, Box<dyn std::error::Error>> {
        if query_embedding.len() != BGE_V5_DIMENSIONS {
            return Err(format!(
                "Refusing chunk vector search on {MEMORY_CHUNKS_TABLE}: query is {}-d, expected {}-d BGE. No fallback across embedding dimensions is allowed.",
                query_embedding.len(),
                BGE_V5_DIMENSIONS
            )
            .into());
        }
        let batches: Vec<RecordBatch> = self
            .memory_chunks_table
            .vector_search(query_embedding.to_vec())?
            .column("embedding")
            .limit(limit.max(1))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut chunks = Vec::new();
        for batch in &batches {
            chunks.extend(batch_to_memory_chunk_search_results(batch));
        }
        Ok(chunks)
    }

    pub async fn has_chunk_retrieval_index(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let parent_count = self.memories_v5_table.count_rows(None).await?;
        if parent_count == 0 {
            return Ok(false);
        }
        let chunk_count = self.memory_chunks_table.count_rows(None).await?;
        Ok(chunk_count > 0)
    }

    pub async fn get_v5_search_results_by_ids(
        &self,
        memory_ids: &[String],
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let ids = memory_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let Some(id_filter) = build_string_match_filter("id", &ids) else {
            return Ok(Vec::new());
        };
        let filter = match build_filter(time_filter, app_filter) {
            Some(extra) => format!("({id_filter}) AND ({extra})"),
            None => id_filter,
        };
        let batches: Vec<RecordBatch> = self
            .memories_v5_table
            .query()
            .select(Select::columns(SEARCH_RESULT_COLUMNS))
            .only_if(filter)
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut results = Vec::new();
        for batch in &batches {
            results.extend(batch_to_search_results(batch));
        }
        Ok(results)
    }

    pub async fn list_v5_reindex_identities(
        &self,
    ) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self
            .memories_v5_table
            .query()
            .select(Select::Columns(vec![
                "id".to_string(),
                "content_hash".to_string(),
                "dedup_fingerprint".to_string(),
            ]))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut identities = HashSet::new();
        for batch in &batches {
            let ids = str_col(batch, "id");
            let content_hashes = str_col(batch, "content_hash");
            let dedup_fingerprints = str_col(batch, "dedup_fingerprint");
            for i in 0..batch.num_rows() {
                let content_hash = content_hashes
                    .as_ref()
                    .filter(|col| !col.is_null(i))
                    .map(|col| col.value(i))
                    .unwrap_or_default();
                if !content_hash.trim().is_empty() {
                    identities.insert(format!("content:{}", content_hash.trim()));
                    continue;
                }
                let dedup = dedup_fingerprints
                    .as_ref()
                    .filter(|col| !col.is_null(i))
                    .map(|col| col.value(i))
                    .unwrap_or_default();
                if !dedup.trim().is_empty() {
                    identities.insert(format!("dedup:{}", dedup.trim()));
                    continue;
                }
                let id = ids
                    .as_ref()
                    .filter(|col| !col.is_null(i))
                    .map(|col| col.value(i))
                    .unwrap_or_default();
                if !id.trim().is_empty() {
                    identities.insert(format!("id:{}", id.trim()));
                }
            }
        }
        Ok(identities)
    }

    /// Replace the entire memories table in one write, preserving caller ids.
    pub async fn replace_all_memories_preserving_ids(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), String> {
        if records.is_empty() {
            self.delete_all().await.map_err(|e| e.to_string())?;
            return Ok(());
        }

        let normalized = records
            .iter()
            .map(normalize_record_for_index)
            .collect::<Vec<_>>();
        let batch = records_to_batch(&normalized).map_err(|e| e.to_string())?;
        let schema = Arc::new(memory_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Approximate nearest-neighbour search over `embedding` column.
    pub async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let filter = build_filter(time_filter, app_filter);
        let query_vec: Vec<f32> = query_embedding.to_vec();
        let base_limit = limit.max(1);
        let retrieval_limit = if base_limit >= 300 {
            base_limit
        } else {
            base_limit.saturating_mul(VECTOR_QUERY_MULTIPLIER).min(300)
        };

        let mut vq = self
            .table
            .vector_search(query_vec)?
            .column("embedding")
            .limit(retrieval_limit);

        if let Some(f) = filter {
            vq = vq.only_if(f);
        }

        let batches: Vec<RecordBatch> = vq.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            results.extend(batch_to_search_results(batch));
        }
        Ok(dedup_search_results(results, limit))
    }

    /// ANN search over the `snippet_embedding` column (second semantic tower).
    pub async fn snippet_vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let filter = build_filter(time_filter, app_filter);
        let query_vec: Vec<f32> = query_embedding.to_vec();
        let base_limit = limit.max(1);
        let retrieval_limit = if base_limit >= 300 {
            base_limit
        } else {
            base_limit.saturating_mul(VECTOR_QUERY_MULTIPLIER).min(300)
        };

        let mut vq = self
            .table
            .vector_search(query_vec)?
            .column("snippet_embedding")
            .limit(retrieval_limit);

        if let Some(f) = filter {
            vq = vq.only_if(f);
        }

        let batches: Vec<RecordBatch> = vq.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            results.extend(batch_to_search_results(batch));
        }
        Ok(dedup_search_results(results, limit))
    }

    /// ANN search over the `image_embedding` column (CLIP 512-d vision tower).
    ///
    /// Used by the image-to-image retrieval surface; cross-modal text->image
    /// queries are intentionally out of scope here (would need a CLIP text
    /// tower and a separate privacy review per ADR-004).
    pub async fn image_vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let filter = build_filter(time_filter, app_filter);
        let query_vec: Vec<f32> = query_embedding.to_vec();
        let base_limit = limit.max(1);
        let retrieval_limit = if base_limit >= 300 {
            base_limit
        } else {
            base_limit.saturating_mul(VECTOR_QUERY_MULTIPLIER).min(300)
        };

        let mut vq = self
            .table
            .vector_search(query_vec)?
            .column("image_embedding")
            .limit(retrieval_limit);

        if let Some(f) = filter {
            vq = vq.only_if(f);
        }

        let batches: Vec<RecordBatch> = vq.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            results.extend(batch_to_search_results(batch));
        }
        Ok(dedup_search_results(results, limit))
    }

    /// Image-to-image similarity from a seed memory id.
    ///
    /// Returns an empty Vec when the seed is missing or carries the legacy
    /// zero image vector (older captures that predated CLIP wiring, or rows
    /// where CLIP failed at capture time). The seed itself is filtered out
    /// of the result list so callers always get neighbors only.
    pub async fn similar_by_image_embedding(
        &self,
        seed_memory_id: &str,
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let Some(seed) = self.get_memory_by_id(seed_memory_id).await? else {
            return Ok(Vec::new());
        };
        if seed.image_embedding.iter().all(|v| *v == 0.0) {
            return Ok(Vec::new());
        }
        let mut hits = self
            .image_vector_search(
                &seed.image_embedding,
                limit.saturating_add(1),
                time_filter,
                app_filter,
            )
            .await?;
        hits.retain(|r| r.id != seed_memory_id);
        hits.truncate(limit);
        Ok(hits)
    }

    async fn insert_memory_batch(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(());
        }
        let batch = records_to_batch(records)?;
        let schema = Arc::new(memory_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await?;
        Ok(())
    }

    /// Batch-apply Ebbinghaus decay scores. `updates` is a vec of (id, new_decay_score).
    pub async fn apply_decay_batch(
        &self,
        updates: &[(String, f32)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (id, new_decay) in updates {
            let escaped_id = sql_escape(id);
            self.table
                .update()
                .only_if(format!("id = '{escaped_id}'"))
                .column("decay_score", format!("{new_decay}"))
                .execute()
                .await?;
        }
        Ok(())
    }

    /// Touch accessed records: reset decay to 1.0 and update last_accessed_at.
    pub async fn touch_accessed(&self, ids: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        for id in ids {
            let escaped_id = sql_escape(id);
            self.table
                .update()
                .only_if(format!("id = '{escaped_id}'"))
                .column("decay_score", "1.0".to_string())
                .column("last_accessed_at", format!("{now_ms}"))
                .execute()
                .await?;
        }
        Ok(())
    }

    /// Retroactively delete all memories whose URL or window title matches the blocklist domain
    pub async fn delete_memories_by_domain(
        &self,
        domain: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let escaped = sql_escape(&domain.to_lowercase());
        let filter = format!(
            "LOWER(window_title) LIKE '%{}%' OR LOWER(url) LIKE '%{}%'",
            escaped, escaped
        );
        let ids = self.memory_ids_for_filter(&filter).await?;
        self.table.delete(&filter).await?;
        for id in ids {
            self.delete_chunks_for_memory(&id).await?;
        }
        Ok(())
    }

    /// Return the path to the frames directory (for screenshot eviction).
    pub fn frames_dir(&self) -> PathBuf {
        self.data_dir.join("frames")
    }

    /// Return all memory records whose timestamp falls within [start_ms, end_ms].
    pub async fn get_memories_in_range(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<MemoryRecord>, Box<dyn std::error::Error>> {
        let filter = format!("timestamp >= {start_ms} AND timestamp <= {end_ms}");
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(filter)
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut records = Vec::new();
        for batch in &batches {
            records.extend(batch_to_memory_records(batch));
        }
        Ok(records)
    }

    /// Return lightweight search-style rows whose timestamp falls within [start_ms, end_ms].
    pub async fn get_search_results_in_range(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let filter = format!("timestamp >= {start_ms} AND timestamp <= {end_ms}");
        let mut results = self.query_search_results(Some(filter)).await?;
        results.sort_by_key(|result| result.timestamp);
        Ok(results)
    }

    /// Full-scan keyword search using SQL LIKE predicates.
    pub async fn keyword_search(
        &self,
        query: &str,
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let terms = keyword_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let base_limit = limit.max(1);
        let retrieval_limit = if base_limit >= MAX_KEYWORD_SCAN {
            base_limit
        } else {
            base_limit
                .saturating_mul(KEYWORD_QUERY_MULTIPLIER)
                .min(MAX_KEYWORD_SCAN)
        };
        let mut results = Vec::new();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let per_term_limit = (retrieval_limit / terms.len().max(1))
            .max(base_limit)
            .min(retrieval_limit);

        for term in &terms {
            let escaped = sql_escape(&term.to_lowercase());
            let term_clauses = [
                format!("LOWER(text) LIKE '%{escaped}%'"),
                format!("LOWER(clean_text) LIKE '%{escaped}%'"),
                format!("LOWER(snippet) LIKE '%{escaped}%'"),
                format!("LOWER(lexical_shadow) LIKE '%{escaped}%'"),
                format!("LOWER(window_title) LIKE '%{escaped}%'"),
                format!("LOWER(app_name) LIKE '%{escaped}%'"),
                format!("LOWER(url) LIKE '%{escaped}%'"),
            ];
            let keyword_pred = format!("({})", term_clauses.join(" OR "));
            let filter = match build_filter(time_filter, app_filter) {
                Some(f) => format!("{keyword_pred} AND {f}"),
                None => keyword_pred,
            };

            let batches: Vec<RecordBatch> = self
                .table
                .query()
                .only_if(filter)
                .limit(per_term_limit)
                .execute()
                .await?
                .try_collect()
                .await?;

            for batch in &batches {
                let mut batch_results = batch_to_search_results(batch);
                // Keyword branch gets a lexical relevance score before hybrid fusion.
                for r in &mut batch_results {
                    let lexical = lexical_keyword_score(&terms, r);
                    let recency = recency_score(now_ms, r.timestamp);
                    r.score = (lexical * 0.86 + recency * 0.14).clamp(0.0, 1.0);
                }
                results.extend(batch_results);
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });
        Ok(dedup_search_results(results, limit))
    }

    /// Returns whether at least one memory row exists.
    pub async fn has_memories(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .select(Select::columns(&["id"]))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;
        Ok(batches.iter().any(|batch| batch.num_rows() > 0))
    }

    /// All memory row ids currently in the table (for task ↔ memory integrity).
    pub async fn list_memory_ids(&self) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .select(Select::columns(&["id"]))
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut ids = HashSet::new();
        for batch in &batches {
            let Some(col) = batch
                .column_by_name("id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            else {
                continue;
            };
            for i in 0..batch.num_rows() {
                let value = col.value(i);
                if !value.is_empty() {
                    ids.insert(value.to_string());
                }
            }
        }
        Ok(ids)
    }

    /// Return comprehensive statistics and usage insights about stored data.
    pub async fn get_stats(&self) -> Result<Stats, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self.table.query().execute().await?.try_collect().await?;

        let total_records: usize = batches.iter().map(|b| b.num_rows()).sum();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let one_hour_ago = now_ms - chrono::Duration::hours(1).num_milliseconds();
        let one_day_ago = now_ms - chrono::Duration::hours(24).num_milliseconds();
        let seven_days_ago = now_ms - chrono::Duration::days(7).num_milliseconds();
        let today = local_day_bucket_now();
        let mut days = std::collections::HashSet::new();
        let mut app_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut day_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut domain_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut unique_apps = std::collections::HashSet::new();
        let mut unique_sessions = std::collections::HashSet::new();
        let mut unique_window_titles = std::collections::HashSet::new();
        let mut unique_urls = std::collections::HashSet::new();
        let mut unique_domains = std::collections::HashSet::new();
        let mut hourly_counts = [0usize; 24];
        let mut weekday_counts = [0usize; 7];
        let mut daypart_counts = [0usize; 4]; // Night, Morning, Afternoon, Evening
        let mut first_capture_ts: Option<i64> = None;
        let mut last_capture_ts: Option<i64> = None;
        let mut records_with_url: usize = 0;
        let mut records_with_screenshot: usize = 0;
        let mut records_with_clean_text: usize = 0;
        let mut records_last_hour: usize = 0;
        let mut records_last_24h: usize = 0;
        let mut records_last_7d: usize = 0;
        let mut llm_count: usize = 0;
        let mut vlm_count: usize = 0;
        let mut fallback_count: usize = 0;
        let mut other_summary_count: usize = 0;
        let mut ocr_confidence_sum = 0.0_f64;
        let mut noise_score_sum = 0.0_f64;
        let mut ocr_block_sum = 0.0_f64;
        let mut low_confidence_records: usize = 0;
        let mut high_noise_records: usize = 0;
        let mut timeline_points: Vec<(i64, String)> = Vec::with_capacity(total_records);
        let mut today_count: usize = 0;

        for batch in &batches {
            let timestamp_col = batch
                .column_by_name("timestamp")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>().cloned());
            let app_col = batch
                .column_by_name("app_name")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let session_key_col = batch
                .column_by_name("session_key")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let session_id_col = batch
                .column_by_name("session_id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let title_col = batch
                .column_by_name("window_title")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let url_col = batch
                .column_by_name("url")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let screenshot_col = batch
                .column_by_name("screenshot_path")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let clean_text_col = batch
                .column_by_name("clean_text")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let summary_source_col = batch
                .column_by_name("summary_source")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());
            let ocr_confidence_col = batch
                .column_by_name("ocr_confidence")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>().cloned());
            let noise_score_col = batch
                .column_by_name("noise_score")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>().cloned());
            let ocr_block_col = batch
                .column_by_name("ocr_block_count")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>().cloned());

            for i in 0..batch.num_rows() {
                let timestamp = timestamp_col.as_ref().map(|c| c.value(i)).unwrap_or(0);

                if timestamp >= one_hour_ago {
                    records_last_hour += 1;
                }
                if timestamp >= one_day_ago {
                    records_last_24h += 1;
                }
                if timestamp >= seven_days_ago {
                    records_last_7d += 1;
                }

                first_capture_ts = Some(first_capture_ts.map_or(timestamp, |v| v.min(timestamp)));
                last_capture_ts = Some(last_capture_ts.map_or(timestamp, |v| v.max(timestamp)));

                let day = local_day_bucket_from_timestamp(timestamp);
                if day == today {
                    today_count += 1;
                }
                days.insert(day.clone());
                *day_counts.entry(day).or_insert(0) += 1;

                if let Some(dt) = Local.timestamp_millis_opt(timestamp).single() {
                    let hour_idx = dt.hour() as usize;
                    hourly_counts[hour_idx] += 1;
                    weekday_counts[dt.weekday().num_days_from_monday() as usize] += 1;

                    let daypart_idx = match dt.hour() {
                        4..=11 => 1,
                        12..=15 => 2,
                        16..=19 => 3,
                        _ => 0,
                    };
                    daypart_counts[daypart_idx] += 1;
                }

                let app_name =
                    get_non_empty_str(&app_col, i).unwrap_or_else(|| "Unknown".to_string());
                *app_counts.entry(app_name.clone()).or_insert(0) += 1;
                unique_apps.insert(app_name.clone());
                timeline_points.push((timestamp, app_name));

                if let Some(title) = get_non_empty_str(&title_col, i) {
                    unique_window_titles.insert(title);
                }

                let session = get_non_empty_str(&session_key_col, i)
                    .or_else(|| get_non_empty_str(&session_id_col, i));
                if let Some(session_id) = session {
                    unique_sessions.insert(session_id);
                }

                if let Some(url) = get_non_empty_str(&url_col, i) {
                    records_with_url += 1;
                    unique_urls.insert(url.clone());
                    if let Some(domain) = extract_domain(&url) {
                        unique_domains.insert(domain.clone());
                        *domain_counts.entry(domain).or_insert(0) += 1;
                    }
                }

                if get_non_empty_str(&screenshot_col, i).is_some() {
                    records_with_screenshot += 1;
                }
                if get_non_empty_str(&clean_text_col, i).is_some() {
                    records_with_clean_text += 1;
                }

                let source = get_non_empty_str(&summary_source_col, i)
                    .unwrap_or_else(|| "fallback".to_string())
                    .to_ascii_lowercase();
                match source.as_str() {
                    "llm" => llm_count += 1,
                    "vlm" => vlm_count += 1,
                    "fallback" => fallback_count += 1,
                    _ => other_summary_count += 1,
                }

                let confidence = ocr_confidence_col
                    .as_ref()
                    .map(|c| c.value(i) as f64)
                    .unwrap_or(0.0);
                ocr_confidence_sum += confidence;
                if confidence > 0.0 && confidence < 0.55 {
                    low_confidence_records += 1;
                }

                let noise = noise_score_col
                    .as_ref()
                    .map(|c| c.value(i) as f64)
                    .unwrap_or(0.0);
                noise_score_sum += noise;
                if noise >= 0.40 {
                    high_noise_records += 1;
                }

                let ocr_blocks = ocr_block_col
                    .as_ref()
                    .map(|c| c.value(i).max(0) as f64)
                    .unwrap_or(0.0);
                ocr_block_sum += ocr_blocks;
            }
        }

        let mut apps: Vec<AppCount> = app_counts
            .into_iter()
            .map(|(name, count)| AppCount { name, count })
            .collect();
        apps.sort_by(|a, b| b.count.cmp(&a.count));
        let focus_app_share_pct = if total_records > 0 {
            apps.first()
                .map(|a| (a.count as f64 / total_records as f64) * 100.0)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        apps.truncate(10);

        let mut top_domains: Vec<DomainCount> = domain_counts
            .into_iter()
            .map(|(domain, count)| DomainCount { domain, count })
            .collect();
        top_domains.sort_by(|a, b| b.count.cmp(&a.count));
        top_domains.truncate(10);

        let busiest_day = day_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(day, count)| DayCount {
                day: day.clone(),
                count: *count,
            });

        let quietest_day = day_counts
            .iter()
            .min_by_key(|(_, count)| *count)
            .map(|(day, count)| DayCount {
                day: day.clone(),
                count: *count,
            });

        let hourly_distribution: Vec<HourCount> = hourly_counts
            .iter()
            .enumerate()
            .map(|(hour, count)| HourCount {
                hour: hour as u8,
                count: *count,
            })
            .collect();

        let busiest_hour = hourly_counts
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .and_then(|(hour, count)| {
                if *count == 0 {
                    None
                } else {
                    Some(HourCount {
                        hour: hour as u8,
                        count: *count,
                    })
                }
            });

        let weekday_labels = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        let weekday_distribution: Vec<WeekdayCount> = weekday_counts
            .iter()
            .enumerate()
            .map(|(idx, count)| WeekdayCount {
                weekday: weekday_labels[idx].to_string(),
                count: *count,
            })
            .collect();

        let daypart_labels = ["Night", "Morning", "Afternoon", "Evening"];
        let daypart_distribution: Vec<DaypartCount> = daypart_counts
            .iter()
            .enumerate()
            .map(|(idx, count)| DaypartCount {
                daypart: daypart_labels[idx].to_string(),
                count: *count,
            })
            .collect();

        timeline_points.sort_by_key(|(timestamp, _)| *timestamp);

        let mut app_switches = 0usize;
        let mut total_gap_ms = 0_i64;
        let mut gap_count = 0usize;
        let mut longest_gap_ms = 0_i64;

        for pair in timeline_points.windows(2) {
            let (prev_ts, prev_app) = (&pair[0].0, &pair[0].1);
            let (next_ts, next_app) = (&pair[1].0, &pair[1].1);

            if prev_app != next_app {
                app_switches += 1;
            }

            let gap = (*next_ts - *prev_ts).max(0);
            if gap > 0 {
                total_gap_ms += gap;
                gap_count += 1;
                longest_gap_ms = longest_gap_ms.max(gap);
            }
        }

        let capture_span_hours = match (first_capture_ts, last_capture_ts) {
            (Some(first), Some(last)) if last >= first => (last - first) as f64 / 3_600_000.0,
            _ => 0.0,
        };

        let avg_gap_minutes = if gap_count > 0 {
            total_gap_ms as f64 / gap_count as f64 / 60_000.0
        } else {
            0.0
        };

        let app_switch_rate_per_hour = if capture_span_hours > 0.0 {
            app_switches as f64 / capture_span_hours
        } else {
            0.0
        };

        let avg_records_per_active_day = if !days.is_empty() {
            total_records as f64 / days.len() as f64
        } else {
            0.0
        };

        let avg_records_per_hour = if capture_span_hours > 0.0 {
            total_records as f64 / capture_span_hours
        } else {
            0.0
        };

        let avg_ocr_confidence = if total_records > 0 {
            ocr_confidence_sum / total_records as f64
        } else {
            0.0
        };

        let avg_noise_score = if total_records > 0 {
            noise_score_sum / total_records as f64
        } else {
            0.0
        };

        let avg_ocr_blocks = if total_records > 0 {
            ocr_block_sum / total_records as f64
        } else {
            0.0
        };

        let (current_streak_days, longest_streak_days) = compute_activity_streaks(&day_counts);

        Ok(Stats {
            total_records,
            total_days: days.len(),
            apps,
            today_count,
            unique_apps: unique_apps.len(),
            unique_sessions: unique_sessions.len(),
            unique_window_titles: unique_window_titles.len(),
            unique_urls: unique_urls.len(),
            unique_domains: unique_domains.len(),
            records_with_url,
            records_with_screenshot,
            records_with_clean_text,
            records_last_hour,
            records_last_24h,
            records_last_7d,
            avg_records_per_active_day,
            avg_records_per_hour,
            focus_app_share_pct,
            app_switches,
            app_switch_rate_per_hour,
            avg_gap_minutes,
            longest_gap_minutes: (longest_gap_ms / 60_000).max(0) as u64,
            first_capture_ts,
            last_capture_ts,
            capture_span_hours,
            current_streak_days,
            longest_streak_days,
            avg_ocr_confidence,
            low_confidence_records,
            avg_noise_score,
            high_noise_records,
            avg_ocr_blocks,
            llm_count,
            vlm_count,
            fallback_count,
            other_summary_count,
            top_domains,
            busiest_day,
            quietest_day,
            busiest_hour,
            hourly_distribution,
            weekday_distribution,
            daypart_distribution,
        })
    }

    /// Delete all records.
    pub async fn delete_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.table.delete("id IS NOT NULL").await?;
        self.memory_chunks_table
            .delete("memory_id IS NOT NULL")
            .await?;
        Ok(())
    }

    /// Return sorted list of unique app names.
    pub async fn get_app_names(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self.table.query().execute().await?.try_collect().await?;

        let mut names = std::collections::HashSet::new();
        for batch in &batches {
            let app_col = batch
                .column_by_name("app_name")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>().cloned());

            if let Some(col) = app_col {
                for i in 0..batch.num_rows() {
                    let name = col.value(i);
                    if !name.is_empty() {
                        names.insert(name.to_string());
                    }
                }
            }
        }
        let mut list: Vec<String> = names.into_iter().collect();
        list.sort();
        Ok(list)
    }

    /// Delete records older than `days` days; returns count of deleted rows.
    pub async fn delete_older_than(&self, days: u32) -> Result<usize, Box<dyn std::error::Error>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let cutoff_ms = cutoff.timestamp_millis();
        let filter = format!("timestamp < {cutoff_ms}");
        let ids = self.memory_ids_for_filter(&filter).await?;

        // Count before deletion.
        let before = self.table.count_rows(None).await?;
        self.table.delete(&filter).await?;
        for id in ids {
            self.delete_chunks_for_memory(&id).await?;
        }
        let after = self.table.count_rows(None).await?;

        Ok(before.saturating_sub(after))
    }

    /// Delete a specific memory row by exact id.
    pub async fn delete_memory_by_id(
        &self,
        memory_id: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let before = self.table.count_rows(None).await?;
        let id = sql_escape(memory_id);
        self.table.delete(&format!("id = '{id}'")).await?;
        self.delete_chunks_for_memory(memory_id).await?;
        let after = self.table.count_rows(None).await?;
        Ok(before.saturating_sub(after))
    }

    /// Return recent memory records (last `hours` hours).
    pub async fn get_recent_memories(
        &self,
        hours: u32,
    ) -> Result<Vec<MemoryRecord>, Box<dyn std::error::Error>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
        let cutoff_ms = cutoff.timestamp_millis();

        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(format!("timestamp >= {cutoff_ms}"))
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut records = Vec::new();
        for batch in &batches {
            records.extend(batch_to_memory_records(batch));
        }
        Ok(records)
    }

    /// Return all stored memory records ordered oldest -> newest.
    pub async fn list_all_memories(&self) -> Result<Vec<MemoryRecord>, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self.table.query().execute().await?.try_collect().await?;

        let mut records = Vec::new();
        for batch in &batches {
            records.extend(batch_to_memory_records(batch));
        }
        records.sort_by_key(|record| record.timestamp);
        Ok(records)
    }

    /// Fetch a single record by id.
    pub async fn get_memory_by_id(
        &self,
        memory_id: &str,
    ) -> Result<Option<MemoryRecord>, Box<dyn std::error::Error>> {
        let id = sql_escape(memory_id);
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(format!("id = '{id}'"))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;

        for batch in &batches {
            let records = batch_to_memory_records(batch);
            if let Some(r) = records.into_iter().next() {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    /// Fetch the up-to-`limit` newest memories that share `session_id` or
    /// `project` with the given record id. Used by capture-time durable
    /// `memory_context` synthesis to chain a new card to its predecessors
    /// without storing extra columns.
    pub async fn list_recent_by_session_or_project(
        &self,
        session_id: &str,
        project: &str,
        exclude_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        if session_id.trim().is_empty() && project.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut clauses = Vec::new();
        if !session_id.trim().is_empty() {
            clauses.push(format!("session_id = '{}'", sql_escape(session_id)));
        }
        if !project.trim().is_empty() {
            clauses.push(format!("project = '{}'", sql_escape(project)));
        }
        let mut filter = clauses.join(" OR ");
        if let Some(exclude) = exclude_id.filter(|s| !s.trim().is_empty()) {
            filter = format!("({}) AND id <> '{}'", filter, sql_escape(exclude));
        }

        let mut query = self
            .table
            .query()
            .select(Select::columns(SEARCH_RESULT_COLUMNS));
        query = query.only_if(filter);

        let batches: Vec<RecordBatch> = query.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            let mut batch_results = batch_to_search_results(batch);
            for result in &mut batch_results {
                result.score = 1.0;
            }
            results.extend(batch_results);
        }
        results.sort_by_key(|result| std::cmp::Reverse(result.timestamp));
        results.truncate(limit.max(1));
        Ok(results)
    }

    /// Walk the parent_id chain backward from `memory_id` up to `max_depth`
    /// ancestors. Useful for rendering a memory's full timeline thread.
    pub async fn get_ancestor_chain(
        &self,
        memory_id: &str,
        max_depth: usize,
    ) -> Result<Vec<MemoryRecord>, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        let mut current_id = memory_id.to_string();
        let mut seen = std::collections::HashSet::new();
        seen.insert(current_id.clone());

        for _ in 0..max_depth {
            let Some(record) = self.get_memory_by_id(&current_id).await? else {
                break;
            };
            let Some(parent_id) = record.parent_id.clone() else {
                break;
            };
            if parent_id.trim().is_empty() || !seen.insert(parent_id.clone()) {
                break;
            }
            let Some(parent) = self.get_memory_by_id(&parent_id).await? else {
                break;
            };
            out.push(parent);
            current_id = parent_id;
        }
        Ok(out)
    }

    /// Direct children of `memory_id` (records whose parent_id == memory_id).
    pub async fn get_children(
        &self,
        memory_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, Box<dyn std::error::Error>> {
        if memory_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        let filter = format!("parent_id = '{}'", sql_escape(memory_id));
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(filter)
            .limit(limit.max(1))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut records = Vec::new();
        for batch in &batches {
            records.extend(batch_to_memory_records(batch));
        }
        records.sort_by_key(|r| r.timestamp);
        Ok(records)
    }

    /// List newest memories as raw search-style rows (optionally filtered by app).
    pub async fn list_recent_results(
        &self,
        limit: usize,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let base_limit = limit.max(1);
        // Lance's plain table query does not guarantee timestamp ordering, so
        // we need to sort after scanning matching rows rather than limit first.
        let mut query = self
            .table
            .query()
            .select(Select::columns(SEARCH_RESULT_COLUMNS));
        if let Some(filter) = build_filter(None, app_filter) {
            query = query.only_if(filter);
        }

        let batches: Vec<RecordBatch> = query.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            let mut batch_results = batch_to_search_results(batch);
            for result in &mut batch_results {
                result.score = 1.0;
            }
            results.extend(batch_results);
        }
        results.sort_by_key(|result| std::cmp::Reverse(result.timestamp));
        results.truncate(base_limit);
        Ok(results)
    }

    async fn query_search_results(
        &self,
        filter: Option<String>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let mut query = self
            .table
            .query()
            .select(Select::columns(SEARCH_RESULT_COLUMNS));
        if let Some(filter) = filter {
            query = query.only_if(filter);
        }

        let batches: Vec<RecordBatch> = query.execute().await?.try_collect().await?;
        let mut results = Vec::new();
        for batch in &batches {
            let mut batch_results = batch_to_search_results(batch);
            for result in &mut batch_results {
                result.score = 1.0;
            }
            results.extend(batch_results);
        }
        Ok(results)
    }

    async fn memory_ids_for_filter(
        &self,
        filter: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .select(Select::Columns(vec!["id".to_string()]))
            .only_if(filter)
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut ids = Vec::new();
        for batch in &batches {
            let id_col = str_col(batch, "id");
            for row in 0..batch.num_rows() {
                let id = get_str(&id_col, row);
                if !id.is_empty() {
                    ids.push(id);
                }
            }
        }
        Ok(ids)
    }
}

fn validate_v5_record_vectors(records: &[MemoryRecord]) -> Result<(), Box<dyn std::error::Error>> {
    for record in records {
        for (name, vector) in [
            ("embedding", &record.embedding),
            ("snippet_embedding", &record.snippet_embedding),
            ("support_embedding", &record.support_embedding),
        ] {
            if vector.len() != BGE_V5_DIMENSIONS {
                return Err(format!(
                    "Refusing to write memory {} to {}: {} is {}-d, expected {}-d BGE. No fallback across embedding dimensions is allowed.",
                    record.id,
                    MEMORIES_V5_PARENT_TABLE,
                    name,
                    vector.len(),
                    BGE_V5_DIMENSIONS
                )
                .into());
            }
        }
    }
    Ok(())
}

fn validate_memory_chunk_vectors(
    chunks: &[MemoryChunkRecord],
) -> Result<(), Box<dyn std::error::Error>> {
    for chunk in chunks {
        if chunk.embedding.len() != BGE_V5_DIMENSIONS {
            return Err(format!(
                "Refusing to write chunk {} to {}: embedding is {}-d, expected {}-d BGE. No fallback across embedding dimensions is allowed.",
                chunk.id,
                MEMORY_CHUNKS_TABLE,
                chunk.embedding.len(),
                BGE_V5_DIMENSIONS
            )
            .into());
        }
        if chunk.memory_id.trim().is_empty() {
            return Err(format!("Refusing to write chunk {} without memory_id", chunk.id).into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

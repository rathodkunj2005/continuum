//! LanceDB-backed storage for FNDR memory records.
//!
//! Replaces the JSON-based simple_store with a proper vector database.
//! All methods that touch LanceDB are async.

use super::schema::{
    ActivityEvent, AppCount, ContextDelta, ContextPack, DayCount, DaypartCount,
    DecisionLedgerEntry, DomainCount, EdgeType, EntityAliasRecord, GraphEdge, GraphNode, HourCount,
    KnowledgePage, MeetingSegment, MeetingSession, MemoryRecord, NodeType, ProjectContext,
    SearchResult, Stats, Task, TaskType, WeekdayCount,
};
use crate::config::{
    DEFAULT_IMAGE_EMBEDDING_DIM, DEFAULT_STORE_KEYWORD_QUERY_MULTIPLIER,
    DEFAULT_STORE_MAX_KEYWORD_SCAN, DEFAULT_STORE_VECTOR_QUERY_MULTIPLIER,
    DEFAULT_TEXT_EMBEDDING_DIM,
};
use crate::memory_compaction::{build_lexical_shadow, compact_memory_record_payload};
use crate::memory_quality::{
    classify_storage_outcome, default_memory_quality_config, deterministic_dedup_fingerprint,
    is_supported_dedup_fingerprint, quality_gate_reason as shared_quality_gate_reason,
};
use arrow_array::{
    builder::{Int64Builder, ListBuilder, StringBuilder},
    Array, BooleanArray, FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch,
    RecordBatchIterator, RecordBatchReader, StringArray, UInt32Array,
};
use arrow_schema::{ArrowError, DataType, Field, Schema};
use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::{AddDataMode, NewColumnTransform};
use lancedb::{Connection, Table};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MEMORIES_TABLE: &str = "memories";
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
}

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

    pub async fn upsert_knowledge_pages(
        &self,
        pages: &[KnowledgePage],
    ) -> Result<(), Box<dyn std::error::Error>> {
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
            self.knowledge_pages_table.delete(&filter).await?;
        }

        let batch = knowledge_pages_to_batch(&deduped)?;
        let schema = Arc::new(knowledge_page_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.knowledge_pages_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
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
        if records.is_empty() {
            return Ok(());
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
        tracing::info!(
            incoming_count,
            inserted_count = compacted.len(),
            skipped_count,
            deduped_count,
            "lancedb:add_batch"
        );
        if compacted.is_empty() {
            return Ok(());
        }
        self.insert_memory_batch(&compacted).await
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

    /// Replace the entire memories table in one write, preserving caller ids.
    pub async fn replace_all_memories_preserving_ids(
        &self,
        records: &[MemoryRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            self.delete_all().await?;
            return Ok(());
        }

        let normalized = records
            .iter()
            .map(normalize_record_for_index)
            .collect::<Vec<_>>();
        let batch = records_to_batch(&normalized)?;
        let schema = Arc::new(memory_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
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
        self.table.delete(&filter).await?;
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

    /// Return comprehensive statistics and usage insights about stored data.
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

        // Count before deletion.
        let before = self.table.count_rows(None).await?;
        self.table
            .delete(&format!("timestamp < {cutoff_ms}"))
            .await?;
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
}

// ── Schema ────────────────────────────────────────────────────────────────────

fn memory_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("timestamp", DataType::Int64, false),
        Field::new("day_bucket", DataType::Utf8, false),
        Field::new("app_name", DataType::Utf8, false),
        Field::new("bundle_id", DataType::Utf8, true),
        Field::new("window_title", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("clean_text", DataType::Utf8, false),
        Field::new("ocr_confidence", DataType::Float32, false),
        Field::new("ocr_block_count", DataType::Int64, false),
        Field::new("snippet", DataType::Utf8, false),
        Field::new("display_summary", DataType::Utf8, false),
        Field::new("internal_context", DataType::Utf8, false),
        Field::new("summary_source", DataType::Utf8, false),
        Field::new("noise_score", DataType::Float32, false),
        Field::new("session_key", DataType::Utf8, false),
        Field::new("lexical_shadow", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                TEXT_EMBED_DIM,
            ),
            false,
        ),
        Field::new(
            "image_embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                IMAGE_EMBED_DIM,
            ),
            false,
        ),
        Field::new("screenshot_path", DataType::Utf8, true),
        Field::new("url", DataType::Utf8, true),
        Field::new(
            "snippet_embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                TEXT_EMBED_DIM,
            ),
            false,
        ),
        Field::new(
            "support_embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                TEXT_EMBED_DIM,
            ),
            false,
        ),
        Field::new("decay_score", DataType::Float32, false),
        Field::new("last_accessed_at", DataType::Int64, false),
        Field::new("timestamp_start", DataType::Int64, false),
        Field::new("timestamp_end", DataType::Int64, false),
        Field::new("source_type", DataType::Utf8, false),
        Field::new("topic", DataType::Utf8, false),
        Field::new("workflow", DataType::Utf8, false),
        Field::new("user_intent", DataType::Utf8, false),
        Field::new("intent_analysis", DataType::Utf8, false),
        Field::new("memory_context", DataType::Utf8, false),
        Field::new(
            "commands",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "blockers",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "todos",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "open_questions",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "results",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "related_tools",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "related_agents",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "related_projects",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("raw_evidence", DataType::Utf8, false),
        Field::new(
            "search_aliases",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "related_memory_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "graph_node_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "graph_edge_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("project_confidence", DataType::Float32, false),
        Field::new("topic_confidence", DataType::Float32, false),
        Field::new("workflow_confidence", DataType::Float32, false),
        Field::new(
            "project_evidence",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "related_project_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("evidence_confidence", DataType::Float32, false),
        Field::new("confidence_score", DataType::Float32, false),
        Field::new("importance_score", DataType::Float32, false),
        Field::new("specificity_score", DataType::Float32, false),
        Field::new("intent_score", DataType::Float32, false),
        Field::new("entity_score", DataType::Float32, false),
        Field::new("agent_usefulness_score", DataType::Float32, false),
        Field::new("ocr_noise_score", DataType::Float32, false),
        Field::new("graph_readiness_score", DataType::Float32, false),
        Field::new("retrieval_value_score", DataType::Float32, false),
        Field::new("storage_outcome", DataType::Utf8, false),
        Field::new("quality_gate_reason", DataType::Utf8, false),
        Field::new("extracted_entities_structured", DataType::Utf8, false),
        Field::new("action_items", DataType::Utf8, false),
        Field::new("schema_version", DataType::UInt32, false),
        Field::new("activity_type", DataType::Utf8, false),
        Field::new(
            "files_touched",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "symbols_changed",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("session_duration_mins", DataType::UInt32, false),
        Field::new("project", DataType::Utf8, false),
        Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "entities",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "decisions",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "errors",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "next_steps",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("git_stats", DataType::Utf8, true),
        Field::new("outcome", DataType::Utf8, false),
        Field::new("extraction_confidence", DataType::Float32, false),
        Field::new("anchor_coverage_score", DataType::Float32, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("dedup_fingerprint", DataType::Utf8, false),
        Field::new("embedding_text", DataType::Utf8, false),
        Field::new("embedding_model", DataType::Utf8, false),
        Field::new("embedding_dim", DataType::UInt32, false),
        Field::new("is_consolidated", DataType::Boolean, false),
        Field::new("is_soft_deleted", DataType::Boolean, false),
        Field::new("parent_id", DataType::Utf8, true),
        Field::new(
            "related_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "consolidated_from",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
    ])
}

fn task_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, false),
        Field::new("source_app", DataType::Utf8, false),
        Field::new("source_memory_id", DataType::Utf8, true),
        Field::new("created_at", DataType::Int64, false),
        Field::new("due_date", DataType::Int64, true),
        Field::new("is_completed", DataType::Boolean, false),
        Field::new("is_dismissed", DataType::Boolean, false),
        Field::new("task_type", DataType::Utf8, false),
        Field::new(
            "linked_urls",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "linked_memory_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
    ])
}

fn meeting_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new(
            "participants",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("model", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("start_timestamp", DataType::Int64, false),
        Field::new("end_timestamp", DataType::Int64, true),
        Field::new("created_at", DataType::Int64, false),
        Field::new("updated_at", DataType::Int64, false),
        Field::new("segment_count", DataType::Int64, false),
        Field::new("duration_seconds", DataType::Int64, false),
        Field::new("meeting_dir", DataType::Utf8, false),
        Field::new("audio_dir", DataType::Utf8, false),
        Field::new("transcript_path", DataType::Utf8, true),
        Field::new("breakdown_json", DataType::Utf8, true),
    ])
}

fn segment_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("index", DataType::UInt32, false),
        Field::new("start_timestamp", DataType::Int64, false),
        Field::new("end_timestamp", DataType::Int64, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("audio_chunk_path", DataType::Utf8, false),
        Field::new("model", DataType::Utf8, false),
        Field::new("created_at", DataType::Int64, false),
    ])
}

fn node_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("node_type", DataType::Utf8, false),
        Field::new("label", DataType::Utf8, false),
        Field::new("created_at", DataType::Int64, false),
        Field::new("metadata_json", DataType::Utf8, false),
    ])
}

fn edge_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, false),
        Field::new("target", DataType::Utf8, false),
        Field::new("edge_type", DataType::Utf8, false),
        Field::new("timestamp", DataType::Int64, false),
        Field::new("metadata_json", DataType::Utf8, false),
    ])
}

fn activity_event_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("memory_id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, true),
        Field::new("activity_type", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("start_time", DataType::Int64, false),
        Field::new("end_time", DataType::Int64, false),
        Field::new("privacy_class", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn project_context_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, false),
        Field::new("updated_at", DataType::Int64, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("privacy_class", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn decision_ledger_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, true),
        Field::new("title", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("created_at", DataType::Int64, false),
        Field::new("privacy_class", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn context_pack_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, true),
        Field::new("project", DataType::Utf8, true),
        Field::new("agent_type", DataType::Utf8, false),
        Field::new("generated_at", DataType::Int64, false),
        Field::new("budget_tokens", DataType::UInt32, false),
        Field::new("tokens_used", DataType::UInt32, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn context_delta_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("since", DataType::Int64, false),
        Field::new("generated_at", DataType::Int64, false),
        Field::new("tokens_used", DataType::UInt32, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn entity_alias_schema() -> Schema {
    Schema::new(vec![
        Field::new("alias_key", DataType::Utf8, false),
        Field::new("canonical_id", DataType::Utf8, false),
        Field::new("canonical_name", DataType::Utf8, false),
        Field::new("entity_type", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, true),
        Field::new("confidence", DataType::Float32, false),
        Field::new("updated_at", DataType::Int64, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn knowledge_page_schema() -> Schema {
    Schema::new(vec![
        Field::new("page_id", DataType::Utf8, false),
        Field::new("page_type", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, true),
        Field::new("topic", DataType::Utf8, true),
        Field::new("title", DataType::Utf8, false),
        Field::new("stability", DataType::Utf8, false),
        Field::new("last_updated", DataType::Int64, false),
        Field::new("confidence_score", DataType::Float32, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

fn edge_type_literal(edge_type: EdgeType) -> &'static str {
    match edge_type {
        EdgeType::PartOfSession => "PART_OF_SESSION",
        EdgeType::ReferenceForTask => "REFERENCE_FOR_TASK",
        EdgeType::OccurredAt => "OCCURRED_AT",
        EdgeType::ClipboardCopied => "CLIPBOARD_COPIED",
        EdgeType::OccurredDuringAudio => "OCCURRED_DURING_AUDIO",
        EdgeType::BelongsTo => "BELONGS_TO",
        EdgeType::MentionedIn => "MENTIONED_IN",
        EdgeType::EditedFile => "EDITED_FILE",
        EdgeType::FixedBy => "FIXED_BY",
        EdgeType::BlockedBy => "BLOCKED_BY",
        EdgeType::InformedBy => "INFORMED_BY",
        EdgeType::ResultedIn => "RESULTED_IN",
    }
}

fn build_string_match_filter(column: &str, values: &[String]) -> Option<String> {
    let clauses = values
        .iter()
        .map(|value| format!("{column} = '{}'", sql_escape(value)))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" OR "))
    }
}

fn build_edge_identity_filter(edges: &[GraphEdge]) -> Option<String> {
    let clauses = edges
        .iter()
        .map(|edge| {
            format!(
                "(source = '{}' AND target = '{}' AND edge_type = '{}')",
                sql_escape(&edge.source),
                sql_escape(&edge.target),
                edge_type_literal(edge.edge_type)
            )
        })
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" OR "))
    }
}

// ── Arrow ↔ MemoryRecord conversion ─────────────────────────────────────────

fn records_to_batch(records: &[MemoryRecord]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(memory_schema());

    // Scalar string columns
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    let timestamps: Vec<i64> = records.iter().map(|r| r.timestamp).collect();
    let day_buckets: Vec<&str> = records.iter().map(|r| r.day_bucket.as_str()).collect();
    let app_names: Vec<&str> = records.iter().map(|r| r.app_name.as_str()).collect();
    let bundle_ids: Vec<Option<&str>> = records.iter().map(|r| r.bundle_id.as_deref()).collect();
    let window_titles: Vec<&str> = records.iter().map(|r| r.window_title.as_str()).collect();
    let session_ids: Vec<&str> = records.iter().map(|r| r.session_id.as_str()).collect();
    let texts: Vec<&str> = records.iter().map(|r| r.text.as_str()).collect();
    let clean_texts: Vec<&str> = records.iter().map(|r| r.clean_text.as_str()).collect();
    let ocr_confidences: Vec<f32> = records.iter().map(|r| r.ocr_confidence).collect();
    let ocr_block_counts: Vec<i64> = records.iter().map(|r| r.ocr_block_count as i64).collect();
    let snippets: Vec<&str> = records.iter().map(|r| r.snippet.as_str()).collect();
    let display_summaries: Vec<&str> = records.iter().map(|r| r.display_summary.as_str()).collect();
    let internal_contexts: Vec<&str> = records
        .iter()
        .map(|r| r.internal_context.as_str())
        .collect();
    let summary_sources: Vec<&str> = records.iter().map(|r| r.summary_source.as_str()).collect();
    let noise_scores: Vec<f32> = records.iter().map(|r| r.noise_score).collect();
    let session_keys: Vec<&str> = records.iter().map(|r| r.session_key.as_str()).collect();
    let lexical_shadows: Vec<&str> = records.iter().map(|r| r.lexical_shadow.as_str()).collect();
    let screenshot_paths: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.screenshot_path.as_deref())
        .collect();
    let urls: Vec<Option<&str>> = records.iter().map(|r| r.url.as_deref()).collect();

    // Text embeddings — flatten all embeddings into one Float32Array.
    let flat_text: Vec<f32> = records
        .iter()
        .flat_map(|r| r.embedding.iter().copied())
        .collect();
    let text_values = Arc::new(Float32Array::from(flat_text)) as Arc<dyn Array>;
    let embedding_array = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        TEXT_EMBED_DIM,
        text_values,
        None,
    )?;

    // Image embeddings
    let flat_img: Vec<f32> = records
        .iter()
        .flat_map(|r| r.image_embedding.iter().copied())
        .collect();
    let img_values = Arc::new(Float32Array::from(flat_img)) as Arc<dyn Array>;
    let image_embedding_array = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        IMAGE_EMBED_DIM,
        img_values,
        None,
    )?;

    // Snippet embeddings (second semantic tower)
    let flat_snip: Vec<f32> = records
        .iter()
        .flat_map(|r| r.snippet_embedding.iter().copied())
        .collect();
    let snip_values = Arc::new(Float32Array::from(flat_snip)) as Arc<dyn Array>;
    let snippet_embedding_array = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        TEXT_EMBED_DIM,
        snip_values,
        None,
    )?;

    let flat_support: Vec<f32> = records
        .iter()
        .flat_map(|r| r.support_embedding.iter().copied())
        .collect();
    let support_values = Arc::new(Float32Array::from(flat_support)) as Arc<dyn Array>;
    let support_embedding_array = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        TEXT_EMBED_DIM,
        support_values,
        None,
    )?;

    let decay_scores: Vec<f32> = records.iter().map(|r| r.decay_score).collect();
    let last_accessed: Vec<i64> = records.iter().map(|r| r.last_accessed_at).collect();
    let timestamp_starts: Vec<i64> = records.iter().map(|r| r.timestamp_start).collect();
    let timestamp_ends: Vec<i64> = records.iter().map(|r| r.timestamp_end).collect();
    let source_types: Vec<&str> = records.iter().map(|r| r.source_type.as_str()).collect();
    let topics: Vec<&str> = records.iter().map(|r| r.topic.as_str()).collect();
    let workflows: Vec<&str> = records.iter().map(|r| r.workflow.as_str()).collect();
    let user_intents: Vec<&str> = records.iter().map(|r| r.user_intent.as_str()).collect();
    let intent_analysis_json: Vec<String> = records
        .iter()
        .map(|r| serde_json::to_string(&r.intent_analysis).unwrap_or_else(|_| "{}".to_string()))
        .collect();
    let intent_analysis_refs: Vec<&str> = intent_analysis_json.iter().map(|s| s.as_str()).collect();
    let memory_contexts: Vec<&str> = records.iter().map(|r| r.memory_context.as_str()).collect();
    let raw_evidences: Vec<&str> = records.iter().map(|r| r.raw_evidence.as_str()).collect();
    let project_confidences: Vec<f32> = records.iter().map(|r| r.project_confidence).collect();
    let topic_confidences: Vec<f32> = records.iter().map(|r| r.topic_confidence).collect();
    let workflow_confidences: Vec<f32> = records.iter().map(|r| r.workflow_confidence).collect();
    let evidence_confidences: Vec<f32> = records.iter().map(|r| r.evidence_confidence).collect();
    let confidence_scores: Vec<f32> = records.iter().map(|r| r.confidence_score).collect();
    let importance_scores: Vec<f32> = records.iter().map(|r| r.importance_score).collect();
    let specificity_scores: Vec<f32> = records.iter().map(|r| r.specificity_score).collect();
    let intent_scores: Vec<f32> = records.iter().map(|r| r.intent_score).collect();
    let entity_scores: Vec<f32> = records.iter().map(|r| r.entity_score).collect();
    let agent_usefulness_scores: Vec<f32> =
        records.iter().map(|r| r.agent_usefulness_score).collect();
    let ocr_noise_scores: Vec<f32> = records.iter().map(|r| r.ocr_noise_score).collect();
    let graph_readiness_scores: Vec<f32> =
        records.iter().map(|r| r.graph_readiness_score).collect();
    let retrieval_value_scores: Vec<f32> =
        records.iter().map(|r| r.retrieval_value_score).collect();
    let storage_outcomes: Vec<&str> = records.iter().map(|r| r.storage_outcome.as_str()).collect();
    let quality_gate_reasons: Vec<&str> = records
        .iter()
        .map(|r| r.quality_gate_reason.as_str())
        .collect();
    let extracted_entities_structured_json: Vec<String> = records
        .iter()
        .map(|r| {
            serde_json::to_string(&r.extracted_entities_structured)
                .unwrap_or_else(|_| "[]".to_string())
        })
        .collect();
    let extracted_entities_structured_refs: Vec<&str> = extracted_entities_structured_json
        .iter()
        .map(|s| s.as_str())
        .collect();
    let action_items_json: Vec<String> = records
        .iter()
        .map(|r| serde_json::to_string(&r.action_items).unwrap_or_else(|_| "[]".to_string()))
        .collect();
    let action_items_refs: Vec<&str> = action_items_json.iter().map(|s| s.as_str()).collect();

    // V2 Fields
    let schema_versions: Vec<u32> = records.iter().map(|r| r.schema_version).collect();
    let activity_types: Vec<&str> = records.iter().map(|r| r.activity_type.as_str()).collect();
    let session_duration_mins: Vec<u32> = records.iter().map(|r| r.session_duration_mins).collect();
    let projects: Vec<&str> = records.iter().map(|r| r.project.as_str()).collect();
    let outcomes: Vec<&str> = records.iter().map(|r| r.outcome.as_str()).collect();
    let extraction_confidences: Vec<f32> =
        records.iter().map(|r| r.extraction_confidence).collect();
    let anchor_coverage_scores: Vec<f32> =
        records.iter().map(|r| r.anchor_coverage_score).collect();
    let content_hashes: Vec<&str> = records.iter().map(|r| r.content_hash.as_str()).collect();
    let dedup_fingerprints: Vec<&str> = records
        .iter()
        .map(|r| r.dedup_fingerprint.as_str())
        .collect();
    let embedding_texts: Vec<&str> = records.iter().map(|r| r.embedding_text.as_str()).collect();
    let embedding_models: Vec<&str> = records.iter().map(|r| r.embedding_model.as_str()).collect();
    let embedding_dims: Vec<u32> = records.iter().map(|r| r.embedding_dim).collect();
    let is_consolidated_flags: Vec<bool> = records.iter().map(|r| r.is_consolidated).collect();
    let is_soft_deleted_flags: Vec<bool> = records.iter().map(|r| r.is_soft_deleted).collect();
    let parent_ids: Vec<Option<&str>> = records.iter().map(|r| r.parent_id.as_deref()).collect();
    let git_stats_json: Vec<Option<String>> = records
        .iter()
        .map(|r| {
            r.git_stats
                .as_ref()
                .and_then(|g| serde_json::to_string(g).ok())
        })
        .collect();
    let git_stats_refs: Vec<Option<&str>> = git_stats_json.iter().map(|o| o.as_deref()).collect();

    let build_str_list = |extractor: &dyn Fn(&MemoryRecord) -> &Vec<String>| {
        let mut builder = ListBuilder::new(StringBuilder::new());
        for record in records {
            let list = extractor(record);
            for item in list {
                builder.values().append_value(item);
            }
            builder.append(true);
        }
        builder.finish()
    };

    let files_touched_array = build_str_list(&|r| &r.files_touched);
    let symbols_changed_array = build_str_list(&|r| &r.symbols_changed);
    let tags_array = build_str_list(&|r| &r.tags);
    let entities_array = build_str_list(&|r| &r.entities);
    let decisions_array = build_str_list(&|r| &r.decisions);
    let errors_array = build_str_list(&|r| &r.errors);
    let next_steps_array = build_str_list(&|r| &r.next_steps);
    let commands_array = build_str_list(&|r| &r.commands);
    let blockers_array = build_str_list(&|r| &r.blockers);
    let todos_array = build_str_list(&|r| &r.todos);
    let open_questions_array = build_str_list(&|r| &r.open_questions);
    let results_array = build_str_list(&|r| &r.results);
    let related_tools_array = build_str_list(&|r| &r.related_tools);
    let related_agents_array = build_str_list(&|r| &r.related_agents);
    let related_projects_array = build_str_list(&|r| &r.related_projects);
    let search_aliases_array = build_str_list(&|r| &r.search_aliases);
    let related_memory_ids_array = build_str_list(&|r| &r.related_memory_ids);
    let graph_node_ids_array = build_str_list(&|r| &r.graph_node_ids);
    let graph_edge_ids_array = build_str_list(&|r| &r.graph_edge_ids);
    let project_evidence_array = build_str_list(&|r| &r.project_evidence);
    let related_project_ids_array = build_str_list(&|r| &r.related_project_ids);
    let related_ids_array = build_str_list(&|r| &r.related_ids);
    let consolidated_from_array = build_str_list(&|r| &r.consolidated_from);

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(Int64Array::from(timestamps)),
            Arc::new(StringArray::from(day_buckets)),
            Arc::new(StringArray::from(app_names)),
            Arc::new(StringArray::from(bundle_ids)),
            Arc::new(StringArray::from(window_titles)),
            Arc::new(StringArray::from(session_ids)),
            Arc::new(StringArray::from(texts)),
            Arc::new(StringArray::from(clean_texts)),
            Arc::new(Float32Array::from(ocr_confidences)),
            Arc::new(Int64Array::from(ocr_block_counts)),
            Arc::new(StringArray::from(snippets)),
            Arc::new(StringArray::from(display_summaries)),
            Arc::new(StringArray::from(internal_contexts)),
            Arc::new(StringArray::from(summary_sources)),
            Arc::new(Float32Array::from(noise_scores)),
            Arc::new(StringArray::from(session_keys)),
            Arc::new(StringArray::from(lexical_shadows)),
            Arc::new(embedding_array),
            Arc::new(image_embedding_array),
            Arc::new(StringArray::from(screenshot_paths)),
            Arc::new(StringArray::from(urls)),
            Arc::new(snippet_embedding_array),
            Arc::new(support_embedding_array),
            Arc::new(Float32Array::from(decay_scores)),
            Arc::new(Int64Array::from(last_accessed)),
            Arc::new(Int64Array::from(timestamp_starts)),
            Arc::new(Int64Array::from(timestamp_ends)),
            Arc::new(StringArray::from(source_types)),
            Arc::new(StringArray::from(topics)),
            Arc::new(StringArray::from(workflows)),
            Arc::new(StringArray::from(user_intents)),
            Arc::new(StringArray::from(intent_analysis_refs)),
            Arc::new(StringArray::from(memory_contexts)),
            Arc::new(commands_array),
            Arc::new(blockers_array),
            Arc::new(todos_array),
            Arc::new(open_questions_array),
            Arc::new(results_array),
            Arc::new(related_tools_array),
            Arc::new(related_agents_array),
            Arc::new(related_projects_array),
            Arc::new(StringArray::from(raw_evidences)),
            Arc::new(search_aliases_array),
            Arc::new(related_memory_ids_array),
            Arc::new(graph_node_ids_array),
            Arc::new(graph_edge_ids_array),
            Arc::new(Float32Array::from(project_confidences)),
            Arc::new(Float32Array::from(topic_confidences)),
            Arc::new(Float32Array::from(workflow_confidences)),
            Arc::new(project_evidence_array),
            Arc::new(related_project_ids_array),
            Arc::new(Float32Array::from(evidence_confidences)),
            Arc::new(Float32Array::from(confidence_scores)),
            Arc::new(Float32Array::from(importance_scores)),
            Arc::new(Float32Array::from(specificity_scores)),
            Arc::new(Float32Array::from(intent_scores)),
            Arc::new(Float32Array::from(entity_scores)),
            Arc::new(Float32Array::from(agent_usefulness_scores)),
            Arc::new(Float32Array::from(ocr_noise_scores)),
            Arc::new(Float32Array::from(graph_readiness_scores)),
            Arc::new(Float32Array::from(retrieval_value_scores)),
            Arc::new(StringArray::from(storage_outcomes)),
            Arc::new(StringArray::from(quality_gate_reasons)),
            Arc::new(StringArray::from(extracted_entities_structured_refs)),
            Arc::new(StringArray::from(action_items_refs)),
            Arc::new(UInt32Array::from(schema_versions)),
            Arc::new(StringArray::from(activity_types)),
            Arc::new(files_touched_array),
            Arc::new(symbols_changed_array),
            Arc::new(UInt32Array::from(session_duration_mins)),
            Arc::new(StringArray::from(projects)),
            Arc::new(tags_array),
            Arc::new(entities_array),
            Arc::new(decisions_array),
            Arc::new(errors_array),
            Arc::new(next_steps_array),
            Arc::new(StringArray::from(git_stats_refs)),
            Arc::new(StringArray::from(outcomes)),
            Arc::new(Float32Array::from(extraction_confidences)),
            Arc::new(Float32Array::from(anchor_coverage_scores)),
            Arc::new(StringArray::from(content_hashes)),
            Arc::new(StringArray::from(dedup_fingerprints)),
            Arc::new(StringArray::from(embedding_texts)),
            Arc::new(StringArray::from(embedding_models)),
            Arc::new(UInt32Array::from(embedding_dims)),
            Arc::new(BooleanArray::from(is_consolidated_flags)),
            Arc::new(BooleanArray::from(is_soft_deleted_flags)),
            Arc::new(StringArray::from(parent_ids)),
            Arc::new(related_ids_array),
            Arc::new(consolidated_from_array),
        ],
    )
}

fn batch_to_memory_records(batch: &RecordBatch) -> Vec<MemoryRecord> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let timestamps = i64_col(batch, "timestamp");
    let day_buckets = str_col(batch, "day_bucket");
    let app_names = str_col(batch, "app_name");
    let bundle_ids = str_col(batch, "bundle_id");
    let window_titles = str_col(batch, "window_title");
    let session_ids = str_col(batch, "session_id");
    let texts = str_col(batch, "text");
    let clean_texts = str_col(batch, "clean_text");
    let ocr_confidences = f32_col(batch, "ocr_confidence");
    let ocr_block_counts = i64_col(batch, "ocr_block_count");
    let snippets = str_col(batch, "snippet");
    let display_summaries = str_col(batch, "display_summary");
    let internal_contexts = str_col(batch, "internal_context");
    let summary_sources = str_col(batch, "summary_source");
    let noise_scores = f32_col(batch, "noise_score");
    let session_keys = str_col(batch, "session_key");
    let lexical_shadows = str_col(batch, "lexical_shadow");
    let screenshot_paths = str_col(batch, "screenshot_path");
    let urls = str_col(batch, "url");

    let embed_col = batch
        .column_by_name("embedding")
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>().cloned());
    let img_col = batch
        .column_by_name("image_embedding")
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>().cloned());
    let snip_embed_col = batch
        .column_by_name("snippet_embedding")
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>().cloned());
    let support_embed_col = batch
        .column_by_name("support_embedding")
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>().cloned());
    let decay_scores = f32_col(batch, "decay_score");
    let last_accessed = i64_col(batch, "last_accessed_at");
    let timestamp_starts = i64_col(batch, "timestamp_start");
    let timestamp_ends = i64_col(batch, "timestamp_end");
    let source_types = str_col(batch, "source_type");
    let topics = str_col(batch, "topic");
    let workflows = str_col(batch, "workflow");
    let user_intents = str_col(batch, "user_intent");
    let intent_analysis_json = str_col(batch, "intent_analysis");
    let memory_contexts = str_col(batch, "memory_context");
    let commands = list_str_col(batch, "commands");
    let blockers = list_str_col(batch, "blockers");
    let todos = list_str_col(batch, "todos");
    let open_questions = list_str_col(batch, "open_questions");
    let results = list_str_col(batch, "results");
    let related_tools = list_str_col(batch, "related_tools");
    let related_agents = list_str_col(batch, "related_agents");
    let related_projects = list_str_col(batch, "related_projects");
    let raw_evidence = str_col(batch, "raw_evidence");
    let search_aliases = list_str_col(batch, "search_aliases");
    let related_memory_ids = list_str_col(batch, "related_memory_ids");
    let graph_node_ids = list_str_col(batch, "graph_node_ids");
    let graph_edge_ids = list_str_col(batch, "graph_edge_ids");
    let project_confidences = f32_col(batch, "project_confidence");
    let topic_confidences = f32_col(batch, "topic_confidence");
    let workflow_confidences = f32_col(batch, "workflow_confidence");
    let project_evidence = list_str_col(batch, "project_evidence");
    let related_project_ids = list_str_col(batch, "related_project_ids");
    let evidence_confidences = f32_col(batch, "evidence_confidence");
    let confidence_scores = f32_col(batch, "confidence_score");
    let importance_scores = f32_col(batch, "importance_score");
    let specificity_scores = f32_col(batch, "specificity_score");
    let intent_scores = f32_col(batch, "intent_score");
    let entity_scores = f32_col(batch, "entity_score");
    let agent_usefulness_scores = f32_col(batch, "agent_usefulness_score");
    let ocr_noise_scores = f32_col(batch, "ocr_noise_score");
    let graph_readiness_scores = f32_col(batch, "graph_readiness_score");
    let retrieval_value_scores = f32_col(batch, "retrieval_value_score");
    let storage_outcomes = str_col(batch, "storage_outcome");
    let quality_gate_reasons = str_col(batch, "quality_gate_reason");
    let extracted_entities_structured_json = str_col(batch, "extracted_entities_structured");
    let action_items_json = str_col(batch, "action_items");

    // V2 columns
    let schema_versions = u32_col(batch, "schema_version");
    let activity_types = str_col(batch, "activity_type");
    let files_touched = list_str_col(batch, "files_touched");
    let symbols_changed = list_str_col(batch, "symbols_changed");
    let session_duration_mins = u32_col(batch, "session_duration_mins");
    let projects = str_col(batch, "project");
    let tags = list_str_col(batch, "tags");
    let entities = list_str_col(batch, "entities");
    let decisions = list_str_col(batch, "decisions");
    let errors = list_str_col(batch, "errors");
    let next_steps = list_str_col(batch, "next_steps");
    let git_stats_json = str_col(batch, "git_stats");
    let outcomes = str_col(batch, "outcome");
    let extraction_confidences = f32_col(batch, "extraction_confidence");
    let anchor_coverage_scores = f32_col(batch, "anchor_coverage_score");
    let content_hashes = str_col(batch, "content_hash");
    let dedup_fingerprints = str_col(batch, "dedup_fingerprint");
    let embedding_texts = str_col(batch, "embedding_text");
    let embedding_models = str_col(batch, "embedding_model");
    let embedding_dims = u32_col(batch, "embedding_dim");
    let is_consolidated_flags = bool_col(batch, "is_consolidated");
    let is_soft_deleted_flags = bool_col(batch, "is_soft_deleted");
    let parent_ids = str_col(batch, "parent_id");
    let related_ids = list_str_col(batch, "related_ids");
    let consolidated_from = list_str_col(batch, "consolidated_from");

    (0..n)
        .map(|i| {
            let embedding = extract_f32_list(&embed_col, i, TEXT_EMBED_DIM as usize);
            let image_embedding = extract_f32_list(&img_col, i, IMAGE_EMBED_DIM as usize);
            let snippet_embedding = extract_f32_list(&snip_embed_col, i, TEXT_EMBED_DIM as usize);
            let support_embedding =
                extract_f32_list(&support_embed_col, i, TEXT_EMBED_DIM as usize);
            MemoryRecord {
                id: get_str(&ids, i),
                timestamp: timestamps.as_ref().map(|c| c.value(i)).unwrap_or(0),
                day_bucket: get_str(&day_buckets, i),
                app_name: get_str(&app_names, i),
                bundle_id: get_opt_str(&bundle_ids, i),
                window_title: get_str(&window_titles, i),
                session_id: get_str(&session_ids, i),
                text: get_str(&texts, i),
                clean_text: get_str(&clean_texts, i),
                ocr_confidence: get_f32(&ocr_confidences, i),
                ocr_block_count: get_i64(&ocr_block_counts, i).max(0) as u32,
                snippet: get_str(&snippets, i),
                display_summary: {
                    let summary = get_str(&display_summaries, i);
                    if summary.trim().is_empty() {
                        get_str(&snippets, i)
                    } else {
                        summary
                    }
                },
                internal_context: get_str(&internal_contexts, i),
                summary_source: get_str(&summary_sources, i),
                noise_score: get_f32(&noise_scores, i),
                session_key: get_str(&session_keys, i),
                lexical_shadow: get_str(&lexical_shadows, i),
                embedding,
                image_embedding,
                screenshot_path: get_opt_str(&screenshot_paths, i),
                url: get_opt_str(&urls, i),
                snippet_embedding,
                support_embedding,
                decay_score: get_f32(&decay_scores, i).max(0.01),
                last_accessed_at: get_i64(&last_accessed, i),
                timestamp_start: get_i64(&timestamp_starts, i),
                timestamp_end: get_i64(&timestamp_ends, i),
                source_type: get_str(&source_types, i),
                topic: get_str(&topics, i),
                workflow: get_str(&workflows, i),
                user_intent: get_str(&user_intents, i),
                intent_analysis: get_opt_str(&intent_analysis_json, i)
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default(),
                memory_context: {
                    let context = get_str(&memory_contexts, i);
                    if context.trim().is_empty() {
                        let internal = get_str(&internal_contexts, i);
                        if internal.trim().is_empty() {
                            get_str(&display_summaries, i)
                        } else {
                            internal
                        }
                    } else {
                        context
                    }
                },
                commands: extract_str_list(&commands, i),
                blockers: extract_str_list(&blockers, i),
                todos: extract_str_list(&todos, i),
                open_questions: extract_str_list(&open_questions, i),
                results: extract_str_list(&results, i),
                related_tools: extract_str_list(&related_tools, i),
                related_agents: extract_str_list(&related_agents, i),
                related_projects: extract_str_list(&related_projects, i),
                raw_evidence: get_str(&raw_evidence, i),
                search_aliases: extract_str_list(&search_aliases, i),
                related_memory_ids: extract_str_list(&related_memory_ids, i),
                graph_node_ids: extract_str_list(&graph_node_ids, i),
                graph_edge_ids: extract_str_list(&graph_edge_ids, i),
                project_confidence: get_f32(&project_confidences, i),
                topic_confidence: get_f32(&topic_confidences, i),
                workflow_confidence: get_f32(&workflow_confidences, i),
                project_evidence: extract_str_list(&project_evidence, i),
                related_project_ids: extract_str_list(&related_project_ids, i),
                evidence_confidence: get_f32(&evidence_confidences, i),
                confidence_score: get_f32(&confidence_scores, i),
                importance_score: get_f32(&importance_scores, i),
                specificity_score: get_f32(&specificity_scores, i),
                intent_score: get_f32(&intent_scores, i),
                entity_score: get_f32(&entity_scores, i),
                agent_usefulness_score: get_f32(&agent_usefulness_scores, i),
                ocr_noise_score: get_f32(&ocr_noise_scores, i),
                graph_readiness_score: get_f32(&graph_readiness_scores, i),
                retrieval_value_score: get_f32(&retrieval_value_scores, i),
                storage_outcome: get_str(&storage_outcomes, i),
                quality_gate_reason: get_str(&quality_gate_reasons, i),
                extracted_entities_structured: get_opt_str(&extracted_entities_structured_json, i)
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default(),
                action_items: get_opt_str(&action_items_json, i)
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default(),
                schema_version: get_u32(&schema_versions, i),
                activity_type: get_str(&activity_types, i),
                files_touched: extract_str_list(&files_touched, i),
                symbols_changed: extract_str_list(&symbols_changed, i),
                session_duration_mins: get_u32(&session_duration_mins, i),
                project: get_str(&projects, i),
                tags: extract_str_list(&tags, i),
                entities: extract_str_list(&entities, i),
                decisions: extract_str_list(&decisions, i),
                errors: extract_str_list(&errors, i),
                next_steps: extract_str_list(&next_steps, i),
                git_stats: get_opt_str(&git_stats_json, i)
                    .and_then(|j| serde_json::from_str(&j).ok()),
                outcome: get_str(&outcomes, i),
                extraction_confidence: get_f32(&extraction_confidences, i),
                anchor_coverage_score: get_f32(&anchor_coverage_scores, i).clamp(0.0, 1.0),
                content_hash: {
                    let hash = get_str(&content_hashes, i);
                    if hash.trim().is_empty() {
                        compute_content_hash(
                            get_opt_str(&urls, i).as_deref(),
                            &get_str(&window_titles, i),
                            timestamps.as_ref().map(|c| c.value(i)).unwrap_or(0),
                        )
                    } else {
                        hash
                    }
                },
                dedup_fingerprint: get_str(&dedup_fingerprints, i),
                embedding_text: get_str(&embedding_texts, i),
                embedding_model: get_str(&embedding_models, i),
                embedding_dim: get_u32(&embedding_dims, i),
                is_consolidated: get_bool(&is_consolidated_flags, i),
                is_soft_deleted: get_bool(&is_soft_deleted_flags, i),
                parent_id: get_opt_str(&parent_ids, i),
                related_ids: extract_str_list(&related_ids, i),
                consolidated_from: extract_str_list(&consolidated_from, i),
            }
        })
        .collect()
}

fn batch_to_search_results(batch: &RecordBatch) -> Vec<SearchResult> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let timestamps = i64_col(batch, "timestamp");
    let app_names = str_col(batch, "app_name");
    let bundle_ids = str_col(batch, "bundle_id");
    let window_titles = str_col(batch, "window_title");
    let session_ids = str_col(batch, "session_id");
    let texts = str_col(batch, "text");
    let clean_texts = str_col(batch, "clean_text");
    let ocr_confidences = f32_col(batch, "ocr_confidence");
    let ocr_block_counts = i64_col(batch, "ocr_block_count");
    let snippets = str_col(batch, "snippet");
    let display_summaries = str_col(batch, "display_summary");
    let internal_contexts = str_col(batch, "internal_context");
    let summary_sources = str_col(batch, "summary_source");
    let noise_scores = f32_col(batch, "noise_score");
    let session_keys = str_col(batch, "session_key");
    let lexical_shadows = str_col(batch, "lexical_shadow");
    let memory_contexts = str_col(batch, "memory_context");
    let user_intents = str_col(batch, "user_intent");
    let topics = str_col(batch, "topic");
    let workflows = str_col(batch, "workflow");
    let search_aliases = list_str_col(batch, "search_aliases");
    let related_memory_ids = list_str_col(batch, "related_memory_ids");
    let evidence_confidences = f32_col(batch, "evidence_confidence");
    let confidence_scores = f32_col(batch, "confidence_score");
    let importance_scores = f32_col(batch, "importance_score");
    let specificity_scores = f32_col(batch, "specificity_score");
    let intent_scores = f32_col(batch, "intent_score");
    let entity_scores = f32_col(batch, "entity_score");
    let agent_usefulness_scores = f32_col(batch, "agent_usefulness_score");
    let ocr_noise_scores = f32_col(batch, "ocr_noise_score");
    let screenshot_paths = str_col(batch, "screenshot_path");
    let urls = str_col(batch, "url");

    // LanceDB appends _distance column to vector search results.
    let dist_col = batch
        .column_by_name("_distance")
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>().cloned());
    let decay_scores = f32_col(batch, "decay_score");

    // V2 columns
    let schema_versions = u32_col(batch, "schema_version");
    let activity_types = str_col(batch, "activity_type");
    let files_touched = list_str_col(batch, "files_touched");
    let session_duration_mins = u32_col(batch, "session_duration_mins");
    let projects = str_col(batch, "project");
    let tags = list_str_col(batch, "tags");
    let entities = list_str_col(batch, "entities");
    let outcomes = str_col(batch, "outcome");
    let extraction_confidences = f32_col(batch, "extraction_confidence");
    let anchor_coverage_scores = f32_col(batch, "anchor_coverage_score");
    let content_hashes = str_col(batch, "content_hash");
    let dedup_fingerprints = str_col(batch, "dedup_fingerprint");
    let is_consolidated_flags = bool_col(batch, "is_consolidated");
    let is_soft_deleted_flags = bool_col(batch, "is_soft_deleted");

    (0..n)
        .map(|i| {
            let score = dist_col
                .as_ref()
                .map(|c| vector_distance_to_similarity(c.value(i)))
                .unwrap_or(1.0);
            SearchResult {
                id: get_str(&ids, i),
                timestamp: timestamps.as_ref().map(|c| c.value(i)).unwrap_or(0),
                app_name: get_str(&app_names, i),
                bundle_id: get_opt_str(&bundle_ids, i),
                window_title: get_str(&window_titles, i),
                session_id: get_str(&session_ids, i),
                text: get_str(&texts, i),
                clean_text: get_str(&clean_texts, i),
                ocr_confidence: get_f32(&ocr_confidences, i),
                ocr_block_count: get_i64(&ocr_block_counts, i).max(0) as u32,
                snippet: get_str(&snippets, i),
                display_summary: {
                    let summary = get_str(&display_summaries, i);
                    if summary.trim().is_empty() {
                        get_str(&snippets, i)
                    } else {
                        summary
                    }
                },
                internal_context: get_str(&internal_contexts, i),
                summary_source: get_str(&summary_sources, i),
                noise_score: get_f32(&noise_scores, i),
                session_key: get_str(&session_keys, i),
                lexical_shadow: get_str(&lexical_shadows, i),
                memory_context: {
                    let context = get_str(&memory_contexts, i);
                    if context.trim().is_empty() {
                        let internal = get_str(&internal_contexts, i);
                        if internal.trim().is_empty() {
                            get_str(&display_summaries, i)
                        } else {
                            internal
                        }
                    } else {
                        context
                    }
                },
                user_intent: get_str(&user_intents, i),
                topic: get_str(&topics, i),
                workflow: get_str(&workflows, i),
                search_aliases: extract_str_list(&search_aliases, i),
                related_memory_ids: extract_str_list(&related_memory_ids, i),
                evidence_confidence: get_f32(&evidence_confidences, i),
                confidence_score: get_f32(&confidence_scores, i),
                importance_score: get_f32(&importance_scores, i),
                specificity_score: get_f32(&specificity_scores, i),
                intent_score: get_f32(&intent_scores, i),
                entity_score: get_f32(&entity_scores, i),
                agent_usefulness_score: get_f32(&agent_usefulness_scores, i),
                ocr_noise_score: get_f32(&ocr_noise_scores, i),
                score,
                screenshot_path: get_opt_str(&screenshot_paths, i),
                url: get_opt_str(&urls, i),
                decay_score: get_f32(&decay_scores, i).max(0.15),
                schema_version: get_u32(&schema_versions, i),
                activity_type: get_str(&activity_types, i),
                files_touched: extract_str_list(&files_touched, i),
                session_duration_mins: get_u32(&session_duration_mins, i),
                project: get_str(&projects, i),
                tags: extract_str_list(&tags, i),
                outcome: get_str(&outcomes, i),
                extraction_confidence: get_f32(&extraction_confidences, i),
                anchor_coverage_score: get_f32(&anchor_coverage_scores, i).clamp(0.0, 1.0),
                extracted_entities: extract_str_list(&entities, i),
                content_hash: {
                    let hash = get_str(&content_hashes, i);
                    if hash.trim().is_empty() {
                        compute_content_hash(
                            get_opt_str(&urls, i).as_deref(),
                            &get_str(&window_titles, i),
                            timestamps.as_ref().map(|c| c.value(i)).unwrap_or(0),
                        )
                    } else {
                        hash
                    }
                },
                dedup_fingerprint: get_str(&dedup_fingerprints, i),
                is_consolidated: get_bool(&is_consolidated_flags, i),
                is_soft_deleted: get_bool(&is_soft_deleted_flags, i),
            }
        })
        .collect()
}

// ── Arrow column helpers ─────────────────────────────────────────────────────

fn str_col(batch: &RecordBatch, name: &str) -> Option<StringArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<StringArray>()
        .cloned()
}

fn i64_col(batch: &RecordBatch, name: &str) -> Option<Int64Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<Int64Array>()
        .cloned()
}

fn f32_col(batch: &RecordBatch, name: &str) -> Option<Float32Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<Float32Array>()
        .cloned()
}

fn bool_col(batch: &RecordBatch, name: &str) -> Option<BooleanArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<BooleanArray>()
        .cloned()
}

fn u32_col(batch: &RecordBatch, name: &str) -> Option<UInt32Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<UInt32Array>()
        .cloned()
}

fn list_str_col(batch: &RecordBatch, name: &str) -> Option<ListArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<ListArray>()
        .cloned()
}

fn get_str(col: &Option<StringArray>, i: usize) -> String {
    col.as_ref()
        .map(|c| c.value(i).to_string())
        .unwrap_or_default()
}

fn get_opt_str(col: &Option<StringArray>, i: usize) -> Option<String> {
    col.as_ref().and_then(|c| {
        if c.is_null(i) {
            None
        } else {
            Some(c.value(i).to_string())
        }
    })
}

fn get_non_empty_str(col: &Option<StringArray>, i: usize) -> Option<String> {
    get_opt_str(col, i).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn get_i64(col: &Option<Int64Array>, i: usize) -> i64 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0)
}

fn get_f32(col: &Option<Float32Array>, i: usize) -> f32 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0.0)
}

fn get_u32(col: &Option<UInt32Array>, i: usize) -> u32 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0)
}

fn extract_domain(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);

    let host_and_path = without_scheme.split('/').next().unwrap_or("");
    let without_credentials = host_and_path.rsplit('@').next().unwrap_or(host_and_path);
    let host = without_credentials.split(':').next().unwrap_or("").trim();
    if host.is_empty() {
        return None;
    }

    let host = host.to_ascii_lowercase();
    let normalized = host
        .strip_prefix("www.")
        .map(|h| h.to_string())
        .unwrap_or(host);

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn compute_activity_streaks(
    day_counts: &std::collections::HashMap<String, usize>,
) -> (usize, usize) {
    let mut days: Vec<NaiveDate> = day_counts
        .keys()
        .filter_map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .collect();

    if days.is_empty() {
        return (0, 0);
    }

    days.sort_unstable();
    days.dedup();

    let mut longest_streak = 1usize;
    let mut run = 1usize;
    for i in 1..days.len() {
        if days[i] == days[i - 1] + chrono::Duration::days(1) {
            run += 1;
        } else {
            run = 1;
        }
        longest_streak = longest_streak.max(run);
    }

    let mut current_streak = 1usize;
    for i in (1..days.len()).rev() {
        if days[i] == days[i - 1] + chrono::Duration::days(1) {
            current_streak += 1;
        } else {
            break;
        }
    }

    (current_streak, longest_streak)
}

fn get_bool(col: &Option<BooleanArray>, i: usize) -> bool {
    col.as_ref().map(|c| c.value(i)).unwrap_or(false)
}

fn get_opt_i64(col: &Option<Int64Array>, i: usize) -> Option<i64> {
    col.as_ref()
        .and_then(|c| if c.is_null(i) { None } else { Some(c.value(i)) })
}

fn extract_str_list(col: &Option<arrow_array::ListArray>, i: usize) -> Vec<String> {
    if let Some(list) = col {
        if let Some(values) = list
            .value(i)
            .as_any()
            .downcast_ref::<StringArray>()
            .cloned()
        {
            return (0..values.len())
                .map(|j| values.value(j).to_string())
                .collect();
        }
    }
    Vec::new()
}

fn extract_f32_list(col: &Option<FixedSizeListArray>, i: usize, dim: usize) -> Vec<f32> {
    if let Some(list) = col {
        if let Some(values) = list
            .value(i)
            .as_any()
            .downcast_ref::<Float32Array>()
            .cloned()
        {
            return (0..values.len()).map(|j| values.value(j)).collect();
        }
    }
    vec![0.0; dim]
}

fn vector_distance_to_similarity(distance: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }

    // Stored/query text vectors are L2-normalized. Lance returns a lower-is-better
    // L2-family distance, so compressing it with `0.01` made unrelated neighbors
    // look almost identical. This mapping preserves useful separation for both
    // squared-L2 and L2 distances in the normalized 0..4 range.
    if distance <= 4.0 {
        (1.0 - distance / 2.0).clamp(0.0, 1.0)
    } else {
        (1.0 / (1.0 + distance)).clamp(0.0, 1.0)
    }
}

// ── Arrow ↔ Task conversion ──────────────────────────────────────────────────

fn task_to_batch(tasks: &[Task]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(task_schema());
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
    let descriptions: Vec<&str> = tasks.iter().map(|t| t.description.as_str()).collect();
    let source_apps: Vec<&str> = tasks.iter().map(|t| t.source_app.as_str()).collect();
    let source_memory_ids: Vec<Option<&str>> = tasks
        .iter()
        .map(|t| t.source_memory_id.as_deref())
        .collect();
    let created_at: Vec<i64> = tasks.iter().map(|t| t.created_at).collect();
    let due_date: Vec<Option<i64>> = tasks.iter().map(|t| t.due_date).collect();
    let is_completed: Vec<bool> = tasks.iter().map(|t| t.is_completed).collect();
    let is_dismissed: Vec<bool> = tasks.iter().map(|t| t.is_dismissed).collect();
    let task_types: Vec<String> = tasks.iter().map(|t| format!("{:?}", t.task_type)).collect();

    // List columns
    let mut url_builder =
        arrow_array::builder::ListBuilder::new(arrow_array::builder::StringBuilder::new());
    let mut mem_id_builder =
        arrow_array::builder::ListBuilder::new(arrow_array::builder::StringBuilder::new());

    for t in tasks {
        for url in &t.linked_urls {
            url_builder.values().append_value(url);
        }
        url_builder.append(true);

        for mid in &t.linked_memory_ids {
            mem_id_builder.values().append_value(mid);
        }
        mem_id_builder.append(true);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(titles)),
            Arc::new(StringArray::from(descriptions)),
            Arc::new(StringArray::from(source_apps)),
            Arc::new(StringArray::from(source_memory_ids)),
            Arc::new(Int64Array::from(created_at)),
            Arc::new(Int64Array::from(due_date)),
            Arc::new(arrow_array::BooleanArray::from(is_completed)),
            Arc::new(arrow_array::BooleanArray::from(is_dismissed)),
            Arc::new(StringArray::from(task_types)),
            Arc::new(url_builder.finish()),
            Arc::new(mem_id_builder.finish()),
        ],
    )
}

fn nodes_to_batch(nodes: &[GraphNode]) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let mut ids = StringBuilder::new();
    let mut types = StringBuilder::new();
    let mut labels = StringBuilder::new();
    let mut created = Int64Builder::new();
    let mut metadata = StringBuilder::new();

    for n in nodes {
        ids.append_value(&n.id);
        types.append_value(match n.node_type {
            NodeType::Entity => "Entity",
            NodeType::Task => "Task",
            NodeType::Url => "Url",
            NodeType::MemoryChunk => "MemoryChunk",
            NodeType::Clipboard => "Clipboard",
            NodeType::AudioSegment => "AudioSegment",
            NodeType::Project => "Project",
            NodeType::File => "File",
            NodeType::Error => "Error",
            NodeType::Command => "Command",
            NodeType::Decision => "Decision",
            NodeType::AgentSession => "AgentSession",
            NodeType::ActivityEvent => "ActivityEvent",
            NodeType::Issue => "Issue",
            NodeType::Concept => "Concept",
        });
        labels.append_value(&n.label);
        created.append_value(n.created_at);
        metadata.append_value(serde_json::to_string(&n.metadata).unwrap_or_default());
    }

    RecordBatch::try_new(
        Arc::new(node_schema()),
        vec![
            Arc::new(ids.finish()),
            Arc::new(types.finish()),
            Arc::new(labels.finish()),
            Arc::new(created.finish()),
            Arc::new(metadata.finish()),
        ],
    )
    .map_err(|e| e.into())
}

fn edges_to_batch(edges: &[GraphEdge]) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let mut ids = StringBuilder::new();
    let mut sources = StringBuilder::new();
    let mut targets = StringBuilder::new();
    let mut types = StringBuilder::new();
    let mut timestamps = Int64Builder::new();
    let mut metadata = StringBuilder::new();

    for e in edges {
        ids.append_value(&e.id);
        sources.append_value(&e.source);
        targets.append_value(&e.target);
        types.append_value(match e.edge_type {
            edge_type => edge_type_literal(edge_type),
        });
        timestamps.append_value(e.timestamp);
        metadata.append_value(serde_json::to_string(&e.metadata).unwrap_or_default());
    }

    RecordBatch::try_new(
        Arc::new(edge_schema()),
        vec![
            Arc::new(ids.finish()),
            Arc::new(sources.finish()),
            Arc::new(targets.finish()),
            Arc::new(types.finish()),
            Arc::new(timestamps.finish()),
            Arc::new(metadata.finish()),
        ],
    )
    .map_err(|e| e.into())
}

fn batch_to_nodes(batch: &RecordBatch) -> Vec<GraphNode> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let types = str_col(batch, "node_type");
    let labels = str_col(batch, "label");
    let created = i64_col(batch, "created_at");
    let meta = str_col(batch, "metadata_json");

    let mut nodes = Vec::with_capacity(n);
    for i in 0..n {
        let node_type = match get_str(&types, i).as_str() {
            "Entity" => NodeType::Entity,
            "Task" => NodeType::Task,
            "Url" => NodeType::Url,
            "Clipboard" => NodeType::Clipboard,
            "AudioSegment" => NodeType::AudioSegment,
            "Project" => NodeType::Project,
            "File" => NodeType::File,
            "Error" => NodeType::Error,
            "Command" => NodeType::Command,
            "Decision" => NodeType::Decision,
            "AgentSession" => NodeType::AgentSession,
            "ActivityEvent" => NodeType::ActivityEvent,
            "Issue" => NodeType::Issue,
            "Concept" => NodeType::Concept,
            _ => NodeType::MemoryChunk,
        };
        nodes.push(GraphNode {
            id: get_str(&ids, i),
            node_type,
            label: get_str(&labels, i),
            created_at: get_i64(&created, i),
            metadata: serde_json::from_str(&get_str(&meta, i)).unwrap_or_default(),
        });
    }
    nodes
}

fn batch_to_edges(batch: &RecordBatch) -> Vec<GraphEdge> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let sources = str_col(batch, "source");
    let targets = str_col(batch, "target");
    let types = str_col(batch, "edge_type");
    let ts = i64_col(batch, "timestamp");
    let meta = str_col(batch, "metadata_json");

    let mut edges = Vec::with_capacity(n);
    for i in 0..n {
        let edge_type = match get_str(&types, i).as_str() {
            "PART_OF_SESSION" | "PartOfSession" => EdgeType::PartOfSession,
            "REFERENCE_FOR_TASK" | "ReferenceForTask" | "References" => EdgeType::ReferenceForTask,
            "CLIPBOARD_COPIED" | "ClipboardCopied" => EdgeType::ClipboardCopied,
            "OCCURRED_DURING_AUDIO" | "OccurredDuringAudio" => EdgeType::OccurredDuringAudio,
            "BELONGS_TO" | "BelongsTo" => EdgeType::BelongsTo,
            "MENTIONED_IN" | "MentionedIn" => EdgeType::MentionedIn,
            "EDITED_FILE" | "EditedFile" => EdgeType::EditedFile,
            "FIXED_BY" | "FixedBy" => EdgeType::FixedBy,
            "BLOCKED_BY" | "BlockedBy" => EdgeType::BlockedBy,
            "INFORMED_BY" | "InformedBy" => EdgeType::InformedBy,
            "RESULTED_IN" | "ResultedIn" => EdgeType::ResultedIn,
            "OCCURRED_AT" | "OccurredAt" | "LinkedTo" => EdgeType::OccurredAt,
            _ => EdgeType::OccurredAt,
        };
        edges.push(GraphEdge {
            id: get_str(&ids, i),
            source: get_str(&sources, i),
            target: get_str(&targets, i),
            edge_type,
            timestamp: get_i64(&ts, i),
            metadata: serde_json::from_str(&get_str(&meta, i)).unwrap_or_default(),
        });
    }
    edges
}

fn activity_events_to_batch(events: &[ActivityEvent]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(activity_event_schema());
    let ids: Vec<&str> = events.iter().map(|event| event.id.as_str()).collect();
    let memory_ids: Vec<&str> = events
        .iter()
        .map(|event| event.memory_id.as_str())
        .collect();
    let projects: Vec<Option<&str>> = events
        .iter()
        .map(|event| event.project.as_deref())
        .collect();
    let activity_types: Vec<&str> = events
        .iter()
        .map(|event| event.activity_type.as_str())
        .collect();
    let titles: Vec<&str> = events.iter().map(|event| event.title.as_str()).collect();
    let summaries: Vec<&str> = events.iter().map(|event| event.summary.as_str()).collect();
    let starts: Vec<i64> = events.iter().map(|event| event.start_time).collect();
    let ends: Vec<i64> = events.iter().map(|event| event.end_time).collect();
    let privacy: Vec<String> = events
        .iter()
        .map(|event| privacy_class_literal(&event.privacy_class).to_string())
        .collect();
    let payloads: Vec<String> = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(memory_ids)),
            Arc::new(StringArray::from(projects)),
            Arc::new(StringArray::from(activity_types)),
            Arc::new(StringArray::from(titles)),
            Arc::new(StringArray::from(summaries)),
            Arc::new(Int64Array::from(starts)),
            Arc::new(Int64Array::from(ends)),
            Arc::new(StringArray::from(privacy)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_activity_events(batch: &RecordBatch) -> Vec<ActivityEvent> {
    let payloads = str_col(batch, "payload_json");
    let mut events = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(event) = serde_json::from_str::<ActivityEvent>(payloads.value(i)) {
                events.push(event);
            }
        }
    }
    events
}

fn project_contexts_to_batch(contexts: &[ProjectContext]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(project_context_schema());
    let ids: Vec<&str> = contexts.iter().map(|context| context.id.as_str()).collect();
    let projects: Vec<&str> = contexts
        .iter()
        .map(|context| context.project.as_str())
        .collect();
    let updated_at: Vec<i64> = contexts.iter().map(|context| context.updated_at).collect();
    let summaries: Vec<&str> = contexts
        .iter()
        .map(|context| context.summary.as_str())
        .collect();
    let privacy: Vec<String> = contexts
        .iter()
        .map(|context| privacy_class_literal(&context.privacy_class).to_string())
        .collect();
    let payloads: Vec<String> = contexts
        .iter()
        .map(|context| serde_json::to_string(context).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(projects)),
            Arc::new(Int64Array::from(updated_at)),
            Arc::new(StringArray::from(summaries)),
            Arc::new(StringArray::from(privacy)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_project_contexts(batch: &RecordBatch) -> Vec<ProjectContext> {
    let payloads = str_col(batch, "payload_json");
    let mut contexts = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(context) = serde_json::from_str::<ProjectContext>(payloads.value(i)) {
                contexts.push(context);
            }
        }
    }
    contexts
}

fn decision_ledger_to_batch(entries: &[DecisionLedgerEntry]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(decision_ledger_schema());
    let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    let projects: Vec<Option<&str>> = entries
        .iter()
        .map(|entry| entry.project.as_deref())
        .collect();
    let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
    let statuses: Vec<&str> = entries.iter().map(|entry| entry.status.as_str()).collect();
    let created_at: Vec<i64> = entries.iter().map(|entry| entry.created_at).collect();
    let privacy: Vec<String> = entries
        .iter()
        .map(|entry| privacy_class_literal(&entry.privacy_class).to_string())
        .collect();
    let payloads: Vec<String> = entries
        .iter()
        .map(|entry| serde_json::to_string(entry).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(projects)),
            Arc::new(StringArray::from(titles)),
            Arc::new(StringArray::from(statuses)),
            Arc::new(Int64Array::from(created_at)),
            Arc::new(StringArray::from(privacy)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_decision_ledger_entries(batch: &RecordBatch) -> Vec<DecisionLedgerEntry> {
    let payloads = str_col(batch, "payload_json");
    let mut entries = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(entry) = serde_json::from_str::<DecisionLedgerEntry>(payloads.value(i)) {
                entries.push(entry);
            }
        }
    }
    entries
}

fn context_packs_to_batch(packs: &[ContextPack]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(context_pack_schema());
    let ids: Vec<&str> = packs.iter().map(|pack| pack.id.as_str()).collect();
    let session_ids: Vec<Option<&str>> = packs
        .iter()
        .map(|pack| pack.session_id.as_deref())
        .collect();
    let projects: Vec<Option<&str>> = packs.iter().map(|pack| pack.project.as_deref()).collect();
    let agent_types: Vec<&str> = packs.iter().map(|pack| pack.agent_type.as_str()).collect();
    let generated_at: Vec<i64> = packs.iter().map(|pack| pack.generated_at).collect();
    let budget_tokens: Vec<u32> = packs.iter().map(|pack| pack.budget_tokens).collect();
    let tokens_used: Vec<u32> = packs.iter().map(|pack| pack.tokens_used).collect();
    let summaries: Vec<&str> = packs.iter().map(|pack| pack.summary.as_str()).collect();
    let payloads: Vec<String> = packs
        .iter()
        .map(|pack| serde_json::to_string(pack).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(session_ids)),
            Arc::new(StringArray::from(projects)),
            Arc::new(StringArray::from(agent_types)),
            Arc::new(Int64Array::from(generated_at)),
            Arc::new(UInt32Array::from(budget_tokens)),
            Arc::new(UInt32Array::from(tokens_used)),
            Arc::new(StringArray::from(summaries)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_context_packs(batch: &RecordBatch) -> Vec<ContextPack> {
    let payloads = str_col(batch, "payload_json");
    let mut packs = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(pack) = serde_json::from_str::<ContextPack>(payloads.value(i)) {
                packs.push(pack);
            }
        }
    }
    packs
}

fn context_deltas_to_batch(deltas: &[ContextDelta]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(context_delta_schema());
    let ids: Vec<&str> = deltas.iter().map(|delta| delta.id.as_str()).collect();
    let session_ids: Vec<&str> = deltas
        .iter()
        .map(|delta| delta.session_id.as_str())
        .collect();
    let since: Vec<i64> = deltas.iter().map(|delta| delta.since).collect();
    let generated_at: Vec<i64> = deltas.iter().map(|delta| delta.generated_at).collect();
    let tokens_used: Vec<u32> = deltas.iter().map(|delta| delta.tokens_used).collect();
    let payloads: Vec<String> = deltas
        .iter()
        .map(|delta| serde_json::to_string(delta).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(session_ids)),
            Arc::new(Int64Array::from(since)),
            Arc::new(Int64Array::from(generated_at)),
            Arc::new(UInt32Array::from(tokens_used)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_context_deltas(batch: &RecordBatch) -> Vec<ContextDelta> {
    let payloads = str_col(batch, "payload_json");
    let mut deltas = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(delta) = serde_json::from_str::<ContextDelta>(payloads.value(i)) {
                deltas.push(delta);
            }
        }
    }
    deltas
}

fn entity_aliases_to_batch(aliases: &[EntityAliasRecord]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(entity_alias_schema());
    let alias_keys: Vec<&str> = aliases
        .iter()
        .map(|alias| alias.alias_key.as_str())
        .collect();
    let canonical_ids: Vec<&str> = aliases
        .iter()
        .map(|alias| alias.canonical_id.as_str())
        .collect();
    let canonical_names: Vec<&str> = aliases
        .iter()
        .map(|alias| alias.canonical_name.as_str())
        .collect();
    let entity_types: Vec<&str> = aliases
        .iter()
        .map(|alias| alias.entity_type.as_str())
        .collect();
    let projects: Vec<Option<&str>> = aliases
        .iter()
        .map(|alias| alias.project.as_deref())
        .collect();
    let confidence: Vec<f32> = aliases.iter().map(|alias| alias.confidence).collect();
    let updated_at: Vec<i64> = aliases.iter().map(|alias| alias.updated_at).collect();
    let payloads: Vec<String> = aliases
        .iter()
        .map(|alias| serde_json::to_string(alias).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(alias_keys)),
            Arc::new(StringArray::from(canonical_ids)),
            Arc::new(StringArray::from(canonical_names)),
            Arc::new(StringArray::from(entity_types)),
            Arc::new(StringArray::from(projects)),
            Arc::new(Float32Array::from(confidence)),
            Arc::new(Int64Array::from(updated_at)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_entity_aliases(batch: &RecordBatch) -> Vec<EntityAliasRecord> {
    let payloads = str_col(batch, "payload_json");
    let mut aliases = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(alias) = serde_json::from_str::<EntityAliasRecord>(payloads.value(i)) {
                aliases.push(alias);
            }
        }
    }
    aliases
}

fn knowledge_page_type_literal(value: &super::schema::KnowledgePageType) -> &'static str {
    match value {
        super::schema::KnowledgePageType::ProjectPage => "project_page",
        super::schema::KnowledgePageType::TopicPage => "topic_page",
        super::schema::KnowledgePageType::ClaimPage => "claim_page",
        super::schema::KnowledgePageType::DecisionPage => "decision_page",
        super::schema::KnowledgePageType::PatternPage => "pattern_page",
        super::schema::KnowledgePageType::BreakthroughPage => "breakthrough_page",
        super::schema::KnowledgePageType::ContradictionPage => "contradiction_page",
        super::schema::KnowledgePageType::FrameworkPage => "framework_page",
    }
}

fn knowledge_stability_literal(value: &super::schema::KnowledgeStability) -> &'static str {
    match value {
        super::schema::KnowledgeStability::Emerging => "emerging",
        super::schema::KnowledgeStability::Stable => "stable",
        super::schema::KnowledgeStability::Contradicted => "contradicted",
        super::schema::KnowledgeStability::Deprecated => "deprecated",
    }
}

fn knowledge_pages_to_batch(pages: &[KnowledgePage]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(knowledge_page_schema());
    let ids: Vec<&str> = pages.iter().map(|page| page.page_id.as_str()).collect();
    let types: Vec<&str> = pages
        .iter()
        .map(|page| knowledge_page_type_literal(&page.page_type))
        .collect();
    let projects: Vec<Option<&str>> = pages.iter().map(|page| page.project.as_deref()).collect();
    let topics: Vec<Option<&str>> = pages.iter().map(|page| page.topic.as_deref()).collect();
    let titles: Vec<&str> = pages.iter().map(|page| page.title.as_str()).collect();
    let stability: Vec<&str> = pages
        .iter()
        .map(|page| knowledge_stability_literal(&page.stability))
        .collect();
    let last_updated: Vec<i64> = pages.iter().map(|page| page.last_updated).collect();
    let confidence: Vec<f32> = pages.iter().map(|page| page.confidence_score).collect();
    let payloads: Vec<String> = pages
        .iter()
        .map(|page| serde_json::to_string(page).unwrap_or_default())
        .collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(types)),
            Arc::new(StringArray::from(projects)),
            Arc::new(StringArray::from(topics)),
            Arc::new(StringArray::from(titles)),
            Arc::new(StringArray::from(stability)),
            Arc::new(Int64Array::from(last_updated)),
            Arc::new(Float32Array::from(confidence)),
            Arc::new(StringArray::from(payloads)),
        ],
    )
}

fn batch_to_knowledge_pages(batch: &RecordBatch) -> Vec<KnowledgePage> {
    let payloads = str_col(batch, "payload_json");
    let mut pages = Vec::new();
    if let Some(payloads) = payloads {
        for i in 0..batch.num_rows() {
            if let Ok(page) = serde_json::from_str::<KnowledgePage>(payloads.value(i)) {
                pages.push(page);
            }
        }
    }
    pages
}

fn privacy_class_literal(value: &super::schema::PrivacyClass) -> &'static str {
    match value {
        super::schema::PrivacyClass::Public => "public",
        super::schema::PrivacyClass::Project => "project",
        super::schema::PrivacyClass::Personal => "personal",
        super::schema::PrivacyClass::Sensitive => "sensitive",
        super::schema::PrivacyClass::Secret => "secret",
        super::schema::PrivacyClass::Blocked => "blocked",
        super::schema::PrivacyClass::Ephemeral => "ephemeral",
    }
}

async fn count_table_rows(table: &Table) -> Result<usize, Box<dyn std::error::Error>> {
    let batches = table
        .query()
        .execute()
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(batches.into_iter().map(|batch| batch.num_rows()).sum())
}

fn batch_to_meetings(batch: &RecordBatch) -> Vec<MeetingSession> {
    let n = batch.num_rows();
    let id = str_col(batch, "id");
    let title = str_col(batch, "title");
    let participants = batch
        .column_by_name("participants")
        .and_then(|c| c.as_any().downcast_ref::<arrow_array::ListArray>().cloned());
    let model = str_col(batch, "model");
    let status = str_col(batch, "status");
    let start = i64_col(batch, "start_timestamp");
    let end = i64_col(batch, "end_timestamp");
    let created = i64_col(batch, "created_at");
    let updated = i64_col(batch, "updated_at");
    let segment_count = i64_col(batch, "segment_count");
    let duration = i64_col(batch, "duration_seconds");
    let mdir = str_col(batch, "meeting_dir");
    let adir = str_col(batch, "audio_dir");
    let tpath = str_col(batch, "transcript_path");
    let breakdown = str_col(batch, "breakdown_json");

    let mut results = Vec::with_capacity(n);
    for i in 0..n {
        results.push(MeetingSession {
            id: get_str(&id, i),
            title: get_str(&title, i),
            participants: extract_str_list(&participants, i),
            model: get_str(&model, i),
            status: get_str(&status, i),
            start_timestamp: get_i64(&start, i),
            end_timestamp: Some(get_i64(&end, i)).filter(|t| *t > 0),
            created_at: get_i64(&created, i),
            updated_at: get_i64(&updated, i),
            segment_count: get_i64(&segment_count, i) as usize,
            duration_seconds: get_i64(&duration, i) as u64,
            meeting_dir: get_str(&mdir, i),
            audio_dir: get_str(&adir, i),
            transcript_path: Some(get_str(&tpath, i)).filter(|s| !s.is_empty()),
            breakdown: serde_json::from_str(&get_str(&breakdown, i)).ok(),
        });
    }
    results
}

fn batch_to_segments(batch: &RecordBatch) -> Vec<MeetingSegment> {
    let n = batch.num_rows();
    let id = str_col(batch, "id");
    let mid = str_col(batch, "meeting_id");
    let index = u32_col(batch, "index");
    let start = i64_col(batch, "start_timestamp");
    let end = i64_col(batch, "end_timestamp");
    let text = str_col(batch, "text");
    let audio = str_col(batch, "audio_chunk_path");
    let model = str_col(batch, "model");
    let created = i64_col(batch, "created_at");

    let mut results = Vec::with_capacity(n);
    for i in 0..n {
        results.push(MeetingSegment {
            id: get_str(&id, i),
            meeting_id: get_str(&mid, i),
            index: get_u32(&index, i),
            start_timestamp: get_i64(&start, i),
            end_timestamp: get_i64(&end, i),
            text: get_str(&text, i),
            audio_chunk_path: get_str(&audio, i),
            model: get_str(&model, i),
            created_at: get_i64(&created, i),
        });
    }
    results
}

fn batch_to_tasks(batch: &RecordBatch) -> Vec<Task> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let titles = str_col(batch, "title");
    let descriptions = str_col(batch, "description");
    let source_apps = str_col(batch, "source_app");
    let source_memory_ids = str_col(batch, "source_memory_id");
    let created_at = i64_col(batch, "created_at");
    let due_date = i64_col(batch, "due_date");
    let is_completed = bool_col(batch, "is_completed");
    let is_dismissed = bool_col(batch, "is_dismissed");
    let task_types = str_col(batch, "task_type");

    let url_col = batch
        .column_by_name("linked_urls")
        .and_then(|c| c.as_any().downcast_ref::<arrow_array::ListArray>().cloned());
    let mem_id_col = batch
        .column_by_name("linked_memory_ids")
        .and_then(|c| c.as_any().downcast_ref::<arrow_array::ListArray>().cloned());

    (0..n)
        .map(|i| {
            let t_type = match get_str(&task_types, i).as_str() {
                "Reminder" => TaskType::Reminder,
                "Followup" => TaskType::Followup,
                _ => TaskType::Todo,
            };

            Task {
                id: get_str(&ids, i),
                title: get_str(&titles, i),
                description: get_str(&descriptions, i),
                source_app: get_str(&source_apps, i),
                source_memory_id: get_opt_str(&source_memory_ids, i),
                created_at: get_i64(&created_at, i),
                due_date: get_opt_i64(&due_date, i),
                is_completed: get_bool(&is_completed, i),
                is_dismissed: get_bool(&is_dismissed, i),
                task_type: t_type,
                linked_urls: extract_str_list(&url_col, i),
                linked_memory_ids: extract_str_list(&mem_id_col, i),
            }
        })
        .collect()
}

// ── Arrow ↔ Meeting conversion ───────────────────────────────────────────────

fn meeting_to_batch(meetings: &[MeetingSession]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(meeting_schema());
    let ids: Vec<&str> = meetings.iter().map(|m| m.id.as_str()).collect();
    let titles: Vec<&str> = meetings.iter().map(|m| m.title.as_str()).collect();
    let models: Vec<&str> = meetings.iter().map(|m| m.model.as_str()).collect();
    let statuses: Vec<&str> = meetings.iter().map(|m| m.status.as_str()).collect();
    let starts: Vec<i64> = meetings.iter().map(|m| m.start_timestamp).collect();
    let ends: Vec<Option<i64>> = meetings.iter().map(|m| m.end_timestamp).collect();
    let created: Vec<i64> = meetings.iter().map(|m| m.created_at).collect();
    let updated: Vec<i64> = meetings.iter().map(|m| m.updated_at).collect();
    let counts: Vec<i64> = meetings.iter().map(|m| m.segment_count as i64).collect();
    let durations: Vec<i64> = meetings.iter().map(|m| m.duration_seconds as i64).collect();
    let meeting_dirs: Vec<&str> = meetings.iter().map(|m| m.meeting_dir.as_str()).collect();
    let audio_dirs: Vec<&str> = meetings.iter().map(|m| m.audio_dir.as_str()).collect();
    let transcript_paths: Vec<Option<&str>> = meetings
        .iter()
        .map(|m| m.transcript_path.as_deref())
        .collect();
    let breakdowns: Vec<Option<String>> = meetings
        .iter()
        .map(|m| {
            m.breakdown
                .as_ref()
                .and_then(|b| serde_json::to_string(b).ok())
        })
        .collect();

    let mut participants_builder =
        arrow_array::builder::ListBuilder::new(arrow_array::builder::StringBuilder::new());
    for m in meetings {
        for p in &m.participants {
            participants_builder.values().append_value(p);
        }
        participants_builder.append(true);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(titles)),
            Arc::new(participants_builder.finish()),
            Arc::new(StringArray::from(models)),
            Arc::new(StringArray::from(statuses)),
            Arc::new(Int64Array::from(starts)),
            Arc::new(Int64Array::from(ends)),
            Arc::new(Int64Array::from(created)),
            Arc::new(Int64Array::from(updated)),
            Arc::new(Int64Array::from(counts)),
            Arc::new(Int64Array::from(durations)),
            Arc::new(StringArray::from(meeting_dirs)),
            Arc::new(StringArray::from(audio_dirs)),
            Arc::new(StringArray::from(transcript_paths)),
            Arc::new(StringArray::from(breakdowns)),
        ],
    )
}

fn segment_to_batch(segments: &[MeetingSegment]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(segment_schema());
    let ids: Vec<&str> = segments.iter().map(|s| s.id.as_str()).collect();
    let m_ids: Vec<&str> = segments.iter().map(|s| s.meeting_id.as_str()).collect();
    let indices: Vec<u32> = segments.iter().map(|s| s.index).collect();
    let starts: Vec<i64> = segments.iter().map(|s| s.start_timestamp).collect();
    let ends: Vec<i64> = segments.iter().map(|s| s.end_timestamp).collect();
    let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
    let paths: Vec<&str> = segments
        .iter()
        .map(|s| s.audio_chunk_path.as_str())
        .collect();
    let models: Vec<&str> = segments.iter().map(|s| s.model.as_str()).collect();
    let created: Vec<i64> = segments.iter().map(|s| s.created_at).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(m_ids)),
            Arc::new(arrow_array::UInt32Array::from(indices)),
            Arc::new(Int64Array::from(starts)),
            Arc::new(Int64Array::from(ends)),
            Arc::new(StringArray::from(texts)),
            Arc::new(StringArray::from(paths)),
            Arc::new(StringArray::from(models)),
            Arc::new(Int64Array::from(created)),
        ],
    )
}

// ── Arrow ↔ Graph conversion ─────────────────────────────────────────────────

fn node_to_batch(nodes: &[GraphNode]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(node_schema());
    let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let types: Vec<String> = nodes.iter().map(|n| format!("{:?}", n.node_type)).collect();
    let labels: Vec<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
    let created: Vec<i64> = nodes.iter().map(|n| n.created_at).collect();
    let metadata: Vec<String> = nodes.iter().map(|n| n.metadata.to_string()).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(types)),
            Arc::new(StringArray::from(labels)),
            Arc::new(Int64Array::from(created)),
            Arc::new(StringArray::from(metadata)),
        ],
    )
}

fn edge_to_batch(edges: &[GraphEdge]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(edge_schema());
    let ids: Vec<&str> = edges.iter().map(|e| e.id.as_str()).collect();
    let sources: Vec<&str> = edges.iter().map(|e| e.source.as_str()).collect();
    let targets: Vec<&str> = edges.iter().map(|e| e.target.as_str()).collect();
    let types: Vec<String> = edges.iter().map(|e| format!("{:?}", e.edge_type)).collect();
    let timestamps: Vec<i64> = edges.iter().map(|e| e.timestamp).collect();
    let metadata: Vec<String> = edges.iter().map(|e| e.metadata.to_string()).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(StringArray::from(sources)),
            Arc::new(StringArray::from(targets)),
            Arc::new(StringArray::from(types)),
            Arc::new(Int64Array::from(timestamps)),
            Arc::new(StringArray::from(metadata)),
        ],
    )
}

fn build_filter(time_filter: Option<&str>, app_filter: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(tf) = time_filter.and_then(time_filter_to_sql) {
        parts.push(tf);
    }
    if let Some(app) = app_filter {
        parts.push(format!("app_name = '{}'", sql_escape(app)));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" AND "))
    }
}

fn time_filter_to_sql(tf: &str) -> Option<String> {
    use chrono::Duration;
    let now = chrono::Utc::now();
    match tf {
        "1h" => Some(format!(
            "timestamp >= {}",
            (now - Duration::hours(1)).timestamp_millis()
        )),
        "24h" => Some(format!(
            "timestamp >= {}",
            (now - Duration::hours(24)).timestamp_millis()
        )),
        "7d" | "week" => Some(format!(
            "timestamp >= {}",
            (now - Duration::days(7)).timestamp_millis()
        )),
        "today" => local_day_range_filter(0),
        "yesterday" => local_day_range_filter(1),
        _ => None,
    }
}

fn local_day_bucket_now() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn local_day_bucket_from_timestamp(timestamp: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}

fn local_day_range_filter(days_ago: i64) -> Option<String> {
    let target_day = Local::now().date_naive() - chrono::Duration::days(days_ago);
    let start = target_day.and_hms_opt(0, 0, 0)?;
    let end = (target_day + chrono::Duration::days(1)).and_hms_opt(0, 0, 0)?;

    let start_ms = Local
        .from_local_datetime(&start)
        .earliest()
        .or_else(|| Local.from_local_datetime(&start).latest())?
        .timestamp_millis();
    let end_ms = Local
        .from_local_datetime(&end)
        .earliest()
        .or_else(|| Local.from_local_datetime(&end).latest())?
        .timestamp_millis();

    Some(format!(
        "timestamp >= {} AND timestamp < {}",
        start_ms, end_ms
    ))
}

fn normalize_keyword_text(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out = value.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn is_keyword_stop_word(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "for"
            | "from"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "who"
            | "why"
            | "with"
    )
}

fn keyword_terms(query: &str) -> Vec<String> {
    let normalized = normalize_keyword_text(query);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |value: String| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        if seen.insert(trimmed.to_string()) {
            terms.push(trimmed.to_string());
        }
    };

    // Keep the normalized query as a phrase candidate first.
    push(normalized.clone());

    for token in normalized.split_whitespace() {
        if token.len() <= 1 {
            continue;
        }
        if is_keyword_stop_word(token) && !token.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        push(token.to_string());

        if token.len() >= 3 {
            push(token[..3].to_string());
        }
        if token.len() >= 4 {
            push(token[..4].to_string());
        }
        if token.len() >= 5 {
            push(drop_middle_char(token));
        }
        if token.len() >= 6 {
            for gram in char_ngrams(token, 3).into_iter().take(3) {
                push(gram);
            }
        }
    }

    terms.truncate(24);
    terms
}

fn drop_middle_char(token: &str) -> String {
    let mut chars = token.chars().collect::<Vec<_>>();
    if chars.len() <= 3 {
        return token.to_string();
    }
    chars.remove(chars.len() / 2);
    chars.into_iter().collect()
}

fn char_ngrams(token: &str, n: usize) -> Vec<String> {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() < n {
        return Vec::new();
    }
    let mut grams = Vec::new();
    for idx in 0..=(chars.len() - n) {
        grams.push(chars[idx..idx + n].iter().collect());
    }
    grams
}

fn lexical_keyword_score(terms: &[String], result: &SearchResult) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }

    let title = normalize_keyword_text(&result.window_title);
    let snippet = normalize_keyword_text(&result.snippet);
    let memory_context = normalize_keyword_text(&result.memory_context);
    let lexical_shadow = normalize_keyword_text(&result.lexical_shadow);
    let alias_blob = normalize_keyword_text(&result.search_aliases.join(" "));
    let clean = normalize_keyword_text(if !result.clean_text.trim().is_empty() {
        &result.clean_text
    } else {
        &result.text
    });
    let app = normalize_keyword_text(&result.app_name);
    let url = result
        .url
        .as_ref()
        .map(|value| normalize_keyword_text(value))
        .unwrap_or_default();
    let merged = format!(
        "{} {} {} {} {} {} {}",
        title, snippet, memory_context, clean, lexical_shadow, alias_blob, url
    );

    let mut matched_terms = 0usize;
    let mut weighted = 0.0f32;

    for (idx, term) in terms.iter().enumerate() {
        let mut matched = false;
        if title.contains(term) {
            weighted += 1.8;
            matched = true;
        }
        if snippet.contains(term) {
            weighted += 1.35;
            matched = true;
        }
        if clean.contains(term) {
            weighted += 1.1;
            matched = true;
        }
        if memory_context.contains(term) {
            weighted += 1.25;
            matched = true;
        }
        if lexical_shadow.contains(term) {
            weighted += 1.05;
            matched = true;
        }
        if alias_blob.contains(term) {
            weighted += 1.0;
            matched = true;
        }
        if app.contains(term) {
            weighted += 0.75;
            matched = true;
        }
        if !url.is_empty() && url.contains(term) {
            weighted += 0.95;
            matched = true;
        }

        // Reward full sentence/phrase hits for sentence queries.
        if idx == 0 && term.split_whitespace().count() >= 2 && merged.contains(term) {
            weighted += 1.1;
            matched = true;
        }

        if matched {
            matched_terms += 1;
        }
    }

    let coverage = matched_terms as f32 / terms.len() as f32;
    let normalized = (weighted / (terms.len() as f32 * 2.8)).min(1.0);
    (normalized * 0.7 + coverage * 0.3).clamp(0.0, 1.0)
}

fn recency_score(now_ms: i64, timestamp_ms: i64) -> f32 {
    let age_hours = ((now_ms - timestamp_ms).max(0) as f32 / 3_600_000.0).min(24.0 * 30.0);
    (1.0 / (1.0 + age_hours * 0.03)).clamp(0.0, 1.0)
}

fn estimate_signal_strength(
    summary_source: &str,
    ocr_confidence: f32,
    noise_score: f32,
    snippet: &str,
    clean_text: &str,
) -> f32 {
    let summary_weight = match summary_source.trim().to_ascii_lowercase().as_str() {
        "llm" => 1.0,
        "vlm" => 0.9,
        "fallback" => 0.66,
        _ => 0.58,
    };
    let snippet_density = (normalize_keyword_text(snippet)
        .split_whitespace()
        .count()
        .min(24) as f32
        / 24.0)
        .clamp(0.0, 1.0);
    let text_density = (normalize_keyword_text(clean_text)
        .split_whitespace()
        .count()
        .min(80) as f32
        / 80.0)
        .clamp(0.0, 1.0);

    (ocr_confidence.clamp(0.0, 1.0) * 0.24
        + (1.0 - noise_score.clamp(0.0, 1.0)) * 0.28
        + summary_weight * 0.18
        + snippet_density * 0.16
        + text_density * 0.14)
        .clamp(0.0, 1.0)
}

fn estimate_record_signal_strength(record: &MemoryRecord) -> f32 {
    estimate_signal_strength(
        &record.summary_source,
        record.ocr_confidence,
        record.noise_score,
        &record.snippet,
        &record.clean_text,
    )
}

fn normalize_record_for_index(record: &MemoryRecord) -> MemoryRecord {
    let lexical_shadow = if record.lexical_shadow.trim().is_empty() {
        build_lexical_shadow(
            &record.window_title,
            &record.snippet,
            &record.clean_text,
            record.url.as_deref(),
        )
    } else {
        record.lexical_shadow.clone()
    };
    let mut normalized = compact_memory_record_payload(record);
    normalized.url = sanitize_index_url(
        normalized.url.as_deref(),
        &normalized.window_title,
        &normalized.snippet,
    );
    normalized.session_key = build_index_session_key(&normalized);
    normalized.lexical_shadow = lexical_shadow;
    normalized.embedding = normalize_vector_dim(
        &normalized.id,
        "embedding",
        &normalized.embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.snippet_embedding = normalize_vector_dim(
        &normalized.id,
        "snippet_embedding",
        &normalized.snippet_embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.support_embedding = normalize_vector_dim(
        &normalized.id,
        "support_embedding",
        &normalized.support_embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.image_embedding = normalize_vector_dim(
        &normalized.id,
        "image_embedding",
        &normalized.image_embedding,
        IMAGE_EMBED_DIM as usize,
    );
    if normalized.display_summary.trim().is_empty() {
        normalized.display_summary = normalized.snippet.clone();
    }
    if normalized.internal_context.trim().is_empty() {
        normalized.internal_context = normalized.clean_text.clone();
    }
    normalized.clean_text = strip_low_conf_markers(&normalized.clean_text);
    normalized.snippet = strip_low_conf_markers(&normalized.snippet);
    normalized.display_summary = strip_low_conf_markers(&normalized.display_summary);
    normalized.internal_context = strip_low_conf_markers(&normalized.internal_context);
    normalized.memory_context = strip_low_conf_markers(&normalized.memory_context);
    normalized.embedding_text = strip_low_conf_markers(&normalized.embedding_text);
    if normalized.timestamp_start <= 0 {
        normalized.timestamp_start = normalized.timestamp;
    }
    if normalized.timestamp_end <= 0 {
        normalized.timestamp_end = normalized.timestamp;
    }
    if normalized.source_type.trim().is_empty() {
        normalized.source_type = infer_source_type(&normalized);
    }
    normalize_event_fields(&mut normalized);

    if normalized.memory_context.trim().is_empty() {
        normalized.memory_context = derive_memory_context(&normalized);
    }
    if normalized.user_intent.trim().is_empty()
        || normalized.intent_analysis.intent_label.is_empty()
    {
        let analysis = infer_intent_analysis(&normalized);
        normalized.user_intent = analysis.intent_label.clone();
        normalized.intent_analysis = analysis;
    }
    if normalized.topic.trim().is_empty() || normalized.topic == "unknown" {
        normalized.topic = infer_topic(&normalized);
    }
    if normalized.workflow.trim().is_empty() || normalized.workflow == "unknown" {
        normalized.workflow = infer_workflow(&normalized);
    }
    if normalized.embedding_text.trim().is_empty() {
        normalized.embedding_text = compose_embedding_text(&normalized);
    }
    if !is_supported_dedup_fingerprint(&normalized.dedup_fingerprint) {
        normalized.dedup_fingerprint =
            deterministic_dedup_fingerprint(&normalized, Some(&normalized.memory_context));
    }
    if normalized.search_aliases.is_empty() {
        normalized.search_aliases = generate_search_aliases(&normalized);
    }
    if normalized.raw_evidence.trim().is_empty() {
        normalized.raw_evidence = build_raw_evidence_payload(&normalized);
    }
    if normalized.extracted_entities_structured.is_empty() {
        normalized.extracted_entities_structured = derive_structured_entities(&normalized);
    }
    if normalized.action_items.is_empty() {
        normalized.action_items = derive_action_items(&normalized);
    }
    if normalized.topic_confidence <= 0.0 {
        normalized.topic_confidence = estimate_topic_confidence(&normalized);
    }
    if normalized.workflow_confidence <= 0.0 {
        normalized.workflow_confidence = estimate_workflow_confidence(&normalized);
    }
    if normalized.project_confidence <= 0.0 {
        normalized.project_confidence = estimate_project_confidence(&normalized);
    }
    if normalized.ocr_noise_score <= 0.0 {
        normalized.ocr_noise_score = normalized.noise_score.clamp(0.0, 1.0);
    }
    if normalized.evidence_confidence <= 0.0 {
        normalized.evidence_confidence = normalized
            .ocr_confidence
            .max(normalized.extraction_confidence)
            .clamp(0.0, 1.0);
    }
    if normalized.specificity_score <= 0.0 {
        normalized.specificity_score = estimate_specificity_score(&normalized);
    }
    if normalized.intent_score <= 0.0 {
        normalized.intent_score = normalized.intent_analysis.confidence.clamp(0.0, 1.0);
    }
    if normalized.entity_score <= 0.0 {
        normalized.entity_score = estimate_entity_score(&normalized);
    }
    if normalized.importance_score <= 0.0 {
        normalized.importance_score = estimate_importance_score(&normalized);
    }
    if normalized.agent_usefulness_score <= 0.0 {
        normalized.agent_usefulness_score = estimate_agent_usefulness_score(&normalized);
    }
    if normalized.confidence_score <= 0.0 {
        normalized.confidence_score = ((normalized.evidence_confidence * 0.55)
            + (normalized.intent_analysis.confidence * 0.20)
            + (normalized.extraction_confidence.clamp(0.0, 1.0) * 0.25))
            .clamp(0.0, 1.0);
    }
    if normalized.graph_readiness_score <= 0.0 {
        normalized.graph_readiness_score = ((normalized.entity_score * 0.4)
            + (normalized.evidence_confidence * 0.35)
            + (normalized.specificity_score * 0.25))
            .clamp(0.0, 1.0);
    }
    if normalized.retrieval_value_score <= 0.0 {
        normalized.retrieval_value_score = ((normalized.agent_usefulness_score * 0.45)
            + (normalized.specificity_score * 0.30)
            + (normalized.intent_score * 0.25))
            .clamp(0.0, 1.0);
    }
    if normalized.storage_outcome.trim().is_empty() {
        normalized.storage_outcome =
            classify_storage_outcome(&normalized, &default_memory_quality_config());
    }
    if normalized.quality_gate_reason.trim().is_empty() {
        normalized.quality_gate_reason = shared_quality_gate_reason(&normalized);
    }
    normalized.anchor_coverage_score = normalized.anchor_coverage_score.clamp(0.0, 1.0);
    if normalized.content_hash.trim().is_empty() {
        normalized.content_hash = compute_content_hash(
            normalized.url.as_deref(),
            &normalized.window_title,
            normalized.timestamp,
        );
    }
    normalized
}

fn strip_low_conf_markers(value: &str) -> String {
    value
        .replace("[LOW_CONF]", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_event_fields(record: &mut MemoryRecord) {
    if record.topic.trim().is_empty() {
        record.topic = "unknown".to_string();
    }
    if record.workflow.trim().is_empty() {
        record.workflow = "unknown".to_string();
    }
    if record.source_type.trim().is_empty() {
        record.source_type = "screen".to_string();
    }
}

fn infer_source_type(record: &MemoryRecord) -> String {
    let app = record.app_name.to_ascii_lowercase();
    if record.url.is_some() {
        "browser".to_string()
    } else if app.contains("terminal") || app.contains("iterm") || app.contains("warp") {
        "terminal".to_string()
    } else if app.contains("cursor") || app.contains("code") || app.contains("xcode") {
        "ide".to_string()
    } else if app.contains("word") || app.contains("pages") || app.contains("notion") {
        "document".to_string()
    } else {
        "screen".to_string()
    }
}

fn derive_memory_context(record: &MemoryRecord) -> String {
    let summary = if !record.display_summary.trim().is_empty() {
        record.display_summary.trim()
    } else {
        record.snippet.trim()
    };
    let detail = if !record.internal_context.trim().is_empty() {
        record.internal_context.trim()
    } else {
        record.clean_text.trim()
    };

    let mut parts = Vec::new();
    if !summary.is_empty() {
        parts.push(summary.to_string());
    }

    let intent = if !record.user_intent.trim().is_empty() {
        record.user_intent.trim().to_string()
    } else if !record.activity_type.trim().is_empty() {
        record.activity_type.trim().to_string()
    } else {
        String::new()
    };
    let mut context_bits = Vec::new();
    if !intent.is_empty() {
        context_bits.push(format!("Intent: {intent}"));
    }
    if !record.project.trim().is_empty() {
        context_bits.push(format!("Project: {}", record.project.trim()));
    }
    if !record.topic.trim().is_empty() && record.topic != "unknown" {
        context_bits.push(format!("Topic: {}", record.topic.trim()));
    }
    if !record.workflow.trim().is_empty() && record.workflow != "unknown" {
        context_bits.push(format!("Workflow: {}", record.workflow.trim()));
    }
    if !record.files_touched.is_empty() {
        context_bits.push(format!(
            "Files: {}",
            record
                .files_touched
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !record.decisions.is_empty() {
        context_bits.push(format!(
            "Decisions: {}",
            record
                .decisions
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !record.errors.is_empty() {
        context_bits.push(format!(
            "Errors: {}",
            record
                .errors
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !record.next_steps.is_empty() {
        context_bits.push(format!(
            "Next actions: {}",
            record
                .next_steps
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !context_bits.is_empty() {
        parts.push(context_bits.join(" | "));
    }

    if !detail.is_empty() && !summary.eq_ignore_ascii_case(detail) {
        parts.push(trim_chars(detail, 700));
    }
    if parts.is_empty() {
        return trim_chars(&record.window_title, 220);
    }
    trim_chars(&parts.join("\n"), 1_200)
}

fn infer_topic(record: &MemoryRecord) -> String {
    if !record.project.trim().is_empty() {
        return trim_chars(record.project.trim(), 80);
    }
    if let Some(url) = record.url.as_deref() {
        if let Some(path) = extract_path_segments(url, 2) {
            let normalized = path.replace('/', " ").replace('_', " ");
            let topic = normalized
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            if !topic.is_empty() {
                return topic;
            }
        }
    }
    let from_title = normalize_keyword_text(&record.window_title)
        .split_whitespace()
        .filter(|token| token.len() >= 4 && !is_keyword_stop_word(token))
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");
    if !from_title.is_empty() {
        return from_title;
    }
    "unknown".to_string()
}

fn infer_workflow(record: &MemoryRecord) -> String {
    let intent = infer_intent_analysis(record);
    let label = intent.intent_label;
    if label.is_empty() {
        "unknown".to_string()
    } else {
        label
    }
}

fn infer_intent_analysis(record: &MemoryRecord) -> crate::store::IntentAnalysis {
    let mut scores: HashMap<&str, f32> = HashMap::new();
    let mut evidence: HashMap<&str, Vec<String>> = HashMap::new();
    let text = normalize_keyword_text(&format!(
        "{} {} {} {}",
        record.window_title, record.clean_text, record.internal_context, record.memory_context
    ));
    let app = record.app_name.to_ascii_lowercase();

    let add = |scores: &mut HashMap<&str, f32>,
               evidence: &mut HashMap<&str, Vec<String>>,
               label: &'static str,
               weight: f32,
               reason: &str| {
        *scores.entry(label).or_insert(0.0) += weight;
        evidence.entry(label).or_default().push(reason.to_string());
    };

    if !record.files_touched.is_empty() || app.contains("cursor") || app.contains("code") {
        add(
            &mut scores,
            &mut evidence,
            "coding",
            0.42,
            "code/IDE artifacts present",
        );
    }
    if !record.errors.is_empty() || text.contains("failed") || text.contains("error") {
        add(
            &mut scores,
            &mut evidence,
            "debugging",
            0.45,
            "failure/error signals present",
        );
    }
    if !record.decisions.is_empty() || text.contains("decided") || text.contains("chose") {
        add(
            &mut scores,
            &mut evidence,
            "planning",
            0.32,
            "decision/planning language detected",
        );
    }
    if record.url.is_some() {
        add(
            &mut scores,
            &mut evidence,
            "researching",
            0.22,
            "web/document context detected",
        );
    }
    if app.contains("terminal") || !record.commands.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "testing_workflow",
            0.30,
            "terminal/command evidence present",
        );
    }
    if text.contains("todo") || !record.next_steps.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "organizing_information",
            0.22,
            "explicit next-step/task language",
        );
    }
    if scores.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "unknown",
            0.20,
            "insufficient high-confidence intent evidence",
        );
    }

    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top = ranked.first().copied().unwrap_or(("unknown", 0.0));
    let total: f32 = ranked
        .iter()
        .map(|(_, score)| *score)
        .sum::<f32>()
        .max(0.001);
    let confidence = (top.1 / total).clamp(0.0, 1.0);
    let competing = ranked
        .iter()
        .skip(1)
        .take(2)
        .map(|(label, score)| crate::store::IntentCandidate {
            label: (*label).to_string(),
            confidence: (*score / total).clamp(0.0, 1.0),
        })
        .collect::<Vec<_>>();

    crate::store::IntentAnalysis {
        intent_label: top.0.to_string(),
        confidence,
        supporting_evidence: evidence.get(top.0).cloned().unwrap_or_default(),
        competing_intents: competing,
    }
}

fn compose_embedding_text(record: &MemoryRecord) -> String {
    let mut segments = Vec::new();
    push_text_segment(&mut segments, &record.user_intent, "intent");
    push_text_segment(&mut segments, &record.project, "project");
    push_text_segment(&mut segments, &record.topic, "topic");
    push_text_segment(&mut segments, &record.workflow, "workflow");
    push_text_segment(&mut segments, &record.memory_context, "context");

    let entity_blob = record.entities.join(", ");
    push_text_segment(&mut segments, &entity_blob, "entities");
    let alias_blob = record.search_aliases.join(", ");
    push_text_segment(&mut segments, &alias_blob, "aliases");
    let decisions = record.decisions.join("; ");
    push_text_segment(&mut segments, &decisions, "decisions");
    let errors = record.errors.join("; ");
    push_text_segment(&mut segments, &errors, "errors");
    let blockers = record.blockers.join("; ");
    push_text_segment(&mut segments, &blockers, "blockers");
    let todos = if !record.todos.is_empty() {
        record.todos.join("; ")
    } else {
        record.next_steps.join("; ")
    };
    push_text_segment(&mut segments, &todos, "todos");
    let results = record.results.join("; ");
    push_text_segment(&mut segments, &results, "results");
    push_text_segment(&mut segments, &record.files_touched.join(", "), "files");
    if let Some(url) = record.url.as_deref() {
        push_text_segment(&mut segments, url, "urls");
    }
    push_text_segment(&mut segments, &record.commands.join("; "), "commands");
    push_text_segment(
        &mut segments,
        &trim_chars(&record.clean_text, 360),
        "raw_evidence",
    );

    trim_chars(&segments.join("\n"), 2_000)
}

fn push_text_segment(out: &mut Vec<String>, value: &str, label: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return;
    }
    out.push(format!("{label}: {trimmed}"));
}

fn generate_search_aliases(record: &MemoryRecord) -> Vec<String> {
    let mut aliases = HashSet::new();
    for value in record
        .entities
        .iter()
        .chain(record.tags.iter())
        .chain(record.files_touched.iter())
        .chain(record.related_tools.iter())
    {
        let base = value.trim();
        if base.is_empty() {
            continue;
        }
        aliases.insert(base.to_ascii_lowercase());
        aliases.insert(base.replace('_', " ").to_ascii_lowercase());
        aliases.insert(base.replace('-', " ").to_ascii_lowercase());
        let acronym = acronym_for(base);
        if acronym.len() >= 2 {
            aliases.insert(acronym.to_ascii_lowercase());
        }
        let compact = normalize_keyword_text(base).replace(' ', "");
        if compact.len() >= 3 {
            aliases.insert(compact);
        }
    }
    let mut out = aliases.into_iter().collect::<Vec<_>>();
    out.sort();
    out.truncate(24);
    out
}

fn acronym_for(value: &str) -> String {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.chars().next())
        .collect::<String>()
}

fn build_raw_evidence_payload(record: &MemoryRecord) -> String {
    serde_json::json!({
        "timestamp_start": record.timestamp_start,
        "timestamp_end": record.timestamp_end,
        "app_name": record.app_name,
        "window_title": record.window_title,
        "url": record.url,
        "source_type": record.source_type,
        "ocr_confidence": record.ocr_confidence,
        "noise_score": record.noise_score,
        "clean_text_excerpt": trim_chars(&record.clean_text, 400),
    })
    .to_string()
}

fn derive_structured_entities(record: &MemoryRecord) -> Vec<crate::store::ExtractedEntity> {
    let mut entities = Vec::new();
    let now = chrono::Utc::now().timestamp_millis();
    let base_evidence = vec![format!("memory_id={}", record.id), format!("ts={}", now)];

    for entity in dedup_strings(record.entities.clone()) {
        entities.push(crate::store::ExtractedEntity {
            text: entity.clone(),
            normalized_name: normalize_keyword_text(&entity),
            entity_type: "entity".to_string(),
            confidence: 0.72,
            source: "memory_context".to_string(),
            evidence: base_evidence.clone(),
            aliases: vec![entity.to_ascii_lowercase()],
            related_entity_ids: Vec::new(),
        });
    }
    for file in dedup_strings(record.files_touched.clone()) {
        entities.push(crate::store::ExtractedEntity {
            text: file.clone(),
            normalized_name: normalize_keyword_text(&file),
            entity_type: "file".to_string(),
            confidence: 0.86,
            source: "path".to_string(),
            evidence: vec!["detected file path".to_string()],
            aliases: vec![basename(&file)],
            related_entity_ids: Vec::new(),
        });
    }
    if let Some(url) = record.url.as_ref() {
        entities.push(crate::store::ExtractedEntity {
            text: url.clone(),
            normalized_name: normalize_keyword_text(url),
            entity_type: "url".to_string(),
            confidence: 0.9,
            source: "url".to_string(),
            evidence: vec!["record url metadata".to_string()],
            aliases: extract_domain(url).into_iter().collect(),
            related_entity_ids: Vec::new(),
        });
    }
    entities
}

fn derive_action_items(record: &MemoryRecord) -> Vec<crate::store::MemoryActionItem> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut items = Vec::new();
    for decision in dedup_strings(record.decisions.clone()) {
        items.push(build_action_item(
            "decision", decision, "observed", 0.8, &record.id, now,
        ));
    }
    for err in dedup_strings(record.errors.clone()) {
        items.push(build_action_item(
            "error", err, "observed", 0.85, &record.id, now,
        ));
    }
    for blocker in dedup_strings(record.blockers.clone()) {
        items.push(build_action_item(
            "blocker", blocker, "inferred", 0.68, &record.id, now,
        ));
    }
    for todo in dedup_strings(record.todos.clone()) {
        items.push(build_action_item(
            "todo", todo, "observed", 0.78, &record.id, now,
        ));
    }
    for todo in dedup_strings(record.next_steps.clone()) {
        items.push(build_action_item(
            "todo", todo, "inferred", 0.66, &record.id, now,
        ));
    }
    for result in dedup_strings(record.results.clone()) {
        items.push(build_action_item(
            "result", result, "observed", 0.74, &record.id, now,
        ));
    }
    items
}

fn build_action_item(
    kind: &str,
    text: String,
    status: &str,
    confidence: f32,
    memory_id: &str,
    now: i64,
) -> crate::store::MemoryActionItem {
    crate::store::MemoryActionItem {
        kind: kind.to_string(),
        text: trim_chars(text.trim(), 220),
        confidence: confidence.clamp(0.0, 1.0),
        status: status.to_string(),
        evidence: vec![format!("source_memory={memory_id}")],
        source_memory_id: memory_id.to_string(),
        related_entities: Vec::new(),
        related_files: Vec::new(),
        related_urls: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

fn estimate_topic_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.18;
    if !record.topic.trim().is_empty() && record.topic != "unknown" {
        score += 0.22;
    }
    if !record.project.trim().is_empty() {
        score += 0.20;
    }
    if record.url.is_some() {
        score += 0.14;
    }
    if !record.files_touched.is_empty() {
        score += 0.18;
    }
    score.clamp(0.0, 1.0)
}

fn estimate_workflow_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.2;
    if !record.workflow.trim().is_empty() && record.workflow != "unknown" {
        score += 0.22;
    }
    score += record.intent_analysis.confidence * 0.36;
    if !record.commands.is_empty() || !record.errors.is_empty() {
        score += 0.18;
    }
    score.clamp(0.0, 1.0)
}

fn estimate_project_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.12;
    if !record.project.trim().is_empty() {
        score += 0.30;
    }
    if !record.files_touched.is_empty() {
        score += 0.22;
    }
    if record.url.is_some() {
        score += 0.14;
    }
    if !record.related_projects.is_empty() {
        score += 0.14;
    }
    score.clamp(0.0, 1.0)
}

fn estimate_specificity_score(record: &MemoryRecord) -> f32 {
    let context_tokens = normalize_keyword_text(&record.memory_context)
        .split_whitespace()
        .count()
        .min(120) as f32
        / 120.0;
    let artifact_count = (record.files_touched.len()
        + record.entities.len()
        + record.decisions.len()
        + record.errors.len()
        + record.next_steps.len()) as f32;
    let artifact_score = (artifact_count / 16.0).clamp(0.0, 1.0);
    (context_tokens * 0.55 + artifact_score * 0.45).clamp(0.0, 1.0)
}

fn estimate_entity_score(record: &MemoryRecord) -> f32 {
    let count = (record.entities.len()
        + record.files_touched.len()
        + usize::from(record.url.is_some())) as f32;
    (count / 12.0).clamp(0.0, 1.0)
}

fn estimate_importance_score(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.25;
    if !record.errors.is_empty() {
        score += 0.22;
    }
    if !record.decisions.is_empty() {
        score += 0.2;
    }
    if !record.next_steps.is_empty() || !record.todos.is_empty() {
        score += 0.18;
    }
    if !record.project.trim().is_empty() {
        score += 0.14;
    }
    score.clamp(0.0, 1.0)
}

fn estimate_agent_usefulness_score(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.20;
    if !record.memory_context.trim().is_empty() {
        score += 0.22;
    }
    score += estimate_specificity_score(record) * 0.24;
    score += estimate_entity_score(record) * 0.18;
    score += record.intent_analysis.confidence * 0.16;
    score += record.evidence_confidence * 0.10;
    score -= record.noise_score.clamp(0.0, 1.0) * 0.14;
    score.clamp(0.0, 1.0)
}

fn basename(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn is_indexable_memory_record(record: &MemoryRecord) -> bool {
    let text_signal = format!(
        "{} {} {} {}",
        record.clean_text, record.snippet, record.memory_context, record.lexical_shadow
    );
    let alnum = text_signal
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .count();
    if alnum < 6 {
        tracing::debug!(
            memory_id = %record.id,
            "memory_record:skip_low_signal_text"
        );
        return false;
    }
    true
}

fn normalize_vector_dim(id: &str, field: &str, vector: &[f32], expected_dim: usize) -> Vec<f32> {
    if vector.len() == expected_dim {
        return vector.to_vec();
    }

    tracing::warn!(
        memory_id = %id,
        field,
        actual_dim = vector.len(),
        expected_dim,
        "memory_record:vector_dimension_mismatch"
    );

    if vector.is_empty() || vector.iter().all(|value| *value == 0.0) {
        return vec![0.0; expected_dim];
    }

    let mut repaired = vec![0.0; expected_dim];
    let copy_len = vector.len().min(expected_dim);
    repaired[..copy_len].copy_from_slice(&vector[..copy_len]);
    repaired
}

fn sanitize_index_url(url: Option<&str>, title: &str, snippet: &str) -> Option<String> {
    let raw = url?.trim();
    if raw.is_empty() {
        return None;
    }

    let normalized = canonicalize_index_url(raw);
    let domain = extract_domain(&normalized)?;
    let context = normalize_keyword_text(&format!("{title} {snippet}"));
    let path = extract_path_segments(&normalized, 3).unwrap_or_default();

    if INDEX_NOISE_HOSTS.iter().any(|host| domain == *host) {
        return None;
    }
    if looks_like_auth_or_error_context(&context) {
        return None;
    }
    if !path.is_empty() && is_low_entropy_path(&path) && context.split_whitespace().count() < 6 {
        return None;
    }

    Some(normalized)
}

fn canonicalize_index_url(url: &str) -> String {
    let no_fragment = url.split('#').next().unwrap_or(url);
    let no_query = no_fragment.split('?').next().unwrap_or(no_fragment);
    no_query.trim_end_matches('/').to_string()
}

fn build_index_session_key(record: &MemoryRecord) -> String {
    if record.session_key.starts_with("meeting:") {
        return record.session_key.clone();
    }

    let app = normalize_app_key(&record.app_name);
    if let Some(url) = record.url.as_deref() {
        if let Some(domain) = extract_domain(url) {
            if let Some(path) = extract_path_segments(url, 2) {
                if !path.is_empty() {
                    return format!("{app}:{domain}:{path}");
                }
            }
            return format!("{app}:{domain}");
        }
    }

    let title_key = normalize_anchor_key(&record.window_title);
    if !title_key.is_empty() {
        return format!("{app}:title:{title_key}");
    }

    let snippet_key = normalize_anchor_key(&record.snippet);
    if !snippet_key.is_empty() {
        return format!("{app}:snippet:{snippet_key}");
    }

    if !record.session_key.trim().is_empty() {
        return record.session_key.clone();
    }

    app
}

fn normalize_app_key(app_name: &str) -> String {
    let normalized = app_name
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let compact = normalized
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if compact.is_empty() {
        "unknown".to_string()
    } else {
        compact
    }
}

fn normalize_anchor_key(text: &str) -> String {
    normalize_keyword_text(text)
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .take(8)
        .collect::<Vec<_>>()
        .join("_")
}

fn extract_path_segments(url: &str, count: usize) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let mut parts = without_scheme.split('/');
    let _host = parts.next()?;
    let segments = parts
        .filter(|segment| !segment.trim().is_empty())
        .map(|segment| {
            normalize_keyword_text(segment)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("_")
        })
        .filter(|segment| !segment.is_empty())
        .take(count)
        .collect::<Vec<_>>();

    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

fn is_low_entropy_path(path: &str) -> bool {
    let normalized = normalize_keyword_text(path);
    let tokens = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return true;
    }

    let unique = tokens
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .len();
    unique <= 2
        && tokens.iter().all(|token| {
            matches!(
                *token,
                "404" | "500" | "account" | "auth" | "error" | "login" | "signin"
            )
        })
}

fn looks_like_auth_or_error_context(context: &str) -> bool {
    context.contains("sign in")
        || context.contains("log in")
        || context.contains("authenticate")
        || context.contains("authorization")
        || context.contains("404")
        || context.contains("500")
        || context.contains("not found")
        || context.starts_with("error ")
}

fn dedup_records_for_insert(records: &[MemoryRecord]) -> Vec<MemoryRecord> {
    let mut by_key: HashMap<String, MemoryRecord> = HashMap::new();

    for record in records {
        let key = record_insert_dedup_key(record);
        by_key
            .entry(key)
            .and_modify(|existing| {
                let existing_rank = estimate_record_signal_strength(existing);
                let incoming_rank = estimate_record_signal_strength(record);
                if incoming_rank > existing_rank
                    || (incoming_rank == existing_rank && record.timestamp > existing.timestamp)
                {
                    *existing = record.clone();
                }
            })
            .or_insert_with(|| record.clone());
    }

    by_key.into_values().collect()
}

fn dedup_search_results(mut results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    if results.is_empty() {
        return results;
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    let mut by_key: HashMap<String, SearchResult> = HashMap::new();
    for result in results {
        let key = search_result_dedup_key(&result);
        by_key
            .entry(key)
            .and_modify(|existing| {
                if result.score > existing.score
                    || (result.score == existing.score && result.timestamp > existing.timestamp)
                {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }

    let mut out: Vec<SearchResult> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });
    out.truncate(limit.max(1));
    out
}

fn record_insert_dedup_key(record: &MemoryRecord) -> String {
    if !record.content_hash.trim().is_empty() {
        return record.content_hash.trim().to_string();
    }
    compute_content_hash(
        record.url.as_deref(),
        &record.window_title,
        record.timestamp,
    )
}

fn search_result_dedup_key(result: &SearchResult) -> String {
    if !result.content_hash.trim().is_empty() {
        return result.content_hash.trim().to_string();
    }
    compute_content_hash(
        result.url.as_deref(),
        &result.window_title,
        result.timestamp,
    )
}

fn compute_content_hash(url: Option<&str>, page_title: &str, timestamp_ms: i64) -> String {
    let canonical_url = url.map(canonicalize_index_url).unwrap_or_default();
    let normalized_title = normalize_keyword_text(page_title);
    let five_min_bucket = timestamp_ms.div_euclid(300_000);
    let payload = format!("{canonical_url}|{normalized_title}|{five_min_bucket}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Escape single quotes for SQL string literals.
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

// ── DB initialization ─────────────────────────────────────────────────────────

async fn open_all_tables(
    db_path: &Path,
) -> Result<
    (
        Table,
        Option<Table>,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
    ),
    lancedb::Error,
> {
    let uri = db_path.to_string_lossy();
    let conn: Connection = lancedb::connect(&uri).execute().await?;
    let names = conn.table_names().execute().await?;

    let (table, legacy_table) = if names.contains(&"memories_v2_1024".to_string()) {
        let table = conn.open_table("memories_v2_1024").execute().await?;
        let legacy_table = if names.contains(&MEMORIES_TABLE.to_string()) {
            Some(conn.open_table(MEMORIES_TABLE).execute().await?)
        } else {
            None
        };
        (table, legacy_table)
    } else if names.contains(&MEMORIES_TABLE.to_string()) {
        let existing = conn.open_table(MEMORIES_TABLE).execute().await?;
        let schema = existing.schema().await?;
        let is_384 = schema.fields().iter().any(|f| {
            if f.name() == "embedding" {
                if let DataType::FixedSizeList(_, dim) = f.data_type() {
                    return *dim == 384;
                }
            }
            false
        });

        if is_384 && TEXT_EMBED_DIM == 1024 {
            let table = open_or_create_named_table(
                &conn,
                &names,
                "memories_v2_1024",
                Arc::new(memory_schema()),
            )
            .await?;
            (table, Some(existing))
        } else {
            (existing, None)
        }
    } else {
        (
            open_or_create_named_table(&conn, &names, MEMORIES_TABLE, Arc::new(memory_schema()))
                .await?,
            None,
        )
    };
    ensure_memory_schema_columns(&table).await?;

    let tasks =
        open_or_create_named_table(&conn, &names, TASKS_TABLE, Arc::new(task_schema())).await?;
    let meetings =
        open_or_create_named_table(&conn, &names, MEETINGS_TABLE, Arc::new(meeting_schema()))
            .await?;
    let segments =
        open_or_create_named_table(&conn, &names, SEGMENTS_TABLE, Arc::new(segment_schema()))
            .await?;
    let nodes =
        open_or_create_named_table(&conn, &names, NODES_TABLE, Arc::new(node_schema())).await?;
    let edges =
        open_or_create_named_table(&conn, &names, EDGES_TABLE, Arc::new(edge_schema())).await?;
    let activity_events = open_or_create_named_table(
        &conn,
        &names,
        ACTIVITY_EVENTS_TABLE,
        Arc::new(activity_event_schema()),
    )
    .await?;
    let project_contexts = open_or_create_named_table(
        &conn,
        &names,
        PROJECT_CONTEXTS_TABLE,
        Arc::new(project_context_schema()),
    )
    .await?;
    let decision_ledger = open_or_create_named_table(
        &conn,
        &names,
        DECISION_LEDGER_TABLE,
        Arc::new(decision_ledger_schema()),
    )
    .await?;
    let context_packs = open_or_create_named_table(
        &conn,
        &names,
        CONTEXT_PACKS_TABLE,
        Arc::new(context_pack_schema()),
    )
    .await?;
    let context_deltas = open_or_create_named_table(
        &conn,
        &names,
        CONTEXT_DELTAS_TABLE,
        Arc::new(context_delta_schema()),
    )
    .await?;
    let entity_aliases = open_or_create_named_table(
        &conn,
        &names,
        ENTITY_ALIASES_TABLE,
        Arc::new(entity_alias_schema()),
    )
    .await?;
    let knowledge_pages = open_or_create_named_table(
        &conn,
        &names,
        KNOWLEDGE_PAGES_TABLE,
        Arc::new(knowledge_page_schema()),
    )
    .await?;

    Ok((
        table,
        legacy_table,
        tasks,
        meetings,
        segments,
        nodes,
        edges,
        activity_events,
        project_contexts,
        decision_ledger,
        context_packs,
        context_deltas,
        entity_aliases,
        knowledge_pages,
    ))
}

async fn open_or_create_named_table(
    conn: &Connection,
    existing_tables: &[String],
    name: &str,
    schema: Arc<Schema>,
) -> Result<Table, lancedb::Error> {
    if existing_tables.contains(&name.to_string()) {
        conn.open_table(name).execute().await
    } else {
        let empty = RecordBatchIterator::new(
            std::iter::empty::<Result<RecordBatch, ArrowError>>(),
            schema,
        );
        conn.create_table(name, Box::new(empty) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
    }
}

async fn ensure_memory_schema_columns(table: &Table) -> Result<(), lancedb::Error> {
    let schema = table.schema().await?;
    let existing: std::collections::HashSet<String> = schema
        .fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect();

    let mut transforms: Vec<(String, String)> = Vec::new();
    if !existing.contains("clean_text") {
        transforms.push(("clean_text".to_string(), "text".to_string()));
    }
    if !existing.contains("ocr_confidence") {
        transforms.push((
            "ocr_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("ocr_block_count") {
        transforms.push((
            "ocr_block_count".to_string(),
            "CAST(0 AS BIGINT)".to_string(),
        ));
    }
    if !existing.contains("summary_source") {
        transforms.push(("summary_source".to_string(), "'fallback'".to_string()));
    }
    if !existing.contains("display_summary") {
        transforms.push(("display_summary".to_string(), "snippet".to_string()));
    }
    if !existing.contains("internal_context") {
        transforms.push(("internal_context".to_string(), "clean_text".to_string()));
    }
    if !existing.contains("noise_score") {
        transforms.push(("noise_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("session_key") {
        transforms.push(("session_key".to_string(), "''".to_string()));
    }
    if !existing.contains("lexical_shadow") {
        transforms.push(("lexical_shadow".to_string(), "''".to_string()));
    }
    if !existing.contains("snippet_embedding") {
        // Placeholder zeros — will be computed properly for new captures.
        transforms.push(("snippet_embedding".to_string(), "embedding".to_string()));
    }
    if !existing.contains("support_embedding") {
        transforms.push(("support_embedding".to_string(), "embedding".to_string()));
    }
    if !existing.contains("decay_score") {
        transforms.push(("decay_score".to_string(), "CAST(1.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("last_accessed_at") {
        transforms.push(("last_accessed_at".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("timestamp_start") {
        transforms.push(("timestamp_start".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("timestamp_end") {
        transforms.push(("timestamp_end".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("source_type") {
        transforms.push(("source_type".to_string(), "'screen'".to_string()));
    }
    if !existing.contains("topic") {
        transforms.push(("topic".to_string(), "'unknown'".to_string()));
    }
    if !existing.contains("workflow") {
        transforms.push(("workflow".to_string(), "'unknown'".to_string()));
    }
    if !existing.contains("user_intent") {
        transforms.push(("user_intent".to_string(), "''".to_string()));
    }
    if !existing.contains("intent_analysis") {
        transforms.push(("intent_analysis".to_string(), "'{}'".to_string()));
    }
    if !existing.contains("memory_context") {
        transforms.push(("memory_context".to_string(), "display_summary".to_string()));
    }
    if !existing.contains("commands") {
        transforms.push(("commands".to_string(), "[]".to_string()));
    }
    if !existing.contains("blockers") {
        transforms.push(("blockers".to_string(), "[]".to_string()));
    }
    if !existing.contains("todos") {
        transforms.push(("todos".to_string(), "[]".to_string()));
    }
    if !existing.contains("open_questions") {
        transforms.push(("open_questions".to_string(), "[]".to_string()));
    }
    if !existing.contains("results") {
        transforms.push(("results".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_tools") {
        transforms.push(("related_tools".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_agents") {
        transforms.push(("related_agents".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_projects") {
        transforms.push(("related_projects".to_string(), "[]".to_string()));
    }
    if !existing.contains("raw_evidence") {
        transforms.push(("raw_evidence".to_string(), "'{}'".to_string()));
    }
    if !existing.contains("search_aliases") {
        transforms.push(("search_aliases".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_memory_ids") {
        transforms.push(("related_memory_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("graph_node_ids") {
        transforms.push(("graph_node_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("graph_edge_ids") {
        transforms.push(("graph_edge_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("project_confidence") {
        transforms.push((
            "project_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("topic_confidence") {
        transforms.push((
            "topic_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("workflow_confidence") {
        transforms.push((
            "workflow_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("project_evidence") {
        transforms.push(("project_evidence".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_project_ids") {
        transforms.push(("related_project_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("evidence_confidence") {
        transforms.push((
            "evidence_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("confidence_score") {
        transforms.push((
            "confidence_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("importance_score") {
        transforms.push((
            "importance_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("specificity_score") {
        transforms.push((
            "specificity_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("intent_score") {
        transforms.push(("intent_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("entity_score") {
        transforms.push(("entity_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("agent_usefulness_score") {
        transforms.push((
            "agent_usefulness_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("ocr_noise_score") {
        transforms.push(("ocr_noise_score".to_string(), "noise_score".to_string()));
    }
    if !existing.contains("graph_readiness_score") {
        transforms.push((
            "graph_readiness_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("retrieval_value_score") {
        transforms.push((
            "retrieval_value_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("storage_outcome") {
        transforms.push((
            "storage_outcome".to_string(),
            "'enriched_memory_card'".to_string(),
        ));
    }
    if !existing.contains("quality_gate_reason") {
        transforms.push(("quality_gate_reason".to_string(), "''".to_string()));
    }
    if !existing.contains("extracted_entities_structured") {
        transforms.push((
            "extracted_entities_structured".to_string(),
            "'[]'".to_string(),
        ));
    }
    if !existing.contains("action_items") {
        transforms.push(("action_items".to_string(), "'[]'".to_string()));
    }
    if !existing.contains("anchor_coverage_score") {
        transforms.push((
            "anchor_coverage_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("content_hash") {
        transforms.push(("content_hash".to_string(), "''".to_string()));
    }

    if !transforms.is_empty() {
        tracing::info!(
            "Migrating LanceDB memories table schema with {} new columns",
            transforms.len()
        );
        table
            .add_columns(NewColumnTransform::SqlExpressions(transforms), None)
            .await?;
    }

    validate_memory_vector_schema(table).await?;

    Ok(())
}

async fn validate_memory_vector_schema(table: &Table) -> Result<(), lancedb::Error> {
    let schema = table.schema().await?;
    for (column, expected_dim) in [
        ("embedding", TEXT_EMBED_DIM),
        ("snippet_embedding", TEXT_EMBED_DIM),
        ("support_embedding", TEXT_EMBED_DIM),
        ("image_embedding", IMAGE_EMBED_DIM),
    ] {
        let Some(field) = schema.field_with_name(column).ok() else {
            continue;
        };
        let actual_dim = fixed_size_list_dim(field.data_type());
        if actual_dim != Some(expected_dim) {
            return Err(lancedb::Error::Schema {
                message: format!(
                    "LanceDB table '{}' column '{}' has vector dimension {:?}, but FNDR is configured for {}. Existing 384-dimensional tables must be migrated or reset before using the 1024-dimensional embedding path. To reset local prototype data, stop FNDR and remove the app data LanceDB directory.",
                    MEMORIES_TABLE,
                    column,
                    actual_dim,
                    expected_dim
                ),
            });
        }
    }
    Ok(())
}

fn fixed_size_list_dim(data_type: &DataType) -> Option<i32> {
    match data_type {
        DataType::FixedSizeList(_, dim) => Some(*dim),
        _ => None,
    }
}

// ── Migration from legacy JSON store ─────────────────────────────────────────

async fn migrate_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let mut records: Vec<MemoryRecord> = serde_json::from_slice(&data)?;
        if records.is_empty() {
            return Ok(());
        }

        // Backfill day_bucket for legacy records that predate the field.
        for r in &mut records {
            if r.day_bucket.is_empty() {
                r.day_bucket = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.timestamp)
                    .unwrap_or_else(chrono::Utc::now)
                    .format("%Y-%m-%d")
                    .to_string();
            }
        }
        records = records.iter().map(normalize_record_for_index).collect();
        records = dedup_records_for_insert(&records);

        tracing::info!(
            "Migrating {} records from memories.json to LanceDB",
            records.len()
        );

        // Insert in chunks to avoid huge Arrow batches.
        for chunk in records.chunks(500) {
            let batch = records_to_batch(chunk)?;
            let schema = Arc::new(memory_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .execute()
                .await?;
        }

        // Remove the legacy JSON source once migration has completed successfully.
        std::fs::remove_file(json_path)?;

        tracing::info!("Migration complete");
        Ok(())
    })
    .await;

    if let Err(e) = result {
        tracing::warn!("JSON migration failed (data not lost): {}", e);
    }
}
async fn migrate_tasks_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let tasks: Vec<Task> = serde_json::from_slice(&data)?;
        if tasks.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} tasks to LanceDB", tasks.len());
        let batch = task_to_batch(&tasks)?;
        let schema = Arc::new(task_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Task migration failed: {}", e);
    }
}

async fn migrate_meetings_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let meetings: Vec<MeetingSession> = serde_json::from_slice(&data)?;
        if meetings.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} meetings to LanceDB", meetings.len());
        let batch = meeting_to_batch(&meetings)?;
        let schema = Arc::new(meeting_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Meeting migration failed: {}", e);
    }
}

async fn migrate_segments_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let segments: Vec<MeetingSegment> = serde_json::from_slice(&data)?;
        if segments.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} segments to LanceDB", segments.len());
        let batch = segment_to_batch(&segments)?;
        let schema = Arc::new(segment_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Segment migration failed: {}", e);
    }
}

async fn migrate_graph_from_json(nodes_table: &Table, edges_table: &Table, json_path: &Path) {
    #[derive(serde::Deserialize)]
    struct LegacyGraph {
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    }

    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let graph: LegacyGraph = serde_json::from_slice(&data)?;
        if !graph.nodes.is_empty() {
            tracing::info!("Migrating {} graph nodes to LanceDB", graph.nodes.len());
            let batch = node_to_batch(&graph.nodes)?;
            let schema = Arc::new(node_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            nodes_table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .mode(AddDataMode::Overwrite)
                .execute()
                .await?;
        }
        if !graph.edges.is_empty() {
            tracing::info!("Migrating {} graph edges to LanceDB", graph.edges.len());
            let batch = edge_to_batch(&graph.edges)?;
            let schema = Arc::new(edge_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            edges_table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .mode(AddDataMode::Overwrite)
                .execute()
                .await?;
        }
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Graph migration failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(url: Option<&str>, title: &str, snippet: &str) -> MemoryRecord {
        MemoryRecord {
            id: "memory-1".to_string(),
            timestamp: 1_000,
            day_bucket: "2026-04-17".to_string(),
            app_name: "Chrome".to_string(),
            bundle_id: None,
            window_title: title.to_string(),
            session_id: "session-1".to_string(),
            text: snippet.to_string(),
            clean_text: snippet.to_string(),
            ocr_confidence: 0.9,
            ocr_block_count: 4,
            snippet: snippet.to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.1,
            session_key: String::new(),
            lexical_shadow: String::new(),
            embedding: vec![0.0; DEFAULT_TEXT_EMBEDDING_DIM],
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            screenshot_path: None,
            url: url.map(|value| value.to_string()),
            snippet_embedding: vec![0.0; DEFAULT_TEXT_EMBEDDING_DIM],
            support_embedding: vec![0.0; DEFAULT_TEXT_EMBEDDING_DIM],
            decay_score: 1.0,
            last_accessed_at: 0,
            ..Default::default()
        }
    }

    #[test]
    fn normalize_record_for_index_suppresses_auth_urls() {
        let normalized = normalize_record_for_index(&record(
            Some("https://accounts.google.com/signin/v2/challenge?foo=bar"),
            "Sign in",
            "Sign in to continue",
        ));
        assert!(normalized.url.is_none());
        assert_eq!(normalized.session_key, "chrome:title:sign");
    }

    #[test]
    fn normalize_record_for_index_keeps_specific_paths() {
        let normalized = normalize_record_for_index(&record(
            Some("https://docs.example.com/projects/fndr/pipeline?view=full"),
            "Pipeline design",
            "Reviewed the FNDR pipeline design and search notes",
        ));
        assert_eq!(
            normalized.url.as_deref(),
            Some("https://docs.example.com/projects/fndr/pipeline")
        );
        assert_eq!(
            normalized.session_key,
            "chrome:docs.example.com:projects/fndr"
        );
    }

    #[test]
    fn normalize_record_for_index_compacts_payload_fields() {
        let mut source = record(
            Some("https://example.com/research"),
            "Research notes",
            "Summarized the research notes for memory card storage.",
        );
        source.text = "raw noisy ocr block".to_string();
        source.clean_text = "raw noisy ocr block with repeated lines".to_string();
        source.screenshot_path = Some("/tmp/frame.png".to_string());

        let normalized = normalize_record_for_index(&source);
        assert!(normalized.text.is_empty());
        assert!(normalized.screenshot_path.is_none());
        assert_eq!(normalized.clean_text, source.snippet);
    }

    #[test]
    fn normalize_record_for_index_repairs_vector_dimensions() {
        let mut source = record(
            Some("https://example.com/research"),
            "Research notes",
            "Summarized the research notes for memory card storage.",
        );
        source.embedding = vec![0.25; 384];
        source.snippet_embedding = Vec::new();
        source.support_embedding = vec![0.5; DEFAULT_TEXT_EMBEDDING_DIM + 8];
        source.image_embedding = vec![0.0; 12];

        let normalized = normalize_record_for_index(&source);

        assert_eq!(normalized.embedding.len(), DEFAULT_TEXT_EMBEDDING_DIM);
        assert_eq!(
            normalized.snippet_embedding.len(),
            DEFAULT_TEXT_EMBEDDING_DIM
        );
        assert_eq!(
            normalized.support_embedding.len(),
            DEFAULT_TEXT_EMBEDDING_DIM
        );
        assert_eq!(
            normalized.image_embedding.len(),
            DEFAULT_IMAGE_EMBEDDING_DIM
        );
        assert_eq!(normalized.embedding[0], 0.25);
        assert!(normalized
            .snippet_embedding
            .iter()
            .all(|value| *value == 0.0));
    }

    #[test]
    fn normalize_record_for_index_strips_low_confidence_markers() {
        let mut source = record(
            Some("https://example.com/research"),
            "Research notes",
            "Summarized the research notes for memory card storage.",
        );
        source.clean_text = "[LOW_CONF] Toolbar\nImplemented OCR grounding checks".to_string();
        source.embedding_text =
            "[LOW_CONF] toolbar noise\nintent: improve extraction quality".to_string();
        source.display_summary = "[LOW_CONF] random nav\nImproved extraction quality".to_string();

        let normalized = normalize_record_for_index(&source);
        assert!(!normalized.clean_text.contains("[LOW_CONF]"));
        assert!(!normalized.embedding_text.contains("[LOW_CONF]"));
        assert!(!normalized.display_summary.contains("[LOW_CONF]"));
    }

    #[test]
    fn normalize_record_for_index_builds_fingerprint_fallback_when_invalid() {
        let mut source = record(
            Some("https://docs.example.com/fndr/search"),
            "Search quality",
            "Improved search quality for memory cards",
        );
        source.project = "FNDR".to_string();
        source.activity_type = "coding".to_string();
        source.dedup_fingerprint = "invalid fingerprint ###".to_string();

        let normalized = normalize_record_for_index(&source);
        assert!(!normalized.dedup_fingerprint.trim().is_empty());
        assert!(is_supported_dedup_fingerprint(
            &normalized.dedup_fingerprint
        ));
    }
}

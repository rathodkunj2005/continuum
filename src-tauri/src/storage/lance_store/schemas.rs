//! Arrow `Schema` definitions for LanceDB tables plus small filter builders.

use std::sync::Arc;

use crate::storage::schema::{EdgeType, GraphEdge};
use arrow_schema::{DataType, Field, Schema};

use super::{IMAGE_EMBED_DIM, TEXT_EMBED_DIM};

pub fn memory_schema() -> Schema {
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
        Field::new("reopen_kind", DataType::Utf8, false),
        Field::new("reopen_url", DataType::Utf8, true),
        Field::new("reopen_file_path", DataType::Utf8, true),
        Field::new("reopen_app_bundle_id", DataType::Utf8, true),
        Field::new("reopen_app_name", DataType::Utf8, true),
        Field::new("reopen_app_deep_link", DataType::Utf8, true),
        Field::new("reopen_captured_at_ms", DataType::Int64, false),
        Field::new("reopen_confidence", DataType::Float32, false),
        Field::new("reopen_validation_status", DataType::Utf8, false),
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
        Field::new("synthesis_branch", DataType::Utf8, false),
        Field::new(
            "topic_categories",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("insight_what_happened", DataType::Utf8, false),
        Field::new("insight_why_mattered", DataType::Utf8, false),
        Field::new("insight_what_changed", DataType::Utf8, false),
        Field::new("insight_context_thread", DataType::Utf8, false),
        Field::new("insight_spans_json", DataType::Utf8, false),
        Field::new("insight_card_confidence", DataType::Float32, false),
    ])
}

pub(super) fn task_schema() -> Schema {
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

pub(super) fn meeting_schema() -> Schema {
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

pub(super) fn segment_schema() -> Schema {
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

pub(super) fn node_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("node_type", DataType::Utf8, false),
        Field::new("label", DataType::Utf8, false),
        Field::new("created_at", DataType::Int64, false),
        Field::new("metadata_json", DataType::Utf8, false),
    ])
}

pub(super) fn edge_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, false),
        Field::new("target", DataType::Utf8, false),
        Field::new("edge_type", DataType::Utf8, false),
        Field::new("timestamp", DataType::Int64, false),
        Field::new("metadata_json", DataType::Utf8, false),
    ])
}

pub(super) fn activity_event_schema() -> Schema {
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

pub(super) fn project_context_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, false),
        Field::new("updated_at", DataType::Int64, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("privacy_class", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

pub(super) fn decision_ledger_schema() -> Schema {
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

pub(super) fn context_pack_schema() -> Schema {
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

pub(super) fn context_delta_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("since", DataType::Int64, false),
        Field::new("generated_at", DataType::Int64, false),
        Field::new("tokens_used", DataType::UInt32, false),
        Field::new("payload_json", DataType::Utf8, false),
    ])
}

pub(super) fn entity_alias_schema() -> Schema {
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

pub(super) fn knowledge_page_schema() -> Schema {
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

pub(super) fn edge_type_literal(edge_type: EdgeType) -> &'static str {
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

/// Lance table `graph_nodes` — insight graph (distinct from legacy `knowledge_nodes`).
pub fn insight_graph_node_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("node_type", DataType::Utf8, false),
        Field::new("label", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("source_memory_ids", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                TEXT_EMBED_DIM,
            ),
            false,
        ),
        Field::new("created_at_ms", DataType::Int64, false),
        Field::new("updated_at_ms", DataType::Int64, false),
        Field::new("stale", DataType::Boolean, false),
        Field::new("metadata", DataType::Utf8, false),
    ])
}

/// Lance table `graph_edges` — insight graph edges.
pub fn insight_graph_edge_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("target_id", DataType::Utf8, false),
        Field::new("edge_type", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("conflict_flag", DataType::Boolean, false),
        Field::new("created_at_ms", DataType::Int64, false),
        Field::new("metadata", DataType::Utf8, false),
    ])
}

pub(super) fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}

pub(super) fn build_string_match_filter(column: &str, values: &[String]) -> Option<String> {
    let clauses = values
        .iter()
        .map(|value| format!("{column} = '{}'", escape_sql_literal(value)))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" OR "))
    }
}

pub(super) fn build_edge_identity_filter(edges: &[GraphEdge]) -> Option<String> {
    let clauses = edges
        .iter()
        .map(|edge| {
            format!(
                "(source = '{}' AND target = '{}' AND edge_type = '{}')",
                escape_sql_literal(&edge.source),
                escape_sql_literal(&edge.target),
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

/// Arrow schema for `memories_v3_egemma_256` — EmbeddingGemma 256-dim vectors.
pub fn memories_v3_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("timestamp", DataType::Int64, false),
        Field::new("timestamp_start", DataType::Int64, false),
        Field::new("timestamp_end", DataType::Int64, false),
        Field::new("day_bucket", DataType::Utf8, false),
        Field::new("app_name", DataType::Utf8, false),
        Field::new("bundle_id", DataType::Utf8, true),
        Field::new("window_title", DataType::Utf8, false),
        Field::new("url", DataType::Utf8, true),
        Field::new("source_type", DataType::Utf8, false),
        Field::new("ocr_confidence", DataType::Float32, false),
        Field::new("memory_context", DataType::Utf8, false),
        Field::new("summary_short", DataType::Utf8, false),
        Field::new("topic", DataType::Utf8, false),
        Field::new("activity_type", DataType::Utf8, false),
        Field::new("user_intent", DataType::Utf8, false),
        Field::new(
            "entities",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "files",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "urls",
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
        Field::new(
            "search_aliases",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new("confidence_score", DataType::Float32, false),
        Field::new("importance_score", DataType::Float32, false),
        Field::new("enrichment_status", DataType::Utf8, false),
        Field::new("fallback_reason", DataType::Utf8, true),
        Field::new("embedding_model", DataType::Utf8, false),
        Field::new("embedding_dimensions", DataType::Int32, false),
        Field::new("raw_screenshot_stored", DataType::Boolean, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                256,
            ),
            false,
        ),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("dedup_fingerprint", DataType::Utf8, false),
        Field::new("is_soft_deleted", DataType::Boolean, false),
        Field::new("schema_version", DataType::Utf8, false),
    ])
}

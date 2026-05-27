//! Arrow batch conversions, column readers, and time/app filter SQL fragments.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{
    Array, BooleanArray, FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch,
    StringArray, UInt32Array,
};
use arrow_schema::{ArrowError, DataType, Field};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use futures::TryStreamExt;
use lancedb::query::ExecutableQuery;
use lancedb::Table;

use crate::memory::reopen::{ReopenKind, ReopenValidationStatus};
use crate::storage::schema::{
    ActivityEvent, ContextDelta, ContextPack, DecisionLedgerEntry, EdgeType, EntityAliasRecord,
    GraphEdge, GraphNode, KnowledgePage, KnowledgePageType, KnowledgeStability, MeetingSegment,
    MeetingSession, MemoryChunkRecord, MemoryChunkSearchResult, MemoryRecord, NodeType,
    PrivacyClass, ProjectContext, SearchResult, Task, TaskType,
};

use super::schemas::{
    activity_event_schema, context_delta_schema, context_pack_schema, decision_ledger_schema,
    edge_schema, edge_type_literal, entity_alias_schema, escape_sql_literal, knowledge_page_schema,
    meeting_schema, memory_chunk_schema, memory_schema_for_text_dim, node_schema,
    project_context_schema, segment_schema, task_schema,
};
use super::{
    IMAGE_EMBED_DIM, KEYWORD_QUERY_MULTIPLIER, MAX_KEYWORD_SCAN, SEARCH_RESULT_COLUMNS,
    TEXT_EMBED_DIM, VECTOR_QUERY_MULTIPLIER,
};
use arrow_array::builder::{Int64Builder, ListBuilder, StringBuilder};
use sha2::{Digest, Sha256};

use super::text_kw::{canonicalize_index_url, normalize_keyword_text};

pub(super) fn compute_content_hash(
    url: Option<&str>,
    page_title: &str,
    timestamp_ms: i64,
) -> String {
    let canonical_url = url.map(canonicalize_index_url).unwrap_or_default();
    let normalized_title = normalize_keyword_text(page_title);
    let five_min_bucket = timestamp_ms.div_euclid(300_000);
    let payload = format!("{canonical_url}|{normalized_title}|{five_min_bucket}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn records_to_batch(records: &[MemoryRecord]) -> Result<RecordBatch, ArrowError> {
    records_to_batch_with_text_dim(records, TEXT_EMBED_DIM)
}

pub(super) fn records_to_batch_with_text_dim(
    records: &[MemoryRecord],
    text_embed_dim: i32,
) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(memory_schema_for_text_dim(text_embed_dim));

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
        text_embed_dim,
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
        text_embed_dim,
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
        text_embed_dim,
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
    let reopen_kinds: Vec<&str> = records.iter().map(|r| r.reopen_kind.as_str()).collect();
    let reopen_urls: Vec<Option<&str>> = records.iter().map(|r| r.reopen_url.as_deref()).collect();
    let reopen_file_paths: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.reopen_file_path.as_deref())
        .collect();
    let reopen_app_bundle_ids: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.reopen_app_bundle_id.as_deref())
        .collect();
    let reopen_app_names: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.reopen_app_name.as_deref())
        .collect();
    let reopen_app_deep_links: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.reopen_app_deep_link.as_deref())
        .collect();
    let reopen_captured_at: Vec<i64> = records.iter().map(|r| r.reopen_captured_at_ms).collect();
    let reopen_confidences: Vec<f32> = records.iter().map(|r| r.reopen_confidence).collect();
    let reopen_validation_statuses: Vec<&str> = records
        .iter()
        .map(|r| r.reopen_validation_status.as_str())
        .collect();
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
    let topic_categories_array = build_str_list(&|r| &r.topic_categories);

    let synthesis_branches: Vec<&str> = records
        .iter()
        .map(|r| r.synthesis_branch.as_str())
        .collect();

    let insight_what: Vec<&str> = records
        .iter()
        .map(|r| r.insight_what_happened.as_str())
        .collect();
    let insight_why: Vec<&str> = records
        .iter()
        .map(|r| r.insight_why_mattered.as_str())
        .collect();
    let insight_changed: Vec<&str> = records
        .iter()
        .map(|r| r.insight_what_changed.as_str())
        .collect();
    let insight_thread: Vec<&str> = records
        .iter()
        .map(|r| r.insight_context_thread.as_str())
        .collect();
    let insight_spans: Vec<&str> = records
        .iter()
        .map(|r| r.insight_spans_json.as_str())
        .collect();
    let insight_conf: Vec<f32> = records.iter().map(|r| r.insight_card_confidence).collect();
    let enrichment_statuses: Vec<&str> = records
        .iter()
        .map(|r| r.enrichment_status.as_str())
        .collect();
    let reviewed_at_ms: Vec<i64> = records.iter().map(|r| r.reviewed_at_ms).collect();
    let reviewer_generations: Vec<u32> = records
        .iter()
        .map(|r| r.reviewer_generation.parse::<u32>().unwrap_or(0))
        .collect();
    let fallback_reasons: Vec<Option<&str>> = records
        .iter()
        .map(|r| r.fallback_reason.as_deref())
        .collect();
    let raw_screenshot_stored: Vec<bool> = records.iter().map(|r| r.raw_screenshot_stored).collect();

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
            Arc::new(StringArray::from(reopen_kinds)),
            Arc::new(StringArray::from(reopen_urls)),
            Arc::new(StringArray::from(reopen_file_paths)),
            Arc::new(StringArray::from(reopen_app_bundle_ids)),
            Arc::new(StringArray::from(reopen_app_names)),
            Arc::new(StringArray::from(reopen_app_deep_links)),
            Arc::new(Int64Array::from(reopen_captured_at)),
            Arc::new(Float32Array::from(reopen_confidences)),
            Arc::new(StringArray::from(reopen_validation_statuses)),
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
            Arc::new(StringArray::from(synthesis_branches)),
            Arc::new(topic_categories_array),
            Arc::new(StringArray::from(insight_what)),
            Arc::new(StringArray::from(insight_why)),
            Arc::new(StringArray::from(insight_changed)),
            Arc::new(StringArray::from(insight_thread)),
            Arc::new(StringArray::from(insight_spans)),
            Arc::new(Float32Array::from(insight_conf)),
            Arc::new(StringArray::from(enrichment_statuses)),
            Arc::new(Int64Array::from(reviewed_at_ms)),
            Arc::new(UInt32Array::from(reviewer_generations)),
            Arc::new(StringArray::from(fallback_reasons)),
            Arc::new(BooleanArray::from(raw_screenshot_stored)),
        ],
    )
}

pub(super) fn batch_to_memory_chunks(batch: &RecordBatch) -> Vec<MemoryChunkRecord> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let memory_ids = str_col(batch, "memory_id");
    let chunk_indexes = u32_col(batch, "chunk_index");
    let line_kinds = str_col(batch, "line_kind");
    let texts = str_col(batch, "text");
    let embed_col = batch
        .column_by_name("embedding")
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>().cloned());
    let created_at = i64_col(batch, "created_at");
    let app_names = str_col(batch, "app_name");
    let window_titles = str_col(batch, "window_title");
    let day_buckets = str_col(batch, "day_bucket");
    let content_hashes = str_col(batch, "content_hash");

    (0..n)
        .map(|i| MemoryChunkRecord {
            id: get_str(&ids, i),
            memory_id: get_str(&memory_ids, i),
            chunk_index: get_u32(&chunk_indexes, i),
            line_kind: get_str(&line_kinds, i),
            text: get_str(&texts, i),
            embedding: extract_f32_list(
                &embed_col,
                i,
                crate::inference::model_config::BGE_V5_DIMENSIONS,
            ),
            created_at: get_i64(&created_at, i),
            app_name: get_str(&app_names, i),
            window_title: get_str(&window_titles, i),
            day_bucket: get_str(&day_buckets, i),
            content_hash: get_str(&content_hashes, i),
        })
        .collect()
}

pub(super) fn batch_to_memory_chunk_search_results(
    batch: &RecordBatch,
) -> Vec<MemoryChunkSearchResult> {
    let distances = batch
        .column_by_name("_distance")
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>().cloned());

    batch_to_memory_chunks(batch)
        .into_iter()
        .enumerate()
        .map(|(idx, chunk)| {
            let distance = distances.as_ref().map(|c| c.value(idx)).unwrap_or(0.0);
            MemoryChunkSearchResult {
                chunk,
                score: vector_distance_to_similarity(distance),
                distance,
            }
        })
        .collect()
}

pub(super) fn batch_to_memory_records(batch: &RecordBatch) -> Vec<MemoryRecord> {
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
    let reopen_kinds = str_col(batch, "reopen_kind");
    let reopen_urls = str_col(batch, "reopen_url");
    let reopen_file_paths = str_col(batch, "reopen_file_path");
    let reopen_app_bundle_ids = str_col(batch, "reopen_app_bundle_id");
    let reopen_app_names = str_col(batch, "reopen_app_name");
    let reopen_app_deep_links = str_col(batch, "reopen_app_deep_link");
    let reopen_captured_at = i64_col(batch, "reopen_captured_at_ms");
    let reopen_confidences = f32_col(batch, "reopen_confidence");
    let reopen_validation_statuses = str_col(batch, "reopen_validation_status");
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
    let synthesis_branches = str_col(batch, "synthesis_branch");
    let topic_categories = list_str_col(batch, "topic_categories");
    let insight_what = str_col(batch, "insight_what_happened");
    let insight_why = str_col(batch, "insight_why_mattered");
    let insight_changed = str_col(batch, "insight_what_changed");
    let insight_thread = str_col(batch, "insight_context_thread");
    let insight_spans = str_col(batch, "insight_spans_json");
    let insight_conf = f32_col(batch, "insight_card_confidence");
    let enrichment_statuses = str_col(batch, "enrichment_status");
    let reviewed_at = i64_col(batch, "reviewed_at_ms");
    let reviewer_generations = u32_col(batch, "reviewer_generation");
    let fallback_reasons = str_col(batch, "fallback_reason");
    let raw_screenshot_stored = bool_col(batch, "raw_screenshot_stored");

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
                reopen_kind: ReopenKind::from_label(&get_str(&reopen_kinds, i)),
                reopen_url: get_opt_str(&reopen_urls, i),
                reopen_file_path: get_opt_str(&reopen_file_paths, i),
                reopen_app_bundle_id: get_opt_str(&reopen_app_bundle_ids, i),
                reopen_app_name: get_opt_str(&reopen_app_names, i),
                reopen_app_deep_link: get_opt_str(&reopen_app_deep_links, i),
                reopen_captured_at_ms: get_i64(&reopen_captured_at, i),
                reopen_confidence: get_f32(&reopen_confidences, i),
                reopen_validation_status: ReopenValidationStatus::from_label(&get_str(
                    &reopen_validation_statuses,
                    i,
                )),
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
                enrichment_status: get_str(&enrichment_statuses, i),
                reviewed_at_ms: get_i64(&reviewed_at, i),
                reviewer_generation: get_u32(&reviewer_generations, i).to_string(),
                fallback_reason: get_opt_str(&fallback_reasons, i),
                raw_screenshot_stored: get_bool(&raw_screenshot_stored, i),
                is_consolidated: get_bool(&is_consolidated_flags, i),
                is_soft_deleted: get_bool(&is_soft_deleted_flags, i),
                parent_id: get_opt_str(&parent_ids, i),
                related_ids: extract_str_list(&related_ids, i),
                consolidated_from: extract_str_list(&consolidated_from, i),
                synthesis_branch: get_str(&synthesis_branches, i),
                topic_categories: extract_str_list(&topic_categories, i),
                insight_what_happened: get_str(&insight_what, i),
                insight_why_mattered: get_str(&insight_why, i),
                insight_what_changed: get_str(&insight_changed, i),
                insight_context_thread: get_str(&insight_thread, i),
                insight_spans_json: get_str(&insight_spans, i),
                insight_card_confidence: get_f32(&insight_conf, i),
            }
        })
        .collect()
}

pub(super) fn batch_to_search_results(batch: &RecordBatch) -> Vec<SearchResult> {
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
    let reopen_kinds = str_col(batch, "reopen_kind");
    let reopen_urls = str_col(batch, "reopen_url");
    let reopen_file_paths = str_col(batch, "reopen_file_path");
    let reopen_app_bundle_ids = str_col(batch, "reopen_app_bundle_id");
    let reopen_app_names = str_col(batch, "reopen_app_name");
    let reopen_app_deep_links = str_col(batch, "reopen_app_deep_link");
    let reopen_captured_at = i64_col(batch, "reopen_captured_at_ms");
    let reopen_confidences = f32_col(batch, "reopen_confidence");
    let reopen_validation_statuses = str_col(batch, "reopen_validation_status");
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
    let insight_what = str_col(batch, "insight_what_happened");
    let insight_why = str_col(batch, "insight_why_mattered");
    let insight_changed = str_col(batch, "insight_what_changed");
    let insight_thread = str_col(batch, "insight_context_thread");
    let insight_spans = str_col(batch, "insight_spans_json");
    let insight_conf = f32_col(batch, "insight_card_confidence");
    let search_synthesis_branches = str_col(batch, "synthesis_branch");
    let search_topic_categories = list_str_col(batch, "topic_categories");
    let search_enrichment_statuses = str_col(batch, "enrichment_status");
    let search_reviewed_at_ms = i64_col(batch, "reviewed_at_ms");
    let search_reviewer_generations = u32_col(batch, "reviewer_generation");
    let search_storage_outcomes = str_col(batch, "storage_outcome");

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
                reopen_kind: ReopenKind::from_label(&get_str(&reopen_kinds, i)),
                reopen_url: get_opt_str(&reopen_urls, i),
                reopen_file_path: get_opt_str(&reopen_file_paths, i),
                reopen_app_bundle_id: get_opt_str(&reopen_app_bundle_ids, i),
                reopen_app_name: get_opt_str(&reopen_app_names, i),
                reopen_app_deep_link: get_opt_str(&reopen_app_deep_links, i),
                reopen_captured_at_ms: get_i64(&reopen_captured_at, i),
                reopen_confidence: get_f32(&reopen_confidences, i),
                reopen_validation_status: ReopenValidationStatus::from_label(&get_str(
                    &reopen_validation_statuses,
                    i,
                )),
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
                insight_what_happened: get_str(&insight_what, i),
                insight_why_mattered: get_str(&insight_why, i),
                insight_what_changed: get_str(&insight_changed, i),
                insight_context_thread: get_str(&insight_thread, i),
                insight_spans_json: get_str(&insight_spans, i),
                insight_card_confidence: get_f32(&insight_conf, i),
                synthesis_branch: get_str(&search_synthesis_branches, i),
                topic_categories: extract_str_list(&search_topic_categories, i),
                matched_routes: Vec::new(),
                matched_chunk_ids: Vec::new(),
                chunk_evidence: Vec::new(),
                enrichment_status: get_str(&search_enrichment_statuses, i),
                reviewed_at_ms: search_reviewed_at_ms
                    .as_ref()
                    .map(|c| c.value(i))
                    .unwrap_or(0),
                reviewer_generation: get_u32(&search_reviewer_generations, i),
                storage_outcome: {
                    let outcome = get_str(&search_storage_outcomes, i);
                    if outcome.trim().is_empty() {
                        "enriched_memory_card".to_string()
                    } else {
                        outcome
                    }
                },
            }
        })
        .collect()
}

// ── Arrow column helpers ─────────────────────────────────────────────────────

pub(super) fn str_col(batch: &RecordBatch, name: &str) -> Option<StringArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<StringArray>()
        .cloned()
}

pub(super) fn i64_col(batch: &RecordBatch, name: &str) -> Option<Int64Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<Int64Array>()
        .cloned()
}

pub(super) fn f32_col(batch: &RecordBatch, name: &str) -> Option<Float32Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<Float32Array>()
        .cloned()
}

pub(super) fn bool_col(batch: &RecordBatch, name: &str) -> Option<BooleanArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<BooleanArray>()
        .cloned()
}

pub(super) fn u32_col(batch: &RecordBatch, name: &str) -> Option<UInt32Array> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<UInt32Array>()
        .cloned()
}

pub(super) fn list_str_col(batch: &RecordBatch, name: &str) -> Option<ListArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<ListArray>()
        .cloned()
}

pub(super) fn get_str(col: &Option<StringArray>, i: usize) -> String {
    col.as_ref()
        .map(|c| c.value(i).to_string())
        .unwrap_or_default()
}

pub(super) fn get_opt_str(col: &Option<StringArray>, i: usize) -> Option<String> {
    col.as_ref().and_then(|c| {
        if c.is_null(i) {
            None
        } else {
            Some(c.value(i).to_string())
        }
    })
}

pub(super) fn get_non_empty_str(col: &Option<StringArray>, i: usize) -> Option<String> {
    get_opt_str(col, i).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(super) fn get_i64(col: &Option<Int64Array>, i: usize) -> i64 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0)
}

pub(super) fn get_f32(col: &Option<Float32Array>, i: usize) -> f32 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0.0)
}

pub(super) fn get_u32(col: &Option<UInt32Array>, i: usize) -> u32 {
    col.as_ref().map(|c| c.value(i)).unwrap_or(0)
}

pub(super) fn extract_domain(url: &str) -> Option<String> {
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

pub(super) fn compute_activity_streaks(
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

pub(super) fn get_bool(col: &Option<BooleanArray>, i: usize) -> bool {
    col.as_ref().map(|c| c.value(i)).unwrap_or(false)
}

pub(super) fn get_opt_i64(col: &Option<Int64Array>, i: usize) -> Option<i64> {
    col.as_ref()
        .and_then(|c| if c.is_null(i) { None } else { Some(c.value(i)) })
}

pub(super) fn extract_str_list(col: &Option<arrow_array::ListArray>, i: usize) -> Vec<String> {
    if let Some(list) = col {
        if let Some(values) = list
            .value(i)
            .as_any()
            .downcast_ref::<StringArray>()
            .cloned()
        {
            return (0..values.len())
                .filter_map(|j| {
                    let value = values.value(j).trim();
                    if value.is_empty() {
                        None
                    } else {
                        Some(value.to_string())
                    }
                })
                .collect();
        }
    }
    Vec::new()
}

pub(super) fn extract_f32_list(col: &Option<FixedSizeListArray>, i: usize, dim: usize) -> Vec<f32> {
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

pub(super) fn vector_distance_to_similarity(distance: f32) -> f32 {
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

pub(super) fn task_to_batch(tasks: &[Task]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn nodes_to_batch(
    nodes: &[GraphNode],
) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let mut ids = StringBuilder::new();
    let mut types = StringBuilder::new();
    let mut labels = StringBuilder::new();
    let mut created = Int64Builder::new();
    let mut metadata = StringBuilder::new();

    for n in nodes {
        ids.append_value(&n.id);
        types.append_value(match n.node_type {
            NodeType::Memory => "Memory",
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

pub(super) fn edges_to_batch(
    edges: &[GraphEdge],
) -> Result<RecordBatch, Box<dyn std::error::Error>> {
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

pub(super) fn batch_to_nodes(batch: &RecordBatch) -> Vec<GraphNode> {
    let n = batch.num_rows();
    let ids = str_col(batch, "id");
    let types = str_col(batch, "node_type");
    let labels = str_col(batch, "label");
    let created = i64_col(batch, "created_at");
    let meta = str_col(batch, "metadata_json");

    let mut nodes = Vec::with_capacity(n);
    for i in 0..n {
        let node_type = match get_str(&types, i).as_str() {
            "Memory" => NodeType::Memory,
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

pub(super) fn batch_to_edges(batch: &RecordBatch) -> Vec<GraphEdge> {
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

pub(super) fn activity_events_to_batch(
    events: &[ActivityEvent],
) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_activity_events(batch: &RecordBatch) -> Vec<ActivityEvent> {
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

pub(super) fn project_contexts_to_batch(
    contexts: &[ProjectContext],
) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_project_contexts(batch: &RecordBatch) -> Vec<ProjectContext> {
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

pub(super) fn decision_ledger_to_batch(
    entries: &[DecisionLedgerEntry],
) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_decision_ledger_entries(batch: &RecordBatch) -> Vec<DecisionLedgerEntry> {
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

pub(super) fn context_packs_to_batch(packs: &[ContextPack]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_context_packs(batch: &RecordBatch) -> Vec<ContextPack> {
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

pub(super) fn context_deltas_to_batch(deltas: &[ContextDelta]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_context_deltas(batch: &RecordBatch) -> Vec<ContextDelta> {
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

pub(super) fn entity_aliases_to_batch(
    aliases: &[EntityAliasRecord],
) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_entity_aliases(batch: &RecordBatch) -> Vec<EntityAliasRecord> {
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

pub(super) fn knowledge_page_type_literal(
    value: &crate::storage::schema::KnowledgePageType,
) -> &'static str {
    match value {
        crate::storage::schema::KnowledgePageType::ProjectPage => "project_page",
        crate::storage::schema::KnowledgePageType::TopicPage => "topic_page",
        crate::storage::schema::KnowledgePageType::ClaimPage => "claim_page",
        crate::storage::schema::KnowledgePageType::DecisionPage => "decision_page",
        crate::storage::schema::KnowledgePageType::PatternPage => "pattern_page",
        crate::storage::schema::KnowledgePageType::BreakthroughPage => "breakthrough_page",
        crate::storage::schema::KnowledgePageType::ContradictionPage => "contradiction_page",
        crate::storage::schema::KnowledgePageType::FrameworkPage => "framework_page",
    }
}

pub(super) fn knowledge_stability_literal(
    value: &crate::storage::schema::KnowledgeStability,
) -> &'static str {
    match value {
        crate::storage::schema::KnowledgeStability::Emerging => "emerging",
        crate::storage::schema::KnowledgeStability::Stable => "stable",
        crate::storage::schema::KnowledgeStability::Contradicted => "contradicted",
        crate::storage::schema::KnowledgeStability::Deprecated => "deprecated",
    }
}

pub(super) fn knowledge_pages_to_batch(pages: &[KnowledgePage]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn batch_to_knowledge_pages(batch: &RecordBatch) -> Vec<KnowledgePage> {
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

pub(super) fn privacy_class_literal(value: &crate::storage::schema::PrivacyClass) -> &'static str {
    match value {
        crate::storage::schema::PrivacyClass::Public => "public",
        crate::storage::schema::PrivacyClass::Project => "project",
        crate::storage::schema::PrivacyClass::Personal => "personal",
        crate::storage::schema::PrivacyClass::Sensitive => "sensitive",
        crate::storage::schema::PrivacyClass::Secret => "secret",
        crate::storage::schema::PrivacyClass::Blocked => "blocked",
        crate::storage::schema::PrivacyClass::Ephemeral => "ephemeral",
    }
}

pub(super) async fn count_table_rows(table: &Table) -> Result<usize, Box<dyn std::error::Error>> {
    let batches = table
        .query()
        .execute()
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(batches.into_iter().map(|batch| batch.num_rows()).sum())
}

pub(super) fn batch_to_meetings(batch: &RecordBatch) -> Vec<MeetingSession> {
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

pub(super) fn batch_to_segments(batch: &RecordBatch) -> Vec<MeetingSegment> {
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

pub(super) fn batch_to_tasks(batch: &RecordBatch) -> Vec<Task> {
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

pub(super) fn meeting_to_batch(meetings: &[MeetingSession]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn segment_to_batch(segments: &[MeetingSegment]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn node_to_batch(nodes: &[GraphNode]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn edge_to_batch(edges: &[GraphEdge]) -> Result<RecordBatch, ArrowError> {
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

pub(super) fn build_filter(time_filter: Option<&str>, app_filter: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(tf) = time_filter.and_then(time_filter_to_sql) {
        parts.push(tf);
    }
    if let Some(app) = app_filter {
        parts.push(format!("app_name = '{}'", escape_sql_literal(app)));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" AND "))
    }
}

pub(super) fn time_filter_to_sql(tf: &str) -> Option<String> {
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

pub(super) fn local_day_bucket_now() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub(super) fn local_day_bucket_from_timestamp(timestamp: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}

pub(super) fn local_day_range_filter(days_ago: i64) -> Option<String> {
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

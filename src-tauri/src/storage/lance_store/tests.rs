use super::*;
use arrow_array::{RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use lancedb::table::NewColumnTransform;
use std::collections::HashSet;
use std::sync::Arc;

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

fn memory_chunk(id: &str, memory_id: &str, dim: usize) -> MemoryChunkRecord {
    MemoryChunkRecord {
        id: id.to_string(),
        memory_id: memory_id.to_string(),
        chunk_index: 0,
        line_kind: "plain".to_string(),
        text: "High signal memory chunk text for parent child RAG.".to_string(),
        embedding: vec![0.01; dim],
        created_at: 1_000,
        app_name: "Chrome".to_string(),
        window_title: "Research notes".to_string(),
        day_bucket: "2026-04-17".to_string(),
        content_hash: format!("hash-{id}"),
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
        Some("https://docs.example.com/projects/continuum/pipeline?view=full"),
        "Pipeline design",
        "Reviewed the Continuum pipeline design and search notes",
    ));
    assert_eq!(
        normalized.url.as_deref(),
        Some("https://docs.example.com/projects/continuum/pipeline")
    );
    assert_eq!(
        normalized.session_key,
        "chrome:docs.example.com:projects/continuum"
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
    assert!(normalized
        .support_embedding
        .iter()
        .all(|value| *value == 0.0));
    assert!(normalized.image_embedding.iter().all(|value| *value == 0.0));
}

#[test]
fn memory_schema_migration_covers_current_writer_columns() {
    let existing = HashSet::from([
        "id".to_string(),
        "timestamp".to_string(),
        "memory_context".to_string(),
    ]);
    let mut transforms = Vec::new();

    push_current_memory_writer_column_transforms(&existing, &mut transforms);

    let added = transforms
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<HashSet<_>>();
    for required in [
        "schema_version",
        "activity_type",
        "files_touched",
        "symbols_changed",
        "session_duration_mins",
        "project",
        "tags",
        "entities",
        "decisions",
        "errors",
        "next_steps",
        "git_stats",
        "outcome",
        "extraction_confidence",
        "dedup_fingerprint",
        "embedding_text",
        "embedding_model",
        "embedding_dim",
        "is_consolidated",
        "is_soft_deleted",
        "parent_id",
        "related_ids",
        "consolidated_from",
        "synthesis_branch",
        "topic_categories",
    ] {
        assert!(added.contains(required), "missing transform for {required}");
    }
    assert!(transforms
        .iter()
        .any(|(name, expr)| name == "embedding_dim" && expr.contains("INTEGER UNSIGNED")));
}

#[test]
fn memory_schema_dimensions_keep_v4_and_v5_separate() {
    let v4 = memory_schema();
    let v5 = memory_v5_schema();
    let chunks = memory_chunk_schema();

    assert_eq!(schema_vector_dim(&v4, "embedding"), Some(384));
    assert_eq!(schema_vector_dim(&v4, "snippet_embedding"), Some(384));
    assert_eq!(schema_vector_dim(&v4, "support_embedding"), Some(384));
    assert_eq!(schema_vector_dim(&v4, "image_embedding"), Some(512));

    assert_eq!(schema_vector_dim(&v5, "embedding"), Some(1024));
    assert_eq!(schema_vector_dim(&v5, "snippet_embedding"), Some(1024));
    assert_eq!(schema_vector_dim(&v5, "support_embedding"), Some(1024));
    assert_eq!(schema_vector_dim(&v5, "image_embedding"), Some(512));

    assert_eq!(schema_vector_dim(&chunks, "embedding"), Some(1024));
    assert!(chunks.field_with_name("memory_id").is_ok());
    assert!(chunks.field_with_name("content_hash").is_ok());
}

fn schema_vector_dim(schema: &Schema, column: &str) -> Option<i32> {
    schema
        .field_with_name(column)
        .ok()
        .and_then(|field| fixed_size_list_dim(field.data_type()))
}

#[tokio::test]
async fn current_writer_schema_transforms_create_append_compatible_types() {
    let conn = lancedb::connect("memory://").execute().await.unwrap();
    let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Utf8, false)]));
    let batch = RecordBatch::try_new(
        schema,
        vec![Arc::new(StringArray::from(vec!["legacy-row"]))],
    )
    .unwrap();
    let table = conn
        .create_table("legacy_current_writer_schema", batch)
        .execute()
        .await
        .unwrap();

    let existing = HashSet::from(["id".to_string()]);
    let mut transforms = Vec::new();
    push_current_memory_writer_column_transforms(&existing, &mut transforms);

    table
        .add_columns(NewColumnTransform::SqlExpressions(transforms), None)
        .await
        .unwrap();

    let migrated = table.schema().await.unwrap();
    assert_eq!(
        migrated
            .field_with_name("schema_version")
            .unwrap()
            .data_type(),
        &DataType::UInt32
    );
    assert_eq!(
        migrated
            .field_with_name("embedding_dim")
            .unwrap()
            .data_type(),
        &DataType::UInt32
    );
    assert_eq!(
        migrated
            .field_with_name("topic_categories")
            .unwrap()
            .data_type(),
        &DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)))
    );
}

#[tokio::test]
async fn store_opens_v4_and_creates_v5_parent_table_without_resetting_v4() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let records = vec![record(
        Some("https://example.com/v4"),
        "v4 source",
        "Body for v4 source memory",
    )];
    store.add_batch(&records).await.expect("add v4 record");
    drop(store);

    let db_path = dir.path().join("lancedb");
    let db_uri = db_path.to_string_lossy().to_string();
    let conn = lancedb::connect(&db_uri).execute().await.unwrap();
    let names = conn.table_names().execute().await.unwrap();
    assert!(names.contains(&MEMORIES_TABLE.to_string()));
    assert!(names.contains(&MEMORIES_V5_PARENT_TABLE.to_string()));
    assert!(names.contains(&MEMORY_CHUNKS_TABLE.to_string()));

    let v4 = conn.open_table(MEMORIES_TABLE).execute().await.unwrap();
    let v5 = conn
        .open_table(MEMORIES_V5_PARENT_TABLE)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        schema_vector_dim(&v4.schema().await.unwrap(), "embedding"),
        Some(384)
    );
    assert_eq!(
        schema_vector_dim(&v5.schema().await.unwrap(), "embedding"),
        Some(1024)
    );
    let chunk_table = conn
        .open_table(MEMORY_CHUNKS_TABLE)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        schema_vector_dim(&chunk_table.schema().await.unwrap(), "embedding"),
        Some(1024)
    );
}

#[tokio::test]
async fn store_migrates_legacy_memories_table_into_active_v4_without_dimension_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("lancedb");
    std::fs::create_dir_all(&db_path).expect("db dir");
    let conn = lancedb::connect(db_path.to_string_lossy().as_ref())
        .execute()
        .await
        .expect("connect");
    let mut legacy = record(
        Some("https://example.com/legacy"),
        "Legacy 256-d memory",
        "Legacy memory should move to the active v4 table.",
    );
    legacy.id = "legacy-256".to_string();
    legacy.embedding = vec![0.7; 256];
    legacy.snippet_embedding = vec![0.7; 256];
    legacy.support_embedding = vec![0.7; 256];
    legacy.embedding_dim = 256;
    let batch = records_to_batch_with_text_dim(&[legacy], 256).expect("legacy batch");
    conn.create_table("memories", batch)
        .execute()
        .await
        .expect("legacy table");

    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("store task");
    let migrated = store
        .get_memory_by_id("legacy-256")
        .await
        .expect("read migrated")
        .expect("migrated row");

    assert_eq!(migrated.embedding.len(), DEFAULT_TEXT_EMBEDDING_DIM);
    assert_eq!(migrated.snippet_embedding.len(), DEFAULT_TEXT_EMBEDDING_DIM);
    assert_eq!(migrated.support_embedding.len(), DEFAULT_TEXT_EMBEDDING_DIM);
    assert_eq!(migrated.image_embedding.len(), DEFAULT_IMAGE_EMBEDDING_DIM);
    assert!(migrated.embedding.iter().all(|value| *value == 0.0));
    assert_eq!(migrated.embedding_dim, DEFAULT_TEXT_EMBEDDING_DIM as u32);
    assert_eq!(
        migrated.embedding_model,
        crate::config::DEFAULT_EMBEDDING_MODEL_NAME
    );

    let conn = lancedb::connect(db_path.to_string_lossy().as_ref())
        .execute()
        .await
        .expect("reconnect");
    let legacy_table = conn.open_table("memories").execute().await.expect("legacy");
    assert_eq!(
        legacy_table.count_rows(None).await.expect("legacy count"),
        1
    );
}

#[tokio::test]
async fn v5_writer_rejects_wrong_dimension_vectors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let records = vec![record(
        Some("https://example.com/v5"),
        "v5 target",
        "Body for v5 target memory",
    )];
    let err = store
        .add_v5_batch_preserving_ids(&records)
        .await
        .expect_err("384-d vectors must not be written to v5");
    assert!(err.to_string().contains("expected 1024-d BGE"));
}

#[tokio::test]
async fn memory_chunk_writer_rejects_wrong_dimension_vectors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let chunks = vec![memory_chunk(
        "chunk-1",
        "memory-1",
        DEFAULT_TEXT_EMBEDDING_DIM,
    )];
    let err = store
        .upsert_memory_chunks(&chunks)
        .await
        .expect_err("384-d vectors must not be written to chunk table");
    assert!(err.to_string().contains("expected 1024-d BGE"));
}

#[tokio::test]
async fn memory_chunks_upsert_list_and_parent_delete_are_linked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let mut parent = record(
        Some("https://example.com/chunks"),
        "Chunk source",
        "Parent memory for chunk table deletion",
    );
    parent.id = "parent-memory".to_string();
    store.add_batch(&[parent]).await.expect("add parent");

    let chunks = vec![
        memory_chunk("chunk-1", "parent-memory", BGE_V5_DIMENSIONS),
        MemoryChunkRecord {
            id: "chunk-2".to_string(),
            chunk_index: 1,
            ..memory_chunk("chunk-2", "parent-memory", BGE_V5_DIMENSIONS)
        },
    ];
    store
        .upsert_memory_chunks(&chunks)
        .await
        .expect("upsert chunks");

    let listed = store
        .list_chunks_for_memory("parent-memory")
        .await
        .expect("list chunks");
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .all(|chunk| chunk.memory_id == "parent-memory"));

    let deleted = store
        .delete_memory_by_id("parent-memory")
        .await
        .expect("delete parent");
    assert_eq!(deleted, 1);
    let listed_after = store
        .list_chunks_for_memory("parent-memory")
        .await
        .expect("list chunks after parent delete");
    assert!(listed_after.is_empty());
}

#[tokio::test]
async fn chunk_vector_search_ranks_chunks_and_returns_scores() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let chunks = vec![
        MemoryChunkRecord {
            text: "Needle chunk: parent child RAG retrieval".to_string(),
            embedding: vec![1.0; BGE_V5_DIMENSIONS],
            ..memory_chunk("chunk-a", "memory-a", BGE_V5_DIMENSIONS)
        },
        MemoryChunkRecord {
            text: "Distractor chunk about unrelated planning".to_string(),
            embedding: vec![0.0; BGE_V5_DIMENSIONS],
            ..memory_chunk("chunk-b", "memory-b", BGE_V5_DIMENSIONS)
        },
    ];
    store
        .upsert_memory_chunks(&chunks)
        .await
        .expect("upsert chunks");

    let results = store
        .chunk_vector_search(&vec![1.0; BGE_V5_DIMENSIONS], 2)
        .await
        .expect("chunk vector search");

    assert_eq!(results[0].chunk.id, "chunk-a");
    assert_eq!(results[0].chunk.memory_id, "memory-a");
    assert!(results[0].score > results[1].score);
    assert!(results[0].distance <= results[1].distance);
}

#[tokio::test]
async fn chunk_vector_search_rejects_wrong_dimension_query() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let err = store
        .chunk_vector_search(&vec![0.0; DEFAULT_TEXT_EMBEDDING_DIM], 4)
        .await
        .expect_err("384-d chunk query must be rejected");

    assert!(err.to_string().contains("expected 1024-d BGE"));
    assert!(err
        .to_string()
        .contains("No fallback across embedding dimensions"));
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
fn normalize_record_for_index_preserves_existing_embedding_text_and_flags_mismatch() {
    let mut source = record(
        Some("https://docs.example.com/continuum/search"),
        "Search quality",
        "Improved search quality for memory cards",
    );
    source.embedding_text = "intent: existing vector source text".to_string();
    source.memory_context =
        "Canonical context changed after the vector was already computed.".to_string();
    source.user_intent = "Align embedding provenance".to_string();
    source.project = "Memory search".to_string();
    source.topic = "embedding contract".to_string();
    source.raw_evidence = r#"{"source_kind":"test"}"#.to_string();

    let normalized = normalize_record_for_index(&source);

    assert_eq!(
        normalized.embedding_text, "intent: existing vector source text",
        "normalization must not silently rewrite source text for an existing vector"
    );
    let manifest =
        crate::memory_embedding_document::read_embedding_manifest(&normalized.raw_evidence)
            .expect("embedding manifest");
    assert!(manifest.statuses.iter().any(|status| {
        status.role == crate::memory_embedding_document::EmbeddingRole::Primary
            && status.status == crate::memory_embedding_document::EmbeddingStatus::StaleSourceText
    }));
}

#[test]
fn batch_to_search_results_exposes_safe_embedding_provenance() {
    let mut source = record(
        Some("https://docs.example.com/continuum/search"),
        "Search quality",
        "Improved search quality for memory cards",
    );
    source.memory_context =
        "Canonical context for the memory embedding document and search provenance.".to_string();
    let document =
        crate::memory_embedding_document::compose_memory_embedding_document(&source, None);
    let manifest = crate::memory_embedding_document::build_embedding_manifest(
        &document,
        crate::memory_embedding_document::EmbeddingStatus::StaleSourceText,
        crate::memory_embedding_document::EmbeddingStatus::ZeroVectorFallback,
        crate::memory_embedding_document::VisualSemanticSource::TextCapture,
    );
    source.raw_evidence =
        crate::memory_embedding_document::upsert_embedding_manifest("{}", &manifest);

    let batch = records_to_batch(&[source]).expect("record batch");
    let results = batch_to_search_results(&batch);

    let provenance = results[0]
        .embedding_provenance
        .as_ref()
        .expect("embedding provenance");
    let primary = provenance
        .role(crate::memory_embedding_document::EmbeddingRole::Primary)
        .expect("primary provenance");
    assert_eq!(
        primary.status,
        crate::memory_embedding_document::EmbeddingStatus::StaleSourceText
    );
    assert!(provenance
        .status_labels
        .contains(&"embedding:primary:stale_source_text".to_string()));
}

#[test]
fn normalize_record_for_index_builds_fingerprint_fallback_when_invalid() {
    let mut source = record(
        Some("https://docs.example.com/continuum/search"),
        "Search quality",
        "Improved search quality for memory cards",
    );
    source.project = "Continuum".to_string();
    source.activity_type = "coding".to_string();
    source.dedup_fingerprint = "invalid fingerprint ###".to_string();

    let normalized = normalize_record_for_index(&source);
    assert!(!normalized.dedup_fingerprint.trim().is_empty());
    assert!(is_supported_dedup_fingerprint(
        &normalized.dedup_fingerprint
    ));
}

#[test]
fn generate_search_aliases_noun_phrases_compact_and_acronym() {
    let mut r = record(
        Some("https://example.com/doc"),
        "Notes",
        "Supporting snippet for alias coverage.",
    );
    r.topic = "Memory Card Storage".to_string();
    r.workflow = "unknown".to_string();
    r.project = "unknown".to_string();
    let aliases = generate_search_aliases_public(&r);
    assert!(
        aliases.contains(&"memory card storage".to_string()),
        "expected lowercase phrase, got {aliases:?}"
    );
    assert!(
        aliases.contains(&"memorycardstorage".to_string()),
        "expected ≤3-token compact alias, got {aliases:?}"
    );

    r.topic = "HTTP API Gateway".to_string();
    let aliases = generate_search_aliases_public(&r);
    assert!(aliases.contains(&"http api gateway".to_string()));
    assert!(
        aliases.contains(&"hag".to_string()),
        "expected acronym for multi-token proper noun phrase, got {aliases:?}"
    );
}

#[test]
fn generate_search_aliases_skips_pipe_bearing_phrases() {
    let mut r = record(
        Some("https://example.com/doc"),
        "Notes",
        "Supporting snippet for alias coverage.",
    );
    // Mimic a model that echoed the whole activity enum into the topic; the
    // alias generator must never derive acronyms or compact forms from it.
    r.topic = "coding|debugging|reviewing_agent_output|researching|planning|writing".to_string();
    r.workflow = "unknown".to_string();
    r.project = "unknown".to_string();
    let aliases = generate_search_aliases_public(&r);
    assert!(
        !aliases.iter().any(|a| a.contains('|')),
        "no alias may contain '|': {aliases:?}"
    );
    // The acronym derived from the polluted phrase (tsapoeacdraorpws / cdrarpw)
    // must never appear.
    assert!(
        !aliases.iter().any(|a| a.len() >= 8 && !a.contains(' ')),
        "long opaque acronyms must not be generated: {aliases:?}"
    );
}

#[test]
fn generate_search_aliases_entity_underscore_variant_without_acronym_noise() {
    let mut r = record(
        Some("https://example.com/x"),
        "Editor",
        "Touched config files.",
    );
    r.topic = "unknown".to_string();
    r.entities = vec!["continuum_search_pipeline".to_string()];
    let aliases = generate_search_aliases_public(&r);
    assert!(aliases.contains(&"continuum_search_pipeline".to_string()));
    assert!(aliases.contains(&"continuum search pipeline".to_string()));
    assert!(
        !aliases
            .iter()
            .any(|a| a.len() == 2 && a.chars().all(|c| c.is_ascii_lowercase())),
        "single-token entities must not produce two-letter acronym noise: {aliases:?}"
    );
}

// Helpers for the image-to-image similarity tests below. Synthetic vectors are
// inserted directly so the test does not require the CLIP ONNX file on disk.
fn record_with_image_embedding(id: &str, title: &str, image_vec: Vec<f32>) -> MemoryRecord {
    let mut r = record(Some("https://example.com/x"), title, "Body");
    r.id = id.to_string();
    r.image_embedding = image_vec;
    r
}

fn unit_vec_with_one_at(index: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; DEFAULT_IMAGE_EMBEDDING_DIM];
    v[index] = 1.0;
    v
}

#[tokio::test]
async fn similar_by_image_embedding_returns_neighbors_excluding_seed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    // Seed (basis vector 0) plus a near neighbor (mostly 0, tiny 1) and a far
    // neighbor (basis vector 1, orthogonal).
    let seed_vec = unit_vec_with_one_at(0);
    let mut near_vec = vec![0.0f32; DEFAULT_IMAGE_EMBEDDING_DIM];
    near_vec[0] = 0.9;
    near_vec[2] = 0.435_889_894; // chosen so the vector is unit-norm
    let far_vec = unit_vec_with_one_at(1);

    let records = vec![
        record_with_image_embedding("seed", "seed window", seed_vec.clone()),
        record_with_image_embedding("near", "near window", near_vec),
        record_with_image_embedding("far", "far window", far_vec),
    ];
    store.add_batch(&records).await.expect("add records");

    let hits = store
        .similar_by_image_embedding("seed", 5, None, None)
        .await
        .expect("similar");

    assert!(
        !hits.iter().any(|h| h.id == "seed"),
        "seed must be excluded"
    );
    let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"near"), "expected near neighbor in {ids:?}");
    assert!(ids.contains(&"far"), "expected far neighbor in {ids:?}");
}

#[tokio::test]
async fn similar_by_image_embedding_returns_empty_for_zero_vector_seed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let records = vec![
        record_with_image_embedding(
            "legacy",
            "legacy window",
            vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        ),
        record_with_image_embedding("other", "other window", unit_vec_with_one_at(0)),
    ];
    store.add_batch(&records).await.expect("add records");

    let hits = store
        .similar_by_image_embedding("legacy", 5, None, None)
        .await
        .expect("similar");
    assert!(
        hits.is_empty(),
        "legacy zero-vector seed must yield no neighbors, got {hits:?}"
    );
}

#[tokio::test]
async fn similar_by_image_embedding_returns_empty_for_missing_seed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).unwrap())
        .await
        .unwrap();

    let hits = store
        .similar_by_image_embedding("does-not-exist", 5, None, None)
        .await
        .expect("similar");
    assert!(hits.is_empty(), "missing seed must yield no neighbors");
}

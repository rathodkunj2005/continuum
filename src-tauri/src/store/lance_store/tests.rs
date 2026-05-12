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
fn generate_search_aliases_entity_underscore_variant_without_acronym_noise() {
    let mut r = record(
        Some("https://example.com/x"),
        "Editor",
        "Touched config files.",
    );
    r.topic = "unknown".to_string();
    r.entities = vec!["fndr_search_pipeline".to_string()];
    let aliases = generate_search_aliases_public(&r);
    assert!(aliases.contains(&"fndr_search_pipeline".to_string()));
    assert!(aliases.contains(&"fndr search pipeline".to_string()));
    assert!(
        !aliases.iter().any(|a| a.len() == 2 && a.chars().all(|c| c.is_ascii_lowercase())),
        "single-token entities must not produce two-letter acronym noise: {aliases:?}"
    );
}

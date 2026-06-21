//! Integration coverage for browsing many memories without vector/LLM work.

use continuum_lib::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use continuum_lib::embedding::EMBEDDING_DIM;
use continuum_lib::storage::{MemoryRecord, Store};

fn record(index: usize, now_ms: i64) -> MemoryRecord {
    MemoryRecord {
        id: format!("record-{index:04}"),
        timestamp: now_ms - index as i64,
        day_bucket: chrono::Local::now().format("%Y-%m-%d").to_string(),
        app_name: if index.is_multiple_of(2) {
            "VS Code".to_string()
        } else {
            "Chrome".to_string()
        },
        bundle_id: None,
        window_title: format!("Memory browse regression {index}"),
        session_id: format!("session-{}", index / 10),
        text: format!("Investigated all memory card loading for record {index}."),
        clean_text: format!("Investigated all memory card loading for record {index}."),
        ocr_confidence: 0.95,
        ocr_block_count: 4,
        snippet: format!("Investigated all memory card loading for record {index}."),
        summary_source: "fallback".to_string(),
        noise_score: 0.02,
        session_key: format!("memory-browse-{}", index / 10),
        lexical_shadow: String::new(),
        embedding: vec![0.0; EMBEDDING_DIM],
        image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        screenshot_path: None,
        url: None,
        snippet_embedding: vec![0.0; EMBEDDING_DIM],
        support_embedding: vec![0.0; EMBEDDING_DIM],
        decay_score: 1.0,
        last_accessed_at: 0,
        ..Default::default()
    }
}

#[test]
fn list_recent_results_returns_requested_all_app_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path()).expect("store");
    let now_ms = chrono::Utc::now().timestamp_millis();
    let records = (0..1600)
        .map(|index| record(index, now_ms))
        .collect::<Vec<_>>();
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    rt.block_on(async {
        store.add_batch(&records).await.expect("add records");
    });

    let hits = rt
        .block_on(async { store.list_recent_results(1500, None).await })
        .expect("list recent results");

    assert_eq!(hits.len(), 1500);
    assert_eq!(hits.first().map(|hit| hit.id.as_str()), Some("record-0000"));
    assert_eq!(hits.last().map(|hit| hit.id.as_str()), Some("record-1499"));
    assert!(hits
        .windows(2)
        .all(|pair| pair[0].timestamp >= pair[1].timestamp));
}

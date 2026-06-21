use continuum_lib::embedding::{embedding_runtime_status, Embedder, EMBEDDING_DIM};
use continuum_lib::search::HybridSearcher;
use continuum_lib::storage::{MemoryRecord, Store};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Serialize)]
struct DiagnosticReport {
    embedding_backend: String,
    embedding_degraded: bool,
    embedding_detail: String,
    embedding_model_name: String,
    expected_dimension: usize,
    actual_dimension: usize,
    non_zero_embedding: bool,
    repeated_similarity: f32,
    inserted_records: usize,
    search_result_count: usize,
    top_result_id: Option<String>,
    top_result_score: Option<f32>,
    timings_ms: DiagnosticTimings,
}

#[derive(Debug, Default, Serialize)]
struct DiagnosticTimings {
    embed_ms: u128,
    store_open_ms: u128,
    insert_ms: u128,
    search_ms: u128,
    total_ms: u128,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--allow-mock") {
        std::env::set_var("CONTINUUM_ALLOW_MOCK_EMBEDDER", "1");
    }

    let total_start = Instant::now();
    let embed_start = Instant::now();
    let embedder = Embedder::new().map_err(|err| {
        format!(
            "Embedding diagnostic failed to initialize the real embedder: {err}. Run ./download_embedding_model.sh or pass --allow-mock for plumbing-only checks."
        )
    })?;

    let probe_text = "Continuum should find semantic memories about OAuth login errors.";
    let repeated_text = "Continuum should find semantic memories about OAuth login errors.";
    let embeddings = embedder.embed_batch(&[
        probe_text.to_string(),
        repeated_text.to_string(),
        "A grocery list with bananas and rice.".to_string(),
    ])?;
    let embed_ms = embed_start.elapsed().as_millis();

    let first = embeddings
        .first()
        .ok_or("Embedder returned no vectors for diagnostic probe")?;
    let actual_dimension = first.len();
    let non_zero_embedding = first.iter().any(|value| value.abs() > 1e-6);
    let repeated_similarity = cosine(first, &embeddings[1]);
    if actual_dimension != EMBEDDING_DIM {
        return Err(format!(
            "Embedding dimension mismatch: model returned {actual_dimension}, schema expects {EMBEDDING_DIM}"
        )
        .into());
    }
    if !non_zero_embedding {
        return Err("Embedding probe returned an all-zero vector".into());
    }

    let db_dir = diagnostic_dir();
    if db_dir.exists() {
        std::fs::remove_dir_all(&db_dir)?;
    }
    std::fs::create_dir_all(&db_dir)?;

    let store_start = Instant::now();
    let store = Store::new(&db_dir)?;
    let store_open_ms = store_start.elapsed().as_millis();
    let runtime = tokio::runtime::Runtime::new()?;

    let now = chrono::Utc::now().timestamp_millis();
    let records = vec![
        diagnostic_record(
            "diag-auth",
            now,
            "Terminal",
            "OAuth Debug",
            "Debugged an OAuth callback redirect URI mismatch after a login error.",
            embeddings[0].clone(),
        ),
        diagnostic_record(
            "diag-grocery",
            now - 1_000,
            "Notes",
            "Shopping",
            "Made a grocery list with bananas, rice, and coffee.",
            embeddings[2].clone(),
        ),
    ];

    let insert_start = Instant::now();
    runtime.block_on(async { store.add_batch_preserving_ids(&records).await })?;
    let insert_ms = insert_start.elapsed().as_millis();

    let search_start = Instant::now();
    let results = runtime.block_on(async {
        HybridSearcher::search(
            &store,
            &embedder,
            "that login callback problem",
            5,
            None,
            None,
        )
        .await
    })?;
    let search_ms = search_start.elapsed().as_millis();

    let status = embedding_runtime_status();
    let report = DiagnosticReport {
        embedding_backend: status.backend,
        embedding_degraded: status.degraded,
        embedding_detail: status.detail,
        embedding_model_name: status.model_name,
        expected_dimension: EMBEDDING_DIM,
        actual_dimension,
        non_zero_embedding,
        repeated_similarity,
        inserted_records: records.len(),
        search_result_count: results.len(),
        top_result_id: results.first().map(|result| result.id.clone()),
        top_result_score: results.first().map(|result| result.score),
        timings_ms: DiagnosticTimings {
            embed_ms,
            store_open_ms,
            insert_ms,
            search_ms,
            total_ms: total_start.elapsed().as_millis(),
        },
    };

    println!("{}", serde_json::to_string_pretty(&report)?);

    let _ = std::fs::remove_dir_all(&db_dir);
    Ok(())
}

fn diagnostic_dir() -> PathBuf {
    std::env::temp_dir().join(format!("continuum-diagnostic-{}", std::process::id()))
}

fn diagnostic_record(
    id: &str,
    timestamp: i64,
    app_name: &str,
    window_title: &str,
    text: &str,
    embedding: Vec<f32>,
) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        timestamp,
        day_bucket: "2026-04-29".to_string(),
        app_name: app_name.to_string(),
        bundle_id: None,
        window_title: window_title.to_string(),
        session_id: "diagnostic-session".to_string(),
        text: String::new(),
        clean_text: text.to_string(),
        ocr_confidence: 0.99,
        ocr_block_count: 1,
        snippet: text.to_string(),
        summary_source: "diagnostic".to_string(),
        noise_score: 0.0,
        session_key: format!("diagnostic:{}", app_name.to_ascii_lowercase()),
        lexical_shadow: text.to_string(),
        embedding: embedding.clone(),
        image_embedding: vec![0.0; continuum_lib::config::DEFAULT_IMAGE_EMBEDDING_DIM],
        screenshot_path: None,
        url: None,
        snippet_embedding: embedding.clone(),
        support_embedding: embedding,
        decay_score: 1.0,
        last_accessed_at: 0,
        ..Default::default()
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
    let a_norm = a.iter().map(|value| value * value).sum::<f32>().sqrt();
    let b_norm = b.iter().map(|value| value * value).sum::<f32>().sqrt();
    if a_norm <= 1e-6 || b_norm <= 1e-6 {
        0.0
    } else {
        dot / (a_norm * b_norm)
    }
}

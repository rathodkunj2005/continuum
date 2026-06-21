use continuum_lib::config::{SearchConfig, DEFAULT_IMAGE_EMBEDDING_DIM};
use continuum_lib::embedding::{Embedder, EMBEDDING_DIM};
use continuum_lib::search::HybridSearcher;
use continuum_lib::storage::{MemoryRecord, Store};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
struct EvalCase {
    query: String,
    relevant_ids: Vec<String>,
    expect_empty: bool,
}

fn eval_rows() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
)> {
    vec![
        (
            "mem_4000_overview",
            "Google Chrome",
            "ChatGPT - 4000",
            "User asked what is 4000 and explored Continuum intelligence layer concepts.",
            Some("https://chatgpt.com"),
        ),
        (
            "mem_4000_stats",
            "Google Chrome",
            "ChatGPT - 4000",
            "ChatGPT processed 141 memories in 3.1 hours with 100 percent screenshot coverage.",
            Some("https://chatgpt.com"),
        ),
        (
            "mem_canva_present",
            "Google Chrome",
            "Canva Presentation",
            "Created a Canva investor presentation with brand kit slides and narrative structure.",
            Some("https://www.canva.com"),
        ),
        (
            "mem_canva_resize",
            "Google Chrome",
            "Canva Resize",
            "Resized Canva design assets for Instagram and LinkedIn social media formats.",
            Some("https://www.canva.com"),
        ),
        (
            "mem_cricket_highlights",
            "Google Chrome",
            "IPL Highlights",
            "Watched IPL 2026 cricket highlights and reviewed match statistics on YouTube.",
            Some("https://www.youtube.com"),
        ),
        (
            "mem_cricket_schedule",
            "Google Chrome",
            "Cricket Schedule",
            "Checked cricket match schedule and IPL standings for upcoming fixtures.",
            Some("https://www.espncricinfo.com"),
        ),
        (
            "mem_knowledge_graph",
            "Codex",
            "Continuum Knowledge Graph",
            "Improved Continuum knowledge graph layout and discussed graph UI miniaturization.",
            None,
        ),
        (
            "mem_finder_recent",
            "Finder",
            "Recent Files",
            "Reviewed recent Finder files and project folders for context recall.",
            None,
        ),
        (
            "mem_weather_herriman",
            "Weather",
            "Herriman Weather",
            "Herriman weather was 34 degrees feels like with a freeze watch until April 17.",
            None,
        ),
        (
            "mem_activity_monitor",
            "Activity Monitor",
            "Energy Usage",
            "Monitored app energy impact and CPU usage over the last 12 hours.",
            None,
        ),
        (
            "mem_display_settings",
            "System Settings",
            "Display Settings",
            "Configured MacBook Pro display resolution and brightness in System Settings.",
            None,
        ),
        (
            "mem_video_abs",
            "Google Chrome",
            "Abs In 60 Days",
            "Viewed YouTube fitness video titled Its Tough But It Gets You Abs In 60 Days.",
            Some("https://www.youtube.com"),
        ),
        (
            "mem_global_capitals",
            "Google Chrome",
            "196 Capital Cities",
            "Watched challenge video about learning all 196 capital cities worldwide.",
            Some("https://www.youtube.com"),
        ),
        (
            "mem_continuum_uiux",
            "Google Chrome",
            "Continuum Stats",
            "Queried Continuum stats, video generation, and UI UX file analysis in ChatGPT.",
            Some("https://chatgpt.com"),
        ),
        (
            "mem_terminal_rust",
            "Terminal",
            "Rust Build",
            "Ran cargo test and investigated Rust compile errors in terminal logs.",
            None,
        ),
    ]
}

fn build_records(embedder: &Embedder) -> Vec<MemoryRecord> {
    let rows = eval_rows();
    let embedding_inputs = rows
        .iter()
        .map(|(_, app, title, text, _)| format!("{} {} {}", app, title, text))
        .collect::<Vec<_>>();
    let embeddings = embedder
        .embed_batch(&embedding_inputs)
        .expect("embedding generation for eval corpus");

    let now = chrono::Utc::now().timestamp_millis();

    rows.into_iter()
        .enumerate()
        .map(|(idx, (id, app, title, text, url))| MemoryRecord {
            id: id.to_string(),
            timestamp: now - (idx as i64 * 60_000),
            day_bucket: "2026-04-15".to_string(),
            app_name: app.to_string(),
            bundle_id: None,
            window_title: title.to_string(),
            session_id: format!("eval-session-{}", idx),
            text: text.to_string(),
            clean_text: text.to_string(),
            ocr_confidence: 0.93,
            ocr_block_count: 10,
            snippet: text.to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.06,
            session_key: format!("{}:{}", app.to_lowercase(), id),
            lexical_shadow: String::new(),
            embedding: embeddings[idx].clone(),
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            screenshot_path: None,
            url: url.map(|value| value.to_string()),
            snippet_embedding: vec![0.0; EMBEDDING_DIM],
            support_embedding: vec![0.0; EMBEDDING_DIM],
            decay_score: 1.0,
            last_accessed_at: 0,
            ..Default::default()
        })
        .collect()
}

fn precision_at_k(hit_ids: &[String], relevant: &HashSet<String>, k: usize) -> f32 {
    if k == 0 {
        return 0.0;
    }
    let matched = hit_ids
        .iter()
        .take(k)
        .filter(|id| relevant.contains(*id))
        .count();
    matched as f32 / k as f32
}

fn reciprocal_rank(hit_ids: &[String], relevant: &HashSet<String>) -> f32 {
    for (index, id) in hit_ids.iter().enumerate() {
        if relevant.contains(id) {
            return 1.0 / (index as f32 + 1.0);
        }
    }
    0.0
}

#[test]
fn hybrid_search_relevance_eval_suite() {
    std::env::set_var("CONTINUUM_ALLOW_MOCK_EMBEDDER", "1");
    let cases: Vec<EvalCase> =
        serde_json::from_str(include_str!("fixtures/search_eval_cases.json"))
            .expect("valid search eval fixture");

    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path()).expect("store");
    let embedder = Embedder::new().expect("embedder");

    let records = build_records(&embedder);
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        store.add_batch(&records).await.expect("add records");
    });

    let mut positive_cases = 0usize;
    let mut precision_sum = 0.0f32;
    let mut mrr_sum = 0.0f32;
    let mut negative_failures = Vec::new();

    // This suite measures ranking quality, not latency. The production default
    // search budgets (keyword 900ms total / 320ms per variant, etc.) intermittently
    // fire under the multi-threaded test runtime / loaded CI, making a route return
    // EMPTY and tanking precision/MRR nondeterministically. Use generous budgets so
    // retrieval is deterministic and we measure the variable under test.
    let search_config = SearchConfig {
        semantic_timeout_ms: 10_000,
        snippet_timeout_ms: 10_000,
        keyword_timeout_ms: 10_000,
        keyword_variant_timeout_ms: 5_000,
        ..SearchConfig::default()
    };

    for case in &cases {
        let hits = rt
            .block_on(async {
                HybridSearcher::search_with_config(
                    &store,
                    &embedder,
                    &case.query,
                    6,
                    None,
                    None,
                    &search_config,
                )
                .await
            })
            .expect("search query");
        let hit_ids = hits.into_iter().map(|item| item.id).collect::<Vec<_>>();
        println!("case {:?} -> {:?}", case.query, hit_ids);

        if case.expect_empty {
            if !hit_ids.is_empty() {
                negative_failures.push((case.query.clone(), hit_ids));
            }
            continue;
        }

        positive_cases += 1;
        let relevant = case
            .relevant_ids
            .iter()
            .cloned()
            .collect::<HashSet<String>>();
        precision_sum += precision_at_k(&hit_ids, &relevant, 6);
        mrr_sum += reciprocal_rank(&hit_ids, &relevant);
    }

    if !negative_failures.is_empty() {
        panic!("negative queries returned hits: {:?}", negative_failures);
    }

    let avg_precision_at_6 = precision_sum / positive_cases as f32;
    let avg_mrr = mrr_sum / positive_cases as f32;

    println!(
        "search eval -> cases={} avg_precision@6={:.3} avg_mrr={:.3}",
        positive_cases, avg_precision_at_6, avg_mrr
    );

    // Most positive cases have a single relevant id → precision@6 = 1/6 ≈ 0.167
    // is the natural floor for "right answer in top 6". MRR catches ranking quality.
    assert!(
        avg_precision_at_6 >= 0.16,
        "expected avg precision@6 >= 0.16, got {:.3}",
        avg_precision_at_6
    );
    assert!(
        avg_mrr >= 0.72,
        "expected avg MRR >= 0.72, got {:.3}",
        avg_mrr
    );
}

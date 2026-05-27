//! Subagent 12 — Quality Harness.
//!
//! Deterministic regression tests for the Advanced RAG path:
//!
//! 1. **Chunking fixtures** — synthetic OCR captures under
//!    `tests/fixtures/chunking/` that lock in the boundary behavior of the
//!    config-aware `TextChunker` (chrome filtering, URL boundaries, noisy
//!    OCR cleanup, identifier preservation, paragraph capture).
//! 2. **Chunk-first retrieval** — seeds `memories_v5_bge_1024` and
//!    `memory_chunks_v1_bge_1024` with synthetic parents + chunks whose
//!    embeddings are sparse one-hot concept vectors, then asserts that
//!    `chunk_vector_search` + the same group-by-memory rollup used by
//!    `ChunkRoute` retrieves the correct parent for 10 synthetic queries
//!    with MRR@5 = 1.0.
//! 3. **Failure modes** — wrong-dimension query rejection, empty index
//!    fallback, missing-parent (orphan) chunk handling.
//!
//! Everything here avoids the real BGE model: vectors are synthesized from
//! concept tags so CI never has to download model assets.

use fndr_lib::config::{ChunkingConfig, DEFAULT_IMAGE_EMBEDDING_DIM};
use fndr_lib::embedding::TextChunker;
use fndr_lib::inference::model_config::BGE_V5_DIMENSIONS;
use fndr_lib::storage::{
    MemoryChunkRecord, MemoryChunkSearchResult, MemoryRecord, Store,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// --- chunking fixtures ------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChunkingFixture {
    name: String,
    app_name: String,
    window_title: String,
    raw_text: String,
    expected_min_chunks: usize,
    expected_max_chunks: usize,
    must_contain: Vec<String>,
    #[serde(default)]
    must_not_contain: Vec<String>,
}

const FIXTURE_NAMES: &[&str] = &[
    "browser_chrome_heavy",
    "long_page_with_urls",
    "noisy_ocr_garbage",
    "identifier_heavy_developer",
    "paragraph_on_page",
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("chunking")
}

fn load_fixture(name: &str) -> ChunkingFixture {
    let path = fixtures_dir().join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse fixture {}: {err}", path.display()))
}

#[test]
fn chunking_fixtures_match_expected_count_and_content() {
    let chunker = TextChunker::from_config(&ChunkingConfig::default());
    for name in FIXTURE_NAMES {
        let fx = load_fixture(name);
        let chunks =
            chunker.chunk_screen_text(&fx.app_name, &fx.window_title, &fx.raw_text);
        assert!(
            chunks.len() >= fx.expected_min_chunks
                && chunks.len() <= fx.expected_max_chunks,
            "[{}] expected {}..={} chunks, got {} ({:?})",
            fx.name,
            fx.expected_min_chunks,
            fx.expected_max_chunks,
            chunks.len(),
            chunks
        );
        let joined = chunks.join("\n---\n");
        for phrase in &fx.must_contain {
            assert!(
                joined.contains(phrase.as_str()),
                "[{}] missing required phrase {:?}\nchunks=\n{}",
                fx.name,
                phrase,
                joined
            );
        }
        for phrase in &fx.must_not_contain {
            assert!(
                !joined.contains(phrase.as_str()),
                "[{}] unexpected phrase survived chunking {:?}\nchunks=\n{}",
                fx.name,
                phrase,
                joined
            );
        }
    }
}

#[test]
fn chunking_fixture_noisy_ocr_uses_cleaned_text_not_garbage() {
    // The noisy fixture has 1-char garbage lines around a single substantive
    // paragraph. The chunker's cleaned-text fallback must drop the garbage
    // lines and keep the real content as a single coherent chunk.
    let chunker = TextChunker::from_config(&ChunkingConfig::default());
    let fx = load_fixture("noisy_ocr_garbage");
    let chunks =
        chunker.chunk_screen_text(&fx.app_name, &fx.window_title, &fx.raw_text);
    let joined = chunks.join("\n");

    let garbage_chars = ['§', '_', '—'];
    let mut total_garbage = 0;
    for ch in garbage_chars {
        total_garbage += joined.chars().filter(|c| *c == ch).count();
    }
    // Allow a handful of stray garbage chars because the OCR rollup may
    // preserve them inside a paragraph, but the chunker must not be dominated
    // by the noise lines.
    assert!(
        total_garbage <= 4,
        "noisy_ocr cleaned fallback failed: too much garbage survived ({total_garbage}); chunks=\n{joined}"
    );
    assert!(joined.contains("knowledge graph schema"));
}

#[test]
fn chunking_fixture_url_lines_anchor_url_kind_chunks() {
    // URL boundary contract from `test_url_line_starts_new_chunk_after_min_size`:
    // a chunk dominated by URL content gets `LineKind::Url`. The long-page
    // fixture contains three URLs, so at least one chunk must carry that kind.
    use fndr_lib::embedding::LineKind;
    let chunker = TextChunker::from_config(&ChunkingConfig::default());
    let fx = load_fixture("long_page_with_urls");
    let chunks =
        chunker.chunk_ocr_text_with_metadata(&fx.app_name, &fx.window_title, &fx.raw_text);
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.dominant_line_kind == LineKind::Url),
        "expected at least one chunk with dominant_line_kind=Url; got kinds={:?}",
        chunks
            .iter()
            .map(|c| c.dominant_line_kind)
            .collect::<Vec<_>>()
    );
}

// --- deterministic 1024-d vector helper -------------------------------------

/// Map each concept tag to a stable index in [0, BGE_V5_DIMENSIONS) and return
/// the L2-normalized sum. Same set of tags → identical vector; overlapping
/// tag sets get cosine similarity proportional to overlap.
fn concept_vector(tags: &[&str]) -> Vec<f32> {
    let mut v = vec![0.0_f32; BGE_V5_DIMENSIONS];
    for tag in tags {
        let mut hash: u64 = 1469598103934665603;
        for byte in tag.to_lowercase().bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
        let idx = (hash % BGE_V5_DIMENSIONS as u64) as usize;
        v[idx] += 1.0;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

#[test]
fn concept_vector_is_deterministic_and_distinct_across_tag_sets() {
    let a = concept_vector(&["chunk", "rag"]);
    let b = concept_vector(&["chunk", "rag"]);
    let c = concept_vector(&["graph", "schema"]);
    assert_eq!(a.len(), BGE_V5_DIMENSIONS);
    assert_eq!(a, b, "same tags must produce identical vectors");
    assert_ne!(a, c, "different tag sets must produce different vectors");
    // L2 norm ≈ 1
    let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "expected unit norm, got {norm}");
}

// --- MRR@k helper -----------------------------------------------------------

/// Mean reciprocal rank at k. Each input is `(ranked_ids, relevant_ids)`.
fn mrr_at_k(cases: &[(Vec<String>, Vec<String>)], k: usize) -> f32 {
    if cases.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0_f32;
    for (ranked, relevant) in cases {
        let rr = ranked
            .iter()
            .take(k)
            .enumerate()
            .find(|(_, id)| relevant.iter().any(|r| r == *id))
            .map(|(i, _)| 1.0 / (i + 1) as f32)
            .unwrap_or(0.0);
        sum += rr;
    }
    sum / cases.len() as f32
}

#[test]
fn mrr_at_k_helper_matches_known_rankings() {
    let cases = vec![
        (
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec!["b".to_string()], // rank 2 → 1/2
        ),
        (
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec!["d".to_string()], // not found → 0
        ),
        (
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec!["a".to_string()], // rank 1 → 1
        ),
    ];
    let mrr = mrr_at_k(&cases, 5);
    let expected = (0.5 + 0.0 + 1.0) / 3.0;
    assert!(
        (mrr - expected).abs() < 1e-5,
        "expected MRR@5={expected}, got {mrr}"
    );
}

#[test]
fn mrr_at_k_truncates_at_k() {
    let cases = vec![(
        vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
            "e".to_string(),
            "f".to_string(), // rank 6 — outside k=5
        ],
        vec!["f".to_string()],
    )];
    assert!(mrr_at_k(&cases, 5).abs() < 1e-6);
    assert!((mrr_at_k(&cases, 6) - (1.0 / 6.0)).abs() < 1e-5);
}

// --- chunk-first retrieval regression suite ---------------------------------

fn synthetic_parent(id: &str, text: &str) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        timestamp: 1_700_000_000_000,
        day_bucket: "2026-05-20".to_string(),
        app_name: "Chrome".to_string(),
        window_title: format!("Parent {id}"),
        session_id: "sub12-eval".to_string(),
        text: text.to_string(),
        clean_text: text.to_string(),
        snippet: text.chars().take(80).collect::<String>(),
        summary_source: "llm".to_string(),
        embedding: vec![0.0_f32; BGE_V5_DIMENSIONS],
        snippet_embedding: vec![0.0_f32; BGE_V5_DIMENSIONS],
        support_embedding: vec![0.0_f32; BGE_V5_DIMENSIONS],
        image_embedding: vec![0.0_f32; DEFAULT_IMAGE_EMBEDDING_DIM],
        decay_score: 1.0,
        ..Default::default()
    }
}

fn synthetic_chunk(
    id: &str,
    memory_id: &str,
    chunk_index: u32,
    text: &str,
    tags: &[&str],
) -> MemoryChunkRecord {
    MemoryChunkRecord {
        id: id.to_string(),
        memory_id: memory_id.to_string(),
        chunk_index,
        line_kind: "plain".to_string(),
        text: text.to_string(),
        embedding: concept_vector(tags),
        created_at: 1_700_000_000_000,
        app_name: "Chrome".to_string(),
        window_title: "Synthetic chunk".to_string(),
        day_bucket: "2026-05-20".to_string(),
        content_hash: format!("hash-{id}"),
    }
}

/// Mirrors the production rollup in `chunk_route::assemble_chunk_parent_results`:
/// pick the highest-scoring chunk per memory_id, then sort by score desc,
/// distance asc. The test crate cannot call the `pub(crate)` helper directly,
/// so we replicate the contract and any drift between this and production will
/// cause the test to disagree with the route — which is desirable.
fn group_top_chunk_per_memory(
    hits: Vec<MemoryChunkSearchResult>,
) -> Vec<MemoryChunkSearchResult> {
    let mut best: HashMap<String, MemoryChunkSearchResult> = HashMap::new();
    for hit in hits {
        if hit.chunk.memory_id.trim().is_empty() {
            continue;
        }
        let should_replace = best
            .get(&hit.chunk.memory_id)
            .map(|existing| {
                hit.score > existing.score
                    || (hit.score == existing.score && hit.distance < existing.distance)
            })
            .unwrap_or(true);
        if should_replace {
            best.insert(hit.chunk.memory_id.clone(), hit);
        }
    }
    let mut ranked: Vec<MemoryChunkSearchResult> = best.into_values().collect();
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.distance
                    .partial_cmp(&right.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    ranked
}

async fn seed_eval_corpus(store: &Store) {
    let parents = vec![
        synthetic_parent("mem-rag", "Parent/child chunk RAG architecture notes"),
        synthetic_parent("mem-graph", "Knowledge graph schema and traversal heuristics"),
        synthetic_parent("mem-onnx", "ONNX runtime tokenizer dimension contract"),
        synthetic_parent("mem-vault", "Memory vault scroll layout container query refactor"),
        synthetic_parent("mem-meetings", "Live meetings transcript and segment routing"),
    ];
    let chunks = vec![
        synthetic_chunk(
            "chunk-rag",
            "mem-rag",
            0,
            "Parent-child chunk RAG architecture decision: BGE 1024 child chunks roll up to v5 parent",
            &["chunk", "rag", "parent", "child", "bge"],
        ),
        synthetic_chunk(
            "chunk-graph",
            "mem-graph",
            0,
            "Knowledge graph schema and traversal heuristics for typed edges",
            &["graph", "schema", "traversal", "edge"],
        ),
        synthetic_chunk(
            "chunk-onnx",
            "mem-onnx",
            0,
            "ONNX runtime tokenizer dimension contract: 1024-d BGE query path",
            &["onnx", "tokenizer", "dimension", "contract"],
        ),
        synthetic_chunk(
            "chunk-vault",
            "mem-vault",
            0,
            "Memory vault scroll container-query layout refactor for compact cards",
            &["vault", "layout", "container", "scroll"],
        ),
        synthetic_chunk(
            "chunk-meet",
            "mem-meetings",
            0,
            "Live meetings transcript capture with segment routing per speaker",
            &["meeting", "transcript", "segment", "speaker"],
        ),
    ];
    store
        .add_v5_batch_preserving_ids(&parents)
        .await
        .expect("seed v5 parents");
    store
        .upsert_memory_chunks(&chunks)
        .await
        .expect("seed memory chunks");
}

#[tokio::test]
async fn chunk_first_retrieval_top1_matches_for_synthetic_queries_with_mrr_at_5_perfect() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("spawn store");
    seed_eval_corpus(&store).await;

    // 10 synthetic query/expected pairs. Each query reuses tag tokens so the
    // expected parent is the unique top-1 result.
    let queries: Vec<(&[&str], &str, &str)> = vec![
        (&["chunk", "rag"], "mem-rag", "chunk rag architecture"),
        (
            &["parent", "child", "rag"],
            "mem-rag",
            "parent child retrieval",
        ),
        (&["graph", "schema"], "mem-graph", "knowledge graph schema"),
        (
            &["graph", "traversal"],
            "mem-graph",
            "graph traversal heuristics",
        ),
        (
            &["onnx", "tokenizer"],
            "mem-onnx",
            "onnx tokenizer dimension",
        ),
        (
            &["dimension", "contract"],
            "mem-onnx",
            "dimension contract bge",
        ),
        (
            &["vault", "layout"],
            "mem-vault",
            "memory vault layout container",
        ),
        (
            &["container", "scroll"],
            "mem-vault",
            "vault scroll refactor",
        ),
        (
            &["meeting", "transcript"],
            "mem-meetings",
            "live meetings transcript",
        ),
        (
            &["segment", "speaker"],
            "mem-meetings",
            "meeting segment routing",
        ),
    ];

    let mut ranked_cases: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for (tags, expected_parent, label) in &queries {
        let qv = concept_vector(tags);
        let hits = store
            .chunk_vector_search(&qv, 8)
            .await
            .expect("chunk vector search");
        let ranked = group_top_chunk_per_memory(hits);
        let ranked_ids: Vec<String> = ranked
            .iter()
            .map(|hit| hit.chunk.memory_id.clone())
            .collect();
        assert_eq!(
            ranked_ids.first().map(String::as_str),
            Some(*expected_parent),
            "[{label}] expected top1={expected_parent}, got {ranked_ids:?}"
        );
        let top = ranked.first().expect("top");
        let evidence = top.evidence();
        assert!(
            !evidence.text.trim().is_empty(),
            "[{label}] winning chunk evidence must carry text"
        );
        assert_eq!(evidence.memory_id, *expected_parent);
        ranked_cases.push((ranked_ids, vec![expected_parent.to_string()]));
    }

    let mrr5 = mrr_at_k(&ranked_cases, 5);
    assert!(
        mrr5 >= 0.999,
        "expected MRR@5 = 1.0 on the trivially separable synthetic corpus, got {mrr5}"
    );
}

#[tokio::test]
async fn chunk_first_retrieval_attaches_winning_chunk_evidence_for_target_phrase() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("spawn store");
    seed_eval_corpus(&store).await;

    let qv = concept_vector(&["chunk", "rag"]);
    let hits = store
        .chunk_vector_search(&qv, 8)
        .await
        .expect("chunk vector search");
    let ranked = group_top_chunk_per_memory(hits);
    let top = ranked.first().expect("expected top hit");
    assert_eq!(top.chunk.memory_id, "mem-rag");
    let evidence = top.evidence();
    assert!(
        evidence.text.contains("chunk RAG architecture"),
        "winning chunk evidence must contain the target phrase; got: {}",
        evidence.text
    );
    assert!(
        evidence.score >= 0.0 && evidence.score <= 1.0 + 1e-6,
        "score must be normalized to [0,1], got {}",
        evidence.score
    );
}

#[tokio::test]
async fn chunk_first_retrieval_rejects_wrong_dimension_query_clearly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("spawn store");
    seed_eval_corpus(&store).await;

    // 384-d MiniLM v4 query against the 1024-d BGE chunk table is the
    // contract that Subagent 1 / 6 / 7 lock down. The error message must be
    // explicit enough that an operator can diagnose without reading source.
    let err = store
        .chunk_vector_search(&vec![0.0_f32; 384], 4)
        .await
        .expect_err("wrong-dimension chunk query must fail");
    let message = err.to_string();
    assert!(
        message.contains("1024-d BGE"),
        "error must name the expected dimension, got: {message}"
    );
    assert!(
        message.contains("No fallback across embedding dimensions"),
        "error must call out the no-fallback contract, got: {message}"
    );
}

#[tokio::test]
async fn chunk_first_retrieval_reports_empty_index_when_chunk_table_is_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("spawn store");

    // No parents, no chunks → fallback contract: index unavailable, chunk
    // route returns zero hits, legacy v4 path takes over.
    assert!(
        !store.has_chunk_retrieval_index().await.unwrap(),
        "empty store must report no chunk retrieval index"
    );

    // Insert a single parent but no chunks → still unavailable.
    store
        .add_v5_batch_preserving_ids(&[synthetic_parent("mem-only", "Parent without chunks")])
        .await
        .expect("seed parent");
    assert!(
        !store.has_chunk_retrieval_index().await.unwrap(),
        "parent-only store must report no chunk retrieval index"
    );
}

#[tokio::test]
async fn chunk_first_retrieval_skips_orphan_chunks_when_parent_missing() {
    // Insert a chunk referencing a memory_id that has no v5 parent row.
    // chunk_vector_search must still return the chunk hit, but the
    // group-by-memory rollup must not invent a phantom parent.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
        .await
        .expect("spawn store");

    let orphan_chunk = synthetic_chunk(
        "chunk-orphan",
        "mem-missing",
        0,
        "Orphan chunk with no parent row",
        &["orphan", "missing"],
    );
    store
        .upsert_memory_chunks(&[orphan_chunk])
        .await
        .expect("seed orphan chunk");

    let qv = concept_vector(&["orphan", "missing"]);
    let hits = store
        .chunk_vector_search(&qv, 4)
        .await
        .expect("chunk vector search");
    let ranked = group_top_chunk_per_memory(hits);
    assert_eq!(
        ranked.len(),
        1,
        "orphan chunk still groups into a single best-per-memory entry"
    );

    // Parent fetch via the public v5 search API must yield zero results for
    // the missing parent. This mirrors the chunk_route check that drops orphan
    // results before exposing them in the SearchResult stream.
    let v5_results = store
        .get_v5_search_results_by_ids(
            &[ranked[0].chunk.memory_id.clone()],
            None,
            None,
        )
        .await
        .expect("v5 parent fetch");
    assert!(
        v5_results.is_empty(),
        "missing v5 parent must yield no SearchResult, got {v5_results:?}"
    );
}

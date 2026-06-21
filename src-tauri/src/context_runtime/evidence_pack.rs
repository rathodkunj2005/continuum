//! Phase 3 evidence collection: join fused hits to their underlying
//! `MemoryRecord` rows and aggregate file / command / decision / error / todo /
//! URL refs, deduplicated and back-pointing to source memories.

use crate::context_runtime::context_pack::{
    CommandRef, DecisionRef, ErrorRef, EvidencePack, FileRef, FusedHit, TaskRef, UrlRef,
};
use crate::storage::Store;
use crate::telemetry::runtime_metrics;
use std::collections::HashMap;

pub async fn collect_evidence(hits: &[FusedHit], store: &Store) -> EvidencePack {
    let mut files: HashMap<String, Vec<String>> = HashMap::new();
    let mut commands: HashMap<String, Vec<String>> = HashMap::new();
    let mut decisions: HashMap<String, Vec<String>> = HashMap::new();
    let mut errors: HashMap<String, Vec<String>> = HashMap::new();
    let mut todos: HashMap<String, Vec<String>> = HashMap::new();
    let mut urls: HashMap<String, Vec<String>> = HashMap::new();

    for hit in hits {
        let Ok(Some(record)) = store.get_memory_by_id(&hit.memory_id).await else {
            continue;
        };
        push_all(&mut files, &record.files_touched, &record.id);
        push_all(&mut commands, &record.commands, &record.id);
        push_all(&mut decisions, &record.decisions, &record.id);
        push_all(&mut errors, &record.errors, &record.id);
        push_all(&mut todos, &record.next_steps, &record.id);
        push_all(&mut todos, &record.todos, &record.id);
        if let Some(url) = record.url.as_ref().filter(|u| !u.trim().is_empty()) {
            push_one(&mut urls, url, &record.id);
        }
    }

    let pack = EvidencePack {
        files: into_refs(files, |key, memory_ids| FileRef {
            path: key,
            memory_ids,
        }),
        commands: into_refs(commands, |key, memory_ids| CommandRef {
            command: key,
            memory_ids,
        }),
        decisions: into_refs(decisions, |key, memory_ids| DecisionRef {
            decision: key,
            memory_ids,
        }),
        errors: into_refs(errors, |key, memory_ids| ErrorRef {
            error: key,
            memory_ids,
        }),
        todos: into_refs(todos, |key, memory_ids| TaskRef {
            task: key,
            memory_ids,
        }),
        urls: into_refs(urls, |key, memory_ids| UrlRef {
            url: key,
            memory_ids,
        }),
    };

    for _ in &pack.files {
        runtime_metrics::bump("continuum.retrieval.evidence.file.count");
    }
    for _ in &pack.decisions {
        runtime_metrics::bump("continuum.retrieval.evidence.decision.count");
    }
    for _ in &pack.commands {
        runtime_metrics::bump("continuum.retrieval.evidence.command.count");
    }

    pack
}

fn push_all(map: &mut HashMap<String, Vec<String>>, values: &[String], memory_id: &str) {
    for value in values {
        push_one(map, value, memory_id);
    }
}

fn push_one(map: &mut HashMap<String, Vec<String>>, raw: &str, memory_id: &str) {
    let key = raw.trim();
    if key.is_empty() {
        return;
    }
    let entry = map.entry(key.to_string()).or_default();
    if !entry.iter().any(|id| id == memory_id) {
        entry.push(memory_id.to_string());
    }
}

fn into_refs<T>(
    map: HashMap<String, Vec<String>>,
    build: impl Fn(String, Vec<String>) -> T,
) -> Vec<T> {
    let mut sorted: Vec<(String, Vec<String>)> = map.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
        .into_iter()
        .map(|(key, ids)| build(key, ids))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_runtime::context_pack::{FusionSignals, SurfacingReason};
    use crate::embedding::EMBEDDING_DIM;
    use crate::storage::MemoryRecord;

    fn fused(memory_id: &str) -> FusedHit {
        FusedHit {
            memory_id: memory_id.to_string(),
            score: 1.0,
            signals: FusionSignals::default(),
            surfacing_reason: SurfacingReason::default(),
            contributing_routes: Vec::new(),
        }
    }

    #[tokio::test]
    async fn evidence_dedupes_across_memories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("blocking");

        let now = chrono::Utc::now().timestamp_millis();
        let make = |id: &str, text: &str, ts: i64, files: Vec<String>| MemoryRecord {
            id: id.to_string(),
            text: text.to_string(),
            clean_text: text.to_string(),
            snippet: text.to_string(),
            app_name: "Terminal".to_string(),
            window_title: format!("Continuum {id}"),
            project: "Continuum".to_string(),
            timestamp: ts,
            embedding: vec![0.0; EMBEDDING_DIM],
            snippet_embedding: vec![0.0; EMBEDDING_DIM],
            support_embedding: vec![0.0; EMBEDDING_DIM],
            confidence_score: 0.8,
            files_touched: files,
            ..Default::default()
        };
        let mut rec_a = make(
            "m-1",
            "Continuum planner debounce fix landed in plan.ts after the alpha bug",
            now,
            vec!["plan.ts".to_string()],
        );
        rec_a.commands = vec!["cargo test".to_string()];
        rec_a.decisions = vec!["use debounce".to_string()];
        let rec_b = make(
            "m-2",
            "Continuum follow-up: refactor plan.ts and fix.ts with the new debounce semantics",
            now + 1_000,
            vec!["plan.ts".to_string(), "fix.ts".to_string()],
        );

        store.add_batch(&[rec_a, rec_b]).await.expect("add");

        let pack = collect_evidence(&[fused("m-1"), fused("m-2")], &store).await;
        let plan_ref = pack.files.iter().find(|r| r.path == "plan.ts").unwrap();
        assert_eq!(plan_ref.memory_ids.len(), 2);
        assert!(pack.files.iter().any(|r| r.path == "fix.ts"));
        assert!(pack.commands.iter().any(|r| r.command == "cargo test"));
        assert!(pack.decisions.iter().any(|r| r.decision == "use debounce"));
    }
}

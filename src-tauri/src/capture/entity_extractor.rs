//! Deterministic entity extraction from **finalized** memory fields only (no raw OCR, no Lance).

use crate::memory::graph::schema::{GraphEdge, GraphEdgeType, GraphNode, GraphNodeType};
use crate::storage::MemoryRecord;
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

const MIN_LABEL: usize = 3;
const MAX_LABEL: usize = 80;
const MAX_NODES: usize = 8;
const EDGE_CONF_MIN: f32 = 0.4;

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Mean of node confidences (empty => 0).
    pub overall_confidence: f32,
}

fn norm_label(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn is_stop_phrase(lower: &str) -> bool {
    const STOPS: &[&str] = &[
        "the", "and", "for", "with", "from", "this", "that", "unknown", "none", "n/a", "untitled",
        "page", "window", "document", "loading", "error", "home", "search",
    ];
    STOPS.iter().any(|w| *w == lower)
}

fn acceptable_label(label: &str) -> bool {
    let t = norm_label(label);
    let c = t.chars().count();
    if c < MIN_LABEL || c > MAX_LABEL {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if is_stop_phrase(&lower) {
        return false;
    }
    true
}

fn stable_node_id(memory_id: &str, kind: GraphNodeType, label_key: &str) -> Uuid {
    let key = format!("{memory_id}|{kind:?}|{}", label_key.to_ascii_lowercase());
    Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes())
}

fn push_node(
    out: &mut Vec<GraphNode>,
    seen: &mut HashSet<(String, GraphNodeType)>,
    memory_id: &str,
    node_type: GraphNodeType,
    label: String,
    confidence: f32,
) {
    if out.len() >= MAX_NODES {
        return;
    }
    if !acceptable_label(&label) {
        return;
    }
    let key = (label.to_ascii_lowercase(), node_type);
    if !seen.insert(key) {
        return;
    }
    let id = stable_node_id(memory_id, node_type, &label);
    let now = Utc::now();
    out.push(GraphNode {
        id,
        node_type,
        label: label.chars().take(MAX_LABEL).collect(),
        confidence: confidence.clamp(0.0, 1.0),
        source_memory_ids: vec![memory_id.to_string()],
        embedding: None,
        created_at: now,
        updated_at: now,
        stale: false,
        metadata: serde_json::json!({}),
    });
}

fn push_edge(
    edges: &mut Vec<GraphEdge>,
    source: Uuid,
    target: Uuid,
    edge_type: GraphEdgeType,
    confidence: f32,
    conflict_flag: bool,
) {
    if confidence < EDGE_CONF_MIN {
        return;
    }
    let now = Utc::now();
    edges.push(GraphEdge {
        id: Uuid::new_v4(),
        source_id: source,
        target_id: target,
        edge_type,
        confidence,
        conflict_flag,
        created_at: now,
        metadata: serde_json::json!({}),
    });
}

/// Extract graph nodes/edges from a persisted memory row using only non-OCR finalized fields.
pub fn extract(record: &MemoryRecord) -> ExtractionResult {
    let mut nodes = Vec::new();
    let mut seen_keys = HashSet::new();
    let mid = record.id.as_str();

    let ic = record.insight_card_confidence.clamp(0.0, 1.0);
    let base = (ic * 0.55 + record.confidence_score.clamp(0.0, 1.0) * 0.45).clamp(0.0, 1.0);

    if !record.project.trim().is_empty() && record.project != "unknown" {
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Project,
            norm_label(&record.project),
            base.max(0.45),
        );
    }

    if !record.topic.trim().is_empty() && record.topic != "unknown" {
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Concept,
            norm_label(&record.topic),
            (base * 0.95).max(0.35),
        );
    }

    if let Some(url) = record.url.as_ref().filter(|u| !u.trim().is_empty()) {
        let hostish = url.split("//").nth(1).unwrap_or(url);
        let label = hostish.chars().take(MAX_LABEL).collect::<String>();
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Url,
            label,
            (base * 0.9).max(0.4),
        );
    }

    for path in record.files_touched.iter().take(3) {
        let leaf = std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path);
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::File,
            norm_label(leaf),
            (base * 0.85).max(0.35),
        );
    }

    for d in record.decisions.iter().take(2) {
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Decision,
            norm_label(d),
            (base * 0.92).max(0.42),
        );
    }

    for e in record.errors.iter().take(2) {
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Error,
            norm_label(e),
            (base * 0.8).max(0.4),
        );
    }

    for t in record.related_tools.iter().take(2) {
        if t.trim().is_empty() {
            continue;
        }
        push_node(
            &mut nodes,
            &mut seen_keys,
            mid,
            GraphNodeType::Tool,
            norm_label(t),
            (base * 0.78).max(0.35),
        );
    }

    let session_label = format!(
        "session {}",
        record.session_id.chars().take(12).collect::<String>()
    );
    push_node(
        &mut nodes,
        &mut seen_keys,
        mid,
        GraphNodeType::Session,
        session_label,
        (base * 0.5).max(0.3),
    );

    // Insight narrative (structured, not raw OCR).
    for (text, boost) in [
        (&record.insight_what_happened, 0.9f32),
        (&record.insight_why_mattered, 0.85),
        (&record.insight_what_changed, 0.75),
    ] {
        let t = norm_label(text);
        if acceptable_label(&t) {
            push_node(
                &mut nodes,
                &mut seen_keys,
                mid,
                GraphNodeType::Concept,
                t,
                (base * boost).max(0.36),
            );
        }
    }

    let mut edges = Vec::new();

    fn first_id(nodes: &[GraphNode], t: GraphNodeType) -> Option<Uuid> {
        nodes.iter().find(|n| n.node_type == t).map(|n| n.id)
    }

    if let (Some(s), Some(p)) = (
        first_id(&nodes, GraphNodeType::Session),
        first_id(&nodes, GraphNodeType::Project),
    ) {
        push_edge(&mut edges, s, p, GraphEdgeType::PartOf, base * 0.75, false);
    }
    if let (Some(s), Some(c)) = (
        first_id(&nodes, GraphNodeType::Session),
        first_id(&nodes, GraphNodeType::Concept),
    ) {
        push_edge(
            &mut edges,
            s,
            c,
            GraphEdgeType::MentionedIn,
            base * 0.7,
            false,
        );
    }
    if let (Some(proj), Some(u)) = (
        first_id(&nodes, GraphNodeType::Project),
        first_id(&nodes, GraphNodeType::Url),
    ) {
        push_edge(
            &mut edges,
            proj,
            u,
            GraphEdgeType::AppliesTo,
            base * 0.72,
            false,
        );
    }
    let dec = nodes
        .iter()
        .find(|n| n.node_type == GraphNodeType::Decision)
        .map(|n| n.id);
    let errn = nodes
        .iter()
        .find(|n| n.node_type == GraphNodeType::Error)
        .map(|n| n.id);
    if let (Some(d), Some(err)) = (dec, errn) {
        push_edge(
            &mut edges,
            d,
            err,
            GraphEdgeType::Contradicts,
            base * 0.65,
            true,
        );
        push_edge(
            &mut edges,
            d,
            err,
            GraphEdgeType::Questions,
            base * 0.55,
            true,
        );
    }

    let overall_confidence = if nodes.is_empty() {
        0.0
    } else {
        nodes.iter().map(|n| n.confidence).sum::<f32>() / nodes.len() as f32
    };

    ExtractionResult {
        nodes,
        edges,
        overall_confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_record() -> MemoryRecord {
        let mut r = MemoryRecord::default();
        r.id = "mem-1".into();
        r.session_id = "sess-abcdef123456".into();
        r.project = "Alpha".into();
        r.topic = "Search ranking".into();
        r.url = Some("https://example.com/path".into());
        r.insight_card_confidence = 0.9;
        r.confidence_score = 0.8;
        r
    }

    #[test]
    fn extracts_file_and_url() {
        let mut r = base_record();
        r.files_touched = vec!["/Users/me/proj/src/lib.rs".into()];
        let ex = extract(&r);
        assert!(ex.nodes.iter().any(|n| n.node_type == GraphNodeType::File));
        assert!(ex.nodes.iter().any(|n| n.node_type == GraphNodeType::Url));
    }

    #[test]
    fn extracts_decision_and_error() {
        let mut r = base_record();
        r.decisions = vec!["Ship dark mode first".into()];
        r.errors = vec!["Flaky integration test".into()];
        let ex = extract(&r);
        assert!(ex
            .nodes
            .iter()
            .any(|n| n.node_type == GraphNodeType::Decision));
        assert!(ex.nodes.iter().any(|n| n.node_type == GraphNodeType::Error));
        assert!(ex
            .edges
            .iter()
            .any(|e| e.edge_type == GraphEdgeType::Contradicts));
    }

    #[test]
    fn duplicate_labels_suppressed() {
        let mut r = base_record();
        r.topic = "SameLabel".into();
        r.insight_what_happened = "SameLabel extra words for length".into();
        let ex = extract(&r);
        let concepts: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.node_type == GraphNodeType::Concept)
            .collect();
        let mut keys: HashSet<String> = HashSet::new();
        for c in &concepts {
            assert!(keys.insert(c.label.to_ascii_lowercase()));
        }
    }

    #[test]
    fn edge_confidence_threshold() {
        let mut r = MemoryRecord::default();
        r.id = "x".into();
        r.session_id = "sess-123456789012".into();
        r.project = "Proj".into();
        r.topic = "Topic long enough".into();
        r.insight_card_confidence = 0.1;
        r.confidence_score = 0.1;
        let ex = extract(&r);
        for e in &ex.edges {
            assert!(e.confidence >= EDGE_CONF_MIN);
        }
    }
}

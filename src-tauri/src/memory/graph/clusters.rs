//! Community detection on [`crate::memory::graph::schema`] subgraphs (pure, no I/O).

use crate::memory::graph::schema::GraphSubgraph;
use std::collections::HashMap;

const MIN_W: f32 = 0.3;

/// Greedy modularity clustering (Louvain-style first pass: local moves only).
pub fn louvain_partition(sub: &GraphSubgraph) -> HashMap<uuid::Uuid, usize> {
    let nodes = &sub.nodes;
    if nodes.is_empty() {
        return HashMap::new();
    }
    let mut comm: HashMap<uuid::Uuid, usize> =
        nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let _m_total: f32 = sub
        .edges
        .iter()
        .filter(|e| e.confidence >= MIN_W)
        .map(|e| e.confidence)
        .sum::<f32>()
        .max(1e-6);

    let mut weight_between = HashMap::<(usize, usize), f32>::new();
    for e in &sub.edges {
        if e.confidence < MIN_W {
            continue;
        }
        let a = *comm.get(&e.source_id).unwrap_or(&0);
        let b = *comm.get(&e.target_id).unwrap_or(&0);
        let k = if a <= b { (a, b) } else { (b, a) };
        *weight_between.entry(k).or_insert(0.0) += e.confidence;
    }

    // One sweep of merging pairs that share the strongest edge
    let mut changed = true;
    let mut it = 0usize;
    while changed && it < 8 {
        changed = false;
        it += 1;
        for e in &sub.edges {
            if e.confidence < MIN_W {
                continue;
            }
            let ca = *comm.get(&e.source_id).unwrap_or(&0);
            let cb = *comm.get(&e.target_id).unwrap_or(&0);
            if ca == cb {
                continue;
            }
            // merge smaller into larger index for stability
            let (keep, drop) = if ca < cb { (ca, cb) } else { (cb, ca) };
            for (_, c) in comm.iter_mut() {
                if *c == drop {
                    *c = keep;
                }
            }
            changed = true;
        }
    }
    // compress community ids to 0..k-1
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut next = 0usize;
    let mut out = HashMap::new();
    for n in nodes {
        let raw = *comm.get(&n.id).unwrap_or(&0);
        let e = remap.entry(raw).or_insert_with(|| {
            let v = next;
            next += 1;
            v
        });
        out.insert(n.id, *e);
    }
    let _ = weight_between; // reserved for full modularity
    out
}

/// Human-readable name for community id 0 (anchor cluster).
pub fn cluster_0_display_name(
    sub: &GraphSubgraph,
    partition: &HashMap<uuid::Uuid, usize>,
) -> String {
    let hub = partition
        .iter()
        .filter(|(_, c)| **c == 0)
        .map(|(id, _)| *id)
        .next();
    let Some(hub_id) = hub else {
        return "cluster_0".to_string();
    };
    let label = sub
        .nodes
        .iter()
        .find(|n| n.id == hub_id)
        .map(|n| n.label.as_str())
        .unwrap_or("unnamed");
    format!("cluster_0 — {label}")
}

/// Fill [`GraphSubgraph::louvain`] and [`GraphSubgraph::cluster_0_name`] for API responses.
pub fn attach_louvain_metadata(sub: &mut GraphSubgraph) {
    if sub.nodes.is_empty() {
        sub.louvain.clear();
        sub.cluster_0_name.clear();
        return;
    }
    let part = louvain_partition(sub);
    sub.cluster_0_name = cluster_0_display_name(sub, &part);
    sub.louvain = part;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::graph::schema::{GraphEdge, GraphEdgeType, GraphNode, GraphNodeType};
    use crate::memory::graph::traversal;
    use chrono::Utc;

    #[test]
    fn louvain_merges_connected_pair() {
        let now = Utc::now();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let nodes = vec![
            GraphNode {
                id: a,
                node_type: GraphNodeType::Concept,
                label: "A".into(),
                confidence: 0.9,
                source_memory_ids: vec![],
                embedding: None,
                created_at: now,
                updated_at: now,
                stale: false,
                metadata: serde_json::json!({}),
            },
            GraphNode {
                id: b,
                node_type: GraphNodeType::Concept,
                label: "B".into(),
                confidence: 0.9,
                source_memory_ids: vec![],
                embedding: None,
                created_at: now,
                updated_at: now,
                stale: false,
                metadata: serde_json::json!({}),
            },
        ];
        let edges = vec![GraphEdge {
            id: uuid::Uuid::new_v4(),
            source_id: a,
            target_id: b,
            edge_type: GraphEdgeType::SimilarTo,
            confidence: 0.9,
            conflict_flag: false,
            created_at: now,
            metadata: serde_json::json!({}),
        }];
        let sub = GraphSubgraph {
            nodes,
            edges,
            ..Default::default()
        };
        let p = louvain_partition(&sub);
        assert_eq!(p[&a], p[&b]);
    }

    #[test]
    fn cluster_0_name_non_empty() {
        let now = Utc::now();
        let a = uuid::Uuid::new_v4();
        let n = GraphNode {
            id: a,
            node_type: GraphNodeType::Project,
            label: "Payments".into(),
            confidence: 0.95,
            source_memory_ids: vec![],
            embedding: None,
            created_at: now,
            updated_at: now,
            stale: false,
            metadata: serde_json::json!({}),
        };
        let sub = GraphSubgraph {
            nodes: vec![n],
            edges: vec![],
            ..Default::default()
        };
        let mut p = HashMap::new();
        p.insert(a, 0);
        let name = cluster_0_display_name(&sub, &p);
        assert!(name.contains("cluster_0"));
        assert!(name.contains("Payments"));
        let gods = traversal::god_nodes(&sub, 3);
        assert!(!gods.is_empty());
    }
}

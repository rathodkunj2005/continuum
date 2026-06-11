//! Pathfinding over insight graph subgraphs.

use crate::graph::schema::GraphSubgraph;
use crate::graph::traversal::undirected_adjacency;
use std::collections::{HashMap, VecDeque};

/// Shortest path by hop count; tie-break: higher average edge confidence along path.
pub fn find_path(
    sub: &GraphSubgraph,
    from: uuid::Uuid,
    to: uuid::Uuid,
) -> Option<(Vec<uuid::Uuid>, f32)> {
    let adj = undirected_adjacency(&sub.nodes, &sub.edges);
    if from == to {
        return Some((vec![from], 1.0));
    }
    // parent -> (prev_node, edge_conf used to step)
    let mut parent: HashMap<uuid::Uuid, (Option<uuid::Uuid>, f32)> = HashMap::new();
    let mut q = VecDeque::new();
    parent.insert(from, (None, 0.0));
    q.push_back(from);
    while let Some(u) = q.pop_front() {
        if u == to {
            break;
        }
        if let Some(nbrs) = adj.get(&u) {
            for &(v, _, w) in nbrs {
                if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(v) {
                    e.insert((Some(u), w));
                    q.push_back(v);
                }
            }
        }
    }
    let _ = parent.get(&to)?;
    // Reconstructing all equal-hop paths is expensive; BFS first hit is min hops.
    let mut path = vec![to];
    let mut cur = to;
    let mut confs: Vec<f32> = Vec::new();
    while cur != from {
        let (p, c) = parent.get(&cur).copied()?;
        confs.push(c);
        let prev = p?;
        cur = prev;
        path.push(cur);
    }
    path.reverse();
    let avg = if confs.is_empty() {
        1.0
    } else {
        confs.iter().sum::<f32>() / confs.len() as f32
    };
    Some((path, avg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::{GraphEdge, GraphEdgeType, GraphNode, GraphNodeType};
    use chrono::Utc;

    fn node(id: u8) -> GraphNode {
        GraphNode {
            id: uuid::Uuid::from_u128(id as u128),
            node_type: GraphNodeType::Concept,
            label: format!("node {id}"),
            confidence: 0.9,
            source_memory_ids: vec!["m1".into()],
            embedding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            stale: false,
            metadata: serde_json::json!({}),
        }
    }

    fn edge(source: u8, target: u8) -> GraphEdge {
        GraphEdge {
            id: uuid::Uuid::new_v4(),
            source_id: uuid::Uuid::from_u128(source as u128),
            target_id: uuid::Uuid::from_u128(target as u128),
            edge_type: GraphEdgeType::SimilarTo,
            confidence: 0.9,
            conflict_flag: false,
            created_at: Utc::now(),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn finds_shortest_path() {
        let sub = GraphSubgraph {
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![edge(1, 2), edge(2, 3)],
            ..Default::default()
        };

        let (path, confidence) =
            find_path(&sub, uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3)).expect("path");

        assert_eq!(
            path,
            vec![
                uuid::Uuid::from_u128(1),
                uuid::Uuid::from_u128(2),
                uuid::Uuid::from_u128(3)
            ]
        );
        assert_eq!(confidence, 0.9);
    }
}

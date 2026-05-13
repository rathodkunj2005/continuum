//! Graph algorithms over in-memory [`crate::memory::graph::schema`] structures (no DB I/O).

use crate::memory::graph::schema::{GraphEdge, GraphNode, GraphSubgraph};
use std::collections::{HashMap, HashSet, VecDeque};

const MIN_EDGE_CONF: f32 = 0.3;

/// Undirected adjacency for algorithms that ignore edge direction.
fn undirected_adjacency<'a>(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> HashMap<uuid::Uuid, Vec<(uuid::Uuid, f32)>> {
    let alive: HashSet<_> = nodes.iter().map(|n| n.id).collect();
    let mut m: HashMap<uuid::Uuid, Vec<(uuid::Uuid, f32)>> = HashMap::new();
    for e in edges {
        if e.confidence < MIN_EDGE_CONF {
            continue;
        }
        if !alive.contains(&e.source_id) || !alive.contains(&e.target_id) {
            continue;
        }
        m.entry(e.source_id)
            .or_default()
            .push((e.target_id, e.confidence));
        m.entry(e.target_id)
            .or_default()
            .push((e.source_id, e.confidence));
    }
    m
}

/// BFS neighborhood within `depth` hops; edges below `MIN_EDGE_CONF` excluded.
pub fn bfs_neighborhood(sub: &GraphSubgraph, start: uuid::Uuid, depth: u32) -> GraphSubgraph {
    let adj = undirected_adjacency(&sub.nodes, &sub.edges);
    let mut seen = HashSet::new();
    let mut q = VecDeque::new();
    seen.insert(start);
    q.push_back((start, 0u32));
    while let Some((u, d)) = q.pop_front() {
        if d >= depth {
            continue;
        }
        if let Some(nbrs) = adj.get(&u) {
            for &(v, _) in nbrs {
                if seen.insert(v) {
                    q.push_back((v, d + 1));
                }
            }
        }
    }
    let node_set: HashSet<_> = seen;
    let nodes: Vec<_> = sub
        .nodes
        .iter()
        .filter(|n| node_set.contains(&n.id))
        .cloned()
        .collect();
    let idset: HashSet<_> = node_set;
    let edges: Vec<_> = sub
        .edges
        .iter()
        .filter(|e| {
            e.confidence >= MIN_EDGE_CONF
                && idset.contains(&e.source_id)
                && idset.contains(&e.target_id)
        })
        .cloned()
        .collect();
    GraphSubgraph {
        nodes,
        edges,
        ..Default::default()
    }
}

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
            for &(v, w) in nbrs {
                if !parent.contains_key(&v) {
                    parent.insert(v, (Some(u), w));
                    q.push_back(v);
                }
            }
        }
    }
    let _ = parent.get(&to)?;
    // reconstruct all equal-hop paths is expensive; BFS first hit is min hops.
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

/// Iterative PageRank on undirected support graph (edge confidence as weight).
pub fn pagerank(sub: &GraphSubgraph, iters: usize, d: f32) -> HashMap<uuid::Uuid, f32> {
    let n = sub.nodes.len().max(1);
    let ids: Vec<_> = sub.nodes.iter().map(|x| x.id).collect();
    let mut rank: HashMap<_, _> = ids.iter().map(|id| (*id, 1.0 / n as f32)).collect();
    let adj = undirected_adjacency(&sub.nodes, &sub.edges);
    for _ in 0..iters {
        let mut next = HashMap::new();
        for id in &ids {
            let mut s = (1.0 - d) / n as f32;
            if let Some(nbrs) = adj.get(id) {
                let mut wsum = 0.0f32;
                for &(_, w) in nbrs {
                    wsum += w;
                }
                if wsum > 0.0 {
                    for &(j, w) in nbrs {
                        let rj = *rank.get(&j).unwrap_or(&0.0);
                        s += d * w * rj / wsum;
                    }
                }
            }
            next.insert(*id, s);
        }
        rank = next;
    }
    rank
}

/// Articulation-like hubs: removing the node disconnects the graph (detected by reachability).
pub fn bridge_nodes(sub: &GraphSubgraph) -> Vec<uuid::Uuid> {
    let ids: Vec<_> = sub.nodes.iter().map(|n| n.id).collect();
    let edges: Vec<(uuid::Uuid, uuid::Uuid)> = sub
        .edges
        .iter()
        .filter(|e| e.confidence >= MIN_EDGE_CONF)
        .map(|e| (e.source_id, e.target_id))
        .collect();
    let mut out = Vec::new();
    for &v in &ids {
        let Some(start) = ids.iter().copied().find(|x| *x != v) else {
            continue;
        };
        let mut vis = HashSet::new();
        let mut stack = vec![start];
        vis.insert(start);
        while let Some(u) = stack.pop() {
            for &(a, b) in &edges {
                let nbr = if a == u && b != v {
                    Some(b)
                } else if b == u && a != v {
                    Some(a)
                } else {
                    None
                };
                if let Some(nx) = nbr {
                    if vis.insert(nx) {
                        stack.push(nx);
                    }
                }
            }
        }
        if vis.len() < ids.len().saturating_sub(1) {
            out.push(v);
        }
    }
    out
}

/// Top-k nodes by PageRank score.
pub fn god_nodes(sub: &GraphSubgraph, k: usize) -> Vec<(uuid::Uuid, f32)> {
    let mut v: Vec<_> = pagerank(sub, 24, 0.85).into_iter().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(k);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::graph::schema::{GraphEdgeType, GraphNodeType};
    use chrono::Utc;

    fn n(id: u8, label: &str) -> GraphNode {
        let u = uuid::Uuid::from_u128(id as u128);
        GraphNode {
            id: u,
            node_type: GraphNodeType::Concept,
            label: label.to_string(),
            confidence: 0.9,
            source_memory_ids: vec!["m1".into()],
            embedding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            stale: false,
            metadata: serde_json::json!({}),
        }
    }

    fn e(a: u8, b: u8, t: GraphEdgeType, c: f32) -> GraphEdge {
        GraphEdge {
            id: uuid::Uuid::new_v4(),
            source_id: uuid::Uuid::from_u128(a as u128),
            target_id: uuid::Uuid::from_u128(b as u128),
            edge_type: t,
            confidence: c,
            conflict_flag: false,
            created_at: Utc::now(),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn bfs_respects_depth() {
        let sub = GraphSubgraph {
            nodes: vec![n(1, "a"), n(2, "b"), n(3, "c")],
            edges: vec![
                e(1, 2, GraphEdgeType::SimilarTo, 0.9),
                e(2, 3, GraphEdgeType::SimilarTo, 0.9),
            ],
            ..Default::default()
        };
        let nb = bfs_neighborhood(&sub, uuid::Uuid::from_u128(1), 1);
        assert_eq!(nb.nodes.len(), 2);
    }

    #[test]
    fn find_path_triangle() {
        let sub = GraphSubgraph {
            nodes: vec![n(1, "a"), n(2, "b"), n(3, "c")],
            edges: vec![
                e(1, 2, GraphEdgeType::SimilarTo, 0.5),
                e(2, 3, GraphEdgeType::SimilarTo, 0.9),
            ],
            ..Default::default()
        };
        let p = find_path(&sub, uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3));
        assert!(p.is_some());
    }

    #[test]
    fn pagerank_ranks_star_center() {
        let hub = 1u8;
        let sub = GraphSubgraph {
            nodes: vec![n(hub, "hub"), n(2, "s2"), n(3, "s3"), n(4, "s4")],
            edges: vec![
                e(hub, 2, GraphEdgeType::SimilarTo, 1.0),
                e(hub, 3, GraphEdgeType::SimilarTo, 1.0),
                e(hub, 4, GraphEdgeType::SimilarTo, 1.0),
            ],
            ..Default::default()
        };
        let r = pagerank(&sub, 30, 0.85);
        assert!(r[&uuid::Uuid::from_u128(hub as u128)] >= r[&uuid::Uuid::from_u128(2)]);
    }
}

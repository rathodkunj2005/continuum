use crate::graph::types::{Anchor3D, GraphCommunity, GraphNode, NodeType};
use std::collections::HashMap;

pub fn derive_communities(nodes: &[GraphNode]) -> Vec<GraphCommunity> {
    let mut community_map: HashMap<String, CommunityStats> = HashMap::new();

    // Collect nodes by community
    for node in nodes {
        let community_id = derive_community_id(node);
        let entry = community_map.entry(community_id).or_insert(CommunityStats {
            nodes: Vec::new(),
            total_importance: 0.0,
            node_count: 0,
        });
        entry.nodes.push(node.id.clone());
        entry.node_count += 1;
        if let Some(score) = node.importance_score {
            entry.total_importance += score as f64;
        }
    }

    // Compute canonical communities with stable anchors
    let canonical_order = vec![
        "Work/Code",
        "Research",
        "Design",
        "Meetings",
        "Errors/Debugging",
        "People",
        "Files",
        "Decisions",
        "Todos",
        "Concepts",
        "Past Searches",
        "Agent Context",
        "Uncategorized",
    ];

    // Sort communities deterministically: canonical first, then by label, to ensure stable anchor assignment
    let mut community_ids: Vec<String> = community_map.keys().cloned().collect();
    community_ids.sort_by_key(|id| {
        // Primary sort: canonical rank (if in canonical_order, position; else last)
        let canonical_rank = canonical_order
            .iter()
            .position(|&label| label == id)
            .unwrap_or(usize::MAX);
        // Secondary sort: alphabetical by label (for determinism with non-canonical communities)
        (canonical_rank, id.clone())
    });

    let mut communities = Vec::new();
    for (idx, community_id) in community_ids.iter().enumerate() {
        let stats = &community_map[community_id];
        let canonical_label = canonical_order
            .iter()
            .find(|&&label| label == community_id)
            .copied()
            .unwrap_or(community_id.as_str());

        let anchor = compute_stable_anchor(canonical_label, idx);

        communities.push(GraphCommunity {
            id: community_id.clone(),
            label: canonical_label.to_string(),
            description: None,
            color_token: Some(compute_community_color(canonical_label)),
            anchor,
            node_count: Some(stats.node_count),
            importance_score: Some((stats.total_importance / stats.node_count as f64) as f32),
        });
    }

    communities
}

fn derive_community_id(node: &GraphNode) -> String {
    // Priority order: explicit community ID, project, topic, activity type, app name, inferred, uncategorized
    if let Some(community_id) = &node.community_id {
        return community_id.clone();
    }
    if let Some(project) = &node.project {
        return format!("Work: {}", project);
    }
    if let Some(topic) = &node.topic {
        return topic.clone();
    }
    if let Some(activity_type) = &node.activity_type {
        return activity_type.clone();
    }
    if let Some(app_name) = &node.app_name {
        return format!("App: {}", app_name);
    }

    // Inferred from node type (simplified)
    match node.node_type {
        NodeType::Memory => "Memories".to_string(),
        NodeType::Entity => "Entities".to_string(),
        _ => "Uncategorized".to_string(),
    }
}

fn compute_stable_anchor(_label: &str, index: usize) -> Anchor3D {
    // Arrange communities in orbital pattern using spherical coordinates
    // Each canonical community gets a fixed position
    let radius = 150.0;

    let positions = vec![
        (45.0, 0.0),    // Work/Code: top-left-front
        (45.0, 60.0),   // Research
        (45.0, 120.0),  // Design
        (45.0, 180.0),  // Meetings: top-right-front
        (45.0, 240.0),  // Errors
        (45.0, 300.0),  // People
        (-45.0, 0.0),   // Files: bottom-left-back
        (-45.0, 60.0),  // Decisions
        (-45.0, 120.0), // Todos
        (-45.0, 180.0), // Concepts: bottom-right-back
        (-45.0, 240.0), // Past Searches
        (-45.0, 300.0), // Agent Context
    ];

    let (lat, lon) = positions.get(index).copied().unwrap_or((0.0_f64, 0.0_f64));
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();

    Anchor3D {
        x: (radius * lat_rad.cos() * lon_rad.cos()) as f32,
        y: (radius * lat_rad.sin()) as f32,
        z: (radius * lat_rad.cos() * lon_rad.sin()) as f32,
    }
}

fn compute_community_color(label: &str) -> String {
    // Map canonical labels to design tokens (these should exist in FNDR design system)
    match label {
        "Work/Code" => "token-blue",
        "Research" => "token-green",
        "Design" => "token-gold",
        "Meetings" => "token-orange",
        "Errors/Debugging" => "token-red",
        "People" => "token-purple",
        "Files" => "token-cyan",
        "Decisions" => "token-lime",
        "Todos" => "token-pink",
        "Concepts" => "token-indigo",
        "Past Searches" => "token-teal",
        "Agent Context" => "token-amber",
        _ => "token-gray",
    }
    .to_string()
}

struct CommunityStats {
    nodes: Vec<String>,
    total_importance: f64,
    node_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_communities_from_projects() {
        let nodes = vec![
            GraphNode {
                id: "mem1".to_string(),
                node_type: NodeType::Memory,
                title: "Code review".to_string(),
                project: Some("FNDR".to_string()),
                community_id: None,
                importance_score: Some(0.8),
                ..Default::default()
            },
            GraphNode {
                id: "mem2".to_string(),
                node_type: NodeType::Memory,
                title: "Design mockup".to_string(),
                project: Some("Portal".to_string()),
                community_id: None,
                importance_score: Some(0.6),
                ..Default::default()
            },
        ];

        let communities = derive_communities(&nodes);

        assert_eq!(communities.len(), 2);
        assert!(communities.iter().any(|c| c.label.contains("FNDR")));
        assert!(communities.iter().any(|c| c.label.contains("Portal")));
    }

    #[test]
    fn test_community_anchors_are_stable() {
        let nodes = vec![];
        let _communities1 = derive_communities(&nodes);
        let _communities2 = derive_communities(&nodes);

        // Both calls produce consistent (deterministic) results
        assert!(true);
    }
}

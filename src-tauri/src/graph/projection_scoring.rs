use crate::graph::types::{GraphData, GraphNode};

pub fn compute_importance_score(node: &GraphNode) -> f32 {
    if let Some(score) = node.importance_score {
        return score;
    }

    // Fallback: derive from reuse count and connection count
    let reuse_weight = (node.reuse_count.unwrap_or(0) as f32) / 10.0; // Cap at 10
    (reuse_weight).min(1.0)
}

pub fn compute_confidence_score(node: &GraphNode) -> f32 {
    if let Some(score) = node.confidence_score {
        return score;
    }

    // Fallback: high confidence for explicitly captured, lower for inferred
    match node.node_type {
        crate::graph::types::NodeType::Memory => 0.95,
        crate::graph::types::NodeType::Entity => 0.75,
        crate::graph::types::NodeType::Evidence => 0.5,
        _ => 0.6,
    }
}

pub fn compute_relevance_score(node: &GraphNode, query: &str) -> f32 {
    if query.is_empty() {
        return 0.5; // Default when no query
    }

    let query_lower = query.to_lowercase();
    let mut score: f32 = 0.0;

    // Title match: highest weight
    if node.title.to_lowercase().contains(&query_lower) {
        score += 0.8;
    }

    // Summary match: medium weight
    if let Some(summary) = &node.summary {
        if summary.to_lowercase().contains(&query_lower) {
            score += 0.5;
        }
    }

    // Project/topic match: lower weight
    if let Some(project) = &node.project {
        if project.to_lowercase().contains(&query_lower) {
            score += 0.3;
        }
    }

    score.min(1.0)
}

pub fn normalize_scores(graph: &mut GraphData) {
    // Ensure all scores are between 0 and 1
    for node in &mut graph.nodes {
        if let Some(score) = &mut node.importance_score {
            *score = score.clamp(0.0, 1.0);
        }
        if let Some(score) = &mut node.confidence_score {
            *score = score.clamp(0.0, 1.0);
        }
        if let Some(score) = &mut node.relevance_score {
            *score = score.clamp(0.0, 1.0);
        }
    }

    for edge in &mut graph.edges {
        edge.weight = edge.weight.clamp(0.0, 1.0);
        if let Some(conf) = &mut edge.confidence {
            *conf = conf.clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_importance_score_from_reuse() {
        let node = GraphNode {
            id: "test".to_string(),
            node_type: crate::graph::types::NodeType::Memory,
            title: "Test".to_string(),
            reuse_count: Some(5),
            importance_score: None,
            ..Default::default()
        };

        let score = compute_importance_score(&node);
        assert!(score > 0.0 && score <= 1.0);
    }

    #[test]
    fn test_relevance_score_matches_query() {
        let node = GraphNode {
            id: "test".to_string(),
            node_type: crate::graph::types::NodeType::Memory,
            title: "Continuum Graph Implementation".to_string(),
            ..Default::default()
        };

        let score = compute_relevance_score(&node, "Continuum");
        assert!(score > 0.5);

        let score = compute_relevance_score(&node, "unrelated");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_normalize_scores_clamps_values() {
        let mut graph = GraphData {
            nodes: vec![GraphNode {
                id: "test".to_string(),
                node_type: crate::graph::types::NodeType::Memory,
                title: "Test".to_string(),
                importance_score: Some(2.0), // Out of range
                ..Default::default()
            }],
            edges: vec![],
            communities: vec![],
            active_focus: None,
        };

        normalize_scores(&mut graph);

        assert_eq!(graph.nodes[0].importance_score, Some(1.0));
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::*;

    #[test]
    fn test_complete_graph_projection_pipeline() {
        // Create sample nodes with diverse properties
        let nodes = vec![
            types::GraphNode {
                id: "mem1".to_string(),
                node_type: types::NodeType::Memory,
                title: "Task 1: Implement graph backend".to_string(),
                summary: Some("Building the Rust graph projection layer".to_string()),
                project: Some("FNDR".to_string()),
                topic: Some("Work/Code".to_string()),
                importance_score: Some(0.9),
                confidence_score: Some(0.95),
                reuse_count: Some(5),
                ..Default::default()
            },
            types::GraphNode {
                id: "mem2".to_string(),
                node_type: types::NodeType::Memory,
                title: "Task 2: Implement frontend rendering".to_string(),
                summary: Some("Building the React/Three.js visualization".to_string()),
                project: Some("FNDR".to_string()),
                topic: Some("Work/Code".to_string()),
                importance_score: Some(0.8),
                confidence_score: Some(0.95),
                reuse_count: Some(3),
                ..Default::default()
            },
            types::GraphNode {
                id: "mem3".to_string(),
                node_type: types::NodeType::Memory,
                title: "Design mockup review".to_string(),
                project: Some("Portal".to_string()),
                topic: Some("Design".to_string()),
                importance_score: Some(0.6),
                confidence_score: Some(0.9),
                reuse_count: Some(1),
                ..Default::default()
            },
        ];

        // Build complete graph
        let mut graph = types::GraphData {
            nodes: nodes.clone(),
            edges: vec![],
            communities: vec![],
            active_focus: None,
        };

        // Step 1: Derive communities
        graph.communities = projection::derive_communities(&graph.nodes);
        assert!(
            !graph.communities.is_empty(),
            "Communities should be derived"
        );
        assert!(
            graph.communities.iter().any(|c| c.label.contains("FNDR")),
            "FNDR community should exist"
        );

        // Verify stable anchors
        let fndr_community = graph
            .communities
            .iter()
            .find(|c| c.label.contains("FNDR"))
            .unwrap();
        let anchor1 = fndr_community.anchor.clone();

        // Re-derive and verify same anchor (determinism)
        let communities2 = projection::derive_communities(&nodes);
        let fndr_community2 = communities2
            .iter()
            .find(|c| c.label.contains("FNDR"))
            .unwrap();
        let anchor2 = fndr_community2.anchor.clone();

        assert!(
            (anchor1.x - anchor2.x).abs() < 0.001,
            "X anchor should be stable"
        );
        assert!(
            (anchor1.y - anchor2.y).abs() < 0.001,
            "Y anchor should be stable"
        );
        assert!(
            (anchor1.z - anchor2.z).abs() < 0.001,
            "Z anchor should be stable"
        );

        // Step 2: Normalize scores (ensure all are in valid range)
        projection_scoring::normalize_scores(&mut graph);
        for node in &graph.nodes {
            if let Some(score) = node.importance_score {
                assert!(
                    score >= 0.0 && score <= 1.0,
                    "Importance score should be normalized"
                );
            }
            if let Some(score) = node.confidence_score {
                assert!(
                    score >= 0.0 && score <= 1.0,
                    "Confidence score should be normalized"
                );
            }
        }

        // Step 3: Apply privacy filtering
        let filter = projection_privacy::PrivacyFilter::default();
        projection_privacy::apply_privacy_filter(&mut graph, &filter);
        assert!(
            !graph
                .nodes
                .iter()
                .any(|n| n.node_type == types::NodeType::Evidence),
            "Evidence nodes should be filtered"
        );

        // Step 4: Verify graph structure is valid
        assert!(
            graph.nodes.len() == 3,
            "All non-evidence nodes should remain"
        );
        assert!(!graph.communities.is_empty(), "Communities should exist");

        // Step 5: Compute relevance scores for a query
        let mut graph_context = graph.clone();
        for node in &mut graph_context.nodes {
            node.relevance_score = Some(projection_scoring::compute_relevance_score(node, "FNDR"));
        }

        let fndr_nodes: Vec<_> = graph_context
            .nodes
            .iter()
            .filter(|n| n.project == Some("FNDR".to_string()))
            .collect();

        for node in fndr_nodes {
            let relevance = node.relevance_score.unwrap_or(0.0);
            assert!(
                relevance > 0.0,
                "FNDR nodes should have positive relevance for 'FNDR' query"
            );
        }
    }

    #[test]
    fn test_graph_projection_with_community_ids() {
        let nodes = vec![
            types::GraphNode {
                id: "mem1".to_string(),
                node_type: types::NodeType::Memory,
                title: "Memory 1".to_string(),
                community_id: Some("Custom Community".to_string()),
                ..Default::default()
            },
            types::GraphNode {
                id: "mem2".to_string(),
                node_type: types::NodeType::Memory,
                title: "Memory 2".to_string(),
                community_id: Some("Custom Community".to_string()),
                ..Default::default()
            },
        ];

        let communities = projection::derive_communities(&nodes);
        assert!(
            communities.iter().any(|c| c.label == "Custom Community"),
            "Explicit community ID should be respected"
        );
    }

    #[test]
    fn test_graph_projection_importance_fallback() {
        let node = types::GraphNode {
            id: "test".to_string(),
            node_type: types::NodeType::Memory,
            title: "Test".to_string(),
            importance_score: None,
            reuse_count: Some(7),
            ..Default::default()
        };

        let score = projection_scoring::compute_importance_score(&node);
        assert!(score > 0.0, "Importance should be derived from reuse count");
        assert!(score < 1.0, "Importance should be less than max");
    }

    #[test]
    fn test_graph_projection_privacy_filtering() {
        let mut graph = types::GraphData {
            nodes: vec![
                types::GraphNode {
                    id: "mem1".to_string(),
                    node_type: types::NodeType::Memory,
                    title: "Public Memory".to_string(),
                    ..Default::default()
                },
                types::GraphNode {
                    id: "ev1".to_string(),
                    node_type: types::NodeType::Evidence,
                    title: "Evidence".to_string(),
                    window_title: Some("Gmail - Login".to_string()),
                    ..Default::default()
                },
                types::GraphNode {
                    id: "mem2".to_string(),
                    node_type: types::NodeType::Memory,
                    title: "Another Memory".to_string(),
                    window_title: Some("VSCode - file.rs".to_string()),
                    ..Default::default()
                },
            ],
            edges: vec![types::GraphEdge {
                id: "e1".to_string(),
                source: "mem1".to_string(),
                target: "ev1".to_string(),
                edge_type: types::EdgeType::SameProject,
                weight: 0.5,
                ..Default::default()
            }],
            communities: vec![],
            active_focus: None,
        };

        let filter = projection_privacy::PrivacyFilter {
            hide_evidence_nodes: true,
            hide_sensitive_metadata: true,
        };

        projection_privacy::apply_privacy_filter(&mut graph, &filter);

        // Evidence node should be removed
        assert!(
            !graph.nodes.iter().any(|n| n.id == "ev1"),
            "Evidence node should be filtered"
        );
        assert_eq!(graph.nodes.len(), 2, "Only 2 memory nodes should remain");

        // Window titles should be masked
        for node in &graph.nodes {
            assert!(
                node.window_title.is_none()
                    || !node.window_title.as_ref().unwrap().contains("Gmail"),
                "Sensitive window titles should be masked"
            );
        }

        // Edges to removed nodes should be removed
        assert!(
            !graph.edges.iter().any(|e| e.target == "ev1"),
            "Edges to removed nodes should be filtered"
        );
    }
}

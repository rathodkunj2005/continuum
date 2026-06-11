use crate::graph::types::{GraphData, GraphNode, NodeType};

pub struct PrivacyFilter {
    pub hide_sensitive_metadata: bool,
    pub hide_evidence_nodes: bool,
}

impl Default for PrivacyFilter {
    fn default() -> Self {
        Self {
            hide_sensitive_metadata: true,
            hide_evidence_nodes: true,
        }
    }
}

pub fn apply_privacy_filter(graph: &mut GraphData, filter: &PrivacyFilter) {
    // Filter nodes
    graph.nodes.iter_mut().for_each(|node| {
        apply_node_privacy(node, filter);
    });

    // Remove evidence nodes if configured
    if filter.hide_evidence_nodes {
        graph.nodes.retain(|n| n.node_type != NodeType::Evidence);
    }

    // Remove edges referencing removed nodes
    let valid_ids: std::collections::HashSet<_> =
        graph.nodes.iter().map(|n| n.id.clone()).collect();
    graph
        .edges
        .retain(|e| valid_ids.contains(&e.source) && valid_ids.contains(&e.target));
}

fn apply_node_privacy(node: &mut GraphNode, filter: &PrivacyFilter) {
    if filter.hide_sensitive_metadata {
        // Mask potentially sensitive fields
        // Keep: title, timestamp, project, topic
        // Mask: window_title (could contain passwords), url (could be sensitive)
        node.window_title = None;
        node.url = None;

        // Keep only non-PII app names
        if let Some(app) = &node.app_name {
            if is_sensitive_app(app) {
                node.app_name = Some("Sensitive App".to_string());
            }
        }
    }

    // Metadata may contain sensitive info; allow user to control
    // For now, allow it but filter specific keys later if needed
}

fn is_sensitive_app(app: &str) -> bool {
    // List of apps where window titles are often sensitive
    let sensitive_apps = ["vault", "password", "credentials", "banking", "email"];

    let app_lower = app.to_lowercase();
    sensitive_apps.iter().any(|&s| app_lower.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_nodes_hidden_when_configured() {
        let mut graph = GraphData {
            nodes: vec![
                GraphNode {
                    id: "mem1".to_string(),
                    node_type: NodeType::Memory,
                    title: "Test Memory".to_string(),
                    ..Default::default()
                },
                GraphNode {
                    id: "ev1".to_string(),
                    node_type: NodeType::Evidence,
                    title: "Raw Evidence".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![],
            communities: vec![],
            active_focus: None,
        };

        let filter = PrivacyFilter {
            hide_evidence_nodes: true,
            ..Default::default()
        };

        apply_privacy_filter(&mut graph, &filter);

        assert_eq!(graph.nodes.len(), 1);
        assert!(!graph
            .nodes
            .iter()
            .any(|n| n.node_type == NodeType::Evidence));
    }

    #[test]
    fn test_window_title_masked_when_configured() {
        let mut node = GraphNode {
            id: "test".to_string(),
            node_type: NodeType::Memory,
            title: "Test".to_string(),
            window_title: Some("Gmail - Password Reset".to_string()),
            ..Default::default()
        };

        let filter = PrivacyFilter {
            hide_sensitive_metadata: true,
            ..Default::default()
        };

        apply_node_privacy(&mut node, &filter);

        assert!(node.window_title.is_none());
    }
}

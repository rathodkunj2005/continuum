use crate::graph::projection_privacy::PrivacyFilter;
use crate::graph::types::*;

#[tauri::command]
pub async fn get_memory_graph_atlas() -> Result<GraphData, String> {
    // TODO: In Phase 1, this returns mock data
    // In Phase 2, integrate with actual memory store

    let mut graph = GraphData {
        nodes: vec![],
        edges: vec![],
        communities: vec![],
        active_focus: Some(ActiveFocus {
            focus_type: FocusType::Atlas,
            id: None,
            label: "Full Memory Atlas".to_string(),
            query: None,
        }),
    };

    // TODO: Fetch actual memory data from memory store

    // Apply privacy filter
    let filter = PrivacyFilter::default();
    crate::graph::projection_privacy::apply_privacy_filter(&mut graph, &filter);

    crate::graph::projection_scoring::normalize_scores(&mut graph);

    Ok(graph)
}

#[tauri::command]
pub async fn get_memory_graph_context(
    focus_id: Option<String>,
    query: Option<String>,
) -> Result<GraphData, String> {
    // TODO: In Phase 1, returns mock data
    // In Phase 2, integrate with actual memory store and search

    let mut graph = GraphData {
        nodes: vec![],
        edges: vec![],
        communities: vec![],
        active_focus: Some(ActiveFocus {
            focus_type: FocusType::Query,
            id: focus_id,
            label: query.clone().unwrap_or_default(),
            query: query.clone(),
        }),
    };

    // TODO: Fetch actual memory data and compute relevance scores

    let filter = PrivacyFilter::default();
    crate::graph::projection_privacy::apply_privacy_filter(&mut graph, &filter);

    crate::graph::projection_scoring::normalize_scores(&mut graph);

    Ok(graph)
}

#[tauri::command]
pub async fn get_graph_node_neighborhood(
    node_id: String,
    depth: u32,
    edge_types: Option<Vec<EdgeType>>,
) -> Result<GraphData, String> {
    // TODO: Fetch neighborhood of a specific node
    // Return: selected node, connected nodes (1-hop, 2-hop if depth > 1), and connecting edges
    let _ = (node_id, depth, edge_types);

    Err("Not yet implemented in Phase 1".to_string())
}

#[tauri::command]
pub async fn get_graph_communities() -> Result<Vec<GraphCommunity>, String> {
    // TODO: Return all canonical communities

    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_memory_graph_atlas_returns_valid_data() {
        let result = get_memory_graph_atlas().await;

        assert!(result.is_ok());
        let graph = result.unwrap();
        assert!(graph.active_focus.is_some());
    }

    #[tokio::test]
    async fn test_get_memory_graph_context_with_query() {
        let result = get_memory_graph_context(None, Some("Continuum".to_string())).await;

        assert!(result.is_ok());
        let graph = result.unwrap();
        assert_eq!(
            graph.active_focus.as_ref().unwrap().query,
            Some("Continuum".to_string())
        );
    }
}

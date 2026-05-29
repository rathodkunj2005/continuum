use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeType {
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "entity")]
    Entity,
    #[serde(rename = "community")]
    Community,
    #[serde(rename = "evidence")]
    Evidence,
    #[serde(rename = "agent_context")]
    AgentContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeType {
    #[serde(rename = "semantic_similarity")]
    SemanticSimilarity,
    #[serde(rename = "explicit_reference")]
    ExplicitReference,
    #[serde(rename = "temporal_adjacency")]
    TemporalAdjacency,
    #[serde(rename = "same_project")]
    SameProject,
    #[serde(rename = "same_session")]
    SameSession,
    #[serde(rename = "agent_inferred")]
    AgentInferred,
    #[serde(rename = "provenance")]
    Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub node_type: NodeType,
    pub title: String,
    pub summary: Option<String>,
    pub community_id: Option<String>,
    pub timestamp_start: Option<String>,
    pub timestamp_end: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub project: Option<String>,
    pub topic: Option<String>,
    pub activity_type: Option<String>,
    pub importance_score: Option<f32>,
    pub relevance_score: Option<f32>,
    pub confidence_score: Option<f32>,
    pub reuse_count: Option<u32>,
    pub source_ids: Option<Vec<String>>,
    pub metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub weight: f32,
    pub confidence: Option<f32>,
    pub reason: Option<String>,
    pub metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphCommunity {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub color_token: Option<String>,
    pub anchor: Anchor3D,
    pub node_count: Option<usize>,
    pub importance_score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FocusType {
    #[serde(rename = "query")]
    Query,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "agent_task")]
    AgentTask,
    #[serde(rename = "atlas")]
    Atlas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveFocus {
    pub focus_type: FocusType,
    pub id: Option<String>,
    pub label: String,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub communities: Vec<GraphCommunity>,
    pub active_focus: Option<ActiveFocus>,
}

impl Default for GraphNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            node_type: NodeType::Memory,
            title: String::new(),
            summary: None,
            community_id: None,
            timestamp_start: None,
            timestamp_end: None,
            app_name: None,
            window_title: None,
            url: None,
            project: None,
            topic: None,
            activity_type: None,
            importance_score: None,
            relevance_score: None,
            confidence_score: None,
            reuse_count: None,
            source_ids: None,
            metadata: None,
        }
    }
}

impl Default for GraphEdge {
    fn default() -> Self {
        Self {
            id: String::new(),
            source: String::new(),
            target: String::new(),
            edge_type: EdgeType::SemanticSimilarity,
            weight: 0.5,
            confidence: None,
            reason: None,
            metadata: None,
        }
    }
}

impl Default for Anchor3D {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

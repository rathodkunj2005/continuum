//! Top-level insight graph module.

pub mod community;
pub mod edges;
pub mod entities;
pub mod graph_index;
pub mod graph_rerank;
pub mod graph_store;
pub mod pathfinding;
pub mod schema;
pub mod traversal;

mod legacy;

pub use legacy::{compress_node_label, GraphStore, MemoryCard, MemoryReconstruction};

// Graph projection layer for 3D visualization
pub mod projection;
pub mod projection_commands;
pub mod projection_privacy;
pub mod projection_scoring;
pub mod types;

#[cfg(test)]
mod integration_test;

pub use types::{
    ActiveFocus, Anchor3D, EdgeType, FocusType, GraphCommunity, GraphData, GraphEdge, GraphNode,
    NodeType,
};

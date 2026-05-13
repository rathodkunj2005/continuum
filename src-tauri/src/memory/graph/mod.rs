//! Insight knowledge graph (schema + algorithms) and legacy memory→graph bridge.
//!
//! New graph types live in [`schema`]. Legacy task/session/url ingestion remains in
//! [`legacy`] as [`GraphStore`](legacy::GraphStore).

pub mod clusters;
pub mod schema;
pub mod traversal;

mod legacy;

pub use legacy::{compress_node_label, GraphStore, MemoryCard, MemoryReconstruction};

//! Text chunking and ONNX embedding generation for the memory pipeline.

mod chunking;
mod clip_vision;
mod onnx;
pub mod prefixes;

pub use chunking::{
    chunk_screen_text, select_salient_memory_chunks, LineKind, TextChunk, TextChunker,
};
pub use clip_vision::{
    clip_session_loaded, embed_imported_image, last_clip_infer_ms, resolve_clip_onnx_path,
};
pub use onnx::{
    embedding_runtime_status, preflight_embedding_environment, Embedder, EmbeddingBackend,
    EmbeddingPreflight, EmbeddingRuntimeStatus, EMBEDDING_DIM,
};

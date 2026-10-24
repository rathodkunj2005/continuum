//! Text chunking and ONNX embedding generation for the memory pipeline.

mod admission;
mod chunking;
mod clip_vision;
mod gemma_helper;
mod onnx;
pub(crate) use onnx::cached_embedder;
pub mod prefixes;

pub use chunking::{
    chunk_screen_text, select_salient_memory_chunks, LineKind, TextChunk, TextChunker,
};
pub use clip_vision::{
    clip_session_loaded, embed_imported_image, last_clip_infer_ms, resolve_clip_onnx_path,
};
pub use onnx::{
    embedding_runtime_status, preflight_embedding_environment, shared_bge_v5_query_embedder,
    Embedder, EmbeddingBackend, EmbeddingInput, EmbeddingPreflight, EmbeddingRuntimeStatus,
    EMBEDDING_DIM,
};

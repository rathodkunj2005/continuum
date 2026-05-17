//! Search module with hybrid search

mod hybrid;
pub mod memory_cards;
mod query_processor;
mod reranker;

pub use crate::context_runtime::retrieval_routes::{
    PathStep, RetrievalRoute, RouteBranch, RouteCtx, RouteHit, RouteHits, RouteRunner, RouteSignals,
};
pub use hybrid::HybridSearcher;
pub use memory_cards::{
    parse_continuation_of, parse_reopen_target, MemoryCard, MemoryCardSynthesizer,
};
pub use query_processor::{QueryContext, QueryExpansionDebug, QueryIntent, QueryProfile};
pub use reranker::{anchor_coverage_score, rerank_results, RerankStats};

//! Storage module

mod lance_store;
pub mod schema;
mod state_store;

pub use lance_store::{
    compose_embedding_text, generate_search_aliases_public, pollution_ratio_score,
    salience_concentration_score, topic_clarity_score, Store, ACTIVITY_EVENTS_TABLE,
    CONTEXT_DELTAS_TABLE, CONTEXT_PACKS_TABLE, DECISION_LEDGER_TABLE, EDGES_TABLE,
    ENTITY_ALIASES_TABLE, KNOWLEDGE_PAGES_TABLE, MEETINGS_TABLE, MEMORIES_TABLE, NODES_TABLE,
    PROJECT_CONTEXTS_TABLE, SEGMENTS_TABLE, TASKS_TABLE,
};
pub use schema::{
    ActivityEvent, AppCount, CodeContext, CommandEvent, CommitRef, ContextDelta, ContextPack,
    ContextPackItemReason, ContextRuntimeStatus, ContextTask, DecisionLedgerEntry, DecisionSummary,
    EdgeType, EntityAliasRecord, EntityRef, ErrorEvent, EvidenceRef, ExcludedContextItem,
    ExtractedEntity, FailureSummary, GraphEdge, GraphNode, HealthStatus, IntentAnalysis,
    IntentCandidate, IssueSummary, KnowledgePage, KnowledgePageType, KnowledgeStability,
    MeetingBreakdown, MeetingSegment, MeetingSession, MemoryActionItem, MemoryRecord, NodeType,
    PrivacyClass, ProjectContext, RelevantFile, SearchResult, Stats, Task, TaskType, WorkingState,
};
pub use state_store::StateStore;

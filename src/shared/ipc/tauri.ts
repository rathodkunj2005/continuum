import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface SearchResult {
    id: string;
    timestamp: number;
    app_name: string;
    bundle_id?: string;
    window_title: string;
    session_id: string;
    text: string;
    snippet: string;
    display_summary?: string;
    internal_context?: string;
    score: number;
    screenshot_path?: string;
    url?: string;
    anchor_coverage_score?: number;
    extracted_entities?: string[];
    content_hash?: string;
    matched_routes?: string[];
    matched_chunk_ids?: string[];
    chunk_evidence?: MatchedChunkEvidence[];
    /** Post-capture review lifecycle:
     *  "" / "pending" / "reviewed_local" / "reviewed_daily" / "review_failed". */
    enrichment_status?: string;
    /** Unix ms timestamp of the last successful review (0 = never). */
    reviewed_at_ms?: number;
    /** Monotonic counter incremented on each successful review pass. */
    reviewer_generation?: number;
    /** Coarse persisted gate outcome — "enriched_memory_card",
     *  "visual_semantics_failed", "metadata_only", etc. */
    storage_outcome?: string;
}

export interface MatchedChunkEvidence {
    chunk_id: string;
    memory_id: string;
    chunk_index: number;
    text: string;
    score: number;
    distance: number;
    app_name?: string;
    window_title?: string;
    day_bucket?: string;
}

export interface MemoryCard {
    id: string;
    title: string;
    summary: string;
    display_summary?: string;
    internal_context?: string;
    action: string;
    context: string[];
    timestamp: number;
    app_name: string;
    window_title: string;
    url?: string;
    score: number;
    source_count: number;
    continuity?: boolean;
    raw_snippets: string[];
    evidence_ids?: string[];
    confidence?: number;
    anchor_coverage_score?: number;
    /** High-level activity category — content-derived, never tied to an app name. */
    activity_type?: string;
    /** File paths or code symbols touched in this session */
    files_touched?: string[];
    /** Approximate session duration in minutes (0 if single capture) */
    session_duration_mins?: number;
    /** Short id of the prior card this one continues from, derived from
     *  the durable memory_context "Continues from <short_id>" marker. */
    continuation_of?: string;
    /** http(s):// URL, file:// path, or app/deep-link derived from typed
     *  reopen provenance (legacy marker parsing may still backfill old rows). */
    reopen_target?: string;
    insight_what_happened?: string;
    insight_why_mattered?: string;
    insight_what_changed?: string;
    insight_context_thread?: string;
    insight_spans_json?: string;
    insight_card_confidence?: number;
    /** Content-derived timeline action class (server-side classifier). */
    timeline_action_class?: string;
    /** Persisted memory project label (best-effort). */
    project?: string;
    /** Count of insight graph nodes citing this memory id. */
    insight_kg_node_count?: number;
    /** Which pipeline branch synthesized this record: vlm|llm|browser_semantic|fallback|url_only */
    synthesis_branch?: string;
    /** Broad semantic category labels e.g. ["sport","entertainment"] */
    topic_categories?: string[];
    /** Semantic search aliases / synonyms */
    search_aliases?: string[];
    matched_routes?: string[];
    matched_chunk_ids?: string[];
    chunk_evidence?: MatchedChunkEvidence[];
    /** Phase 3 — deterministic "Why this surfaced" attached by the
     *  agentic-graph-rag composer. Absent on legacy code paths. */
    surfacing_reason?: SurfacingReason;
    /** Post-capture review lifecycle:
     *  "" / "pending" / "reviewed_local" / "reviewed_daily" / "review_failed". */
    enrichment_status?: string;
    /** Unix ms timestamp of the last successful review (0 = never). */
    reviewed_at_ms?: number;
    /** Monotonic counter incremented on each successful review pass. */
    reviewer_generation?: number;
    /** Coarse persisted gate outcome — "enriched_memory_card",
     *  "visual_semantics_failed", "metadata_only", etc. */
    storage_outcome?: string;
}

// ── Phase 4 agentic-graph-rag types ───────────────────────────────────────

export interface GraphPathStep {
    from_label: string;
    edge: string;
    to_label: string;
}

export interface SurfacingReason {
    headline: string;
    routes: string[];
    graph_path?: GraphPathStep[];
    anchor_terms_hit?: string[];
    recency_boost?: number;
}

export interface FileRef {
    path: string;
    memory_ids: string[];
}
export interface CommandRef { command: string; memory_ids: string[] }
export interface DecisionRef { decision: string; memory_ids: string[] }
export interface ErrorRef { error: string; memory_ids: string[] }
export interface TaskRef { task: string; memory_ids: string[] }
export interface UrlRef { url: string; memory_ids: string[] }

export interface EvidencePack {
    files: FileRef[];
    commands: CommandRef[];
    decisions: DecisionRef[];
    errors: ErrorRef[];
    todos: TaskRef[];
    urls: UrlRef[];
}

export type VerifyOutcome =
    | { kind: "grounded"; confidence: number }
    | { kind: "partial_answer"; missing: string[] }
    | { kind: "not_enough_evidence"; reason: string };

export interface ComposedAnswer {
    query: string;
    answer: string;
    evidence: EvidencePack;
    cards: MemoryCard[];
    verify_outcome: VerifyOutcome;
    surfacing_reasons: SurfacingReason[];
}

export interface FndrSearchResponse {
    query: string;
    cards: MemoryCard[];
}

export async function fndrSearch(query: string, limit?: number): Promise<FndrSearchResponse> {
    return invoke("fndr_search", { query, limit });
}

export async function fndrAnswer(query: string, limit?: number): Promise<ComposedAnswer> {
    return invoke("fndr_answer", { query, limit });
}

export async function fndrBuildContextPack(args: {
    query: string;
    session_id?: string;
    project?: string;
    budget_tokens?: number;
}): Promise<unknown> {
    return invoke("fndr_build_context_pack", args);
}

export async function fndrGetRelatedMemories(
    memoryId: string,
    limit?: number,
): Promise<MemoryCard[]> {
    return invoke("fndr_get_related_memories", { memoryId, limit });
}

export async function fndrGetMemorySubgraph(
    seedIds: string[],
    maxHops = 2,
): Promise<{ seed_ids: string[]; node_count: number; edge_count: number }> {
    return invoke("fndr_get_memory_subgraph", { seedIds, maxHops });
}

export async function fndrTimeline(args?: {
    limit?: number;
    project?: string;
}): Promise<Array<{ memory_id: string; timestamp: number; snippet: string }>> {
    return invoke("fndr_timeline", args ?? {});
}

export async function fndrQualityStatus(): Promise<{
    stored_count: number;
    dropped_count: number;
    flagged_count: number;
}> {
    return invoke("fndr_quality_status");
}

export interface MemoryScoreBreakdown {
    specificity: number;
    intent: number;
    entity: number;
    usefulness: number;
    evidence: number;
    ocr_noise: number;
    graph_readiness: number;
    retrieval_value: number;
    /** Top-k span concentration on clean_text (0..1). */
    salience_concentration?: number;
    /** Topic clarity / token-overlap signal (0..1). */
    topic_clarity?: number;
    /** Composite OCR noise + diffuse-content score (0..1). */
    pollution_ratio?: number;
}

export interface MemoryDebugInspector {
    memory_id: string;
    memory_context: string;
    project: string;
    topic: string;
    workflow: string;
    activity_type: string;
    user_intent: string;
    entities: string[];
    actions: string[];
    quality_scores: MemoryScoreBreakdown;
    grounding_confidence: number;
    extraction_issues: string[];
    ocr_quality_stats: OcrQualityStats;
    embedding_diagnostics: EmbeddingDiagnostics;
    embedding_text: string;
    search_aliases: string[];
    raw_ocr_evidence: unknown;
    /** Import vision pipeline fields (from persisted `raw_evidence` JSON). */
    visual_semantics?: Record<string, unknown>;
    graph: {
        nodes: unknown[];
        edges: unknown[];
        weak_evidence: string[];
    };
    storage_outcome: string;
    quality_gate_reason: string;
    query_match_reasons: string[];
    related_knowledge_pages: unknown[];
}

export interface OcrQualityStats {
    total_lines: number;
    kept_lines: number;
    low_conf_lines: number;
    dropped_noise_lines: number;
    dropped_low_signal_lines: number;
    avg_line_score: number;
    ocr_confidence: number;
    ocr_blocks: number;
}

export interface EmbeddingDiagnostics {
    structured_prefix_ratio: number;
    evidence_tail_chars: number;
    dominated_by_raw_ocr: boolean;
}

export interface MemoryQualityFlag {
    memory_id: string;
    summary: string;
    app_name: string;
    timestamp: number;
    storage_outcome: string;
    issues: string[];
    scores: MemoryScoreBreakdown;
}

export interface MemoryValidationReport {
    generated_at: number;
    lookback_minutes: number;
    total_memories: number;
    flagged_memories: MemoryQualityFlag[];
}

export interface RebuildMemoryPreview {
    memory_id: string;
    before_memory_context: string;
    after_memory_context: string;
    before_embedding_text: string;
    after_embedding_text: string;
    before_aliases: string[];
    after_aliases: string[];
    before_storage_outcome: string;
    after_storage_outcome: string;
}

export interface RebuildMemoryContextReport {
    dry_run: boolean;
    start: number;
    end: number;
    scanned: number;
    changed: number;
    previews: RebuildMemoryPreview[];
}

export interface RetrievalEvalRow {
    category: string;
    query: string;
    top_1_relevant: boolean;
    top_5_relevant: boolean;
    matched_by_semantic: boolean;
    matched_by_prefix: boolean;
    matched_by_fuzzy: boolean;
    matched_by_ngram: boolean;
    matched_by_graph: boolean;
    matched_by_alias: boolean;
    query_expansion_terms: string[];
    bad_match_reason?: string | null;
    top_memory_id?: string | null;
}

export interface RetrievalEvalReport {
    generated_at: number;
    total_queries: number;
    top1_hits: number;
    top5_hits: number;
    rows: RetrievalEvalRow[];
}

/**
 * Per-reason capture-pipeline counters returned alongside {@link CaptureStatus}.
 *
 * Mirrors `crate::ipc::commands::stats::CapturePipelineBreakdown` (Rust).
 * Every terminal branch in the capture loop bumps exactly one counter, so
 * `stored_total + skipped_total` accounts for every evaluated frame —
 * unlike the legacy `frames_captured` / `frames_dropped` numbers which
 * only counted successful stores and dedup drops.
 */
export interface CapturePipelineBreakdown {
    evaluated: number;
    stored_ocr_path: number;
    stored_visual_path: number;
    stored_url_only: number;
    stored_total: number;
    skipped_blocklist: number;
    /** FNDR was frontmost; the app never captures its own window. */
    skipped_self_app: number;
    skipped_surface_policy: number;
    skipped_perceptual_dup: number;
    skipped_semantic_dup: number;
    skipped_ocr_failed: number;
    skipped_low_signal_text: number;
    skipped_noise: number;
    skipped_grounding: number;
    skipped_stacked_extraction: number;
    skipped_visual_small: number;
    skipped_visual_novelty: number;
    skipped_visual_compose_failed: number;
    skipped_screen_capture_failed: number;
    skipped_total: number;
    last_skip_reason: string | null;
    last_skip_app: string | null;
    last_skip_timestamp_ms: number | null;
}

export interface CaptureStatus {
    is_capturing: boolean;
    is_paused: boolean;
    is_incognito: boolean;
    frames_captured: number;
    frames_dropped: number;
    last_capture_time: number;
    ai_model_available: boolean;
    ai_model_loaded: boolean;
    loaded_model_id: string | null;
    embedding_backend: string;
    embedding_degraded: boolean;
    embedding_detail: string;
    embedding_model_name: string;
    embedding_dimension: number;
    pipeline: CapturePipelineBreakdown;
}

export interface McpServerStatus {
    running: boolean;
    mode: string;
    host: string;
    port: number;
    endpoint: string;
    public_endpoint?: string | null;
    public_sse_endpoint?: string | null;
    token: string;
    use_tls: boolean;
    require_auth: boolean;
    auth_mode: string;
    last_error?: string | null;
}

export interface EvidenceRef {
    id: string;
    source_type: string;
    source_id: string;
    summary: string;
    snippet: string;
    timestamp: number;
    privacy_class: string;
}

export interface ContextPackItemReason {
    id: string;
    label: string;
    kind: string;
    reason: string;
}

export interface ExcludedContextItem {
    id: string;
    reason: string;
}

export interface RelevantFile {
    path: string;
    why: string;
}

export interface DecisionSummary {
    id: string;
    title: string;
    summary: string;
    timestamp: number;
    evidence: EvidenceRef[];
}

export interface IssueSummary {
    id: string;
    title: string;
    summary: string;
    status: string;
}

export interface FailureSummary {
    id: string;
    title: string;
    summary: string;
    error: string;
    related_files: string[];
    last_seen_at: number;
    evidence: EvidenceRef[];
}

export interface ContextTask {
    id: string;
    title: string;
    status: string;
    source: string;
    due_at?: number | null;
}

export interface EntityRef {
    canonical_id: string;
    canonical_name: string;
    entity_type: string;
    confidence: number;
    aliases: string[];
}

export interface ActivityEvent {
    id: string;
    memory_id: string;
    start_time: number;
    end_time: number;
    project?: string | null;
    repo?: string | null;
    branch?: string | null;
    activity_type: string;
    title: string;
    summary: string;
    intent?: string | null;
    outcome: string;
    entities: EntityRef[];
    source_memory_ids: string[];
    evidence: EvidenceRef[];
    confidence: number;
    memory_value: number;
    privacy_class: string;
    active_files: string[];
    errors: string[];
    commands: string[];
    decisions: string[];
    next_steps: string[];
    tags: string[];
    created_at: number;
    updated_at: number;
}

export interface ContextPack {
    id: string;
    session_id?: string | null;
    generated_at: number;
    project?: string | null;
    agent_type: string;
    budget_tokens: number;
    tokens_used: number;
    query?: string | null;
    active_goal?: string | null;
    summary: string;
    relevant_files: RelevantFile[];
    recent_decisions: DecisionSummary[];
    open_issues: IssueSummary[];
    known_failures: FailureSummary[];
    open_tasks: ContextTask[];
    recommended_next_action?: string | null;
    do_not_do: string[];
    evidence: EvidenceRef[];
    included: ContextPackItemReason[];
    excluded: ExcludedContextItem[];
    confidence: number;
}

export interface ContextDelta {
    id: string;
    session_id: string;
    since: number;
    generated_at: number;
    query?: string | null;
    new_events: ActivityEvent[];
    changed_entities: EntityRef[];
    resolved_tasks: ContextTask[];
    new_failures: FailureSummary[];
    new_items: string[];
    tokens_used: number;
}

export interface ContextRuntimeStatus {
    status: string;
    mcp_running: boolean;
    active_project?: string | null;
    current_context_pack?: string | null;
    recent_pack_count: number;
    activity_event_count: number;
    decision_count: number;
    failed_writes: number;
    last_error?: string | null;
    latest_pack_summary?: string | null;
    latest_pack_tokens_used: number;
}

export type AgentMode = "ask" | "plan" | "act" | "learn";
export type AgentRiskLevel = "low" | "medium" | "high" | "blocked";

export interface AgentContextRequest {
    user_goal: string;
    mode?: AgentMode;
    project?: string | null;
    app?: string | null;
    domain?: string | null;
    window_minutes?: number | null;
    selected_memory_ids?: string[];
    include_raw_evidence?: boolean;
    budget_tokens?: number;
}

export interface ToolPolicy {
    tool: string;
    scope: string;
    risk: AgentRiskLevel;
    allowed: boolean;
    requires_approval: boolean;
    reason: string;
}

export interface AgentMemoryCard {
    memory_id: string;
    title: string;
    summary: string;
    timestamp: number;
    app_name: string;
    window_title: string;
    url?: string | null;
    confidence: number;
    match_reason: string;
    evidence: EvidenceRef[];
}

export interface AgentContextPack {
    task_id: string;
    user_goal: string;
    mode: AgentMode;
    relevant_memories: AgentMemoryCard[];
    current_project?: unknown | null;
    recent_workflow_trace: Array<{
        timestamp: number;
        title: string;
        app_name: string;
        summary: string;
        source_memory_id: string;
    }>;
    entities: EntityRef[];
    files: Array<{ path: string; reason: string }>;
    urls: Array<{ url: string; source_memory_id: string; reason: string }>;
    commands: Array<{ command: string; source_memory_id: string; timestamp: number }>;
    errors: Array<{ summary: string; error: string; related_files: string[]; source_memory_ids: string[] }>;
    decisions: Array<{ title: string; summary: string; source_memory_ids: string[] }>;
    todos: Array<{ title: string; status: string; source: string }>;
    privacy_scope: {
        local_only: boolean;
        read_only: boolean;
        include_raw_evidence: boolean;
        include_sensitive_context: boolean;
        exclude_private_apps: boolean;
        excluded_apps_or_domains: string[];
        project?: string | null;
        window_minutes?: number | null;
        incognito_active: boolean;
    };
    allowed_tools: ToolPolicy[];
    disallowed_context: Array<{ id: string; reason: string }>;
    token_budget: { requested: number; max: number; used: number; dropped_items: number };
    confidence: number;
    evidence_summary: string;
    source_context_pack_id: string;
}

export interface AgentRunResponse {
    run_id: string;
    mode: AgentMode;
    answer: string;
    context_pack: AgentContextPack;
    proposed_actions: Array<{
        label: string;
        scope: string;
        risk: AgentRiskLevel;
        requires_approval: boolean;
    }>;
    blocked_actions: ToolPolicy[];
    audit_warning?: string | null;
}

export type AgentRunStatus = "success" | "partial" | "blocked" | "failed";
export type RetrievalFeedbackRating = "useful" | "irrelevant" | "wrong" | "stale" | "missing_context";

export interface RedactionNote {
    id: string;
    reason: string;
}

export interface MemoryRetrievalExplanation {
    memory_id: string;
    title: string;
    matched_reason: string;
    app_name: string;
    url?: string | null;
    timestamp: number;
    confidence: number;
    semantic_relevance: string;
    keyword_match: string;
    recency: string;
    project_match: string;
    app_domain_match: string;
    workflow_continuity: string;
}

export interface AgentRetrievalFeedback {
    feedback_id: string;
    run_id: string;
    memory_id?: string | null;
    rating: RetrievalFeedbackRating;
    note?: string | null;
    created_at: number;
}

export interface AgentAuditRecord {
    run_id: string;
    created_at: number;
    user_goal: string;
    mode: AgentMode;
    context_pack_id?: string | null;
    memories_used: string[];
    tools_requested: string[];
    tools_allowed: ToolPolicy[];
    tools_blocked: ToolPolicy[];
    approvals_required: ToolPolicy[];
    redactions_applied: RedactionNote[];
    dropped_context: RedactionNote[];
    confidence: number;
    output_summary: string;
    result_status: AgentRunStatus;
    error_message?: string | null;
    selected_memories: MemoryRetrievalExplanation[];
    feedback: AgentRetrievalFeedback[];
}

export interface RetrievalExplanation {
    run_id?: string | null;
    context_pack_id?: string | null;
    selected_memories: MemoryRetrievalExplanation[];
    dropped_context: RedactionNote[];
    redacted_context: RedactionNote[];
    privacy_policy_reasons: string[];
    limitations: string[];
}

export interface RateResultRequest {
    run_id: string;
    memory_id?: string | null;
    rating: RetrievalFeedbackRating;
    note?: string | null;
}

export interface AgentSkillCandidate {
    draft_id: string;
    name: string;
    category: string;
    source: string;
    created_from_memories: string[];
    risk_level: AgentRiskLevel;
    requires_approval: boolean;
    last_verified?: number | null;
    when_to_use: string;
    required_context: string[];
    procedure: string[];
    verification: string[];
    failure_cases: string[];
    privacy_notes: string[];
}

export interface AgentEvalCase {
    eval_id: string;
    workflow_name: string;
    input_context_pack_id: string;
    expected_outcome: string;
    forbidden_actions: string[];
    required_evidence: string[];
    grading_rules: string[];
    privacy_scope: AgentContextPack["privacy_scope"];
}

export interface AgentPrompt {
    name: string;
    title: string;
    description: string;
    template: string;
}

export interface AppMergeCount {
    app_name: string;
    merged: number;
}

export interface MemoryRepairSummary {
    total_before: number;
    total_after: number;
    merged_count: number;
    anchor_merges: number;
    task_reference_updates: number;
    screenshots_cleaned: number;
    embeddings_refreshed: number;
    chars_before: number;
    chars_after: number;
    chars_reclaimed: number;
    app_merges: AppMergeCount[];
}

export interface StorageReclaimSummary {
    records_scanned: number;
    records_rewritten: number;
    screenshot_paths_cleared: number;
    screenshot_files_deleted: number;
    embeddings_refreshed: number;
    snippet_embeddings_refreshed: number;
    support_embeddings_refreshed: number;
    chars_before: number;
    chars_after: number;
    chars_reclaimed: number;
    bytes_before: number;
    bytes_after: number;
    bytes_reclaimed: number;
}

export interface StorageReclaimProgress {
    is_running: boolean;
    phase: string;
    processed: number;
    total: number;
    records_rewritten: number;
    screenshot_paths_cleared: number;
    screenshot_files_deleted: number;
    embeddings_refreshed: number;
    snippet_embeddings_refreshed: number;
    support_embeddings_refreshed: number;
    timestamp_ms: number;
}

export interface StorageHealth {
    memory_db_bytes: number;
    frames_bytes: number;
    models_bytes: number;
    dev_build_cache_bytes: number;
    runtime_total_bytes: number;
    measured_at_ms: number;
}

export interface MemoryRepairProgress {
    is_running: boolean;
    phase: string;
    processed: number;
    total: number;
    merged_count: number;
    anchor_merges: number;
    timestamp_ms: number;
}

export interface MeetingBreakdown {
    todos: string[];
    reminders: string[];
    followups: string[];
    summary: string;
}

export interface MeetingSession {
    id: string;
    title: string;
    participants: string[];
    model: string;
    status: "recording" | "stopped" | "error" | "analyzing";
    start_timestamp: number;
    end_timestamp?: number | null;
    created_at: number;
    updated_at: number;
    segment_count: number;
    duration_seconds: number;
    meeting_dir: string;
    audio_dir: string;
    transcript_path?: string | null;
    breakdown?: MeetingBreakdown | null;
}

export interface MeetingSegment {
    id: string;
    meeting_id: string;
    index: number;
    start_timestamp: number;
    end_timestamp: number;
    text: string;
    audio_chunk_path: string;
    model: string;
    created_at: number;
}

export interface MeetingRecorderStatus {
    is_recording: boolean;
    current_meeting_id?: string | null;
    current_title?: string | null;
    model?: string | null;
    started_at?: number | null;
    ffmpeg_available: boolean;
    transcription_backend: string;
    is_analyzing: boolean;
    last_error?: string | null;
}

export interface MeetingTranscript {
    meeting: MeetingSession;
    segments: MeetingSegment[];
    full_text: string;
}


export interface Stats {
    total_records: number;
    total_days: number;
    apps: { name: string; count: number }[];
    today_count: number;
    unique_apps: number;
    unique_sessions: number;
    unique_window_titles: number;
    unique_urls: number;
    unique_domains: number;
    records_with_url: number;
    records_with_screenshot: number;
    records_with_clean_text: number;
    records_last_hour: number;
    records_last_24h: number;
    records_last_7d: number;
    avg_records_per_active_day: number;
    avg_records_per_hour: number;
    focus_app_share_pct: number;
    app_switches: number;
    app_switch_rate_per_hour: number;
    avg_gap_minutes: number;
    longest_gap_minutes: number;
    first_capture_ts: number | null;
    last_capture_ts: number | null;
    capture_span_hours: number;
    current_streak_days: number;
    longest_streak_days: number;
    avg_ocr_confidence: number;
    low_confidence_records: number;
    avg_noise_score: number;
    high_noise_records: number;
    avg_ocr_blocks: number;
    llm_count: number;
    vlm_count: number;
    fallback_count: number;
    other_summary_count: number;
    top_domains: { domain: string; count: number }[];
    busiest_day: { day: string; count: number } | null;
    quietest_day: { day: string; count: number } | null;
    busiest_hour: { hour: number; count: number } | null;
    hourly_distribution: { hour: number; count: number }[];
    weekday_distribution: { weekday: string; count: number }[];
    daypart_distribution: { daypart: string; count: number }[];
}

const DEFAULT_STATS: Stats = {
    total_records: 0,
    total_days: 0,
    apps: [],
    today_count: 0,
    unique_apps: 0,
    unique_sessions: 0,
    unique_window_titles: 0,
    unique_urls: 0,
    unique_domains: 0,
    records_with_url: 0,
    records_with_screenshot: 0,
    records_with_clean_text: 0,
    records_last_hour: 0,
    records_last_24h: 0,
    records_last_7d: 0,
    avg_records_per_active_day: 0,
    avg_records_per_hour: 0,
    focus_app_share_pct: 0,
    app_switches: 0,
    app_switch_rate_per_hour: 0,
    avg_gap_minutes: 0,
    longest_gap_minutes: 0,
    first_capture_ts: null,
    last_capture_ts: null,
    capture_span_hours: 0,
    current_streak_days: 0,
    longest_streak_days: 0,
    avg_ocr_confidence: 0,
    low_confidence_records: 0,
    avg_noise_score: 0,
    high_noise_records: 0,
    avg_ocr_blocks: 0,
    llm_count: 0,
    vlm_count: 0,
    fallback_count: 0,
    other_summary_count: 0,
    top_domains: [],
    busiest_day: null,
    quietest_day: null,
    busiest_hour: null,
    hourly_distribution: [],
    weekday_distribution: [],
    daypart_distribution: [],
};

export interface Task {
    id: string;
    title: string;
    description: string;
    source_app: string;
    source_memory_id: string | null;
    created_at: number;
    due_date: number | null;
    is_completed: boolean;
    is_dismissed: boolean;
    task_type: "Todo" | "Reminder" | "Followup";
    linked_urls: string[];
    linked_memory_ids: string[];
}



export interface VoiceTranscriptionResult {
    text: string;
    backend: string;
}

// Search functions
export async function search(
    query: string,
    timeFilter?: string,
    appFilter?: string,
    limit?: number
): Promise<SearchResult[]> {
    return invoke<SearchResult[]>("search", {
        query,
        timeFilter,
        appFilter,
        limit,
    });
}

// Debug-only raw retrieval path (no grouping/synthesis).
export async function searchRawResults(
    query: string,
    timeFilter?: string,
    appFilter?: string,
    limit?: number
): Promise<SearchResult[]> {
    return invoke<SearchResult[]>("search_raw_results", {
        query,
        timeFilter,
        appFilter,
        limit,
    });
}

export async function searchMemoryCards(
    query: string,
    timeFilter?: string,
    appFilter?: string,
    limit?: number
): Promise<MemoryCard[]> {
    return invoke<MemoryCard[]>("search_memory_cards", {
        query,
        timeFilter,
        appFilter,
        limit,
    });
}

export async function listMemoryCards(
    limit?: number,
    appFilter?: string | null
): Promise<MemoryCard[]> {
    return invoke<MemoryCard[]>("list_memory_cards", {
        limit,
        appFilter: appFilter?.trim() || null,
    });
}

export async function reopenMemory(memoryId: string): Promise<boolean> {
    return invoke<boolean>("reopen_memory", {
        memoryId,
    });
}

// Image-to-image retrieval over the CLIP `image_embedding` column. Returns
// an empty list when the seed memory is unknown or pre-dates the CLIP wiring
// (legacy zero image vector). Cross-modal text->image is intentionally NOT
// supported here; the backend rejects it per ADR-004 / ADR-005.
export async function findVisuallySimilarMemories(args: {
    seedMemoryId: string;
    limit?: number;
    timeFilter?: string;
    appFilter?: string;
}): Promise<SearchResult[]> {
    return invoke<SearchResult[]>("find_visually_similar_memories", {
        seedMemoryId: args.seedMemoryId,
        limit: args.limit,
        timeFilter: args.timeFilter,
        appFilter: args.appFilter,
    });
}

export interface InsightGraphNode {
    id: string;
    node_type: string;
    label: string;
    confidence: number;
    source_memory_ids: string[];
    embedding?: number[] | null;
    created_at: string;
    updated_at: string;
    stale: boolean;
    metadata: unknown;
}

export interface InsightGraphEdge {
    id: string;
    source_id: string;
    target_id: string;
    edge_type: string;
    confidence: number;
    conflict_flag: boolean;
    created_at: string;
    metadata: unknown;
}

export interface InsightGraphSubgraph {
    nodes: InsightGraphNode[];
    edges: InsightGraphEdge[];
    /** Graph node id → Louvain community id (backend `attach_louvain_metadata`). */
    louvain?: Record<string, number>;
    cluster_0_name?: string;
}

/** Alias for graph UI code. */
export type GraphNode = InsightGraphNode;
export type GraphEdge = InsightGraphEdge;

export async function getFullGraph(): Promise<InsightGraphSubgraph> {
    return invoke<InsightGraphSubgraph>("get_full_graph");
}

export async function getGraphForProject(projectLabel: string): Promise<InsightGraphSubgraph> {
    return invoke<InsightGraphSubgraph>("get_graph_for_project", { projectLabel });
}

export async function searchGraph(queryEmbedding: number[], k = 8): Promise<InsightGraphNode[]> {
    return invoke<InsightGraphNode[]>("search_graph", { queryEmbedding, k });
}

export async function getNodeDetail(id: string): Promise<InsightGraphNode | null> {
    return invoke<InsightGraphNode | null>("get_node_detail", { id });
}

export async function findGraphPath(
    from: string,
    to: string
): Promise<{ nodes: string[]; avg_confidence: number } | null> {
    return invoke<{ nodes: string[]; avg_confidence: number } | null>("find_graph_path", { from, to });
}

export async function getGodNodes(k = 8): Promise<{
    nodes: [string, number][];
    louvain: Record<string, number>;
    cluster_0_name: string;
}> {
    return invoke("get_god_nodes", { k });
}

export interface GraphBackfillReport {
    scanned: number;
    queued: number;
    low_confidence_queued: number;
}

export async function backfillGraphFromExistingMemories(limit?: number): Promise<GraphBackfillReport> {
    return invoke<GraphBackfillReport>("backfill_graph_from_existing_memories", {
        limit: typeof limit === "number" ? limit : null,
    });
}

export async function deleteMemory(memoryId: string): Promise<boolean> {
    return invoke<boolean>("delete_memory", { memoryId });
}

/** Import a photo (e.g. from Meta glasses) into memory. Omit path to open a file picker. */
export async function importMetaGlassesPhoto(path?: string | null): Promise<string> {
    return invoke<string>("import_meta_glasses_photo", { path: path ?? null });
}

export async function getMemoryDebugInspector(
    memoryId: string,
    query?: string
): Promise<MemoryDebugInspector> {
    return invoke<MemoryDebugInspector>("get_memory_debug_inspector", { memoryId, query: query?.trim() || null });
}

export async function evaluateRecentMemoryQuality(
    lookbackMinutes = 180,
    limit = 180
): Promise<MemoryValidationReport> {
    return invoke<MemoryValidationReport>("evaluate_recent_memory_quality", { lookbackMinutes, limit });
}

export async function rebuildMemoryContextForRange(
    start: number,
    end: number,
    dryRun = true
): Promise<RebuildMemoryContextReport> {
    return invoke<RebuildMemoryContextReport>("rebuild_memory_context_for_range", { start, end, dryRun });
}

export interface InsightBackfillReport {
    changed: number;
    dry_run: boolean;
}

export async function backfillInsightLayersForRange(
    start: number,
    end: number,
    dryRun = true
): Promise<InsightBackfillReport> {
    return invoke<InsightBackfillReport>("backfill_insight_layers_for_range", { start, end, dryRun });
}

export interface IdleWikiCompileSummary {
    pages_upserted: number;
}

export async function runIdleWikiKnowledgeCompile(): Promise<IdleWikiCompileSummary> {
    return invoke<IdleWikiCompileSummary>("run_idle_wiki_knowledge_compile");
}

export async function runMemoryRetrievalEval(): Promise<RetrievalEvalReport> {
    return invoke<RetrievalEvalReport>("run_memory_retrieval_eval");
}

export async function generateDailyBriefing(mode?: "morning" | "evening"): Promise<string> {
    return invoke<string>("generate_daily_briefing", { mode });
}

export async function getFunGreeting(name?: string | null): Promise<string> {
    return invoke<string>("get_fun_greeting", { name });
}



// Capture control
export async function getStatus(): Promise<CaptureStatus> {
    return invoke<CaptureStatus>("get_status");
}

export async function getMcpServerStatus(): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("get_mcp_server_status");
}

export async function startMcpServer(port?: number): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("start_mcp_server", { port });
}

export async function stopMcpServer(): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("stop_mcp_server");
}

export async function getContextRuntimeStatus(): Promise<ContextRuntimeStatus> {
    return invoke<ContextRuntimeStatus>("get_context_runtime_status");
}

export async function listRecentContextPacks(limit = 8): Promise<ContextPack[]> {
    return invoke<ContextPack[]>("list_recent_context_packs", { limit });
}

export async function buildAgentContextPack(
    request: AgentContextRequest
): Promise<AgentContextPack> {
    return invoke<AgentContextPack>("build_agent_context_pack", { request });
}

export async function runAgentRequest(
    request: AgentContextRequest
): Promise<AgentRunResponse> {
    return invoke<AgentRunResponse>("run_agent_request", { request });
}

export async function listAgentAuditRuns(
    limit = 20,
    mode?: AgentMode | null,
    status?: AgentRunStatus | null
): Promise<AgentAuditRecord[]> {
    return invoke<AgentAuditRecord[]>("list_agent_audit_runs", { limit, mode: mode ?? null, status: status ?? null });
}

export async function getAgentAuditRun(runId: string): Promise<AgentAuditRecord | null> {
    return invoke<AgentAuditRecord | null>("get_agent_audit_run", { runId });
}

export async function explainAgentRetrieval(request: {
    run_id?: string | null;
    context_pack_id?: string | null;
    query?: string | null;
    project?: string | null;
}): Promise<RetrievalExplanation> {
    return invoke<RetrievalExplanation>("explain_agent_retrieval", { request });
}

export async function rateAgentResult(request: RateResultRequest): Promise<AgentRetrievalFeedback> {
    return invoke<AgentRetrievalFeedback>("rate_agent_result", { request });
}

export async function proposeSkillFromRun(runId: string): Promise<AgentSkillCandidate> {
    return invoke<AgentSkillCandidate>("propose_skill_from_run", { runId });
}

export async function listAgentSkillDrafts(limit = 20): Promise<AgentSkillCandidate[]> {
    return invoke<AgentSkillCandidate[]>("list_agent_skill_drafts", { limit });
}

export async function proposeEvalFromRun(runId: string): Promise<AgentEvalCase> {
    return invoke<AgentEvalCase>("propose_eval_from_run", { runId });
}

export async function listAgentEvalDrafts(limit = 20): Promise<AgentEvalCase[]> {
    return invoke<AgentEvalCase[]>("list_agent_eval_drafts", { limit });
}

export async function listAgentPrompts(): Promise<AgentPrompt[]> {
    return invoke<AgentPrompt[]>("list_agent_prompts");
}

export async function getAgentPrompt(name: string): Promise<AgentPrompt | null> {
    return invoke<AgentPrompt | null>("get_agent_prompt", { name });
}

export async function fndrSubscribe(sessionId: string): Promise<boolean> {
    return invoke<boolean>("fndr_subscribe", { sessionId });
}

export async function fndrUnsubscribe(sessionId: string): Promise<boolean> {
    return invoke<boolean>("fndr_unsubscribe", { sessionId });
}

export function onContextDelta(handler: (delta: ContextDelta) => void): Promise<() => void> {
    return listen<ContextDelta>("context://delta", (event) => {
        handler(event.payload);
    });
}

// Meeting Recorder
export async function getMeetingStatus(): Promise<MeetingRecorderStatus> {
    return invoke<MeetingRecorderStatus>("get_meeting_status");
}

export function onMeetingStatus(handler: (status: MeetingRecorderStatus) => void): Promise<() => void> {
    return listen<MeetingRecorderStatus>("meeting://status", (event) => {
        handler(event.payload);
    });
}

export async function startMeetingRecording(
    title: string,
    participants: string[],
    model?: string
): Promise<MeetingRecorderStatus> {
    return invoke<MeetingRecorderStatus>("start_meeting_recording", { title, participants, model });
}

export async function stopMeetingRecording(): Promise<MeetingRecorderStatus> {
    return invoke<MeetingRecorderStatus>("stop_meeting_recording");
}

export async function listMeetings(): Promise<MeetingSession[]> {
    return invoke<MeetingSession[]>("list_meetings");
}

export async function deleteMeeting(meetingId: string): Promise<boolean> {
    return invoke<boolean>("delete_meeting", { meetingId });
}

export async function getMeetingTranscript(meetingId: string): Promise<MeetingTranscript> {
    return invoke<MeetingTranscript>("get_meeting_transcript", { meetingId });
}

export async function retranscribeMeeting(meetingId: string): Promise<void> {
    return invoke<void>("retranscribe_meeting", { meetingId });
}


export async function transcribeVoiceInput(
    audioBytes: number[],
    mimeType?: string
): Promise<VoiceTranscriptionResult> {
    return invoke<VoiceTranscriptionResult>("transcribe_voice_input", { audioBytes, mimeType });
}

export async function pauseCapture(): Promise<void> {
    return invoke("pause_capture");
}

export async function resumeCapture(): Promise<void> {
    return invoke("resume_capture");
}

// Privacy
export async function getBlocklist(): Promise<string[]> {
    return invoke<string[]>("get_blocklist");
}

export async function setBlocklist(apps: string[]): Promise<void> {
    return invoke("set_blocklist", { apps });
}

export async function deleteAllData(): Promise<void> {
    return invoke("delete_all_data");
}

export interface PrivacyAlert {
    id: string;
    domain_or_title: string;
    detected_at: number;
}

export async function getPrivacyAlerts(): Promise<PrivacyAlert[]> {
    return invoke("get_privacy_alerts");
}

export async function dismissPrivacyAlert(site: string): Promise<void> {
    return invoke("dismiss_privacy_alert", { site });
}

export async function addSiteToBlocklist(site: string): Promise<void> {
    return invoke("add_to_blocklist", { site });
}

// Stats
export async function getStats(): Promise<Stats> {
    const raw = await invoke<Partial<Stats>>("get_stats");
    return {
        ...DEFAULT_STATS,
        ...raw,
        apps: raw.apps ?? [],
        top_domains: raw.top_domains ?? [],
        busiest_day: raw.busiest_day ?? null,
        quietest_day: raw.quietest_day ?? null,
        busiest_hour: raw.busiest_hour ?? null,
        hourly_distribution: raw.hourly_distribution ?? [],
        weekday_distribution: raw.weekday_distribution ?? [],
        daypart_distribution: raw.daypart_distribution ?? [],
    };
}

export interface RuntimeAggregateSnapshot {
    n: number;
    sum_ms: number;
    max_ms: number;
    avg_ms: number;
    ewma_ms: number;
}

export interface RuntimeRecentSnapshot {
    ts_ms: number;
    op: string;
    ms: number;
    meta: string | null;
}

export interface SystemMetricsSnapshot {
    generated_at_ms: number;
    sample_interval_ms: number;
    process_cpu: {
        cpu_percent: number;
        user_time_ms: number;
        system_time_ms: number;
        threads: number;
    };
    process_memory: {
        rss_bytes: number;
        virtual_bytes: number;
        phys_footprint_bytes: number;
        lifetime_max_phys_footprint_bytes: number;
    };
    process_io: {
        disk_bytes_read: number;
        disk_bytes_written: number;
        disk_read_rate_bps: number;
        disk_write_rate_bps: number;
    };
    process_energy: {
        idle_wakeups: number;
        interrupt_wakeups: number;
        billed_system_time_ns: number;
        label: string;
    };
    host_cpu: {
        cpu_percent_total: number;
        cpu_percent_per_core: number[];
    };
    host_memory: {
        page_size_bytes: number;
        free_bytes: number;
        active_bytes: number;
        inactive_bytes: number;
        wired_bytes: number;
        compressed_bytes: number;
        total_bytes: number;
        pressure_label: string;
    };
    gpu: {
        device_utilization_percent: number | null;
        renderer_utilization_percent: number | null;
        in_use_system_memory_bytes: number | null;
        recovery_count: number | null;
    };
    model_memory: Array<{
        id: string;
        kind: string;
        estimated_bytes: number;
        loaded: boolean;
    }>;
}

export interface RuntimeMetricsSnapshot {
    generated_at_ms: number;
    process_rss_bytes: number | null;
    capture: {
        frames_captured: number;
        frames_dropped: number;
        last_capture_time_ms: number;
    };
    embedding: {
        backend: string;
        degraded: boolean;
        detail: string;
        model_name: string;
        dimension: number;
        clip_session_loaded: boolean;
        last_clip_infer_ms: number;
    };
    inference: {
        ai_model_available: boolean;
        ai_model_loaded: boolean;
        loaded_model_id: string | null;
    };
    aggregates: Record<string, RuntimeAggregateSnapshot>;
    counters: Record<string, number>;
    recent: RuntimeRecentSnapshot[];
    system: SystemMetricsSnapshot;
}

export async function getRuntimeMetrics(): Promise<RuntimeMetricsSnapshot> {
    return invoke<RuntimeMetricsSnapshot>("get_runtime_metrics");
}

export interface MemoryReviewWorkerStatus {
    queue_depth: number;
    last_review_at_ms: number;
    last_error_kind: string | null;
    worker_enabled: boolean;
    pressure_blocked: boolean;
}

/** Snapshot of the local memory-review worker (queue depth, last error, gating).
 *  Surfaced in the engine-metrics panel so users can see whether reviewing is
 *  progressing or blocked by inference unavailability / system pressure. */
export async function getMemoryReviewStatus(): Promise<MemoryReviewWorkerStatus> {
    return invoke<MemoryReviewWorkerStatus>("get_memory_review_status");
}

export type DailyReviewOutcome =
    | { kind: "changed"; memory_id: string }
    | { kind: "invalid_patch"; memory_id: string; reason: string }
    | { kind: "provider_failure"; memory_id: string; reason: string }
    | { kind: "already_reviewed"; memory_id: string };

export interface DailyReviewSummary {
    day: string;
    start_ms: number;
    end_ms: number;
    dry_run: boolean;
    scanned: number;
    changed: number;
    would_change: number;
    failed: number;
    skipped_pressure: number;
    skipped_already_reviewed: number;
    outcomes: DailyReviewOutcome[];
}

export interface BackfillReviewSummary {
    start_ms: number;
    end_ms: number;
    dry_run: boolean;
    scanned: number;
    queued: number;
    would_queue: number;
    already_reviewed: number;
    already_queued: number;
}

/** Manually run the daily memory-review batch for a YYYY-MM-DD date.
 *  Pass `dryRun: true` to compute patches without writing them. Requires the
 *  local inference engine to be loaded. */
export async function runDailyMemoryReview(
    date: string,
    dryRun = false,
): Promise<DailyReviewSummary> {
    return invoke<DailyReviewSummary>("run_daily_memory_review_cmd", { date, dryRun });
}

/** Backfill the post-capture memory-review worker queue for the given range.
 *  Returns the count that was (or would be in dry-run) queued; the worker
 *  drains the queue under the existing pressure gate. */
export async function backfillMemoryReview(
    startMs: number,
    endMs: number,
    dryRun = false,
): Promise<BackfillReviewSummary> {
    return invoke<BackfillReviewSummary>("backfill_memory_review", {
        startMs,
        endMs,
        dryRun,
    });
}

export async function getRetentionDays(): Promise<number> {
    return invoke<number>("get_retention_days");
}

export async function setRetentionDays(days: number): Promise<void> {
    return invoke("set_retention_days", { days });
}

export async function deleteOlderThan(days: number): Promise<number> {
    return invoke<number>("delete_older_than", { days });
}

export async function getAppNames(): Promise<string[]> {
    return invoke<string[]>("get_app_names");
}

// Task functions
export async function getTodos(): Promise<Task[]> {
    return invoke<Task[]>("get_todos");
}

export async function addTodo(
    title: string,
    taskType?: "Todo" | "Reminder" | "Followup"
): Promise<Task> {
    return invoke<Task>("add_todo", { title, taskType });
}

export async function dismissTodo(taskId: string): Promise<boolean> {
    return invoke<boolean>("dismiss_todo", { taskId });
}

export async function updateTodo(
    taskId: string,
    title: string,
    taskType?: "Todo" | "Reminder" | "Followup"
): Promise<Task> {
    return invoke<Task>("update_todo", { taskId, title, taskType });
}

// ========== Agent SDK Functions ==========

export interface AgentStatus {
    is_running: boolean;
    task_title: string | null;
    last_message: string | null;
    status: "idle" | "running" | "completed" | "error";
}

export interface HermesAppContext {
    app_name: string;
    memory_count: number;
}

export interface HermesMemoryDigest {
    title: string;
    app_name: string;
    summary: string;
    timestamp: number;
}

export interface HermesBridgeStatus {
    installed: boolean;
    configured: boolean;
    setup_complete: boolean;
    gateway_running: boolean;
    api_server_ready: boolean;
    version: string | null;
    bundled_repo_available: boolean;
    runtime_source: string | null;
    provider_kind: string | null;
    model_name: string | null;
    base_url: string | null;
    api_url: string;
    gateway_dir: string;
    home_dir: string;
    context_path: string;
    context_ready: boolean;
    last_synced_at: number | null;
    fndr_local_model_id: string | null;
    ollama_installed: boolean;
    ollama_reachable: boolean;
    ollama_models: string[];
    ollama_base_url: string;
    codex_cli_installed: boolean;
    codex_logged_in: boolean;
    codex_auth_path: string;
    profile_name: string | null;
    focus_task: string | null;
    recent_memory_count: number;
    open_task_count: number;
    /** True when Ollama is configured and reachable — chat works without the Hermes CLI. */
    direct_ollama_ready: boolean;
    top_apps: HermesAppContext[];
    recent_memories: HermesMemoryDigest[];
    last_error: string | null;
    install_command: string;
}

export interface HermesSetupPayload {
    provider_kind: string;
    model_name: string;
    api_key?: string | null;
    base_url?: string | null;
}

export interface HermesChatReply {
    response_id: string;
    conversation_id: string;
    content: string;
}

export async function startAgentTask(
    taskTitle: string,
    contextUrls?: string[],
    contextNotes?: string[]
): Promise<AgentStatus> {
    return invoke<AgentStatus>("start_agent_task", { taskTitle, contextUrls, contextNotes });
}

export async function getAgentStatus(): Promise<AgentStatus> {
    return invoke<AgentStatus>("get_agent_status");
}

export async function stopAgent(): Promise<AgentStatus> {
    return invoke<AgentStatus>("stop_agent");
}

export async function getHermesBridgeStatus(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("get_hermes_bridge_status");
}

export async function installHermesBridge(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("install_hermes_bridge");
}

export async function saveHermesSetup(payload: HermesSetupPayload): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("save_hermes_setup", { payload });
}

export async function syncHermesBridgeContext(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("sync_hermes_bridge_context");
}

export async function startHermesGateway(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("start_hermes_gateway");
}

export async function stopHermesGateway(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("stop_hermes_gateway");
}

export async function sendHermesMessage(
    conversationId: string,
    input: string
): Promise<HermesChatReply> {
    return invoke<HermesChatReply>("send_hermes_message", { conversationId, input });
}

export async function quickSetupOllama(): Promise<HermesBridgeStatus> {
    return invoke<HermesBridgeStatus>("quick_setup_ollama");
}

/**
 * Send a message directly to Ollama — no Hermes CLI required.
 * messages is the prior conversation in OpenAI format: [{role, content}].
 */
export async function sendDirectChat(
    messages: Array<{ role: string; content: string }>,
    input: string
): Promise<string> {
    return invoke<string>("send_direct_chat", { messages, input });
}

export async function summarizeSearch(query: string, snippets: string[]): Promise<string> {
    return invoke<string>("summarize_search", { query, resultsSnippets: snippets });
}

export async function runMemoryRepairBackfill(): Promise<MemoryRepairSummary> {
    return invoke<MemoryRepairSummary>("run_memory_repair_backfill");
}

export async function getMemoryRepairProgress(): Promise<MemoryRepairProgress> {
    return invoke<MemoryRepairProgress>("get_memory_repair_progress");
}

export async function getStorageReclaimProgress(): Promise<StorageReclaimProgress> {
    return invoke<StorageReclaimProgress>("get_storage_reclaim_progress");
}

export async function getStorageHealth(): Promise<StorageHealth> {
    return invoke<StorageHealth>("get_storage_health");
}

export async function cleanDevBuildCache(): Promise<StorageHealth> {
    return invoke<StorageHealth>("clean_dev_build_cache");
}

export async function reclaimMemoryStorage(): Promise<StorageReclaimSummary> {
    return invoke<StorageReclaimSummary>("reclaim_memory_storage");
}

export async function generateDailySummaryForDate(dateStr: string): Promise<string> {
    return invoke<string>("generate_daily_summary_for_date", { dateStr });
}

export async function exportDailySummaryPdf(dateStr: string, summaryText: string): Promise<string> {
    return invoke<string>("export_daily_summary_pdf", { dateStr, summaryText });
}

export async function openExportedPdf(path: string): Promise<void> {
    return invoke<void>("open_exported_pdf", { path });
}

// Proactive surface
export interface ProactiveSuggestion {
    memory_id: string;
    snippet: string;
    similarity: number;
    task_title: string | null;
}

export interface FndrNotificationPayload {
    title: string;
    body: string;
    kind: string;
}

export function onProactiveSuggestion(
    callback: (suggestion: ProactiveSuggestion) => void
): Promise<() => void> {
    return listen<ProactiveSuggestion>("proactive_suggestion", (event) => {
        callback(event.payload);
    });
}

export function onFndrNotification(
    callback: (notification: FndrNotificationPayload) => void
): Promise<() => void> {
    return listen<FndrNotificationPayload>("fndr_notification", (event) => {
        callback(event.payload);
    });
}

// Time Tracking
export interface AppTimeEntry {
    app_name: string;
    duration_minutes: number;
    capture_count: number;
    last_seen: number;
}

export interface TimeTrackingResult {
    date: string;
    total_captures: number;
    breakdown: AppTimeEntry[];
}

export async function getTimeTracking(): Promise<TimeTrackingResult> {
    return invoke<TimeTrackingResult>("get_time_tracking");
}

// Focus Mode
export interface FocusStatus {
    task: string | null;
    is_active: boolean;
    drift_count: number;
}

export async function setFocusTask(task: string | null): Promise<FocusStatus> {
    return invoke<FocusStatus>("set_focus_task", { task });
}

export async function getFocusStatus(): Promise<FocusStatus> {
    return invoke<FocusStatus>("get_focus_status");
}

// Auto-fill
export interface FieldContext {
    label: string;
    placeholder: string;
    app_name: string;
    bundle_id?: string | null;
    window_title: string;
    current_value: string;
    screen_context: string;
    inferred_label: string;
}

export interface AutofillSettings {
    enabled: boolean;
    shortcut: string;
    lookback_days: number;
    auto_inject_threshold: number;
    prefer_typed_injection: boolean;
    max_candidates: number;
}

export interface AutofillCandidate {
    value: string;
    confidence: number;
    match_reason: string;
    source_snippet: string;
    source_app: string;
    source_window_title: string;
    timestamp: number;
    memory_id: string;
}

export interface AutofillResolution {
    query: string;
    query_source: string;
    context_hint: string;
    candidates: AutofillCandidate[];
    auto_inject_threshold: number;
    requires_confirmation: boolean;
    used_ocr_fallback: boolean;
}

export interface AutofillScanningState {
    scanning: true;
    message?: string;
}

export type AutofillOverlayPayload =
    | FieldContext
    | { error: string }
    | AutofillScanningState;

export async function getAutofillSettings(): Promise<AutofillSettings> {
    return invoke<AutofillSettings>("get_autofill_settings");
}

export async function setAutofillSettings(settings: AutofillSettings): Promise<AutofillSettings> {
    return invoke<AutofillSettings>("set_autofill_settings", { settings });
}

export async function resolveAutofill(
    context: FieldContext,
    queryOverride?: string | null,
): Promise<AutofillResolution> {
    return invoke<AutofillResolution>("resolve_autofill", { context, queryOverride });
}

export async function injectText(text: string): Promise<void> {
    return invoke("inject_text", { text });
}

export async function dismissAutofill(): Promise<void> {
    return invoke("dismiss_autofill");
}

export async function setAutofillOverlayReady(
    ready: boolean,
): Promise<AutofillOverlayPayload | null> {
    return invoke("set_autofill_overlay_ready", { ready });
}

export async function takePendingAutofillPayload(): Promise<AutofillOverlayPayload | null> {
    return invoke("take_pending_autofill_payload");
}

// ----- Debug / inspection -----

export interface MemoryPipelineInspection {
    memory_id: string;
    synthesis_branch: string;
    app_name: string;
    window_title: string;
    topic: string;
    topic_categories: string[];
    entities: string[];
    search_aliases: string[];
    display_summary: string;
    memory_context: string;
    insight_what_happened: string;
    insight_why_mattered: string;
    insight_what_changed: string;
    insight_card_confidence: number;
    ocr_confidence: number;
    ocr_noise_score: number;
    embedding_document_text: string;
    embedding_aliases: string[];
    embedding_dim: number;
    has_image_embedding: boolean;
    insight_spans_json: string;
    lexical_shadow: string;
}

export async function inspectMemoryPipeline(
    memoryId: string,
): Promise<MemoryPipelineInspection> {
    return invoke("inspect_memory_pipeline", { memoryId });
}

export interface TimelineThreadEntry {
    memory_id: string;
    timestamp: number;
    app_name: string;
    window_title: string;
    insight_what_happened: string;
    project: string;
    topic_categories: string[];
}

export interface MemoryTimelineThread {
    memory_id: string;
    ancestors: TimelineThreadEntry[];
    focus: TimelineThreadEntry;
    descendants: TimelineThreadEntry[];
}

export async function getMemoryTimelineThread(
    memoryId: string,
): Promise<MemoryTimelineThread> {
    return invoke("get_memory_timeline_thread", { memoryId });
}

// ── Companion API (iPhone / Apple Watch) ─────────────────────────────────────

export type CompanionDeviceType = "iphone" | "watch" | "other";

export interface CompanionStatusPayload {
    running: boolean;
    host: string;
    port: number;
    tls: boolean;
    base_url: string;
    mac_name: string;
    last_error: string | null;
}

export interface CompanionEndpoint {
    host: string;
    port: number;
    base_url: string;
    tls: boolean;
    cert_fingerprint_sha256: string | null;
    mac_name: string;
    app_version: string;
}

export interface CompanionPairStartResponse {
    pairing_code: string;
    qr_payload: string;
    expires_at_ms: number;
    host: string;
    port: number;
    cert_fingerprint_sha256: string | null;
}

export interface CompanionDeviceListEntry {
    device_id: string;
    device_name: string;
    device_type: CompanionDeviceType;
    paired_at_ms: number;
    last_seen_at_ms: number;
    revoked_at_ms: number | null;
    app_version: string | null;
}

export async function companionGetStatus(): Promise<CompanionStatusPayload> {
    return invoke<CompanionStatusPayload>("companion_get_status");
}

export async function companionGetEndpoint(): Promise<CompanionEndpoint | null> {
    return invoke<CompanionEndpoint | null>("companion_get_endpoint");
}

export async function companionStartServer(port?: number): Promise<CompanionStatusPayload> {
    return invoke<CompanionStatusPayload>("companion_start_server", { port });
}

export async function companionStopServer(): Promise<CompanionStatusPayload> {
    return invoke<CompanionStatusPayload>("companion_stop_server");
}

export async function companionStartPairing(): Promise<CompanionPairStartResponse> {
    return invoke<CompanionPairStartResponse>("companion_start_pairing");
}

export async function companionListDevices(): Promise<CompanionDeviceListEntry[]> {
    return invoke<CompanionDeviceListEntry[]>("companion_list_devices");
}

export async function companionRevokeDevice(deviceId: string): Promise<boolean> {
    return invoke<boolean>("companion_revoke_device", { deviceId });
}

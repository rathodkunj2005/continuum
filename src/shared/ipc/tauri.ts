import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export {
    importCancel,
    importForget,
    importFirstAnswerMs,
    importGetConsent,
    importListSources,
    importRecordFirstAnswer,
    importSetConsent,
    importStart,
    importStatus,
} from "./imports";
export type { ImportKind, ImportPermission, ImportRunState, ImportSourceInfo, ImportStatus } from "./imports";

export type CaptureTextSource = "ax" | "ocr" | "browser_semantic" | "mixed" | "unknown";

export interface SearchResult {
    id: string;
    timestamp: number;
    app_name: string;
    text_source?: CaptureTextSource;
    source_type?: string;
    added_by?: string;
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
    embedding_provenance?: SearchEmbeddingProvenance;
    embedding_reason_labels?: string[];
    /** Post-capture review lifecycle:
     *  "" / "pending" / "reviewed_local" / "reviewed_daily" / "review_failed". */
    enrichment_status?: string;
    /** Unix ms timestamp of the last successful review (0 = never). */
    reviewed_at_ms?: number;
    /** Monotonic counter incremented on each successful review pass. */
    reviewer_generation?: number;
    /** Coarse persisted gate outcome: "enriched_memory_card",
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
    text_source?: CaptureTextSource;
    source_type?: string;
    added_by?: string;
    window_title: string;
    url?: string;
    score: number;
    source_count: number;
    continuity?: boolean;
    raw_snippets: string[];
    evidence_ids?: string[];
    confidence?: number;
    low_confidence?: boolean;
    anchor_coverage_score?: number;
    /** High-level activity category: content-derived, never tied to an app name. */
    activity_type?: string;
    /** File paths or code symbols touched in this session */
    files_touched?: string[];
    /** Approximate session duration in minutes (0 if single capture) */
    session_duration_mins?: number;
    /** How many decisions, errors and next steps the memory records. Set by
     *  the Vault list only. */
    decision_count?: number;
    error_count?: number;
    next_step_count?: number;
    /** Short id of the prior card this one continues from, derived from
     *  the durable memory_context "Continues from <short_id>" marker. */
    continuation_of?: string;
    /** http(s):// URL, file:// path, or app/deep-link derived from typed
     *  reopen provenance (legacy marker parsing may still backfill old rows). */
    reopen_target?: string;
    /** PDF page captured at memory time; shown on the Vault row. */
    reopen_page?: number;
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
    /** True when no result for the query reached the strong-match bar (VS-12). */
    weak_match?: boolean;
    matched_routes?: string[];
    matched_chunk_ids?: string[];
    chunk_evidence?: MatchedChunkEvidence[];
    embedding_provenance?: SearchEmbeddingProvenance;
    embedding_reason_labels?: string[];
    /** Phase 3: deterministic "Why this surfaced" attached by the
     *  agentic-graph-rag composer. Absent on legacy code paths. */
    surfacing_reason?: SurfacingReason;
    /** Post-capture review lifecycle:
     *  "" / "pending" / "reviewed_local" / "reviewed_daily" / "review_failed". */
    enrichment_status?: string;
    /** Unix ms timestamp of the last successful review (0 = never). */
    reviewed_at_ms?: number;
    /** Monotonic counter incremented on each successful review pass. */
    reviewer_generation?: number;
    /** Coarse persisted gate outcome: "enriched_memory_card",
     *  "visual_semantics_failed", "metadata_only", etc. */
    storage_outcome?: string;
}

export type EmbeddingRole =
    | "primary"
    | "snippet"
    | "support"
    | "chunk"
    | "image"
    | "graph_node";

export type EmbeddingStatus =
    | "ready"
    | "zero_vector_fallback"
    | "unavailable"
    | "stale_source_text"
    | "dimension_mismatch"
    | "legacy_unverified";

export type EmbeddingVectorSpace =
    | "text_mini_lm384"
    | "text_bge1024"
    | "clip_image512"
    | "graph_node_text384";

export type VisualSemanticSource =
    | "none"
    | "text_capture"
    | "pixel_vlm_or_ocr_grounded"
    | "clip_metadata_fallback"
    | "clip_image_embedding"
    | "visual_semantics_failed"
    | "llm_ocr_grounded"
    | "ocr_only"
    | "unknown";

export interface SearchEmbeddingRoleProvenance {
    role: EmbeddingRole;
    status: EmbeddingStatus;
    vector_space?: EmbeddingVectorSpace;
    model_id?: string;
    dimension?: number;
    source_hash?: string;
    source_char_count: number;
    reason?: string;
}

export interface SearchEmbeddingProvenance {
    document_version: number;
    roles: SearchEmbeddingRoleProvenance[];
    visual_semantic_source: VisualSemanticSource;
    status_labels: string[];
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

export interface SourceStatementRef {
    memory_ids: string[];
    kind: string;
    quote: string;
    source_sha256: string;
    line: number;
}

export interface EvidencePack {
    files: FileRef[];
    commands: CommandRef[];
    decisions: DecisionRef[];
    errors: ErrorRef[];
    todos: TaskRef[];
    urls: UrlRef[];
    source_statements?: SourceStatementRef[];
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
    debug_trace?: unknown;
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

export async function fndrGetMemorySourceStatements(memoryId: string): Promise<SourceStatementRef[]> {
    return invoke("fndr_get_memory_source_statements", { memoryId });
}

export async function fndrTimeline(args?: {
    limit?: number;
    project?: string;
}): Promise<Array<{ memory_id: string; timestamp: number; snippet: string }>> {
    return invoke("fndr_timeline", args ?? {});
}

export interface ResumeThread {
    /** Grouping key for `whatChangedSince` and `markThreadSeen`; never shown. Always sent by the backend. */
    key?: string;
    title: string;
    /** The words shared across windows that made this one thread, as written in a window title. Empty or absent when nothing was inferred. */
    grouped_by?: string[];
    app_name?: string;
    last_state: string;
    age_minutes: number;
    next_steps: string[];
    suggested_next_steps: Array<{ title: string; source_memory_id: string; confidence: number }>;
    evidence: string[];
    pack: {
        items: Array<{ memory_id: string; text: string; ts_ms: number }>;
        dropped_for_budget: number;
        estimated_tokens: number;
    };
}

/** Recent observed work, with source citations; does not execute actions. */
export async function resumeWork(): Promise<ResumeThread[]> {
    return invoke("resume_work", { hours: 24, budgetTokens: 800 });
}

/** What is new in a Resume Work thread since it was last marked seen. */
export interface ThreadDigest {
    thread_key: string;
    title: string;
    since_ms: number;
    /** True when the thread was never marked seen; the digest then covers a day. */
    first_view: boolean;
    new_memories: number;
    page_count: number;
    pages: string[];
    file_count: number;
    files: string[];
    task_count: number;
    tasks: string[];
    commit_count: number;
    newest_memory_id: string | null;
}

export async function whatChangedSince(threadKey: string): Promise<ThreadDigest> {
    return invoke<ThreadDigest>("what_changed_since", { threadKey });
}

export async function markThreadSeen(threadKey: string): Promise<void> {
    return invoke("mark_thread_seen", { threadKey });
}

/** macOS calendar access for meeting prep (ADR 029). */
export type CalendarStatus = "not_determined" | "restricted" | "denied" | "granted";

export interface CalendarPrepState {
    /** "Meeting prep from your calendar" is on. */
    enabled: boolean;
    status: CalendarStatus;
}

export async function calendarMeetingPrepState(): Promise<CalendarPrepState> {
    return invoke<CalendarPrepState>("calendar_meeting_prep_state");
}

/** Turning it on asks macOS for calendar access; it stays on only when granted. */
export async function setCalendarMeetingPrep(enabled: boolean): Promise<CalendarPrepState> {
    return invoke<CalendarPrepState>("set_calendar_meeting_prep", { enabled });
}

export type MeetingBriefState = "off" | "no_access" | "private" | "no_meeting" | "ready";

export interface MeetingBriefMemory {
    id: string;
    title: string;
    appName: string;
    timestamp: number;
    snippet: string;
}

/** The next calendar meeting and what FNDR remembers that bears on it. */
export interface MeetingBrief {
    state: MeetingBriefState;
    title: string | null;
    startsAt: number | null;
    strongMatch: boolean;
    thread: { key: string; title: string } | null;
    memories: MeetingBriefMemory[];
}

export async function nextMeetingBrief(): Promise<MeetingBrief> {
    return invoke<MeetingBrief>("next_meeting_brief");
}

export async function openCalendarPrivacySettings(): Promise<void> {
    return invoke("open_calendar_privacy_settings");
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
    review_backlog: {
        pending: number;
        pending_visual_semantics: number;
        review_failed: number;
    };
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
 * `stored_total + skipped_total` accounts for every evaluated frame:
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
    /** Text embedder unavailable; capture blocked so no zero-vector rows are written. */
    skipped_embedder_unavailable: number;
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
    last_text_backend?: string;
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

export const MCP_APPROVAL_EVENT = "mcp-approval://request";

export interface McpApprovalPrompt {
    request_id: string;
    tool: string;
    arguments: Record<string, unknown>;
    expires_at_ms: number;
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

export interface NeedsSignalCard {
    card: MemoryCard;
    reason_code: string;
    reason: string;
}

/** Low-signal captures kept out of Search/Home/Vault/Ask (Vault review queue). */
export async function listNeedsSignalMemoryCards(limit = 200): Promise<NeedsSignalCard[]> {
    return invoke<NeedsSignalCard[]>("list_needs_signal_memory_cards", { limit });
}

export type ReopenOutcome =
    | { kind: "opened" }
    | { kind: "opened_moved"; new_path: string }
    | { kind: "missing"; path: string }
    | { kind: "drive_not_connected"; volume: string; path: string }
    | { kind: "app_missing"; bundle_id: string; app_name?: string | null }
    | { kind: "app_only"; app_name?: string | null }
    | { kind: "blocked"; target: string }
    | { kind: "no_target" }
    | { kind: "unavailable" };

export async function reopenMemory(memoryId: string): Promise<ReopenOutcome> {
    return invoke<ReopenOutcome>("reopen_memory", {
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

export type DailyOutputKind = "standup" | "weekly_status" | "timesheet";
export type DailyOutputFormat = "markdown" | "plain_text" | "html";
export type DailyOutputTool = "outputs.post_slack" | "outputs.create_linear_issue" | "outputs.open_mail_draft";

export interface DailyOutputCitation {
    kind: "memory" | "activity_event";
    id: string;
    memory_id: string | null;
}

export interface DailyOutputDoc {
    id: string;
    kind: DailyOutputKind;
    title: string;
    start_ms: number;
    end_ms: number;
    generated_at_ms: number;
    model_polished: boolean;
    sections: { heading: string; lines: { text: string; project: string | null; citations: DailyOutputCitation[] }[] }[];
}

export interface DailyOutputApprovalCard {
    approval_id: string;
    tool: DailyOutputTool;
    action: string;
    destination: string;
    title: string;
    body: string;
    bytes: number;
    expires_at_ms: number;
    trimmed: boolean;
}

export interface DailyOutputDestinationStatus {
    slack_webhook: boolean;
    linear_api_key: boolean;
    linear_team_id: boolean;
}

export interface DailyOutputSendReceipt {
    tool: DailyOutputTool;
    destination: string;
    detail: string;
    trust_id: string | null;
}

export interface DailyOutputAllowedDestination {
    key: string;
    tool: DailyOutputTool;
    destination: string;
}

export async function generateDailyOutput(kind: DailyOutputKind, startMs: number, endMs: number, polish: boolean): Promise<DailyOutputDoc> {
    return invoke<DailyOutputDoc>("generate_daily_output", { kind, startMs, endMs, polish });
}

export async function renderDailyOutput(id: string, format: DailyOutputFormat): Promise<string> {
    return invoke<string>("render_daily_output", { id, format });
}

export async function saveDailyOutput(id: string, format: DailyOutputFormat): Promise<string | null> {
    return invoke<string | null>("save_daily_output", { id, format });
}

export async function outputDestinationStatus(): Promise<DailyOutputDestinationStatus> {
    return invoke<DailyOutputDestinationStatus>("output_destination_status");
}

export async function setOutputDestination(name: "slack_webhook" | "linear_api_key" | "linear_team_id", value: string): Promise<void> {
    return invoke<void>("set_output_destination", { name, value });
}

export async function proposeDailyOutputSend(id: string, tool: DailyOutputTool, recipient?: string): Promise<DailyOutputApprovalCard> {
    return invoke<DailyOutputApprovalCard>("propose_daily_output_send", { id, tool, recipient });
}

export async function resolveDailyOutputSend(approvalId: string, approved: boolean): Promise<DailyOutputSendReceipt | null> {
    return invoke<DailyOutputSendReceipt | null>("resolve_daily_output_send", { approvalId, approved });
}

export async function sendDailyOutputAllowed(id: string, tool: DailyOutputTool, recipient?: string): Promise<DailyOutputSendReceipt | null> {
    return invoke<DailyOutputSendReceipt | null>("send_daily_output_allowed", { id, tool, recipient });
}

export async function trustOutputDestination(trustId: string): Promise<string> {
    return invoke<string>("trust_output_destination", { trustId });
}

export async function listOutputAllowedDestinations(): Promise<DailyOutputAllowedDestination[]> {
    return invoke<DailyOutputAllowedDestination[]>("list_output_allowed_destinations");
}

export async function forgetOutputAllowedDestination(key: string): Promise<void> {
    return invoke<void>("forget_output_allowed_destination", { key });
}

export async function getFunGreeting(name?: string | null): Promise<string> {
    return invoke<string>("get_fun_greeting", { name });
}

// Capture control
export const CAPTURE_STATUS_EVENT = "capture://status";
export const PRIVACY_ALERTS_EVENT = "privacy://alerts";

export async function getStatus(): Promise<CaptureStatus> {
    return invoke<CaptureStatus>("get_status");
}

export async function getMcpServerStatus(): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("get_mcp_server_status");
}

export interface McpConnectClient {
    id: "claude_code" | "claude_desktop" | "cursor";
    name: string;
    state: "not_connected" | "connected" | "server_off" | "needs_update" | "conflict" | "unreadable";
    detected: boolean;
    configPath: string;
    detail: string | null;
    missingTool: string | null;
}

export interface McpConnectStatus {
    running: boolean;
    endpoint: string | null;
    actionsBlocked: boolean;
    keepRunning: boolean;
    hideMore: boolean;
    clients: McpConnectClient[];
}

export interface McpConnectOutcome {
    message: string;
    configPath: string;
    backupPath: string | null;
    changed: boolean;
}

export interface McpConnectPreview {
    title: string;
    path: string;
    text: string;
    maskedText: string;
    command: string | null;
}

export interface McpConnectTestStep {
    name: string;
    ok: boolean;
    detail: string;
}

export interface McpConnectTestReport {
    ok: boolean;
    steps: McpConnectTestStep[];
    toolCount: number;
}

export async function mcpConnectStatus(): Promise<McpConnectStatus> {
    return invoke<McpConnectStatus>("mcp_connect_status");
}

export async function mcpConnectPreview(clientId: McpConnectClient["id"]): Promise<McpConnectPreview> {
    return invoke<McpConnectPreview>("mcp_connect_preview", { clientId });
}

export async function mcpConnectApply(clientId: McpConnectClient["id"]): Promise<McpConnectOutcome> {
    return invoke<McpConnectOutcome>("mcp_connect_apply", { clientId });
}

export async function mcpConnectTest(): Promise<McpConnectTestReport> {
    return invoke<McpConnectTestReport>("mcp_connect_test");
}

export async function mcpConnectRotateToken(): Promise<McpConnectStatus> {
    return invoke<McpConnectStatus>("mcp_connect_rotate_token");
}

export async function mcpConnectDisconnect(clientId: McpConnectClient["id"]): Promise<McpConnectOutcome> {
    return invoke<McpConnectOutcome>("mcp_connect_disconnect", { clientId });
}

export async function mcpConnectRevoke(): Promise<string> {
    return invoke<string>("mcp_connect_revoke");
}

export async function mcpConnectSetKeepRunning(keepRunning: boolean): Promise<McpConnectStatus> {
    return invoke<McpConnectStatus>("mcp_connect_set_keep_running", { keepRunning });
}

export async function mcpConnectSetHideMore(hideMore: boolean): Promise<void> {
    return invoke<void>("mcp_connect_set_hide_more", { hideMore });
}

export async function startMcpServer(port?: number): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("start_mcp_server", { port });
}

export async function stopMcpServer(): Promise<McpServerStatus> {
    return invoke<McpServerStatus>("stop_mcp_server");
}

export async function resolveMcpApproval(requestId: string, approved: boolean): Promise<boolean> {
    return invoke<boolean>("resolve_mcp_approval", { requestId, approved });
}

export interface ActionSettings {
    actions_kill_switch: boolean;
    confirm_unnamed_apps: boolean;
}

export interface ActionCard {
    id: string;
    caller: string;
    surface: "notch_plan" | "computer_use" | "mcp" | "agent";
    tool: string;
    title: string;
    target: string | null;
    args: { name: string; value: string }[];
    reason: string;
    undo: { kind: "available"; how: string; inverse: string | null } | { kind: "unavailable"; reason: string };
    status: "pending" | "approved" | "denied" | "expired" | "refused" | "ran" | "failed";
    detail: string | null;
    created_at_ms: number;
    expires_at_ms: number;
}

export async function getActionSettings(): Promise<ActionSettings> {
    return invoke<ActionSettings>("get_action_settings");
}

export async function setActionSettings(settings: ActionSettings): Promise<ActionSettings> {
    return invoke<ActionSettings>("set_action_settings", { settings });
}

export async function listActionApprovals(): Promise<ActionCard[]> {
    return invoke<ActionCard[]>("list_action_approvals");
}

export async function resolveActionApproval(id: string, approve: boolean): Promise<ActionCard> {
    return invoke<ActionCard>("resolve_action_approval", { id, approve });
}

export async function getContextRuntimeStatus(): Promise<ContextRuntimeStatus> {
    return invoke<ContextRuntimeStatus>("get_context_runtime_status");
}

export async function listRecentContextPacks(limit = 8): Promise<ContextPack[]> {
    return invoke<ContextPack[]>("list_recent_context_packs", { limit });
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
export async function getAgentNotesEnabled(): Promise<boolean> {
    return invoke<boolean>("get_agent_notes_enabled");
}

export async function setAgentNotesEnabled(enabled: boolean): Promise<void> {
    return invoke("set_agent_notes_enabled", { enabled });
}

export async function getVerbatimMemoryEnabled(): Promise<boolean> {
    return invoke<boolean>("get_verbatim_memory_enabled");
}

export async function setVerbatimMemoryEnabled(enabled: boolean): Promise<void> {
    return invoke("set_verbatim_memory_enabled", { enabled });
}

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

export interface PrivacyProof {
    evaluated: number;
    stored: number;
    skipped_by_reason: Record<string, number>;
    egress_requests: number;
    egress_hosts: string[];
    /** Cloud model requests this session: feature, host and bytes FNDR sent. No content. */
    model_requests?: ModelRequest[];
    /** Recent Notch Do runs, newest first. */
    operator_runs?: OperatorRunSummary[];
}

/** One past Notch Do run, as counts only. */
export interface OperatorRunSummary {
    runId: string;
    startedAt: string;
    done: number;
    asked: number;
    refused: number;
    failed: number;
}

export interface ModelRequest {
    atMs: number;
    /** A `privacy_proof::Feature` name, such as `notch_do_plan`, `hermes_chat` or `hand_off`. */
    feature: string;
    host: string;
    bytesSent: number;
    /** Kinds of context sent along: `memories`, `screen_text`, `screenshot`. */
    included?: string[];
}

export async function getPrivacyProof(): Promise<PrivacyProof> {
    return invoke<PrivacyProof>("get_privacy_proof");
}

export interface EraseRangeReport {
    matched: number;
    deleted: number;
}

export async function deleteMemoriesInRange(
    startMs: number,
    endMs: number,
    dryRun: boolean,
): Promise<EraseRangeReport> {
    return invoke<EraseRangeReport>("delete_memories_in_range", { startMs, endMs, dryRun });
}

export interface MemoryArchiveReport {
    exported: number;
    jsonl_path: string;
    markdown_path: string;
}

export async function exportMemoriesArchive(): Promise<MemoryArchiveReport> {
    return invoke<MemoryArchiveReport>("export_memories_archive");
}

export async function importMemoriesArchive(): Promise<number> {
    return invoke<number>("import_memories_archive");
}

export interface EncryptionStatement {
    filevault: "on" | "off" | "unknown";
    encrypted: string[];
    not_encrypted: string[];
}

export async function getEncryptionStatement(): Promise<EncryptionStatement> {
    return invoke<EncryptionStatement>("get_encryption_statement");
}

export interface PrivacyExclusion {
    id: string;
    label: string;
    examples: string;
}

export async function getPrivacyExclusions(): Promise<PrivacyExclusion[]> {
    return invoke<PrivacyExclusion[]>("get_privacy_exclusions");
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
    p50_ms: number;
    p95_ms: number;
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

export async function completeTodo(taskId: string): Promise<boolean> {
    return invoke<boolean>("complete_todo", { taskId });
}

export async function setTodoCompleted(taskId: string, isCompleted: boolean): Promise<boolean> {
    return invoke<boolean>("set_todo_completed", { taskId, isCompleted });
}

export async function updateTodo(
    taskId: string,
    title: string,
    taskType?: "Todo" | "Reminder" | "Followup"
): Promise<Task> {
    return invoke<Task>("update_todo", { taskId, title, taskType });
}

// ========== Hermes Agent Functions ==========

export interface ConfiguredPeer {
    id: string;
    card_url: string;
    name: string;
    endpoint: string;
    requires_bearer: boolean;
    verified_at_ms: number;
}

export async function listConfiguredPeers(): Promise<ConfiguredPeer[]> {
    return invoke<ConfiguredPeer[]>("list_configured_peers");
}

/** Verifies the person-entered Agent Card before saving its URL and display details. */
export async function addConfiguredPeer(cardUrl: string): Promise<ConfiguredPeer> {
    return invoke<ConfiguredPeer>("add_configured_peer", { cardUrl });
}

export async function removeConfiguredPeer(id: string): Promise<boolean> {
    return invoke<boolean>("remove_configured_peer", { id });
}

export interface DelegationPreview {
    peer_id: string;
    destination: string;
    message_text: string;
    attachments: Array<{ memory_id: string; summary: string }>;
}

export async function previewPeerDelegation(peerId: string, task: string, outputGoal: string, memoryIds: string[]): Promise<DelegationPreview> {
    return invoke<DelegationPreview>("preview_peer_delegation", { peerId, task, outputGoal, memoryIds });
}

export interface PeerRun {
    local_id: string;
    message_id: string;
    peer_id: string;
    host: string;
    attachment_ids: string[];
    payload_bytes: number;
    created_at_ms: number;
    status: string;
    remote_task_id: string | null;
    remote_state: string | null;
}

export type PeerRunView = Pick<PeerRun,
    "local_id" | "peer_id" | "host" | "created_at_ms" | "payload_bytes" | "status" | "remote_task_id" | "remote_state">;

export interface DelegationSendResult {
    run: PeerRun;
    output_text: string | null;
}

export interface DelegationTaskResult {
    run: PeerRunView;
    output_text: string | null;
}

export async function sendPeerDelegation(
    peerId: string, task: string, outputGoal: string, memoryIds: string[],
    reviewedDestination: string, reviewedText: string,
): Promise<DelegationSendResult> {
    return invoke<DelegationSendResult>("send_peer_delegation", {
        peerId, task, outputGoal, memoryIds, reviewedDestination, reviewedText,
    });
}

export async function listPeerRuns(): Promise<PeerRunView[]> {
    return invoke<PeerRunView[]>("list_peer_runs");
}

export async function refreshPeerTask(localId: string): Promise<DelegationTaskResult> {
    return invoke<DelegationTaskResult>("refresh_peer_task", { localId });
}

export async function cancelPeerTask(localId: string): Promise<DelegationTaskResult> {
    return invoke<DelegationTaskResult>("cancel_peer_task", { localId });
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
    /** The saved provider answers on this Mac. */
    provider_is_local?: boolean;
    /** FNDR adds memories it finds itself to each message for this provider. */
    related_memories?: boolean;
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
    open_task_count: number;
    /** True when Ollama is configured and reachable: chat works without the Hermes CLI. */
    direct_ollama_ready: boolean;
    last_error: string | null;
    install_command: string;
    /** `stopped`, `starting`, `running`, `restarting` or `crashed`. */
    gateway_state: string;
    gateway_restarts: number;
    /** The ChatGPT sign-in could not be used or refreshed. */
    reconnect_chatgpt: boolean;
}

export interface HermesSetupPayload {
    provider_kind: string;
    model_name: string;
    api_key?: string | null;
    base_url?: string | null;
    /** Also send memories FNDR finds on its own to a provider that is not on this Mac. */
    related_memories?: boolean;
}

export interface HermesChatReply {
    response_id: string;
    conversation_id: string;
    content: string;
    /** Memories FNDR added on its own to the message this answers. */
    auto_memories?: AttachedMemory[];
    /** What Hermes did on the way to this answer, in words, once each. */
    tools_used?: string[];
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

/** Sends a message to Hermes. `memoryIds` attach FNDR memories as reference
 *  context; the backend loads them itself, capped at 8 per message. */
export async function sendHermesMessage(
    conversationId: string,
    input: string,
    memoryIds: string[] = []
): Promise<HermesChatReply> {
    return invoke<HermesChatReply>("send_hermes_message", { conversationId, input, memoryIds });
}

/** Stops waiting for the reply to the message this chat has in flight. */
export async function cancelHermesMessage(conversationId: string): Promise<void> {
    return invoke<void>("cancel_hermes_message", { conversationId });
}

export interface AttachedMemory {
    id: string;
    title: string;
    appName: string;
    timestamp: number;
}

export interface AgentChatMessage {
    role: "user" | "assistant";
    content: string;
    at: number;
    memories: AttachedMemory[];
    /** Memories FNDR added on its own to this message. */
    autoMemories?: AttachedMemory[];
    /** The send failed; Hermes never answered this message. */
    failed?: boolean;
    /** On an answer: what Hermes did on the way to it. */
    toolsUsed?: string[];
}

export interface AgentChat {
    id: string;
    title: string;
    createdAt: number;
    updatedAt: number;
    messages: AgentChatMessage[];
}

export interface AgentChatSummary {
    id: string;
    title: string;
    updatedAt: number;
    messageCount: number;
}

export async function listAgentChats(): Promise<AgentChatSummary[]> {
    return invoke<AgentChatSummary[]>("list_agent_chats");
}

export async function getAgentChat(id: string): Promise<AgentChat | null> {
    return invoke<AgentChat | null>("get_agent_chat", { id });
}

export async function deleteAgentChat(id: string): Promise<void> {
    return invoke("delete_agent_chat", { id });
}

/** Emitted by the backend when a ChatGPT sign-in started with `codexLoginStart` finishes. */
export const CODEX_LOGIN_COMPLETED_EVENT = "codex-login-completed";

export type CodexCliState = "missing" | "broken" | "ready";

export interface CodexAccount {
    /** `chatgpt`, `apiKey`, `amazonBedrock`, … */
    kind: string;
    email: string | null;
    planType: string | null;
}

export interface CodexUsageWindow {
    usedPercent: number;
    windowMinutes: number | null;
    /** Unix seconds. */
    resetsAt: number | null;
}

export interface CodexModel {
    id: string;
    displayName: string;
    isDefault: boolean;
}

export interface CodexAccountStatus {
    cliState: CodexCliState;
    cliPath: string | null;
    cliError: string | null;
    account: CodexAccount | null;
    /** Only a ChatGPT sign-in can back Hermes's `openai-codex` provider. */
    usableForHermes: boolean;
    primaryWindow: CodexUsageWindow | null;
    secondaryWindow: CodexUsageWindow | null;
    models: CodexModel[];
}

export interface CodexLoginStarted {
    loginId: string;
    authUrl: string;
}

export interface CodexLoginCompleted {
    loginId: string;
    success: boolean;
    error: string | null;
}

/** Account, plan, usage and models from the official Codex app-server. */
export async function codexAccountStatus(): Promise<CodexAccountStatus> {
    return invoke<CodexAccountStatus>("codex_account_status");
}

/** Starts "Sign in with ChatGPT". Open `authUrl` in the browser; the result
 *  arrives as `CODEX_LOGIN_COMPLETED_EVENT`. */
export async function codexLoginStart(): Promise<CodexLoginStarted> {
    return invoke<CodexLoginStarted>("codex_login_start");
}

export async function codexLoginCancel(loginId: string): Promise<void> {
    return invoke<void>("codex_login_cancel", { loginId });
}

export async function codexLogout(): Promise<CodexAccountStatus> {
    return invoke<CodexAccountStatus>("codex_logout");
}

/** Why the ChatGPT plan's voice can or cannot speak now (ADR 028). */
export type VoiceOutState =
    | "ready"
    | "not_installed"
    | "broken"
    | "signed_out"
    | "unsupported"
    | "near_limit"
    | "over_limit"
    | "private_mode";

export interface VoiceOutStatus {
    state: VoiceOutState;
    detail: string | null;
    /** The higher used share of the plan's two usage windows, 0 to 100. */
    usedPercent: number | null;
    /** Above this, FNDR speaks on the Mac instead. */
    fallbackAbovePercent: number;
    connected: boolean;
}

export interface VoiceOutVoices {
    voices: string[];
}

export async function voiceOutStatus(): Promise<VoiceOutStatus> {
    return invoke<VoiceOutStatus>("voice_out_status");
}

export async function voiceOutVoices(): Promise<VoiceOutVoices> {
    return invoke<VoiceOutVoices>("voice_out_voices");
}

/** Sends the peer connection's SDP offer; resolves to Codex's SDP answer. */
export async function voiceOutStart(sdpOffer: string, voice?: string): Promise<string> {
    return invoke<string>("voice_out_start", { sdpOffer, voice: voice ?? null });
}

/** Hands FNDR-authored reply text to the connected voice. Never memory or screen text. */
export async function voiceOutSpeak(text: string): Promise<void> {
    return invoke<void>("voice_out_speak", { text });
}

export async function voiceOutCancel(): Promise<void> {
    return invoke<void>("voice_out_cancel");
}

export async function voiceOutStop(): Promise<void> {
    return invoke<void>("voice_out_stop");
}

// The on-device neural voice, Kokoro-82M (ADR 030).

export type TtsState = "not_installed" | "downloading" | "ready" | "error";

export interface TtsStatus {
    state: TtsState;
    detail: string | null;
    downloadedBytes: number;
    totalBytes: number;
    /** The model is in memory now. */
    loaded: boolean;
}

export interface TtsVoice {
    id: string;
    name: string;
    accent: "us" | "gb";
    gender: "female" | "male";
    /** The model card's grade for the voice's training data. */
    grade: string;
}

export interface TtsVoices {
    voices: TtsVoice[];
    defaultVoice: string;
}

export interface TtsClip {
    /** A 24 kHz mono 16-bit WAV. */
    wav: ArrayBuffer;
    durationMs: number;
}

export async function ttsStatus(): Promise<TtsStatus> {
    return invoke<TtsStatus>("tts_status");
}

export async function ttsVoices(): Promise<TtsVoices> {
    return invoke<TtsVoices>("tts_voices");
}

/** Starts the one-time download; `consent` is the person's answer to the prompt. */
export async function ttsInstall(consent: boolean): Promise<TtsStatus> {
    return invoke<TtsStatus>("tts_install", { consent });
}

export async function ttsRemove(): Promise<TtsStatus> {
    return invoke<TtsStatus>("tts_remove");
}

export async function ttsCancel(): Promise<void> {
    return invoke<void>("tts_cancel");
}

/** Speech for `text`, synthesized on this Mac. */
export async function ttsSynthesize(text: string, voice?: string, speed?: number): Promise<TtsClip> {
    const raw = await invoke<ArrayBuffer | number[]>("tts_synthesize", { text, voice: voice ?? null, speed: speed ?? null });
    const wav = raw instanceof ArrayBuffer ? raw : new Uint8Array(raw).buffer;
    const samples = Math.max(0, wav.byteLength - 44) / 2;
    return { wav, durationMs: Math.round((samples / 24_000) * 1000) };
}

// Notch Do's voice (ADR 020 amendment, 2026-10-09).

/** Esc pressed in another app while a Notch Do run works or waits on an approval. */
export const NOTCH_DO_ESCAPE_EVENT = "notch-do://escape";

/** The persisted mute, or null while it has never been set in `config.toml`. */
export async function getNotchDoMuted(): Promise<boolean | null> {
    return invoke<boolean | null>("get_notch_do_muted");
}

export async function setNotchDoMuted(muted: boolean): Promise<boolean> {
    return invoke<boolean>("set_notch_do_muted", { muted });
}

/** `smooth` asks only before an action that could destroy something; `ask_each` asks before each action. */
export type NotchDoAutonomy = "smooth" | "ask_each";

export async function getNotchDoAutonomy(): Promise<NotchDoAutonomy> {
    return invoke<NotchDoAutonomy>("get_notch_do_autonomy");
}

export async function setNotchDoAutonomy(autonomy: NotchDoAutonomy): Promise<NotchDoAutonomy> {
    return invoke<NotchDoAutonomy>("set_notch_do_autonomy", { autonomy });
}

/** Watches for Esc outside the notch while a run works; FNDR's own key presses are ignored. */
export async function setNotchEscapeMonitor(active: boolean): Promise<void> {
    return invoke<void>("set_notch_escape_monitor", { active });
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

export interface DailyNarrativeParagraph {
    text: string;
    /** Memories behind the paragraph, the best first. */
    memory_ids: string[];
}

export interface DailyNarrativeItem {
    id: string;
    text: string;
    memory_ids: string[];
    /** The task row behind the item, when one exists. */
    task_id: string | null;
    due_label: string | null;
}

export interface DailyNarrative {
    date: string;
    /** "narrative": model text that passed the checks. "basic": composed in code. */
    mode: "narrative" | "basic" | "empty";
    backend: "apple" | "local" | "none";
    basic_reason: string | null;
    paragraphs: DailyNarrativeParagraph[];
    action_items: DailyNarrativeItem[];
    reminders: DailyNarrativeItem[];
    groups_used: number;
    items_dropped: number;
    model_ms: number;
}

export async function generateDailySummaryNarrative(dateStr: string): Promise<DailyNarrative> {
    return invoke<DailyNarrative>("generate_daily_summary_narrative", { dateStr });
}

export async function getDailySummaryFollowups(): Promise<Task[]> {
    return invoke<Task[]>("get_daily_summary_followups");
}

export interface WrappedRank {
    name: string;
    captures: number;
    duration_minutes: number;
}

export interface WrappedCount {
    name: string;
    count: number;
}

export interface WrappedDay {
    day: string;
    count: number;
}

export interface WeeklyWrapped {
    start_date: string;
    end_date: string;
    generated_at_ms: number;
    total_captures: number;
    active_days: number;
    total_minutes: number;
    apps: WrappedRank[];
    websites: WrappedRank[];
    projects_and_topics: WrappedCount[];
    meeting_count: number;
    document_count: number;
    open_followups: number;
    open_tasks: number;
    busiest_day: WrappedDay | null;
    busiest_hour: number | null;
    most_revisited_file: string | null;
}

export async function getWeeklyWrapped(startDate?: string, endDate?: string): Promise<WeeklyWrapped> {
    return invoke<WeeklyWrapped>("get_weekly_wrapped", {
        startDate: startDate ?? null,
        endDate: endDate ?? null,
    });
}

export async function exportWeeklyWrappedPdf(
    startDate: string,
    endDate: string,
    recapText: string
): Promise<string> {
    return invoke<string>("export_weekly_wrapped_pdf", { startDate, endDate, recapText });
}

/** The sections the PDF prints above the per-capture list. */
export interface DailySummaryPdfNarrative {
    basic: boolean;
    paragraphs: string[];
    action_items: string[];
    reminders: string[];
}

export async function exportDailySummaryPdf(
    dateStr: string,
    summaryText: string,
    narrative?: DailySummaryPdfNarrative,
): Promise<string> {
    return invoke<string>("export_daily_summary_pdf", { dateStr, summaryText, narrative: narrative ?? null });
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
    /** The memory a "stuck" toast opens. */
    memory_id?: string;
    /** The Resume Work thread a "thread_update" or "meeting_prep" toast opens. */
    thread_key?: string;
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

// Screen Guide
export type ScreenGuideModel = "local" | "codex";

export interface OpenClickyBridgeStatus {
    /** OpenClicky is running and answered its local /health. */
    reachable: boolean;
    /** FNDR found OPENCLICKY_BRIDGE_TOKEN to authenticate with. */
    tokenFound: boolean;
    bridgeTokenConfigured: boolean;
}

export async function openClickyBridgeStatus(): Promise<OpenClickyBridgeStatus> {
    return invoke<OpenClickyBridgeStatus>("openclicky_bridge_status");
}

export interface ScreenGuideSettings {
    enabled: boolean;
    shortcut: string;
    speak_responses: boolean;
    show_cursor: boolean;
    /** `codex` answers with the user's ChatGPT plan; never the default. */
    model?: ScreenGuideModel;
    /** With `codex`, also send a downscaled screenshot. */
    send_screenshot_to_codex?: boolean;
    /** Point with OpenClicky's cursor via its local bridge. */
    openclicky_bridge?: boolean;
    /** Let the notch operate the Mac via open-computer-use; each action is approved. */
}

export interface ScreenGuideDiagnosticStatus {
    armed: boolean;
    expiresInMs: number | null;
    bundleCount: number;
    partialCount: number;
    totalBytes: number;
    lastResult: ScreenGuideDiagnosticResult | null;
}

export interface ScreenGuideDiagnosticResult {
    kind: "saved" | "error" | "deleted" | "cancelled";
    code:
        | "bundle_saved"
        | "storage_unavailable"
        | "bundle_too_large"
        | "diagnostics_deleted"
        | "private_mode_disarmed"
        | "privacy_settings_disarmed";
    /** Bounded backend-owned copy; never a path, prompt, OCR text, or raw error. */
    message: string;
    screenshotSaved: boolean;
    ocrSaved: boolean;
}

export interface ScreenGuideHistoryEntry {
    role: "user" | "assistant";
    content: string;
}

export interface ScreenGuidePointCue {
    /** Horizontal position normalized to the current screen (0..1). */
    x: number;
    /** Vertical position normalized to the current screen (0..1). */
    y: number;
    label?: string | null;
}

export interface ScreenGuideAnswer {
    answer: string;
    point_cue: ScreenGuidePointCue | null;
}

export interface ScreenGuideCursorPosition {
    /** Overlay-local logical pixels. */
    x: number;
    /** Overlay-local logical pixels. */
    y: number;
}

export type ScreenGuidePhase =
    | "idle"
    | "listening"
    | "transcribing"
    | "thinking"
    | "answer"
    | "error";

export type ScreenGuideActivityStage =
    | "preparing"
    | "searching_file_names"
    | "hiding_fndr"
    | "verifying_target"
    | "capturing"
    | "reading_text"
    | "checking_on_device_model"
    | "answering_on_device"
    | "using_grounded_fallback"
    | "answering_chat_gpt"
    | "speech_started";

export interface ScreenGuideStateEvent {
    phase: ScreenGuidePhase;
    message?: string | null;
    generation: number;
    /** Closed, privacy-safe progress vocabulary. Never contains captured content. */
    activity_stage?: ScreenGuideActivityStage | null;
    /** Process display name only; never a title, URL, path, prompt, or OCR text. */
    target_app?: string | null;
}

export interface ScreenGuideShortcutEvent {
    action: "press" | "release" | "cancel";
    generation: number;
}

export interface ScreenGuideSubmitEvent {
    text: string;
    generation: number;
}

export const SCREEN_GUIDE_SHORTCUT_EVENT = "screen-guide://shortcut";
export const SCREEN_GUIDE_SUBMIT_EVENT = "screen-guide://submit";
export const SCREEN_GUIDE_STATE_EVENT = "screen-guide://state";

export async function getScreenGuideSettings(): Promise<ScreenGuideSettings> {
    return invoke<ScreenGuideSettings>("get_screen_guide_settings");
}

export async function setScreenGuideSettings(
    settings: ScreenGuideSettings,
): Promise<ScreenGuideSettings> {
    return invoke<ScreenGuideSettings>("set_screen_guide_settings", { settings });
}

export async function getScreenGuideDiagnosticStatus(): Promise<ScreenGuideDiagnosticStatus> {
    return invoke<ScreenGuideDiagnosticStatus>("get_screen_guide_diagnostic_status");
}

export async function armScreenGuideDiagnostic(): Promise<ScreenGuideDiagnosticStatus> {
    return invoke<ScreenGuideDiagnosticStatus>("arm_screen_guide_diagnostic");
}

export async function deleteScreenGuideDiagnostics(): Promise<ScreenGuideDiagnosticStatus> {
    return invoke<ScreenGuideDiagnosticStatus>("delete_screen_guide_diagnostics");
}

/** Reveal the backend-owned diagnostics directory without accepting a renderer path. */
export async function revealScreenGuideDiagnostics(): Promise<ScreenGuideDiagnosticStatus> {
    return invoke<ScreenGuideDiagnosticStatus>("reveal_screen_guide_diagnostics");
}

export async function screenGuidePress(): Promise<number> {
    return invoke<number>("screen_guide_press");
}

export async function screenGuideRelease(generation: number): Promise<void> {
    return invoke<void>("screen_guide_release", { generation });
}

/** Gives up on a stuck transcription or ask-the-screen call past its
 *  client-side timeout. A no-op if the generation already finished. */
export async function cancelScreenGuideTurn(generation: number): Promise<void> {
    return invoke<void>("cancel_screen_guide_turn", { generation });
}

export async function submitScreenGuideText(text: string): Promise<void> {
    return invoke<void>("submit_screen_guide_text", { text });
}

export async function transcribeScreenGuideVoiceInput(
    audioBytes: number[],
    mimeType: string,
    requestId: number,
): Promise<VoiceTranscriptionResult> {
    return invoke<VoiceTranscriptionResult>("transcribe_screen_guide_voice_input", {
        audioBytes,
        mimeType,
        requestId,
    });
}

export async function setScreenGuideOverlayReady(ready: boolean): Promise<void> {
    return invoke<void>("set_screen_guide_overlay_ready", { ready });
}

export async function screenGuideMicrophoneStarted(generation: number): Promise<void> {
    return invoke<void>("screen_guide_microphone_started", { generation });
}

export async function acknowledgeScreenGuideMicrophoneStopped(
    generation: number,
): Promise<boolean> {
    return invoke<boolean>("acknowledge_screen_guide_microphone_stopped", { generation });
}

export async function askScreenGuide(
    question: string,
    requestId: number,
    history: ScreenGuideHistoryEntry[] = [],
): Promise<ScreenGuideAnswer> {
    return invoke<ScreenGuideAnswer>("ask_screen_guide", { question, history, requestId });
}

export async function getScreenGuideCursorPosition(): Promise<ScreenGuideCursorPosition> {
    return invoke<ScreenGuideCursorPosition>("get_screen_guide_cursor_position");
}

export async function finishScreenGuideVisual(requestId: number): Promise<boolean> {
    return invoke<boolean>("finish_screen_guide_visual", { requestId });
}

export function onScreenGuideShortcut(
    handler: (payload: ScreenGuideShortcutEvent) => void,
): Promise<() => void> {
    return listen<ScreenGuideShortcutEvent>(SCREEN_GUIDE_SHORTCUT_EVENT, (event) => {
        handler(event.payload);
    });
}

export function onScreenGuideSubmit(
    handler: (payload: ScreenGuideSubmitEvent) => void,
): Promise<() => void> {
    return listen<ScreenGuideSubmitEvent>(SCREEN_GUIDE_SUBMIT_EVENT, (event) => {
        handler(event.payload);
    });
}

export function onScreenGuideState(
    handler: (payload: ScreenGuideStateEvent) => void,
): Promise<() => void> {
    return listen<ScreenGuideStateEvent>(SCREEN_GUIDE_STATE_EVENT, (event) => {
        handler(event.payload);
    });
}

export async function reportScreenGuideState(state: ScreenGuideStateEvent): Promise<void> {
    return invoke("report_screen_guide_state", { state });
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
    /** Smart Fill: two taps of Command write the focused text box with ChatGPT. */
    smart_fill: boolean;
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

export interface AutofillOverlayEvent {
    requestId: number;
    payload: AutofillOverlayPayload;
}

export async function getAutofillSettings(): Promise<AutofillSettings> {
    return invoke<AutofillSettings>("get_autofill_settings");
}

export async function setAutofillSettings(settings: AutofillSettings): Promise<AutofillSettings> {
    return invoke<AutofillSettings>("set_autofill_settings", { settings });
}

/** Smart Fill: two taps of Command write the focused text box with ChatGPT. */
export async function getSmartFillEnabled(): Promise<boolean> {
    return invoke<boolean>("get_smart_fill_enabled");
}

export async function setSmartFillEnabled(enabled: boolean): Promise<boolean> {
    return invoke<boolean>("set_smart_fill_enabled", { enabled });
}

/** How many personal details Smart Fill has saved from labelled lines on screen. */
export async function getSavedDetailsCount(): Promise<number> {
    return invoke<number>("saved_details_count");
}

/** Forgets every saved personal detail; resolves to the new count (0). */
export async function forgetSavedDetails(): Promise<number> {
    return invoke<number>("forget_saved_details");
}

/** How FNDR speaks: `config.toml` [voice_output]. */
export interface VoiceOutputSettings {
    /** "auto" or a speech provider id (`src/shared/voice/speechProvider.ts`). */
    provider: string;
    /** A macOS voice identifier; empty means the best one installed. */
    system_voice: string;
    rate: number;
    /** A Kokoro voice id for the on-device neural voice; empty means its default. */
    neural_voice?: string;
}

export async function getVoiceOutputSettings(): Promise<VoiceOutputSettings> {
    return invoke<VoiceOutputSettings>("get_voice_output_settings");
}

export async function setVoiceOutputSettings(settings: VoiceOutputSettings): Promise<VoiceOutputSettings> {
    return invoke<VoiceOutputSettings>("set_voice_output_settings", { settings });
}

export async function resolveAutofill(
    context: FieldContext,
    queryOverride?: string | null,
): Promise<AutofillResolution> {
    return invoke<AutofillResolution>("resolve_autofill", { context, queryOverride });
}

export async function injectText(text: string, requestId: number): Promise<void> {
    return invoke("inject_text", { text, requestId });
}

export async function dismissAutofill(requestId?: number | null): Promise<boolean> {
    return invoke<boolean>("dismiss_autofill", { requestId: requestId ?? null });
}

export const OMNIBAR_FOCUS_EVENT = "omnibar://focus";
export const OMNIBAR_OPEN_MEMORY_EVENT = "omnibar://open-memory";

export async function dismissOmnibar(): Promise<void> {
    return invoke("dismiss_omnibar");
}

export async function omnibarOpenMemory(memoryId: string): Promise<void> {
    return invoke("omnibar_open_memory", { memoryId });
}

export interface ClipboardEntry {
    id: string;
    text: string;
    app_name?: string | null;
    window_title?: string | null;
    timestamp: number;
}

export async function getClipboardHistory(
    query?: string,
    limit?: number,
): Promise<ClipboardEntry[]> {
    return invoke("get_clipboard_history", { query, limit });
}

export async function copyClipboardEntry(text: string): Promise<void> {
    return invoke("copy_clipboard_entry", { text });
}

export async function pasteClipboardEntry(text: string): Promise<void> {
    return invoke("paste_clipboard_entry", { text });
}

export async function setAutofillOverlayReady(
    ready: boolean,
): Promise<AutofillOverlayEvent | null> {
    return invoke("set_autofill_overlay_ready", { ready });
}

export async function takePendingAutofillPayload(): Promise<AutofillOverlayEvent | null> {
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

// Notch HUD: the panel parked on the display's camera housing.

export const NOTCH_HUD_HOVER_EVENT = "notch-hud://hover";
export const NOTCH_HUD_GEOMETRY_EVENT = "notch-hud://geometry";
/** Alt+N: `true` opens the panel, `false` closes it. */
export const NOTCH_HUD_SUMMON_EVENT = "notch-hud://summon";
/** A request typed in the command bar, for Notch Do to plan. */
export const NOTCH_HUD_DO_EVENT = "notch-hud://do";

/** Logical points, as measured from the display the HUD is parked on. */
export interface NotchHudGeometry {
    closed_width: number;
    closed_height: number;
    is_physical_notch: boolean;
    window_width: number;
    window_height: number;
    screen_width: number;
    screen_height: number;
}

export async function getNotchHudGeometry(): Promise<NotchHudGeometry> {
    return invoke<NotchHudGeometry>("get_notch_hud_geometry");
}

/**
 * The drawn panel's frame in CSS pixels relative to the window's top-left. The
 * window is fixed at its largest footprint and stays click-through outside this
 * rectangle, so it must be re-reported whenever the silhouette resizes.
 */
export async function setNotchHudHitRect(rect: {
    x: number;
    y: number;
    width: number;
    height: number;
}): Promise<void> {
    return invoke("set_notch_hud_hit_rect", { rect });
}

/** Take or release the keyboard: the HUD is not focusable at rest. */
export async function setNotchHudKeyboard(active: boolean): Promise<void> {
    return invoke("set_notch_hud_keyboard", { active });
}

export async function dismissNotchHud(): Promise<void> {
    return invoke("dismiss_notch_hud");
}

export async function toggleNotchHud(): Promise<void> {
    return invoke("toggle_notch_hud");
}

export async function notchHudOpenMemory(memoryId: string): Promise<void> {
    return invoke("notch_hud_open_memory", { memoryId });
}

// Notch Do: a spoken request is planned, shown, then run step by step.

export const COMPUTER_USE_EVENT = "computer-use://event";

export type ComputerUseStepAction = "open_app" | "open_url" | "operate" | "reopen_memory" | "paste";

// Work sets (ADR 027): the places of one thread of work, resolved on the Mac.

export type WorkItemKind = "url" | "file" | "pdf_page" | "app" | "folder";

export interface WorkItem {
    memoryId: string;
    /** What the card shows: a page title, a file name, an app. */
    label: string;
    kind: WorkItemKind;
    reopenRank: number;
    appName: string;
    /** The site of a link, without `www.`. */
    host?: string;
    page?: number;
    capturedAt: number;
}

export interface WorkSet {
    id: string;
    title: string;
    /** Why this set was chosen, composed from the evidence. */
    reason: string;
    score: number;
    items: WorkItem[];
}

export type WorkSetResolution =
    | { kind: "best"; value: WorkSet }
    | { kind: "ambiguous"; value: WorkSet[] }
    | { kind: "none"; value: { why: string } };

export interface WorkItemOutcome {
    memoryId: string;
    label: string;
    kind?: WorkItemKind;
    /** FNDR opened it, judged from the typed outcome. */
    ok: boolean;
    detail: string;
    outcome?: ReopenOutcome;
    /** Set on an opened item when a layout was asked for. */
    arrangement?: WorkSetArrangement;
}

export type ItemOutcome = WorkItemOutcome;

export type WorkSetLayout = "left_right_split" | "top_bottom_split" | "thirds" | "grid2x2" | "maximize";

/** Whether the opened windows were arranged; `detail` says why not in plain words. */
export interface WorkSetArrangement {
    arranged: boolean;
    layout: WorkSetLayout;
    detail: string;
}

// ---- Hand-off (ADR 032) ----

export type HandOffGoal = "doc" | "spreadsheet" | "slides" | "brief";

/** Where a draft comes from: the person's words, or a to-do suggestion's id. */
export type HandOffSource = { kind: "request"; value: string } | { kind: "task"; value: string };

export interface HandOffDraftItem {
    kind: "memory" | "file";
    memoryId: string;
    label: string;
    appName: string;
    capturedAt: number;
    ticked: boolean;
    bytes?: number;
}

/** What a hand-off would send, resolved on this Mac. `rendered` is exactly
 *  the first message; the start compares it again before anything leaves. */
export interface HandOffDraft {
    taskText: string;
    taskMemoryId: string | null;
    goal: HandOffGoal;
    items: HandOffDraftItem[];
    rendered: string;
    estimateBytes: number;
}

export type HandOffJobStatus =
    | "starting"
    | "working"
    | "stopping"
    | "checking"
    | "ready"
    | "ready_with_gaps"
    | "stopped"
    | "failed";

export interface HandOffRecord {
    id: string;
    createdAtMs: number;
    request: string;
    goal: HandOffGoal;
    memoryIds: string[];
    fileMemoryIds: string[];
    folder: string;
    status: HandOffJobStatus;
    reason: string | null;
    finishedAtMs: number | null;
    bytesSent: number;
    checked: number;
    unsupported: number;
    notChecked: number;
    /** Verified deliverables, relative to `folder`. */
    deliverables: string[];
    rating: "worked" | "did_not_work" | null;
}

export interface HandOffStatus {
    toolchain: "missing" | "outdated" | "installed";
    running: HandOffRecord | null;
}

/** What the worker did, as FNDR read it from Codex's stream. */
export type HandOffEvent =
    | { kind: "readFile"; name: string }
    | { kind: "ranCode"; ok: boolean; ms: number }
    | { kind: "wroteFile"; path: string }
    | { kind: "viewedImage"; path: string }
    | { kind: "webSearched"; query: string; urls: string[] }
    | { kind: "answered"; text: string }
    | { kind: "idle" }
    | { kind: "tokens"; total: number }
    | { kind: "rateLimits"; primary: number; secondary: number };

export interface HandOffFinished {
    record: HandOffRecord | null;
    error: string | null;
}

export const HAND_OFF_PROGRESS_EVENT = "hand_off://progress";
export const HAND_OFF_FINISHED_EVENT = "hand_off://finished";

export async function handOffStatus(): Promise<HandOffStatus> {
    return invoke<HandOffStatus>("hand_off_status");
}

/** Installs FNDR's Python and document libraries for workers (about 200 MB). */
export async function handOffInstallToolchain(): Promise<HandOffStatus> {
    return invoke<HandOffStatus>("hand_off_install_toolchain");
}

/** What a hand-off would send. Nothing leaves the Mac. */
export async function handOffDraft(source: HandOffSource): Promise<HandOffDraft> {
    return invoke<HandOffDraft>("hand_off_draft", { source });
}

interface HandOffChoice {
    taskText: string;
    taskMemoryId: string | null;
    goal: HandOffGoal;
    memoryIds: string[];
    fileMemoryIds: string[];
}

/** The draft again with the person's ticks and output. */
export async function handOffUpdateDraft(choice: HandOffChoice): Promise<HandOffDraft> {
    return invoke<HandOffDraft>("hand_off_update_draft", { ...choice });
}

/** Starts the reviewed job; progress and the end arrive as events. */
export async function handOffStart(choice: HandOffChoice & { reviewed: string }): Promise<void> {
    return invoke<void>("hand_off_start", { ...choice });
}

export async function handOffStop(): Promise<void> {
    return invoke<void>("hand_off_stop");
}

/** Opens one deliverable FNDR checked, in its default app. */
export async function handOffOpen(jobId: string, path: string): Promise<void> {
    return invoke<void>("hand_off_open", { jobId, path });
}

export async function handOffRate(jobId: string, worked: boolean): Promise<HandOffRecord> {
    return invoke<HandOffRecord>("hand_off_rate", { jobId, worked });
}

/** Which thread of work a request means. Reads only; nothing leaves the Mac. */
export async function resolveWorkSet(query: string): Promise<WorkSetResolution> {
    return invoke<WorkSetResolution>("resolve_work_set", { query });
}

/** Opens the places of a set the person picked, at most six, then arranges
 *  their windows when `layout` is given (best effort, never an error). */
export async function openWorkSet(memoryIds: string[], layout?: WorkSetLayout): Promise<ItemOutcome[]> {
    return invoke<ItemOutcome[]>("open_work_set", { memoryIds, layout: layout ?? null });
}

/** A work set saved under a name. `items` are re-resolved on every read: a
 *  memory deleted or now private or blocklisted is left out. */
export interface NamedWorkSet {
    id: string;
    name: string;
    memoryIds: string[];
    items: WorkItem[];
    savedAt: number;
    lastOpenedAt?: number;
}

export async function listNamedSets(): Promise<NamedWorkSet[]> {
    return invoke<NamedWorkSet[]>("list_named_sets");
}

/** Names must be unique ignoring case; at most 30 sets. */
export async function saveNamedSet(name: string, memoryIds: string[]): Promise<NamedWorkSet> {
    return invoke<NamedWorkSet>("save_named_set", { name, memoryIds });
}

export async function deleteNamedSet(id: string): Promise<void> {
    return invoke<void>("delete_named_set", { id });
}

/** A set the person opens around this time on days like today. Only data for
 *  a card: FNDR never opens it by itself. */
export interface RoutineOffer {
    id: string;
    label: string;
    setId?: string;
    memoryIds: string[];
    reason: string;
    dueNow: boolean;
}

export async function routineOffers(): Promise<RoutineOffer[]> {
    return invoke<RoutineOffer[]>("routine_offers");
}

/** Hides an offer for the rest of today. */
export async function dismissRoutineOffer(id: string): Promise<void> {
    return invoke<void>("dismiss_routine_offer", { id });
}
export type ComputerUseRisk = "runs" | "confirm" | "never";

export type ComputerUseEvent =
    | { kind: "planning"; runId: string; usedMemories: number }
    | {
          kind: "planned";
          runId: string;
          steps: { label: string; action: ComputerUseStepAction; app: string; item?: WorkItem }[];
          /** No step can need a yes, so the plan may start by itself. */
          autoStart?: boolean;
      }
    | { kind: "stepStarted"; runId: string; index: number; attempt: number }
    | { kind: "action"; runId: string; index: number; itemId: string; tool: string; summary: string; risk?: ComputerUseRisk }
    | { kind: "actionDone"; runId: string; itemId: string; ok: boolean }
    | { kind: "approval"; runId: string; requestKey: string; tool: string; summary: string }
    | { kind: "approvalResolved"; runId: string; requestKey: string }
    | { kind: "blocked"; runId: string; index: number; tool: string; summary: string; reason: string }
    | { kind: "stepDone"; runId: string; index: number; ok: boolean; detail: string; checked?: boolean }
    | { kind: "finished"; runId: string; ok: boolean; summary: string }
    | { kind: "stopped"; runId: string }
    /** A work-set request matched more than one thread about equally; the person picks. */
    | { kind: "choose"; runId: string; options: WorkSet[] }
    | { kind: "failed"; runId: string; error: string; reconnect: boolean };

export interface ComputerUseStatus {
    /** The "Operate my Mac" opt-in. */
    enabled: boolean;
    codexReady: boolean;
    /** `codex_computer_use` or `open_computer_use`; null when neither is installed. */
    backend: string | null;
    backendPath: string | null;
    activeRun: string | null;
}

export async function computerUseStatus(): Promise<ComputerUseStatus> {
    return invoke<ComputerUseStatus>("computer_use_status");
}

/** Turns "Operate my Mac" on or off; returns the saved value. */
export async function setComputerUseEnabled(enabled: boolean): Promise<boolean> {
    return invoke<boolean>("set_computer_use_enabled", { enabled });
}

/** Plans a request and returns the run id. Stops any run in progress first. */
export async function computerUsePlan(text: string): Promise<string> {
    return invoke<string>("computer_use_plan", { text });
}

/** Starts a planned run (the plan card's countdown, "go", or a tap). */
export async function computerUseStart(runId: string): Promise<void> {
    return invoke("computer_use_start", { runId });
}

/** Hands a request to Notch Do, which opens and plans it: from the command bar, or a skill run from Settings. Refused while "Operate my Mac" is off. */
export async function notchDoRequest(text: string): Promise<void> {
    return invoke("notch_do_request", { text });
}

/** A Notch Do run as Settings lists it. */
export interface NotchDoRun {
    runId: string;
    /** RFC 3339. */
    at: string;
    request: string;
    /** `local` (on this Mac), `planner` (the cloud planner) or `skill`. */
    plannedBy: string;
    /** Step labels, in order. */
    steps: string[];
    /** null while the run has not ended. */
    ok: boolean | null;
    summary: string;
    rating: "worked" | "did_not_work" | null;
    /** Whether a skill could repeat it: it worked and reopened no memories. */
    repeatable: boolean;
}

export interface NotchDoSkill {
    id: string;
    name: string;
    /** The request it was saved from. */
    request: string;
    steps: string[];
    savedAt: string;
}

/** The newest Notch Do runs, newest first. */
export async function notchDoRuns(): Promise<NotchDoRun[]> {
    return invoke<NotchDoRun[]>("notch_do_runs");
}

/** Rates a Notch Do run that ended; a run that did not work becomes a draft case for the evaluation suite. */
export async function notchDoRateRun(runId: string, worked: boolean): Promise<void> {
    return invoke("notch_do_rate_run", { runId, worked });
}

/** Saves a Notch Do run that worked as a skill, run again by saying its name. Returns the saved name. */
export async function notchDoSaveSkill(runId: string, name: string): Promise<string> {
    return invoke<string>("notch_do_save_skill", { runId, name });
}

export async function notchDoSkills(): Promise<NotchDoSkill[]> {
    return invoke<NotchDoSkill[]>("notch_do_skills");
}

/** Deletes a skill; false when it was already gone. */
export async function notchDoDeleteSkill(id: string): Promise<boolean> {
    return invoke<boolean>("notch_do_delete_skill", { id });
}

export async function computerUseRespond(requestKey: string, approve: boolean): Promise<void> {
    return invoke("computer_use_respond", { requestKey, approve });
}

export interface OperatorPermissions {
    accessibility: boolean;
    screenRecording: boolean;
    /** Spotify or Music answered; null when neither is running. */
    automationMedia: boolean | null;
    backend: string | null;
    /** The computer-use server answered; null until probed. */
    backendReady: boolean | null;
    backendDetail: string | null;
}

/** Live permission checks for Notch Do. `probe` starts Computer Use once. */
export async function computerUsePermissions(probe: boolean): Promise<OperatorPermissions> {
    return invoke<OperatorPermissions>("computer_use_permissions", { probe });
}

/** Kills the run at once, including an action in flight. */
export async function computerUseStop(): Promise<void> {
    return invoke("computer_use_stop");
}

// Setup center: what FNDR depends on, and installing or updating it from the app.

export type SetupComponentState = "ready" | "missing" | "error";
export type SetupComponentAction = "install" | "sign_in" | "open_url";

export interface SetupComponent {
    id: string;
    name: string;
    purpose: string;
    required: boolean;
    state: SetupComponentState;
    version: string | null;
    detail: string | null;
    action: SetupComponentAction | null;
    url: string | null;
}

export async function setupComponents(): Promise<SetupComponent[]> {
    return invoke<SetupComponent[]>("setup_components");
}

export async function installComponent(id: string): Promise<void> {
    return invoke("install_component", { id });
}

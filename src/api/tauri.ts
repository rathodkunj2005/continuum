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
    /** http(s):// URL, file:// path, or app:// deep-link recovered from
     *  the durable memory_context "Reopen: …" marker. */
    reopen_target?: string;
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

export async function deleteMemory(memoryId: string): Promise<boolean> {
    return invoke<boolean>("delete_memory", { memoryId });
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

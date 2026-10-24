import { emit } from "@tauri-apps/api/event";
import { CAPTURE_STATUS_EVENT, CODEX_LOGIN_COMPLETED_EVENT, SCREEN_GUIDE_STATE_EVENT } from "@/shared/ipc/tauri";
import type {
    CaptureStatus,
    CodexAccountStatus,
    ConfiguredPeer,
    HermesBridgeStatus,
    ComposedAnswer,
    DailyNarrative,
    MemoryCard,
    MemoryReviewWorkerStatus,
    NeedsSignalCard,
    PrivacyAlert,
    PrivacyProof,
    RuntimeMetricsSnapshot,
    ScreenGuideDiagnosticStatus,
    ScreenGuideSettings,
    VoiceOutputSettings,
    Stats,
    ResumeThread,
    Task,
    WeeklyWrapped,
    NotchDoRun,
    NotchDoSkill,
} from "@/shared/ipc/tauri";
import type { ModelInfo, OnboardingState } from "@/shared/ipc/onboarding";

type PreviewIpcHandler = (command: string, payload?: unknown) => Promise<unknown>;

const previewNow = Date.parse("2026-09-22T22:00:00.000Z");

const previewMemoryCards: MemoryCard[] = [
    {
        id: "memory-design-review",
        title: "Reviewed the FNDR home experience",
        summary: "Compared the home hierarchy, search prominence, and wallpaper contrast before the UI overhaul.",
        display_summary: "The search action should stay primary while the wallpaper supports it quietly.",
        action: "Reviewed a product design",
        context: ["Home", "visual hierarchy", "accessibility"],
        timestamp: previewNow - 24 * 60 * 1000,
        app_name: "Figma",
        window_title: "FNDR: Home concepts",
        project: "FNDR UI overhaul",
        score: 0.96,
        source_count: 7,
        continuity: true,
        raw_snippets: [],
        confidence: 0.94,
        activity_type: "docs",
        session_duration_mins: 38,
        insight_what_happened: "Reviewed the landing screen and clarified the search-first hierarchy.",
        insight_why_mattered: "A stable foreground keeps every palette readable and makes the next action obvious.",
        insight_what_changed: "Removed a false scroll cue and moved copy onto semantic contrast tokens.",
        topic_categories: ["product design", "accessibility"],
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-preview-harness",
        title: "Built a safe browser preview for FNDR",
        summary: "Mounted the real React shell behind strict synthetic Tauri IPC fixtures for repeatable UI review.",
        display_summary: "The browser can now exercise the real interface without requesting capture or biometric permissions.",
        action: "Implemented a development workflow",
        context: ["React", "Tauri mocks", "browser QA"],
        timestamp: previewNow - 76 * 60 * 1000,
        app_name: "Visual Studio Code",
        window_title: "previewIpc.ts - FNDR",
        project: "FNDR UI overhaul",
        score: 0.93,
        source_count: 5,
        raw_snippets: [],
        confidence: 0.92,
        activity_type: "coding",
        files_touched: ["src/dev/previewIpc.ts", "src/dev/uiPreview.tsx"],
        session_duration_mins: 51,
        topic_categories: ["frontend", "quality"],
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-accessibility-notes",
        title: "Collected accessibility acceptance criteria",
        summary: "Outlined keyboard, focus, contrast, reduced-motion, and 200% text checks for every major surface.",
        display_summary: "Every feature needs usable empty, loading, error, permission-denied, and long-content states.",
        action: "Researched inclusive interaction patterns",
        context: ["WCAG", "keyboard", "reduced motion"],
        timestamp: previewNow - 3 * 60 * 60 * 1000,
        app_name: "Google Chrome",
        window_title: "Inclusive product interface checklist",
        url: "https://www.w3.org/WAI/WCAG22/quickref/",
        reopen_target: "https://www.w3.org/WAI/WCAG22/quickref/",
        score: 0.9,
        source_count: 4,
        raw_snippets: [],
        confidence: 0.9,
        activity_type: "browsing",
        session_duration_mins: 27,
        topic_categories: ["accessibility"],
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-roadmap",
        title: "Sequenced the UI overhaul into vertical slices",
        summary: "Prioritized trust and accessibility, then shell, search, vault, utility panels, and native verification.",
        display_summary: "Each slice should ship with browser evidence and a narrow regression test.",
        action: "Planned the product roadmap",
        context: ["priorities", "acceptance criteria", "verification"],
        timestamp: previewNow - 22 * 60 * 60 * 1000,
        app_name: "Terminal",
        window_title: "FNDR UI audit notes",
        project: "FNDR UI overhaul",
        score: 0.88,
        source_count: 3,
        raw_snippets: [],
        confidence: 0.91,
        activity_type: "docs",
        session_duration_mins: 19,
        topic_categories: ["roadmap", "product"],
        enrichment_status: "reviewed_daily",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-preview-pdf-page",
        title: "Read the synthetic reopen PDF on page 112",
        summary: "Checked that Vault remembers the Preview page so reopen can state it.",
        display_summary: "The Vault row should show page 112 next to Preview.",
        action: "Read a PDF",
        context: ["Preview", "PDF"],
        timestamp: previewNow - 40 * 60 * 1000,
        app_name: "Preview",
        window_title: "re03-preview.pdf - Page 112 of 150",
        reopen_target: "file:///Users/qa/re03-fixtures/re03-preview.pdf",
        reopen_page: 112,
        score: 0.86,
        source_count: 1,
        raw_snippets: [],
        confidence: 0.88,
        activity_type: "docs",
        files_touched: ["/Users/qa/re03-fixtures/re03-preview.pdf"],
        session_duration_mins: 6,
        topic_categories: ["reopen"],
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    // Yesterday: one project thread with near-duplicate captures, plus a download.
    ...[26, 26.25, 26.5].map((hoursAgo, index): MemoryCard => ({
        id: `memory-beta-script-${index + 1}`,
        title: "Drafted the Beta demo script",
        summary: "Wrote the four-minute Beta walkthrough: capture, search, Ask, and reopen.",
        display_summary: "The demo opens on a real question and ends on the reopened source.",
        action: "Wrote a script",
        context: ["Beta demo", "script"],
        timestamp: previewNow - hoursAgo * 60 * 60 * 1000,
        app_name: "Notes",
        window_title: "Beta demo script",
        project: "FNDR Beta demo",
        score: 0.84,
        source_count: 1,
        raw_snippets: [],
        activity_type: "docs",
        // What a session row adds up: files, and what its moments record.
        files_touched: ["beta-demo-script.md"],
        decision_count: index === 0 ? 1 : 0,
        next_step_count: index === 1 ? 1 : 0,
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    })),
    // Another app, no project, same file within minutes: the Vault links the two sessions.
    {
        id: "memory-beta-script-terminal",
        title: "Counted the words in the demo script",
        summary: "Ran a word count on the demo script to check it fits four minutes.",
        display_summary: "Ran a word count on the demo script: 520 words, about four minutes.",
        action: "Ran a command",
        context: ["Terminal"],
        timestamp: previewNow - 26.3 * 60 * 60 * 1000,
        app_name: "Terminal",
        window_title: "wc beta-demo-script.md",
        score: 0.8,
        source_count: 1,
        raw_snippets: [],
        files_touched: ["beta-demo-script.md"],
        activity_type: "coding",
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-beta-rubric",
        title: "Checked the Beta judging rubric",
        summary: "Read how judges score the Beta: a working demo, measured results, and a clear story.",
        display_summary: "Measured results carry the most weight in the Beta rubric.",
        action: "Read a rubric",
        context: ["Beta demo", "rubric"],
        timestamp: previewNow - 27 * 60 * 60 * 1000,
        app_name: "Google Chrome",
        window_title: "Beta rubric",
        url: "https://example.com/beta-rubric",
        reopen_target: "https://example.com/beta-rubric",
        project: "FNDR Beta demo",
        score: 0.82,
        source_count: 1,
        raw_snippets: [],
        activity_type: "browsing",
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
    {
        id: "memory-beta-download",
        title: "Downloaded beta-checklist.pdf",
        summary: "A synthetic checklist PDF arrived in Downloads.",
        display_summary: "A synthetic checklist PDF arrived in Downloads.",
        action: "Downloaded a file",
        context: ["Downloads"],
        timestamp: previewNow - 29 * 60 * 60 * 1000,
        app_name: "Finder",
        window_title: "Downloads",
        score: 0.7,
        source_count: 1,
        raw_snippets: [],
        enrichment_status: "reviewed_local",
        storage_outcome: "enriched_memory_card",
    },
];

const previewNeedsSignalCards: NeedsSignalCard[] = [
    {
        card: {
            id: "memory-needs-review",
            title: "Unclear browser transition",
            summary: "A brief transition was captured without enough readable task context.",
            action: "Awaiting review",
            context: [],
            timestamp: previewNow - 11 * 60 * 1000,
            app_name: "Google Chrome",
            window_title: "New Tab",
            score: 0.31,
            source_count: 1,
            raw_snippets: [],
            confidence: 0.28,
            activity_type: "browsing",
            enrichment_status: "pending",
            storage_outcome: "metadata_only",
        },
        reason_code: "low_readable_signal",
        reason: "FNDR found too little readable task context to make this memory searchable.",
    },
];

const previewSimilarResults = [
    {
        id: "similar-home-review",
        timestamp: previewNow - 2 * 60 * 60 * 1000,
        app_name: "Figma",
        window_title: "FNDR: Contrast concepts",
        session_id: "preview-design-session",
        text: "Compared a stable foreground card against the ambient Film wallpaper.",
        snippet: "Compared foreground contrast options for the FNDR home experience.",
        display_summary: "A related visual review of the Home foreground and background layers.",
        score: 0.89,
    },
];

const captureStatus: CaptureStatus = {
    is_capturing: true,
    is_paused: false,
    is_incognito: false,
    frames_captured: 1842,
    frames_dropped: 317,
    last_capture_time: previewNow,
    ai_model_available: true,
    ai_model_loaded: true,
    loaded_model_id: "qwen3-vl-2b",
    embedding_backend: "ONNX Runtime",
    embedding_degraded: false,
    embedding_detail: "Local MiniLM embedder ready",
    embedding_model_name: "all-MiniLM-L6-v2",
    embedding_dimension: 384,
    pipeline: {
        evaluated: 2159,
        stored_ocr_path: 1694,
        stored_visual_path: 102,
        stored_url_only: 46,
        stored_total: 1842,
        skipped_blocklist: 42,
        skipped_self_app: 61,
        skipped_surface_policy: 18,
        skipped_perceptual_dup: 93,
        skipped_semantic_dup: 54,
        skipped_ocr_failed: 12,
        skipped_low_signal_text: 21,
        skipped_noise: 8,
        skipped_grounding: 3,
        skipped_stacked_extraction: 0,
        skipped_visual_small: 2,
        skipped_visual_novelty: 1,
        skipped_visual_compose_failed: 2,
        skipped_screen_capture_failed: 2,
        skipped_embedder_unavailable: 0,
        skipped_total: 317,
        last_skip_reason: "semantic duplicate",
        last_skip_app: "Visual Studio Code",
        last_skip_timestamp_ms: previewNow - 18_000,
    },
};

const previewModels: ModelInfo[] = [
    {
        id: "qwen3-vl-2b",
        name: "Qwen3-VL 2B",
        description: "Local visual-language understanding for richer memory context.",
        size_bytes: 2_100_000_000,
        size_label: "2.1 GB",
        quality_label: "Balanced",
        speed_label: "Fast on Apple silicon",
        ram_gb: 4,
        recommended: true,
        required: false,
        filename: "qwen3-vl-2b.gguf",
        download_url: "already_downloaded",
    },
    {
        id: "all-minilm-l6-v2",
        name: "MiniLM embeddings",
        description: "Local semantic search embeddings.",
        size_bytes: 90_000_000,
        size_label: "90 MB",
        quality_label: "Search",
        speed_label: "Fast",
        ram_gb: 0.25,
        recommended: true,
        required: true,
        filename: "all-MiniLM-L6-v2.onnx",
        download_url: "already_downloaded",
    },
];

const previewTasks: Task[] = [
    {
        id: "task-q4-readout",
        title: "Own the Q4 onboarding survey readout",
        description: "can you own a one-page Q4 onboarding survey readout with the top three churn drivers by Thursday",
        source_app: "Memory:Mail",
        source_memory_id: "memory-q4-mail",
        created_at: previewNow - 75 * 60 * 1000,
        due_date: null,
        is_completed: false,
        is_dismissed: false,
        task_type: "Followup",
        linked_urls: [],
        linked_memory_ids: ["memory-q4-mail"],
    },
    {
        id: "task-share-audit",
        title: "Share the UI audit with the capstone team",
        description: "Confirm the browser walkthrough findings and assign the native-only checks.",
        source_app: "memory: FNDR UI audit",
        source_memory_id: "memory-roadmap",
        created_at: previewNow - 52 * 60 * 1000,
        due_date: previewNow + 24 * 60 * 60 * 1000,
        is_completed: false,
        is_dismissed: false,
        task_type: "Followup",
        linked_urls: [],
        linked_memory_ids: ["memory-roadmap"],
    },
    {
        id: "task-keyboard-pass",
        title: "Run the keyboard-only panel pass",
        description: "Verify focus entry, focus return, Escape, and visible focus for every mounted panel.",
        source_app: "manual",
        source_memory_id: "memory-accessibility-notes",
        created_at: previewNow - 29 * 60 * 1000,
        // Relative to the real clock so Home's "Due soon" card renders on any day.
        due_date: Date.now() + 20 * 60 * 60 * 1000,
        is_completed: false,
        is_dismissed: false,
        task_type: "Todo",
        linked_urls: [],
        linked_memory_ids: ["memory-accessibility-notes"],
    },
    {
        id: "task-native-permissions",
        title: "Verify capture permissions in the native app",
        description: "Browser preview cannot prove macOS Screen Recording, Accessibility, or microphone behavior.",
        source_app: "manual",
        source_memory_id: null,
        created_at: previewNow - 12 * 60 * 1000,
        due_date: previewNow + 2 * 24 * 60 * 60 * 1000,
        is_completed: false,
        is_dismissed: false,
        task_type: "Reminder",
        linked_urls: [],
        linked_memory_ids: [],
    },
];

const previewStats: Stats = {
    total_records: 1842,
    total_days: 18,
    apps: [
        { name: "Visual Studio Code", count: 782 },
        { name: "Google Chrome", count: 526 },
        { name: "Figma", count: 318 },
        { name: "Terminal", count: 216 },
    ],
    today_count: 126,
    unique_apps: 9,
    unique_sessions: 41,
    unique_window_titles: 287,
    unique_urls: 164,
    unique_domains: 32,
    records_with_url: 502,
    records_with_screenshot: 1796,
    records_with_clean_text: 1714,
    records_last_hour: 23,
    records_last_24h: 126,
    records_last_7d: 836,
    avg_records_per_active_day: 102.3,
    avg_records_per_hour: 14.8,
    focus_app_share_pct: 42.5,
    app_switches: 97,
    app_switch_rate_per_hour: 7.4,
    avg_gap_minutes: 4.2,
    longest_gap_minutes: 84,
    first_capture_ts: previewNow - 18 * 24 * 60 * 60 * 1000,
    last_capture_ts: previewNow - 18_000,
    capture_span_hours: 432,
    current_streak_days: 6,
    longest_streak_days: 11,
    avg_ocr_confidence: 0.91,
    low_confidence_records: 38,
    avg_noise_score: 0.08,
    high_noise_records: 17,
    avg_ocr_blocks: 11.6,
    llm_count: 402,
    vlm_count: 102,
    fallback_count: 71,
    other_summary_count: 1267,
    top_domains: [
        { domain: "capstone.cs.utah.edu", count: 94 },
        { domain: "developer.mozilla.org", count: 61 },
        { domain: "w3.org", count: 37 },
    ],
    busiest_day: { day: "2026-09-22", count: 126 },
    quietest_day: { day: "2026-09-18", count: 34 },
    busiest_hour: { hour: 16, count: 48 },
    hourly_distribution: [
        { hour: 9, count: 12 },
        { hour: 10, count: 21 },
        { hour: 11, count: 18 },
        { hour: 13, count: 27 },
        { hour: 14, count: 32 },
        { hour: 15, count: 39 },
        { hour: 16, count: 48 },
        { hour: 17, count: 26 },
    ],
    weekday_distribution: [
        { weekday: "Monday", count: 311 },
        { weekday: "Tuesday", count: 426 },
        { weekday: "Wednesday", count: 344 },
        { weekday: "Thursday", count: 296 },
        { weekday: "Friday", count: 284 },
        { weekday: "Saturday", count: 103 },
        { weekday: "Sunday", count: 78 },
    ],
    daypart_distribution: [
        { daypart: "Morning", count: 472 },
        { daypart: "Afternoon", count: 934 },
        { daypart: "Evening", count: 401 },
        { daypart: "Night", count: 35 },
    ],
};

const previewRuntimeMetrics: RuntimeMetricsSnapshot = {
    generated_at_ms: previewNow,
    process_rss_bytes: 284_164_096,
    capture: {
        frames_captured: captureStatus.frames_captured,
        frames_dropped: captureStatus.frames_dropped,
        last_capture_time_ms: previewNow - 18_000,
    },
    embedding: {
        backend: "ONNX Runtime",
        degraded: false,
        detail: "Local MiniLM embedder ready",
        model_name: "all-MiniLM-L6-v2",
        dimension: 384,
        clip_session_loaded: true,
        last_clip_infer_ms: 41,
    },
    inference: {
        ai_model_available: true,
        ai_model_loaded: true,
        loaded_model_id: "qwen3-vl-2b",
    },
    aggregates: {
        search: {
            n: 24,
            sum_ms: 696,
            max_ms: 61,
            avg_ms: 29,
            ewma_ms: 27.4,
            p50_ms: 25,
            p95_ms: 52,
        },
        capture_flush: {
            n: 48,
            sum_ms: 576,
            max_ms: 28,
            avg_ms: 12,
            ewma_ms: 11.7,
            p50_ms: 10,
            p95_ms: 21,
        },
    },
    counters: {
        "capture.skipped_sensitive": 14,
        "search.timeout": 0,
    },
    recent: [
        { ts_ms: previewNow - 14_000, op: "search", ms: 26, meta: "4 cards" },
        { ts_ms: previewNow - 33_000, op: "capture_flush", ms: 11, meta: null },
    ],
    system: {
        generated_at_ms: previewNow,
        sample_interval_ms: 3000,
        process_cpu: {
            cpu_percent: 7.8,
            user_time_ms: 184_000,
            system_time_ms: 31_000,
            threads: 18,
        },
        process_memory: {
            rss_bytes: 284_164_096,
            virtual_bytes: 3_221_225_472,
            phys_footprint_bytes: 301_989_888,
            lifetime_max_phys_footprint_bytes: 356_515_840,
        },
        process_io: {
            disk_bytes_read: 82_575_360,
            disk_bytes_written: 19_922_944,
            disk_read_rate_bps: 32_768,
            disk_write_rate_bps: 12_288,
        },
        process_energy: {
            idle_wakeups: 14,
            interrupt_wakeups: 3,
            billed_system_time_ns: 9_400_000,
            label: "low",
        },
        host_cpu: {
            cpu_percent_total: 21.6,
            cpu_percent_per_core: [34, 29, 18, 14, 11, 9, 8, 7],
        },
        host_memory: {
            page_size_bytes: 16_384,
            free_bytes: 5_368_709_120,
            active_bytes: 9_663_676_416,
            inactive_bytes: 3_221_225_472,
            wired_bytes: 2_147_483_648,
            compressed_bytes: 1_073_741_824,
            total_bytes: 24_000_000_000,
            pressure_label: "low",
        },
        gpu: {
            device_utilization_percent: 11,
            renderer_utilization_percent: 7,
            in_use_system_memory_bytes: 536_870_912,
            recovery_count: 0,
        },
        model_memory: [
            { id: "qwen3-vl-2b", kind: "vlm", estimated_bytes: 2_100_000_000, loaded: true },
            { id: "all-MiniLM-L6-v2", kind: "embedding", estimated_bytes: 90_000_000, loaded: true },
        ],
    },
};

const previewReviewStatus: MemoryReviewWorkerStatus = {
    queue_depth: 3,
    last_review_at_ms: previewNow - 45_000,
    last_error_kind: null,
    worker_enabled: true,
    pressure_blocked: false,
};

const previewPrivacyProof: PrivacyProof = {
    evaluated: 2159,
    stored: 1842,
    skipped_by_reason: {
        sensitive_context: 14,
        blocklist: 42,
        self_app: 61,
        perceptual_dup: 93,
        semantic_dup: 54,
        low_signal_text: 21,
        screen_capture_failed: 2,
    },
    egress_requests: 0,
    egress_hosts: [],
};

function clonePreview<T>(value: T): T {
    return structuredClone(value);
}

/** Preview sign-in finishes on its own after this delay, as if the browser step completed. */
const PREVIEW_CODEX_LOGIN_MS = 2500;

const previewCodexSignedOut: CodexAccountStatus = {
    cliState: "ready",
    cliPath: "/opt/homebrew/bin/codex",
    cliError: null,
    account: null,
    usableForHermes: false,
    primaryWindow: null,
    secondaryWindow: null,
    models: [],
};

const previewCodexSignedIn: CodexAccountStatus = {
    ...previewCodexSignedOut,
    account: { kind: "chatgpt", email: "anurup@example.com", planType: "plus" },
    usableForHermes: true,
    primaryWindow: { usedPercent: 18, windowMinutes: 300, resetsAt: Math.floor(previewNow / 1000) + 3 * 3600 },
    secondaryWindow: { usedPercent: 42, windowMinutes: 10080, resetsAt: Math.floor(previewNow / 1000) + 4 * 86400 },
    models: [
        { id: "gpt-6-sol", displayName: "GPT-6 Sol", isDefault: true },
        { id: "gpt-5.6-sol", displayName: "GPT-5.6 Sol", isDefault: false },
    ],
};

function previewHermesStatus(codexSignedIn: boolean, configured: boolean): HermesBridgeStatus {
    return {
        installed: true,
        configured,
        setup_complete: configured,
        gateway_running: false,
        api_server_ready: false,
        version: "0.18.2",
        bundled_repo_available: false,
        runtime_source: "system",
        provider_kind: configured ? "codex" : null,
        model_name: configured ? "gpt-6-sol" : null,
        base_url: null,
        api_url: "http://127.0.0.1:8742",
        gateway_dir: "~/Library/Application Support/com.fndr.FNDR/hermes-gateway",
        home_dir: "~/Library/Application Support/com.fndr.FNDR/hermes",
        context_path: "~/Library/Application Support/com.fndr.FNDR/hermes-gateway/FNDR_CONTEXT.md",
        context_ready: true,
        last_synced_at: previewNow,
        fndr_local_model_id: null,
        ollama_installed: false,
        ollama_reachable: false,
        ollama_models: [],
        ollama_base_url: "http://127.0.0.1:11434/v1",
        codex_cli_installed: true,
        codex_logged_in: codexSignedIn,
        codex_auth_path: "~/.codex/auth.json",
        profile_name: null,
        focus_task: null,
        open_task_count: 2,
        direct_ollama_ready: false,
        last_error: null,
        install_command: "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash",
        gateway_state: "stopped",
        gateway_restarts: 0,
        reconnect_chatgpt: false,
    };
}

async function emitPreviewEventIfAvailable(event: string, payload: unknown): Promise<void> {
    if (typeof window === "undefined") {
        return;
    }
    const previewWindow = window as Window & {
        __TAURI_INTERNALS__?: { invoke?: unknown };
    };
    if (typeof previewWindow.__TAURI_INTERNALS__?.invoke !== "function") return;
    await emit(event, clonePreview(payload));
}

function payloadRecord(payload: unknown): Record<string, unknown> | null {
    return typeof payload === "object" && payload !== null
        ? payload as Record<string, unknown>
        : null;
}

/** `?home=empty|error|loading` renders that state of Home's work area; anything else is populated. */
type PreviewHomeState = "populated" | "empty" | "error" | "loading";

function previewHomeState(): PreviewHomeState {
    const value = new URLSearchParams(globalThis.location?.search ?? "").get("home");
    return value === "empty" || value === "error" || value === "loading" ? value : "populated";
}

type PreviewDailyState = "narrative" | "basic" | "empty" | "error" | "loading";

/** `?daily=basic|empty|error|loading` previews the other Daily Summary states. */
function previewDailyState(): PreviewDailyState {
    const value = new URLSearchParams(globalThis.location?.search ?? "").get("daily");
    return value === "basic" || value === "empty" || value === "error" || value === "loading" ? value : "narrative";
}

/** A fictional day for Sam Example. No real capture is used. */
function previewDailyNarrative(dateStr: string, state: PreviewDailyState): DailyNarrative {
    const base = {
        date: dateStr,
        backend: "apple" as const,
        groups_used: 4,
        items_dropped: 2,
        model_ms: 3400,
    };
    const actionItems = [
        { id: "action_item-share-audit", text: "Share the UI audit with the capstone team", memory_ids: ["memory-roadmap"], task_id: "task-share-audit", due_label: "Tomorrow" },
        { id: "action_item-keyboard-pass", text: "Run the keyboard-only panel pass", memory_ids: ["memory-accessibility-notes"], task_id: "task-keyboard-pass", due_label: null },
    ];
    const reminders = [
        { id: "reminder-native-permissions", text: "Verify capture permissions in the native app", memory_ids: ["memory-preview-harness"], task_id: "task-native-permissions", due_label: "In 2 days" },
    ];
    if (state === "empty") {
        return { ...base, mode: "empty", backend: "none", basic_reason: null, paragraphs: [], action_items: [], reminders: [], groups_used: 0, items_dropped: 0, model_ms: 0 };
    }
    if (state === "basic") {
        return {
            ...base,
            mode: "basic",
            backend: "none",
            basic_reason: "the on-device model did not answer",
            paragraphs: [
                { text: "Used Figma at 9:42 AM. Compared the home hierarchy and wallpaper contrast.", memory_ids: ["memory-design-review"] },
                { text: "Spent about 1 hour 20 minutes in Visual Studio Code on FNDR (1:05 PM to 2:25 PM). Mounted the real React shell behind synthetic IPC fixtures. Decision noted: keep the preview free of capture permissions.", memory_ids: ["memory-preview-harness"] },
            ],
            action_items: actionItems,
            reminders,
            groups_used: 2,
            items_dropped: 0,
            model_ms: 0,
        };
    }
    return {
        ...base,
        mode: "narrative",
        basic_reason: null,
        paragraphs: [
            { text: "Spent the morning in Figma on the FNDR home experience, comparing the home hierarchy and the wallpaper contrast.", memory_ids: ["memory-design-review"] },
            { text: "Spent about 1 hour 20 minutes in Visual Studio Code building the browser preview, then collected accessibility acceptance criteria in Google Chrome.", memory_ids: ["memory-preview-harness", "memory-accessibility-notes"] },
            { text: "Sequenced the UI overhaul into vertical slices in the audit notes.", memory_ids: ["memory-roadmap"] },
        ],
        action_items: actionItems,
        reminders,
    };
}

const previewResumeThreads: ResumeThread[] = [
    {
        key: "project:fndr-ui-overhaul",
        title: "FNDR UI overhaul",
        app_name: "Figma",
        last_state: "Reviewed the Home hierarchy and the accessibility checklist.",
        age_minutes: 24,
        next_steps: [],
        suggested_next_steps: [{ title: "Run the keyboard-only panel pass", source_memory_id: "memory-accessibility-notes", confidence: 0.8 }],
        evidence: ["memory-accessibility-notes", "memory-preview-pdf-page", "memory-design-review"],
        pack: { items: [], dropped_for_budget: 0, estimated_tokens: 0 },
    },
    {
        key: "project:fndr-beta-demo",
        title: "FNDR Beta demo",
        app_name: "Google Chrome",
        last_state: "Checked how judges score the Beta.",
        age_minutes: 95,
        next_steps: [],
        suggested_next_steps: [],
        evidence: ["memory-beta-rubric"],
        pack: { items: [], dropped_for_budget: 0, estimated_tokens: 0 },
    },
    {
        key: "work:pantry app",
        title: "Pantry App",
        grouped_by: ["Pantry App", "Demo script"],
        app_name: "Keynote",
        last_state: "Cut the demo script to four minutes and moved the architecture slide.",
        age_minutes: 12,
        next_steps: [],
        suggested_next_steps: [],
        evidence: ["memory-beta-rubric", "memory-design-review"],
        pack: { items: [], dropped_for_budget: 0, estimated_tokens: 0 },
    },
];

function requiredString(
    payload: unknown,
    field: string,
    command: string,
): string {
    const value = payloadRecord(payload)?.[field];
    if (typeof value !== "string" || !value.trim()) {
        throw new Error(`Preview ${command} requires ${field}.`);
    }
    return value.trim();
}

function validDate(value: string): boolean {
    return /^\d{4}-\d{2}-\d{2}$/.test(value)
        && Number.isFinite(Date.parse(`${value}T12:00:00.000Z`));
}

function displayPreviewDate(dateStr: string): string {
    return new Intl.DateTimeFormat("en-US", {
        month: "long",
        day: "numeric",
        year: "numeric",
        timeZone: "UTC",
    }).format(new Date(`${dateStr}T12:00:00.000Z`));
}

function searchPreviewCards(
    cards: MemoryCard[],
    query: string,
    appFilter: unknown,
    timeFilter: unknown,
    requestedLimit: unknown,
): MemoryCard[] {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return [];

    const stopWords = new Set(["a", "about", "and", "for", "how", "is", "of", "the", "to", "what"]);
    const tokens = normalized.split(/[^a-z0-9]+/).filter((token) => token && !stopWords.has(token));
    const limit = typeof requestedLimit === "number"
        ? Math.max(0, Math.floor(requestedLimit))
        : 20;

    return cards
        .filter((card) => typeof appFilter !== "string" || !appFilter.trim() || card.app_name === appFilter.trim())
        .filter((card) => {
            if (typeof timeFilter !== "string" || !timeFilter) return true;
            const age = previewNow - card.timestamp;
            if (timeFilter === "last_hour") return age <= 60 * 60 * 1000;
            if (timeFilter === "today" || timeFilter === "last_24h") return age <= 24 * 60 * 60 * 1000;
            if (timeFilter === "last_7d") return age <= 7 * 24 * 60 * 60 * 1000;
            return true;
        })
        .filter((card) => {
            if (normalized === "today" || normalized.includes("happened today")) {
                return previewNow - card.timestamp <= 24 * 60 * 60 * 1000;
            }
            const haystack = [
                card.title,
                card.summary,
                card.display_summary,
                card.action,
                card.app_name,
                card.window_title,
                ...(card.context ?? []),
                ...(card.topic_categories ?? []),
            ].filter(Boolean).join(" ").toLowerCase();
            return tokens.some((token) => haystack.includes(token));
        })
        .slice(0, limit)
        .map((card) => clonePreview(card));
}

/**
 * Synthetic IPC used only by ui-preview.html on Vite's development server.
 * Unknown commands fail intentionally so browser audits never mistake an
 * incomplete fixture for a working product flow.
 */
export function createPreviewIpcHandler(): PreviewIpcHandler {
    let previewCaptureStatus: CaptureStatus = {
        ...captureStatus,
        pipeline: { ...captureStatus.pipeline },
    };
    let previewCards = clonePreview(previewMemoryCards);
    let tasks = clonePreview(previewTasks);
    // Hand-off (ADR 032): starts without the document tools so the install
    // step is visible; a started job plays a short scripted run.
    let handOffToolchain: "missing" | "installed" = "missing";
    let handOffRunning = false;
    const handOffItems = [
        { kind: "memory", memoryId: "memory-q4-mail", label: "Mail: Q4 onboarding readout", appName: "Mail", capturedAt: previewNow - 75 * 60 * 1000 },
        { kind: "memory", memoryId: "memory-churn-notes", label: "Notes: churn hypotheses", appName: "Notes", capturedAt: previewNow - 50 * 60 * 1000 },
        { kind: "file", memoryId: "memory-q4-csv", label: "q4-onboarding-survey.csv", appName: "Finder", capturedAt: previewNow - 70 * 60 * 1000, bytes: 15_184 },
    ] as const;
    const handOffDraftFor = (taskText: string, taskMemoryId: string | null, goal: string, memoryIds?: string[], fileMemoryIds?: string[]) => {
        const items = handOffItems.map((item) => ({
            ...item,
            ticked: item.kind === "memory"
                ? (memoryIds ?? [item.memoryId]).includes(item.memoryId)
                : (fileMemoryIds ?? [item.memoryId]).includes(item.memoryId),
        }));
        const rendered = `TASK\n"${taskText}"\n\nOUTPUT\n${goal}\n\nEVIDENCE\n${items.filter((i) => i.ticked).map((i) => i.memoryId).join("\n")}`;
        const fileBytes = items.filter((i) => i.ticked && i.kind === "file").reduce((sum, i) => sum + ("bytes" in i ? i.bytes : 0), 0);
        return { taskText, taskMemoryId, goal, items, rendered, estimateBytes: rendered.length + fileBytes };
    };
    const handOffRecord = (status: string, reason: string | null) => ({
        id: "preview-hand-off",
        createdAtMs: Date.now() - 140_000,
        request: "Own a one-page Q4 onboarding survey readout with the top three churn drivers by Thursday",
        goal: "doc",
        memoryIds: ["memory-q4-mail", "memory-churn-notes"],
        fileMemoryIds: ["memory-q4-csv"],
        folder: "~/Documents/FNDR Hand-offs/2026-10-16 Own a one-page Q4 onboarding survey readout with the top",
        status,
        reason,
        finishedAtMs: Date.now(),
        bytesSent: 62_156,
        checked: status === "ready" ? 3 : 0,
        unsupported: 0,
        notChecked: 0,
        deliverables: status === "ready" ? ["out/q4-onboarding-survey-readout.docx"] : [],
        rating: null,
    });
    let nextTaskId = 1;
    let screenGuideSettings: ScreenGuideSettings = {
        enabled: true,
        shortcut: "Control+Alt+Space",
        speak_responses: false,
        show_cursor: true,
    };
    let voiceOutputSettings: VoiceOutputSettings = { provider: "auto", system_voice: "", rate: 1, neural_voice: "" };
    let screenGuideGeneration = 0;
    let screenGuideDiagnosticArmExpiresAt: number | null = null;
    let screenGuideDiagnosticStatus: Omit<
        ScreenGuideDiagnosticStatus,
        "armed" | "expiresInMs"
    > = {
        bundleCount: 0,
        partialCount: 0,
        totalBytes: 0,
        lastResult: null,
    };
    const currentScreenGuideDiagnosticStatus = (): ScreenGuideDiagnosticStatus => {
        const expiresInMs = screenGuideDiagnosticArmExpiresAt === null
            ? null
            : Math.max(0, screenGuideDiagnosticArmExpiresAt - Date.now());
        if (expiresInMs === 0) screenGuideDiagnosticArmExpiresAt = null;
        return {
            ...screenGuideDiagnosticStatus,
            armed: expiresInMs !== null && expiresInMs > 0,
            expiresInMs: expiresInMs !== null && expiresInMs > 0 ? expiresInMs : null,
        };
    };
    const consumeScreenGuideDiagnosticArm = () => {
        if (currentScreenGuideDiagnosticStatus().armed) {
            screenGuideDiagnosticArmExpiresAt = null;
        }
    };
    const releasedScreenGuideGenerations = new Set<number>();
    const previewOnboardingStep = (): OnboardingState["step"] => {
        const requested = new URLSearchParams(window.location.search).get("onboarding");
        const steps = ["welcome", "privacy_promise", "model_download", "bring_past"] as const;
        return steps.find((step) => step === requested) ?? "complete";
    };
    let previewOnboardingState: OnboardingState = {
        step: previewOnboardingStep(),
        biometric_enabled: false,
        screen_permission: true,
        accessibility_permission: true,
        model_downloaded: true,
        model_id: "qwen3-vl-2b",
        display_name: "Anurup",
    };
    let previewVoiceSession: string | null = null;
    let previewVoiceSessionCount = 0;
    let previewBlocklist = ["1Password", "bank.example"];
    const importKinds = ["browser", "file", "calendar", "mail", "slack", "git"];
    const importConsent = new Set<string>();
    const importStatuses = new Map<string, { source: string; state: string; window_days: number; scanned: number; stored: number; updated: number; duplicates: number; redacted: number; skipped: number; skip_reasons: Record<string, number>; started_at_ms: number; finished_at_ms: number | null; error: string | null }>();
    let firstImportAnswerMs: number | null = null;
    const importStatusFor = (source: string) => importStatuses.get(source) ?? {
        source, state: "idle", window_days: 30, scanned: 0, stored: 0, updated: 0,
        duplicates: 0, redacted: 0, skipped: 0, skip_reasons: {}, started_at_ms: 0,
        finished_at_ms: null, error: null,
    };
    let previewAgentNotesEnabled = false;
    let previewVerbatimMemoryEnabled = true;
    const previewParam = (name: string) => new URLSearchParams(window.location.search).get(name);
    const previewApprovalCard = {
        id: "preview-approval",
        caller: "Notch Do",
        surface: "computer_use" as const,
        tool: "paste_text",
        title: "Paste a reply into Mail",
        target: "Mail, Re: Atlas plan",
        args: [{ name: "text", value: "Thanks, I will review the Atlas plan today and send comments by Wednesday." }],
        reason: "You asked Notch Do to reply to Dana.",
        undo: { kind: "unavailable" as const, reason: "The text is typed into another app, so FNDR cannot take it back." },
        status: "pending" as const,
        detail: null,
        created_at_ms: Date.now(),
        expires_at_ms: Date.now() + 60_000,
    };
    const previewOutputDoc = () => ({
        id: "preview-output",
        kind: "standup" as const,
        title: "Standup, Thursday",
        start_ms: Date.now() - 86_400_000,
        end_ms: Date.now(),
        generated_at_ms: Date.now(),
        model_polished: false,
        sections: [
            { heading: "Yesterday", lines: [
                { text: "Reviewed the Home hierarchy and the accessibility checklist in Figma.", project: "FNDR", citations: [{ kind: "memory" as const, id: "1", memory_id: "preview-1" }] },
                { text: "Merged the retrieval gate change and checked Recall at 5 stayed at 1.00.", project: "FNDR", citations: [{ kind: "memory" as const, id: "2", memory_id: "preview-2" }, { kind: "memory" as const, id: "3", memory_id: "preview-3" }] },
            ] },
            { heading: "Today", lines: [
                { text: "Run the keyboard-only pass on the new panels.", project: "FNDR", citations: [{ kind: "memory" as const, id: "4", memory_id: "preview-4" }] },
            ] },
            { heading: "Blockers", lines: [] },
        ],
    });
    const previewMcpStatus = () => ({
        running: true,
        endpoint: "http://127.0.0.1:49152/mcp",
        actionsBlocked: false,
        keepRunning: true,
        hideMore: false,
        clients: [
            { id: "claude_code" as const, name: "Claude Code", state: "connected" as const, detected: true, configPath: "~/.claude.json", detail: null, missingTool: null },
            { id: "claude_desktop" as const, name: "Claude Desktop", state: "not_connected" as const, detected: true, configPath: "~/Library/Application Support/Claude/claude_desktop_config.json", detail: null, missingTool: null },
            { id: "cursor" as const, name: "Cursor", state: "conflict" as const, detected: true, configPath: "~/.cursor/mcp.json", detail: "An entry named fndr already exists that FNDR did not write.", missingTool: null },
        ],
    });
    let previewActionSettings = { actions_kill_switch: false, confirm_unnamed_apps: false };
    let previewNotchDoMuted: boolean | null = null;
    let previewSmartFillEnabled = false;
    let previewNotchDoAutonomy: "smooth" | "ask_each" = "smooth";
    const previewSkills: NotchDoSkill[] = [{
        id: "preview-skill-1",
        name: "Monday setup",
        request: "open Notes, Slack and the sprint board",
        steps: ["Open Notes", "Open Slack", "Open linear.app/team/sprint"],
        savedAt: "2026-10-13T15:00:00Z",
    }];
    let previewRuns: NotchDoRun[] = [
        {
            runId: "preview-run-1",
            at: "2026-10-15T15:20:00Z",
            request: "open Notes and look up the weather in Paris",
            plannedBy: "local",
            steps: ["Open Notes", "Search the web for the weather in Paris"],
            ok: true,
            summary: "Done: Open Notes, Search the web for the weather in Paris.",
            rating: null,
            repeatable: true,
        },
        {
            runId: "preview-run-2",
            at: "2026-10-15T14:05:00Z",
            request: "add the next song to the queue in Spotify",
            plannedBy: "planner",
            steps: ["Add the next song to the queue"],
            ok: false,
            summary: "Stopped at the first step.",
            rating: null,
            repeatable: false,
        },
    ];
    let previewPrivacyAlerts: PrivacyAlert[] = [];
    let codexSignedIn = false;
    let calendarMeetingPrep = false;
    let agentChats: Array<{ id: string; title: string; createdAt: number; updatedAt: number; messages: Array<{ role: string; content: string; at: number; memories: Array<{ id: string; title: string; appName: string; timestamp: number }> }> }> = [];
    let configuredPeers: ConfiguredPeer[] = [];
    let previewPeerRuns: Array<{ local_id: string; peer_id: string; host: string; created_at_ms: number;
        payload_bytes: number; status: string; remote_task_id: string | null; remote_state: string | null }> = [];
    let hermesConfigured = false;
    const homeState = previewHomeState();
    const homeWaits = () => new Promise<never>(() => undefined);
    let previewNamedSets: Array<{ id: string; name: string; memoryIds: string[]; savedAt: number }> = homeState === "populated"
        ? [{
            id: "preview-named-beta",
            name: "Beta demo prep",
            memoryIds: ["memory-beta-rubric", "memory-accessibility-notes", "memory-preview-pdf-page", "memory-design-review"],
            savedAt: previewNow,
        }]
        : [];
    let previewRoutineDismissed = false;
    const previewNamedSet = (set: { id: string; name: string; memoryIds: string[]; savedAt: number }) => {
        const items = set.memoryIds.flatMap((memoryId) => {
            const card = previewCards.find((row) => row.id === memoryId);
            return card
                ? [{ memoryId, label: card.title, kind: "url" as const, reopenRank: 4, appName: card.app_name, capturedAt: card.timestamp }]
                : [];
        });
        return { ...set, memoryIds: items.map((item) => item.memoryId), items };
    };
    let codexLoginSeq = 0;
    let pendingCodexLogin: { loginId: string; timer: ReturnType<typeof setTimeout> } | null = null;

    return async (command: string, payload?: unknown) => {
        switch (command) {
            case "import_list_sources":
                return importKinds.map((kind) => ({
                    id: kind, kind, label: kind, permissions: [],
                    row_count: previewCards.filter((card) => card.source_type === `import:${kind}`).length,
                    status: importStatusFor(kind),
                }));
            case "import_get_consent":
                return importConsent.has(requiredString(payload, "source", command));
            case "import_set_consent": {
                const source = requiredString(payload, "source", command);
                if (!importKinds.includes(source)) throw new Error("Unknown import source");
                if (payloadRecord(payload)?.enabled === true) importConsent.add(source);
                else importConsent.delete(source);
                return undefined;
            }
            case "import_start": {
                const source = requiredString(payload, "source", command);
                if (!importConsent.has(source)) throw new Error("Import requires consent for this source.");
                const windowDays = Number(payloadRecord(payload)?.windowDays) || 30;
                if (source === "browser" && !previewCards.some((card) => card.id === "preview-import-browser")) {
                    previewCards.push({ ...previewMemoryCards[0], id: "preview-import-browser",
                        title: "Atlas planning notes", source_type: "import:browser",
                        reopen_target: "https://example.test/atlas-plan" });
                }
                const stored = source === "browser" ? 1 : 0;
                const status = { ...importStatusFor(source), state: "done", window_days: windowDays,
                    scanned: stored, stored, started_at_ms: Date.now(), finished_at_ms: Date.now() };
                importStatuses.set(source, status);
                return status;
            }
            case "import_status":
                return importStatusFor(requiredString(payload, "source", command));
            case "import_cancel":
                return true;
            case "import_forget": {
                const source = requiredString(payload, "source", command);
                const before = previewCards.length;
                previewCards = previewCards.filter((card) => card.source_type !== `import:${source}`);
                importConsent.delete(source);
                importStatuses.delete(source);
                return before - previewCards.length;
            }
            case "import_record_first_answer":
                firstImportAnswerMs ??= Number(payloadRecord(payload)?.elapsedMs) || 0;
                return undefined;
            case "import_first_answer_ms":
                return firstImportAnswerMs;
            case "get_onboarding_state":
                return { ...previewOnboardingState };
            case "save_onboarding_state": {
                const state =
                    typeof payload === "object" && payload !== null && "state" in payload
                        ? (payload as { state?: unknown }).state
                        : null;
                if (!state || typeof state !== "object") {
                    throw new Error("Preview save_onboarding_state requires a state payload.");
                }
                previewOnboardingState = { ...previewOnboardingState, ...(state as OnboardingState) };
                return undefined;
            }
            case "get_status":
                return { ...previewCaptureStatus, pipeline: { ...previewCaptureStatus.pipeline } };
            case "voice_start": {
                if (previewVoiceSession) throw new Error("A preview voice session is already active.");
                const args = payloadRecord(payload);
                if (typeof args?.surface !== "string" || typeof args?.mode !== "string") {
                    throw new Error("Preview voice_start requires a surface and mode.");
                }
                previewVoiceSession = `preview-voice-${++previewVoiceSessionCount}`;
                return { sessionId: previewVoiceSession };
            }
            case "voice_stop":
                if (payloadRecord(payload)?.sessionId !== previewVoiceSession) {
                    throw new Error("Preview voice_stop requires the active session.");
                }
                previewVoiceSession = null;
                return undefined;
            case "voice_cancel":
                if (payloadRecord(payload)?.sessionId === previewVoiceSession) previewVoiceSession = null;
                return undefined;
            case "pause_capture":
                previewCaptureStatus = { ...previewCaptureStatus, is_paused: true };
                await emitPreviewEventIfAvailable(CAPTURE_STATUS_EVENT, previewCaptureStatus);
                return undefined;
            case "resume_capture":
                previewCaptureStatus = { ...previewCaptureStatus, is_paused: false };
                await emitPreviewEventIfAvailable(CAPTURE_STATUS_EVENT, previewCaptureStatus);
                return undefined;
            case "get_fun_greeting":
                return "Good evening, Anurup.";
            case "what_changed_since": {
                const threadKey =
                    typeof payload === "object" && payload !== null && "threadKey" in payload
                        ? String((payload as { threadKey?: unknown }).threadKey)
                        : "";
                return {
                    thread_key: threadKey,
                    title: "FNDR",
                    since_ms: Date.now() - 3 * 60 * 60 * 1000,
                    first_view: false,
                    new_memories: 6,
                    page_count: 1,
                    pages: ["Tauri notification plugin docs"],
                    file_count: 2,
                    files: ["notify.rs", "main.rs"],
                    task_count: 1,
                    tasks: ["Add a test for the toast target"],
                    commit_count: 1,
                    newest_memory_id: "preview-memory-1",
                };
            }
            case "mark_thread_seen":
                return undefined;
            case "calendar_meeting_prep_state":
                return { enabled: calendarMeetingPrep, status: "not_determined" };
            case "set_calendar_meeting_prep": {
                const enabled = payloadRecord(payload)?.enabled;
                if (typeof enabled !== "boolean") {
                    throw new Error("Preview set_calendar_meeting_prep needs a boolean enabled");
                }
                calendarMeetingPrep = enabled;
                return { enabled, status: enabled ? "granted" : "not_determined" };
            }
            case "open_calendar_privacy_settings":
                return undefined;
            case "next_meeting_brief": {
                const startsAt = Date.now() + 12 * 60_000;
                const ago = (minutes: number) => Date.now() - minutes * 60_000;
                return {
                    state: "ready",
                    title: "FNDR demo walkthrough",
                    startsAt,
                    strongMatch: true,
                    thread: { key: "FNDR", title: "FNDR" },
                    memories: [
                        { id: "preview-memory-1", title: "Meeting prep card design", appName: "Cursor", timestamp: ago(14), snippet: "Drafting the Home card that compiles related memories before a calendar event." },
                        { id: "preview-memory-2", title: "FNDR demo video voiceover script", appName: "Terminal", timestamp: ago(41), snippet: "Voiceover script for the demo: capture, recall, Hermes, and meeting prep." },
                        { id: "preview-memory-3", title: "Capstone sprint notes", appName: "Notion", timestamp: ago(190), snippet: "Open items for the demo: polish Hermes chat, fix the memory picker overlay." },
                    ],
                };
            }
            case "get_app_names":
                return ["Visual Studio Code", "Google Chrome", "Terminal", "Figma"];
            case "get_privacy_alerts":
                return previewPrivacyAlerts.map((alert) => ({ ...alert }));
            case "get_blocklist":
                return [...previewBlocklist];
            case "get_agent_notes_enabled":
                return previewAgentNotesEnabled;
            case "get_verbatim_memory_enabled":
                return previewVerbatimMemoryEnabled;
            case "list_action_approvals":
                return previewParam("approval") === "pending" ? [previewApprovalCard] : [];
            case "get_encryption_statement":
                return {
                    filevault: "on",
                    encrypted: [
                        "The few screenshots FNDR saves are sealed with a key in your Keychain.",
                        "Exports you save from FNDR are sealed the same way.",
                    ],
                    not_encrypted: [
                        "The memory database is not encrypted by FNDR. FileVault protects it while your Mac is off.",
                        "Text you copy out of FNDR is as private as the place you paste it.",
                    ],
                };
            case "generate_daily_output":
                return previewOutputDoc();
            case "render_daily_output":
                return "Standup, Thursday\n\nYesterday\n- Reviewed the Home hierarchy [1]\n";
            case "output_destination_status":
                return { slack_webhook: true, linear_api_key: false, linear_team_id: false };
            case "list_output_allowed_destinations":
                return [];
            case "propose_daily_output_send":
                return {
                    approval_id: "preview-send",
                    tool: "outputs.post_slack",
                    action: "Post to Slack",
                    destination: "hooks.slack.com/services/T0/B0/...",
                    title: "Standup, Thursday",
                    body: "Standup, Thursday\n\nYesterday\n- Reviewed the Home hierarchy and the accessibility checklist in Figma. [1]\n- Merged the retrieval gate change. [2][3]\n\nToday\n- Run the keyboard-only pass on the new panels. [4]",
                    bytes: 247,
                    expires_at_ms: Date.now() + 60_000,
                    trimmed: false,
                };
            case "send_daily_output_allowed":
                return null;
            case "resolve_daily_output_send":
                return payloadRecord(payload)?.approved === true
                    ? { tool: "outputs.post_slack", destination: "hooks.slack.com/services/T0/B0/...", detail: "Posted", trust_id: "preview-trust" }
                    : null;
            case "trust_output_destination":
                return "Always allowed.";
            case "mcp_connect_status":
                return previewMcpStatus();
            case "mcp_connect_preview": {
                const client = previewMcpStatus().clients.find(
                    (entry) => entry.id === payloadRecord(payload)?.clientId,
                );
                if (!client) throw new Error("Preview connection setup requires a known client.");
                const text = JSON.stringify({ mcpServers: { fndr: { url: previewMcpStatus().endpoint } } }, null, 2);
                return {
                    title: `Connect ${client.name} to FNDR`,
                    path: client.configPath,
                    text,
                    maskedText: text,
                    command: null,
                };
            }
            case "resolve_action_approval":
                throw new Error("No action is waiting for approval in preview.");
            case "get_action_settings":
                return { ...previewActionSettings };
            case "set_action_settings": {
                const settings = payloadRecord(payload)?.settings;
                if (typeof settings !== "object" || settings === null ||
                    !("actions_kill_switch" in settings) || typeof settings.actions_kill_switch !== "boolean" ||
                    !("confirm_unnamed_apps" in settings) || typeof settings.confirm_unnamed_apps !== "boolean") {
                    throw new Error("Preview action settings require boolean values.");
                }
                previewActionSettings = {
                    actions_kill_switch: settings.actions_kill_switch,
                    confirm_unnamed_apps: settings.confirm_unnamed_apps,
                };
                return { ...previewActionSettings };
            }
            case "notch_do_skills":
                return previewSkills.map((skill) => ({ ...skill }));
            case "notch_do_runs":
                return previewRuns.map((run) => ({ ...run }));
            case "notch_do_rate_run": {
                const { runId, worked } = payload as { runId: string; worked: boolean };
                previewRuns = previewRuns.map((run) => run.runId === runId
                    ? { ...run, rating: worked ? "worked" : "did_not_work", repeatable: worked && run.repeatable }
                    : run);
                return undefined;
            }
            case "notch_do_save_skill": {
                const { runId, name } = payload as { runId: string; name: string };
                const run = previewRuns.find((each) => each.runId === runId);
                if (!run?.repeatable) throw new Error("Only a run that worked can be saved as a skill.");
                previewSkills.push({ id: `preview-skill-${previewSkills.length + 1}`, name, request: run.request, steps: run.steps, savedAt: new Date().toISOString() });
                return name;
            }
            case "notch_do_delete_skill": {
                const { id } = payload as { id: string };
                const index = previewSkills.findIndex((skill) => skill.id === id);
                if (index >= 0) previewSkills.splice(index, 1);
                return index >= 0;
            }
            case "notch_do_request":
                throw new Error('Turn on "Operate my Mac" in Settings for FNDR to do this.');
            case "saved_details_count":
                return 0;
            case "forget_saved_details":
                return 0;
            case "get_smart_fill_enabled":
                return previewSmartFillEnabled;
            case "set_smart_fill_enabled": {
                const { enabled } = payload as { enabled?: unknown };
                if (typeof enabled !== "boolean") {
                    throw new Error("Preview set_smart_fill_enabled requires a boolean.");
                }
                previewSmartFillEnabled = enabled;
                return enabled;
            }
            case "get_notch_do_autonomy":
                return previewNotchDoAutonomy;
            case "set_notch_do_autonomy": {
                const { autonomy } = payload as { autonomy?: unknown };
                if (autonomy !== "smooth" && autonomy !== "ask_each") {
                    throw new Error("Preview set_notch_do_autonomy requires smooth or ask_each.");
                }
                previewNotchDoAutonomy = autonomy;
                return autonomy;
            }
            case "get_notch_do_muted":
                return previewNotchDoMuted;
            case "set_notch_do_muted": {
                const muted = typeof payload === "object" && payload !== null && "muted" in payload
                    ? (payload as { muted?: unknown }).muted
                    : null;
                if (typeof muted !== "boolean") {
                    throw new Error("Preview set_notch_do_muted requires a boolean.");
                }
                previewNotchDoMuted = muted;
                return muted;
            }
            case "set_notch_escape_monitor":
                return undefined;
            case "set_agent_notes_enabled": {
                const enabled = typeof payload === "object" && payload !== null && "enabled" in payload
                    ? (payload as { enabled?: unknown }).enabled
                    : null;
                if (typeof enabled !== "boolean") {
                    throw new Error("Preview set_agent_notes_enabled requires a boolean.");
                }
                previewAgentNotesEnabled = enabled;
                return undefined;
            }
            case "set_verbatim_memory_enabled": {
                const enabled = typeof payload === "object" && payload !== null && "enabled" in payload
                    ? (payload as { enabled?: unknown }).enabled
                    : null;
                if (typeof enabled !== "boolean") {
                    throw new Error("Preview set_verbatim_memory_enabled requires a boolean.");
                }
                previewVerbatimMemoryEnabled = enabled;
                return undefined;
            }
            case "set_blocklist": {
                const apps =
                    typeof payload === "object" && payload !== null && "apps" in payload
                        ? (payload as { apps?: unknown }).apps
                        : null;
                if (!Array.isArray(apps) || !apps.every((app) => typeof app === "string")) {
                    throw new Error("Preview set_blocklist requires a string array.");
                }
                previewBlocklist = [...apps];
                return undefined;
            }
            case "add_to_blocklist": {
                const site =
                    typeof payload === "object" && payload !== null && "site" in payload
                        ? (payload as { site?: unknown }).site
                        : null;
                if (typeof site !== "string" || !site.trim()) {
                    throw new Error("Preview add_to_blocklist requires a site.");
                }
                if (!previewBlocklist.includes(site.trim())) previewBlocklist.push(site.trim());
                previewPrivacyAlerts = previewPrivacyAlerts.filter(
                    (alert) => alert.domain_or_title !== site,
                );
                return undefined;
            }
            case "dismiss_privacy_alert": {
                const site =
                    typeof payload === "object" && payload !== null && "site" in payload
                        ? (payload as { site?: unknown }).site
                        : null;
                if (typeof site !== "string") {
                    throw new Error("Preview dismiss_privacy_alert requires a site.");
                }
                previewPrivacyAlerts = previewPrivacyAlerts.filter(
                    (alert) => alert.domain_or_title !== site,
                );
                return undefined;
            }
            case "list_available_models":
                return previewModels.map((model) => ({ ...model }));
            case "get_apple_model_status":
            case "set_apple_model_enabled":
                return { availability: "unsupported_os", enabled: false };
            case "fndr_quality_status":
                return {
                    stored_count: previewCaptureStatus.pipeline.stored_total,
                    dropped_count: previewCaptureStatus.pipeline.skipped_total,
                    flagged_count: 0,
                };
            case "search_memory_cards": {
                const input = payloadRecord(payload);
                const query = input?.query;
                if (typeof query !== "string") {
                    throw new Error("Preview search_memory_cards requires query.");
                }
                return searchPreviewCards(
                    previewCards,
                    query,
                    input?.appFilter,
                    input?.timeFilter,
                    input?.limit,
                );
            }
            case "summarize_search": {
                const query = requiredString(payload, "query", command);
                const snippets = payloadRecord(payload)?.resultsSnippets;
                if (!Array.isArray(snippets) || !snippets.every((snippet) => typeof snippet === "string")) {
                    throw new Error("Preview summarize_search requires resultsSnippets.");
                }
                return `Preview-only summary for “${query}”: ${snippets.slice(0, 2).join(" ")}`;
            }
            case "transcribe_voice_input": {
                const audioBytes = payloadRecord(payload)?.audioBytes;
                if (!Array.isArray(audioBytes) || !audioBytes.every((byte) => typeof byte === "number")) {
                    throw new Error("Preview transcribe_voice_input requires audioBytes.");
                }
                return {
                    text: "Search for the FNDR browser preview",
                    backend: "preview-synthetic-audio",
                };
            }
            case "fndr_answer": {
                const query = requiredString(payload, "query", command);
                const hasPreviewEvidence = /accessibility|browser|contrast|design|fndr|home|keyboard|permission|preview|roadmap|ui|ux/i.test(query);
                const matchingCards = hasPreviewEvidence
                    ? searchPreviewCards(previewCards, query, null, null, 5)
                    : [];
                const previewHarness = previewCards.find((card) => card.id === "memory-preview-harness");
                const browserQuestion = /browser|permission|preview/i.test(query);
                const cards = browserQuestion && previewHarness
                    ? [clonePreview(previewHarness)]
                    : matchingCards;
                if (cards.length === 0) {
                    const refusal: ComposedAnswer = {
                        query,
                        answer: "I could not find enough evidence in the synthetic preview memories to answer that.",
                        evidence: {
                            files: [], commands: [], decisions: [], errors: [], todos: [], urls: [],
                        },
                        cards: [],
                        verify_outcome: {
                            kind: "not_enough_evidence",
                            reason: "No preview memory matched this question.",
                        },
                        surfacing_reasons: [],
                    };
                    return refusal;
                }
                const answer: ComposedAnswer = {
                    query,
                    answer: browserQuestion
                        ? "The browser preview mounts FNDR's real React shell behind synthetic, local-only IPC fixtures, so it does not request capture, biometric, microphone, file, or external-app permissions."
                        : `The synthetic preview memories point to ${cards[0].title.toLowerCase()}.`,
                    evidence: {
                        files: [{ path: "src/dev/previewIpc.ts", memory_ids: cards.map((card) => card.id) }],
                        commands: [],
                        decisions: [{ decision: "Keep native capabilities outside browser QA.", memory_ids: cards.map((card) => card.id) }],
                        errors: [],
                        todos: [],
                        urls: [],
                    },
                    cards,
                    verify_outcome: { kind: "grounded", confidence: 0.94 },
                    surfacing_reasons: cards.map((card) => ({
                        headline: `Matched ${card.title}`,
                        routes: ["synthetic preview memory"],
                    })),
                };
                return answer;
            }
            case "list_memory_cards": {
                const appFilter =
                    typeof payload === "object" && payload !== null && "appFilter" in payload
                        ? (payload as { appFilter?: unknown }).appFilter
                        : null;
                return typeof appFilter === "string" && appFilter.trim()
                    ? clonePreview(previewCards.filter((card) => card.app_name === appFilter.trim()))
                    : clonePreview(previewCards);
            }
            case "list_needs_signal_memory_cards":
                return clonePreview(previewNeedsSignalCards);
            case "get_full_graph": {
                // Synthetic Connections strip: one node per memory, linked to its project.
                const createdAt = new Date(previewNow).toISOString();
                const projects = [...new Set(previewCards.map((card) => card.project).filter(Boolean))] as string[];
                const node = (id: string, nodeType: string, label: string, sourceIds: string[]) => ({
                    id, node_type: nodeType, label, confidence: 0.9, source_memory_ids: sourceIds,
                    created_at: createdAt, updated_at: createdAt, stale: false, metadata: {},
                });
                return {
                    nodes: [
                        ...projects.map((project) => node(`project:${project}`, "project", project, [])),
                        ...previewCards.map((card) => node(`memory:${card.id}`, "memory", card.title, [card.id])),
                    ],
                    edges: previewCards
                        .filter((card) => card.project)
                        .map((card) => ({
                            id: `edge:${card.id}`, source_id: `memory:${card.id}`, target_id: `project:${card.project}`,
                            edge_type: "part_of_project", confidence: 0.9, conflict_flag: false,
                            created_at: createdAt, metadata: {},
                        })),
                    louvain: {},
                    cluster_0_name: "",
                };
            }
            case "fndr_get_related_memories": {
                const memoryId =
                    typeof payload === "object" && payload !== null && "memoryId" in payload
                        ? (payload as { memoryId?: unknown }).memoryId
                        : null;
                const requestedLimit =
                    typeof payload === "object" && payload !== null && "limit" in payload
                        ? (payload as { limit?: unknown }).limit
                        : 4;
                const limit = typeof requestedLimit === "number" ? requestedLimit : 4;
                return previewCards
                    .filter((card) => card.id !== memoryId)
                    .slice(0, Math.max(0, limit))
                    .map((card) => clonePreview(card));
            }
            case "find_visually_similar_memories":
                return clonePreview(previewSimilarResults);
            case "fndr_build_context_pack": {
                const query =
                    typeof payload === "object" && payload !== null && "query" in payload
                        ? (payload as { query?: unknown }).query
                        : "";
                return {
                    query: typeof query === "string" ? query : "",
                    summary:
                        "A synthetic, local-only context pack for validating the preview copy flow.",
                    relevant_files: [
                        { path: "src/app/HomeHero.tsx" },
                        { path: "src/app/HomeHero.css" },
                    ],
                    recent_decisions: [
                        { summary: "Keep the wallpaper ambient and the task surface readable." },
                    ],
                };
            }
            case "reopen_memory": {
                const memoryId =
                    typeof payload === "object" && payload !== null && "memoryId" in payload
                        ? (payload as { memoryId?: unknown }).memoryId
                        : null;
                // Preview confirms that a target exists but never launches a URL or native app.
                const hasTarget = previewCards.some(
                    (card) => card.id === memoryId && Boolean(card.reopen_target),
                );
                return hasTarget ? { kind: "opened" } : { kind: "no_target" };
            }
            case "delete_memory": {
                const memoryId =
                    typeof payload === "object" && payload !== null && "memoryId" in payload
                        ? (payload as { memoryId?: unknown }).memoryId
                        : null;
                const cardIndex = previewCards.findIndex((card) => card.id === memoryId);
                if (cardIndex < 0) return false;
                previewCards = previewCards.filter((card) => card.id !== memoryId);
                return true;
            }
            case "generate_daily_summary_for_date": {
                const dateStr = requiredString(payload, "dateStr", command);
                if (!validDate(dateStr)) {
                    throw new Error("Preview generate_daily_summary_for_date requires YYYY-MM-DD.");
                }
                return [
                    `- FNDR captured 126 memories across about 8 hours on ${displayPreviewDate(dateStr)}, from 9:42 AM to 5:57 PM.`,
                    "- Visual Studio Code: 41 captures from 1:05 PM to 2:25 PM around \"previewIpc.ts - FNDR\".",
                    "- Figma: 12 captures from 9:42 AM to 10:30 AM around \"FNDR: Home concepts\".",
                    "- Google Chrome: 1 capture at 3:12 PM around \"Inclusive product interface checklist\".",
                ].join("\n");
            }
            case "generate_daily_summary_narrative": {
                const dateStr = requiredString(payload, "dateStr", command);
                if (!validDate(dateStr)) {
                    throw new Error("Preview generate_daily_summary_narrative requires YYYY-MM-DD.");
                }
                const dailyState = previewDailyState();
                if (dailyState === "loading") return homeWaits();
                if (dailyState === "error") throw new Error("Preview: the day could not be summarized.");
                return clonePreview(previewDailyNarrative(dateStr, dailyState));
            }
            case "get_daily_summary_followups":
                // Like the backend (stats.rs): a suggestion nobody accepted is
                // not work the person is carrying.
                return clonePreview(tasks.filter((task) => (
                    task.task_type === "Followup" && !task.is_completed && !task.is_dismissed
                    && !task.source_app.startsWith("Memory:") && task.source_app.toLowerCase() !== "auto"
                )));
            case "generate_daily_briefing":
                return "Your preview briefing: finish the keyboard walkthrough, share the UI audit, and run native permission checks in the Tauri app.";
            case "resume_work":
                if (homeState === "loading") return homeWaits();
                if (homeState === "error") throw new Error("Preview: recent work could not be read.");
                return homeState === "empty" ? [] : clonePreview(previewResumeThreads);
            case "get_todos":
                if (homeState === "loading") return homeWaits();
                if (homeState === "error") throw new Error("Preview: tasks could not be read.");
                return clonePreview(tasks.filter((task) => (
                    !task.is_completed && !task.is_dismissed && (homeState !== "empty" || task.due_date === null)
                )));
            case "add_todo": {
                const title = requiredString(payload, "title", command);
                const requestedType = payloadRecord(payload)?.taskType;
                const taskType = requestedType === "Reminder" || requestedType === "Followup"
                    ? requestedType
                    : "Todo";
                const task: Task = {
                    id: `preview-task-${nextTaskId++}`,
                    title,
                    description: "Added in the permission-free browser preview. This task is not persisted to disk.",
                    source_app: "manual",
                    source_memory_id: null,
                    created_at: previewNow + nextTaskId,
                    due_date: null,
                    is_completed: false,
                    is_dismissed: false,
                    task_type: taskType,
                    linked_urls: [],
                    linked_memory_ids: [],
                };
                tasks = [task, ...tasks];
                return clonePreview(task);
            }
            case "update_todo": {
                const taskId = requiredString(payload, "taskId", command);
                const title = requiredString(payload, "title", command);
                const requestedType = payloadRecord(payload)?.taskType;
                const index = tasks.findIndex((task) => task.id === taskId);
                if (index < 0) {
                    throw new Error("Preview task no longer exists.");
                }
                const taskType = requestedType === "Todo"
                    || requestedType === "Reminder"
                    || requestedType === "Followup"
                    ? requestedType
                    : tasks[index].task_type;
                const updated: Task = { ...tasks[index], title, task_type: taskType };
                tasks = tasks.map((task) => task.id === taskId ? updated : task);
                return clonePreview(updated);
            }
            case "complete_todo":
            case "dismiss_todo": {
                const taskId = requiredString(payload, "taskId", command);
                const index = tasks.findIndex((task) => task.id === taskId);
                if (index < 0) return false;
                tasks = tasks.map((task) => task.id === taskId
                    ? {
                        ...task,
                        is_completed: command === "complete_todo" ? true : task.is_completed,
                        is_dismissed: command === "dismiss_todo" ? true : task.is_dismissed,
                    }
                    : task);
                return true;
            }
            case "set_todo_completed": {
                const taskId = requiredString(payload, "taskId", command);
                const isCompleted = payloadRecord(payload)?.isCompleted;
                if (typeof isCompleted !== "boolean") {
                    throw new Error("Preview set_todo_completed requires isCompleted.");
                }
                if (!tasks.some((task) => task.id === taskId)) return false;
                tasks = tasks.map((task) => task.id === taskId
                    ? { ...task, is_completed: isCompleted }
                    : task);
                return true;
            }
            case "get_stats":
                return clonePreview(previewStats);
            case "get_weekly_wrapped": {
                const input = payloadRecord(payload);
                const startDate = typeof input?.startDate === "string" && validDate(input.startDate)
                    ? input.startDate
                    : "2026-09-21";
                const endDate = typeof input?.endDate === "string" && validDate(input.endDate)
                    ? input.endDate
                    : "2026-09-22";
                const wrapped: WeeklyWrapped = {
                    start_date: startDate,
                    end_date: endDate,
                    generated_at_ms: previewNow,
                    total_captures: 1842,
                    active_days: 6,
                    total_minutes: 2310,
                    apps: [
                        { name: "Visual Studio Code", captures: 782, duration_minutes: 986 },
                        { name: "Google Chrome", captures: 526, duration_minutes: 612 },
                        { name: "Figma", captures: 318, duration_minutes: 438 },
                    ],
                    websites: [
                        { name: "capstone.cs.utah.edu", captures: 94, duration_minutes: 173 },
                        { name: "developer.mozilla.org", captures: 61, duration_minutes: 119 },
                    ],
                    projects_and_topics: [
                        { name: "FNDR UI overhaul", count: 28 },
                        { name: "Accessibility", count: 17 },
                        { name: "Browser QA", count: 14 },
                    ],
                    meeting_count: 2,
                    document_count: 19,
                    open_followups: tasks.filter((task) => task.task_type === "Followup" && !task.is_completed).length,
                    open_tasks: tasks.filter((task) => !task.is_completed && !task.is_dismissed).length,
                    busiest_day: { day: "Tuesday", count: 426 },
                    busiest_hour: 16,
                    most_revisited_file: "src/dev/previewIpc.ts",
                };
                return wrapped;
            }
            case "export_daily_summary_pdf": {
                const dateStr = requiredString(payload, "dateStr", command);
                requiredString(payload, "summaryText", command);
                if (!validDate(dateStr)) {
                    throw new Error("Preview export_daily_summary_pdf requires YYYY-MM-DD.");
                }
                // Virtual scheme: the browser preview never writes a file.
                return `preview-only://exports/daily-summary-${dateStr}.pdf`;
            }
            case "export_weekly_wrapped_pdf": {
                const startDate = requiredString(payload, "startDate", command);
                const endDate = requiredString(payload, "endDate", command);
                requiredString(payload, "recapText", command);
                if (!validDate(startDate) || !validDate(endDate)) {
                    throw new Error("Preview export_weekly_wrapped_pdf requires YYYY-MM-DD dates.");
                }
                // Virtual scheme: the browser preview never writes a file.
                return `preview-only://exports/fndr-wrapped-${startDate}-to-${endDate}.pdf`;
            }
            case "open_exported_pdf": {
                const path = requiredString(payload, "path", command);
                if (!path.startsWith("preview-only://exports/") || !path.endsWith(".pdf")) {
                    throw new Error("Preview open_exported_pdf requires a preview-only export path.");
                }
                // Deliberate no-op: never hand a path to the OS from the browser preview.
                return undefined;
            }
            case "get_voice_output_settings":
                return { ...voiceOutputSettings };
            case "set_voice_output_settings": {
                const settings = payloadRecord(payload)?.settings as Partial<VoiceOutputSettings> | undefined;
                if (!settings || typeof settings.provider !== "string" || typeof settings.rate !== "number") {
                    throw new Error("Preview set_voice_output_settings requires provider and rate.");
                }
                voiceOutputSettings = {
                    provider: settings.provider,
                    system_voice: typeof settings.system_voice === "string" ? settings.system_voice : "",
                    rate: Math.min(2, Math.max(0.5, settings.rate)),
                    neural_voice: typeof settings.neural_voice === "string" ? settings.neural_voice : "",
                };
                return { ...voiceOutputSettings };
            }
            case "tts_status":
                // The browser preview has no model; speech falls back to the Mac's voices.
                return { state: "not_installed", detail: null, downloadedBytes: 0, totalBytes: 134_000_000, loaded: false };
            case "tts_voices":
                return {
                    voices: [
                        { id: "af_heart", name: "Heart", accent: "us", gender: "female", grade: "A" },
                        { id: "bf_emma", name: "Emma", accent: "gb", gender: "female", grade: "B-" },
                    ],
                    defaultVoice: "af_heart",
                };
            case "tts_install":
                throw new Error("The browser preview cannot download the on-device voice.");
            case "tts_remove":
                return { state: "not_installed", detail: null, downloadedBytes: 0, totalBytes: 134_000_000, loaded: false };
            case "tts_cancel":
                return null;
            case "tts_synthesize":
                throw new Error("The browser preview has no on-device voice.");
            case "get_screen_guide_settings":
                return { ...screenGuideSettings };
            case "get_screen_guide_diagnostic_status":
                return currentScreenGuideDiagnosticStatus();
            case "arm_screen_guide_diagnostic":
                screenGuideDiagnosticArmExpiresAt = Date.now() + 5 * 60 * 1_000;
                screenGuideDiagnosticStatus = {
                    ...screenGuideDiagnosticStatus,
                    lastResult: null,
                };
                return currentScreenGuideDiagnosticStatus();
            case "delete_screen_guide_diagnostics":
                screenGuideDiagnosticArmExpiresAt = null;
                screenGuideDiagnosticStatus = {
                    bundleCount: 0,
                    partialCount: 0,
                    totalBytes: 0,
                    lastResult: {
                        kind: "deleted",
                        code: "diagnostics_deleted",
                        message: "Preview cleared its synthetic diagnostic state; no files were written.",
                        screenshotSaved: false,
                        ocrSaved: false,
                    },
                };
                return currentScreenGuideDiagnosticStatus();
            case "reveal_screen_guide_diagnostics":
                if (payload !== undefined) {
                    throw new Error("Preview reveal_screen_guide_diagnostics accepts no path.");
                }
                if (
                    screenGuideDiagnosticStatus.bundleCount === 0
                    && screenGuideDiagnosticStatus.partialCount === 0
                ) {
                    throw new Error("There are no Screen Guide diagnostics to reveal.");
                }
                // Deliberate no-op: the browser preview never opens an OS-owned directory.
                return currentScreenGuideDiagnosticStatus();
            case "set_screen_guide_settings": {
                const settings = payloadRecord(payload)?.settings;
                if (
                    typeof settings !== "object"
                    || settings === null
                    || typeof (settings as Record<string, unknown>).enabled !== "boolean"
                    || typeof (settings as Record<string, unknown>).shortcut !== "string"
                    || !(settings as Record<string, unknown>).shortcut
                    || typeof (settings as Record<string, unknown>).speak_responses !== "boolean"
                    || typeof (settings as Record<string, unknown>).show_cursor !== "boolean"
                ) {
                    throw new Error("Preview set_screen_guide_settings requires complete settings.");
                }
                screenGuideSettings = { ...(settings as ScreenGuideSettings) };
                return { ...screenGuideSettings };
            }
            case "screen_guide_press":
                screenGuideGeneration += 1;
                await emitPreviewEventIfAvailable(SCREEN_GUIDE_STATE_EVENT, {
                    phase: "listening",
                    message: "Preview-only listening simulation; no microphone was opened.",
                });
                return screenGuideGeneration;
            case "screen_guide_release": {
                const generation = payloadRecord(payload)?.generation;
                if (
                    typeof generation !== "number"
                    || generation <= 0
                    || generation > screenGuideGeneration
                ) {
                    throw new Error("Preview screen_guide_release requires an active generation.");
                }
                releasedScreenGuideGenerations.add(generation);
                consumeScreenGuideDiagnosticArm();
                await emitPreviewEventIfAvailable(SCREEN_GUIDE_STATE_EVENT, {
                    phase: "answer",
                    message: "Preview-only response ready; no screen was captured.",
                });
                return undefined;
            }
            case "submit_screen_guide_text":
                requiredString(payload, "text", command);
                consumeScreenGuideDiagnosticArm();
                await emitPreviewEventIfAvailable(SCREEN_GUIDE_STATE_EVENT, {
                    phase: "answer",
                    message: "Preview-only response ready; no screen was captured.",
                });
                // No display capture, OCR, or native overlay is started.
                return undefined;
            case "get_runtime_metrics":
                return clonePreview(previewRuntimeMetrics);
            case "get_memory_review_status":
                return { ...previewReviewStatus };
            case "get_privacy_proof":
                return clonePreview(previewPrivacyProof);
            case "get_meeting_status":
                return {
                    is_recording: false,
                    current_meeting_id: null,
                    current_title: null,
                    model: null,
                    started_at: null,
                    ffmpeg_available: true,
                    transcription_backend: "local Whisper",
                    is_analyzing: false,
                    last_error: null,
                };
            case "get_model_download_status":
                return {
                    state: "idle",
                    model_id: null,
                    filename: null,
                    download_url: null,
                    destination_path: null,
                    temp_path: null,
                    bytes_downloaded: 0,
                    total_bytes: 0,
                    percent: 0,
                    done: false,
                    error: null,
                    logs: [],
                    updated_at_ms: previewNow,
                };
            case "get_context_runtime_status":
                return {
                    status: "ready",
                    mcp_running: false,
                    recent_pack_count: 0,
                    activity_event_count: 0,
                    decision_count: 0,
                    failed_writes: 0,
                    latest_pack_tokens_used: 0,
                };
            case "list_recent_context_packs":
                return [];
            case "fndr_subscribe":
            case "fndr_unsubscribe":
                return true;
            case "get_hermes_bridge_status":
                return previewHermesStatus(codexSignedIn, hermesConfigured);
            case "save_hermes_setup":
                hermesConfigured = true;
                return previewHermesStatus(codexSignedIn, hermesConfigured);
            case "list_configured_peers":
                return clonePreview(configuredPeers);
            case "add_configured_peer": {
                const cardUrl = String(payloadRecord(payload)?.cardUrl ?? "");
                const parsed = new URL(cardUrl);
                if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash) {
                    throw new Error("Peer URL must be HTTPS without credentials, query or fragment");
                }
                const saved: ConfiguredPeer = {
                    id: configuredPeers.find((peer) => peer.card_url === cardUrl)?.id ?? `preview-peer-${configuredPeers.length + 1}`,
                    card_url: cardUrl,
                    name: "Preview research peer",
                    endpoint: `${parsed.origin}/a2a`,
                    requires_bearer: false,
                    verified_at_ms: Date.now(),
                };
                configuredPeers = [...configuredPeers.filter((peer) => peer.id !== saved.id), saved];
                return clonePreview(saved);
            }
            case "remove_configured_peer": {
                const id = String(payloadRecord(payload)?.id ?? "");
                const before = configuredPeers.length;
                configuredPeers = configuredPeers.filter((peer) => peer.id !== id);
                return configuredPeers.length !== before;
            }
            case "preview_peer_delegation": {
                const args = payloadRecord(payload);
                const peer = configuredPeers.find((row) => row.id === String(args?.peerId ?? ""));
                if (!peer) throw new Error("Saved peer was not found");
                const task = String(args?.task ?? "").trim();
                const goal = String(args?.outputGoal ?? "").trim();
                const ids = Array.isArray(args?.memoryIds) ? args.memoryIds : [];
                if (!task || !goal) throw new Error("Task and output goal are required");
                if (ids.length > 0) throw new Error("The preview fixture has no durable memory records");
                return { peer_id: peer.id, destination: peer.endpoint, message_text: `Task:\n${task}\n\nOutput goal:\n${goal}`, attachments: [] };
            }
            case "list_peer_runs":
                return clonePreview(previewPeerRuns);
            case "send_peer_delegation": {
                const args = payloadRecord(payload);
                const peer = configuredPeers.find((row) => row.id === String(args?.peerId ?? ""));
                if (!peer) throw new Error("Saved peer was not found");
                const text = `Task:\n${String(args?.task ?? "").trim()}\n\nOutput goal:\n${String(args?.outputGoal ?? "").trim()}`;
                if (args?.reviewedDestination !== peer.endpoint || args?.reviewedText !== text) {
                    throw new Error("Peer task or destination changed. Preview it again before sending");
                }
                const view = { local_id: `preview-run-${previewPeerRuns.length + 1}`, peer_id: peer.id,
                    host: new URL(peer.endpoint).hostname, created_at_ms: Date.now(), payload_bytes: text.length, status: "acknowledged",
                    remote_task_id: `synthetic-task-${previewPeerRuns.length + 1}`, remote_state: "TASK_STATE_SUBMITTED" };
                previewPeerRuns = [...previewPeerRuns, view];
                return { run: { ...view, message_id: "synthetic-message", attachment_ids: [], payload_bytes: text.length }, output_text: null };
            }
            case "refresh_peer_task": {
                const localId = String(payloadRecord(payload)?.localId ?? "");
                const run = previewPeerRuns.find((row) => row.local_id === localId);
                if (!run) throw new Error("Peer run was not found");
                run.remote_state = "TASK_STATE_COMPLETED";
                return { run: clonePreview(run), output_text: "Synthetic peer report for browser preview." };
            }
            case "cancel_peer_task": {
                const localId = String(payloadRecord(payload)?.localId ?? "");
                const run = previewPeerRuns.find((row) => row.local_id === localId);
                if (!run) throw new Error("Peer run was not found");
                run.remote_state = "TASK_STATE_CANCELED";
                return { run: clonePreview(run), output_text: null };
            }
            case "resolve_work_set": {
                // Preview: the newest cards with a reopen target, as one set.
                const items = previewCards
                    .filter((card) => Boolean(card.reopen_target))
                    .slice(0, 3)
                    .map((card) => ({
                        memoryId: card.id,
                        label: card.title,
                        kind: "url" as const,
                        reopenRank: 4,
                        appName: card.app_name,
                        capturedAt: card.timestamp,
                    }));
                return items.length > 0
                    ? { kind: "best", value: { id: "preview-set", title: items[0].label, reason: "Preview", score: 1, items } }
                    : { kind: "none", value: { why: "Nothing FNDR remembers matches that." } };
            }
            case "open_work_set": {
                // Preview never launches anything; it reports what would open.
                const ids = (payloadRecord(payload)?.memoryIds as string[] | undefined) ?? [];
                const layout = payloadRecord(payload)?.layout as string | null | undefined;
                const arrangement = layout
                    ? { arranged: false, layout, detail: "The preview does not move windows." }
                    : undefined;
                return ids.slice(0, 6).map((memoryId) => {
                    const card = previewCards.find((row) => row.id === memoryId);
                    return card
                        ? { memoryId, label: card.title, kind: "url", ok: true, detail: "Opened", outcome: { kind: "opened" }, arrangement }
                        : { memoryId, label: "", ok: false, detail: "FNDR no longer has this memory" };
                });
            }
            case "list_named_sets":
                if (homeState === "loading") return homeWaits();
                if (homeState === "error") throw new Error("Preview: saved sets could not be read.");
                return previewNamedSets.map((set) => previewNamedSet(set));
            case "save_named_set": {
                const args = payloadRecord(payload);
                const name = String(args?.name ?? "").trim();
                if (!name) throw new Error("Give the set a name.");
                if (previewNamedSets.some((set) => set.name.toLowerCase() === name.toLowerCase())) {
                    throw new Error(`A set is already called “${name}”.`);
                }
                const memoryIds = ((args?.memoryIds as string[] | undefined) ?? []).slice(0, 6);
                const saved = { id: `preview-named-${previewNamedSets.length + 1}`, name, memoryIds, savedAt: Date.now() };
                previewNamedSets = [saved, ...previewNamedSets];
                return previewNamedSet(saved);
            }
            case "delete_named_set": {
                const id = String(payloadRecord(payload)?.id ?? "");
                previewNamedSets = previewNamedSets.filter((set) => set.id !== id);
                return null;
            }
            case "routine_offers": {
                if (homeState !== "populated") return [];
                const memoryIds = previewCards.filter((card) => Boolean(card.reopen_target)).slice(0, 2).map((card) => card.id);
                return previewRoutineDismissed || memoryIds.length === 0
                    ? []
                    : [{ id: "preview-routine", label: "Morning standup notes", memoryIds, reason: "Opened on 4 weekdays around 9:00", dueNow: true }];
            }
            case "dismiss_routine_offer":
                previewRoutineDismissed = true;
                return null;
            case "computer_use_status":
                return { enabled: false, codexReady: true, openComputerUsePath: null, active: false };
            case "computer_use_permissions":
                return {
                    accessibility: true,
                    screenRecording: true,
                    automationMedia: null,
                    backend: null,
                    backendReady: null,
                    backendDetail: null,
                };
            case "setup_components":
                return [
                    {
                        id: "codex",
                        name: "Codex",
                        purpose: "Signs in to ChatGPT for Hermes and Notch Do.",
                        required: false,
                        state: "ready",
                        version: "0.45.0",
                        detail: null,
                        action: null,
                        url: null,
                    },
                    {
                        id: "hermes",
                        name: "Hermes Agent",
                        purpose: "Answers questions with the memories you attach.",
                        required: false,
                        state: "missing",
                        version: null,
                        detail: "Not installed yet.",
                        action: "install",
                        url: null,
                    },
                ];
            case "install_component":
                return null;
            case "openclicky_bridge_status":
                return { reachable: false, tokenFound: false, bridgeTokenConfigured: false };
            case "list_agent_chats":
                return agentChats.map((chat) => ({
                    id: chat.id,
                    title: chat.title,
                    updatedAt: chat.updatedAt,
                    messageCount: chat.messages.length,
                }));
            case "get_agent_chat": {
                const id = payloadRecord(payload)?.id;
                return clonePreview(agentChats.find((chat) => chat.id === id) ?? null);
            }
            case "delete_agent_chat": {
                const id = payloadRecord(payload)?.id;
                agentChats = agentChats.filter((chat) => chat.id !== id);
                return null;
            }
            case "send_hermes_message": {
                const record = payloadRecord(payload) ?? {};
                const conversationId = String(record.conversationId ?? "preview-chat");
                const input = String(record.input ?? "");
                const memoryIds = Array.isArray(record.memoryIds) ? (record.memoryIds as string[]) : [];
                const memories = previewCards
                    .filter((card) => memoryIds.includes(card.id))
                    .map((card) => ({ id: card.id, title: card.title, appName: card.app_name, timestamp: card.timestamp }));
                const content = memories.length
                    ? `Using ${memories.length} attached memor${memories.length === 1 ? "y" : "ies"}: ${memories.map((m, i) => `[${i + 1}] ${m.title}`).join(", ")}. Here's a plan for “${input}”.`
                    : `Here's a first pass at “${input}”.`;
                const now = Date.now();
                const existing = agentChats.find((chat) => chat.id === conversationId);
                const chat = existing ?? { id: conversationId, title: input.slice(0, 60), createdAt: now, updatedAt: now, messages: [] };
                chat.messages.push({ role: "user", content: input, at: now, memories });
                chat.messages.push({ role: "assistant", content, at: now + 1, memories: [] });
                chat.updatedAt = now + 1;
                agentChats = [chat, ...agentChats.filter((c) => c.id !== conversationId)];
                return { response_id: `resp-${now}`, conversation_id: conversationId, content };
            }
            case "codex_account_status":
                return clonePreview(codexSignedIn ? previewCodexSignedIn : previewCodexSignedOut);
            case "codex_login_start": {
                codexLoginSeq += 1;
                const loginId = `preview-login-${codexLoginSeq}`;
                if (pendingCodexLogin) clearTimeout(pendingCodexLogin.timer);
                const timer = setTimeout(() => {
                    pendingCodexLogin = null;
                    codexSignedIn = true;
                    void emitPreviewEventIfAvailable(CODEX_LOGIN_COMPLETED_EVENT, { loginId, success: true, error: null });
                }, PREVIEW_CODEX_LOGIN_MS);
                pendingCodexLogin = { loginId, timer };
                return { loginId, authUrl: "about:blank#preview-chatgpt-sign-in" };
            }
            case "codex_login_cancel": {
                const loginId = payloadRecord(payload)?.loginId;
                if (pendingCodexLogin && pendingCodexLogin.loginId === loginId) {
                    clearTimeout(pendingCodexLogin.timer);
                    pendingCodexLogin = null;
                    await emitPreviewEventIfAvailable(CODEX_LOGIN_COMPLETED_EVENT, {
                        loginId,
                        success: false,
                        error: "Sign-in cancelled.",
                    });
                }
                return null;
            }
            case "voice_out_status":
                // The browser preview has no Codex; speech falls back to the Mac's voices.
                return {
                    state: "not_installed",
                    detail: "The browser preview has no Codex.",
                    usedPercent: null,
                    fallbackAbovePercent: 95,
                    connected: false,
                };
            case "voice_out_voices":
                return { voices: [] };
            case "voice_out_start":
                throw new Error("The browser preview has no Codex.");
            case "voice_out_speak":
                throw new Error("The ChatGPT voice is not connected.");
            case "voice_out_cancel":
            case "voice_out_stop":
                return null;
            case "codex_logout":
                codexSignedIn = false;
                return clonePreview(previewCodexSignedOut);
            case "hand_off_status":
                return { toolchain: handOffToolchain, running: null };
            case "hand_off_install_toolchain":
                await new Promise((resolve) => window.setTimeout(resolve, 1200));
                handOffToolchain = "installed";
                return { toolchain: handOffToolchain, running: null };
            case "hand_off_draft": {
                const source = (payloadRecord(payload)?.source ?? {}) as { kind?: string; value?: string };
                const task = tasks.find((candidate) => candidate.id === source.value);
                const text = task?.description.trim() || task?.title || source.value || "Hand this off";
                return handOffDraftFor(text, task?.source_memory_id ?? null, "doc");
            }
            case "hand_off_update_draft": {
                const choice = payloadRecord(payload) ?? {};
                return handOffDraftFor(
                    String(choice.taskText ?? ""),
                    (choice.taskMemoryId as string | null) ?? null,
                    String(choice.goal ?? "doc"),
                    (choice.memoryIds as string[]) ?? [],
                    (choice.fileMemoryIds as string[]) ?? [],
                );
            }
            case "hand_off_start": {
                if (handOffToolchain !== "installed") throw new Error("Install the document tools first (about 200 MB).");
                handOffRunning = true;
                const script: [number, unknown][] = [
                    [700, { kind: "readFile", name: "q4-onboarding-survey.csv" }],
                    [1400, { kind: "ranCode", ok: true, ms: 1200 }],
                    [2100, { kind: "ranCode", ok: true, ms: 900 }],
                    [2800, { kind: "wroteFile", path: "work/readout-spec.json" }],
                    [3500, { kind: "wroteFile", path: "out/q4-onboarding-survey-readout.docx" }],
                    [4200, { kind: "answered", text: "{}" }],
                ];
                for (const [delay, event] of script) {
                    window.setTimeout(() => {
                        if (handOffRunning) void emitPreviewEventIfAvailable("hand_off://progress", event);
                    }, delay);
                }
                window.setTimeout(() => {
                    if (!handOffRunning) return;
                    handOffRunning = false;
                    void emitPreviewEventIfAvailable("hand_off://finished", { record: handOffRecord("ready", null), error: null });
                }, 4800);
                return null;
            }
            case "hand_off_stop":
                if (!handOffRunning) throw new Error("No hand-off is working.");
                handOffRunning = false;
                window.setTimeout(() => {
                    void emitPreviewEventIfAvailable("hand_off://finished", {
                        record: handOffRecord("stopped", "Stopped. What it wrote is kept in the folder."),
                        error: null,
                    });
                }, 300);
                return null;
            case "hand_off_open":
                return null;
            case "hand_off_rate":
                return { ...handOffRecord("ready", null), rating: payloadRecord(payload)?.worked ? "worked" : "did_not_work" };
            default:
                throw new Error(`Unhandled UI preview command: ${command}`);
        }
    };
}

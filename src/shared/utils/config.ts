// Frontend constants. Keep values here when they would otherwise be magic
// numbers, magic strings, or storage keys hardcoded inside component bodies.
// See `docs/architecture/ARCHITECTURE.md` for runtime/Tauri-side config.

/** localStorage keys used across the frontend. */
export const STORAGE_KEYS = {
    theme: "continuum-theme",
    palette: "continuum-palette",
    /** Interactive motion shader background id (see wallpaper-registry). */
    wallpaper: "continuum-wallpaper",
    automations: "continuum-automations",
    searchHistory: "continuum-search-history",
    /** Selected top-level shell mode: "immersive" (scroll experience) or "work" (productive). */
    appMode: "continuum_app_mode",
} as const;

export type StorageKey = (typeof STORAGE_KEYS)[keyof typeof STORAGE_KEYS];

/** Hybrid-search hook tuning. */
export const SEARCH_LIMITS = {
    /** Max memory cards returned from a hybrid query. */
    resultLimit: 12,
    /** Base timeout before adaptive extension is added per word/char. */
    baseTimeoutMs: 6_000,
    /** Per-character timeout extension (capped). */
    perCharBonusMs: 20,
    /** Per-word timeout extension (capped). */
    perWordBonusMs: 450,
    /** Cap on length-based timeout extension. */
    timeoutBonusCapMs: 6_000,
    /** Extra time granted on retry attempts. */
    retryBonusMs: 4_000,
    /** Per-keystroke debounce while typing. */
    typingDebounceMs: 40,
} as const;

/** Search history persistence. */
export const SEARCH_HISTORY = {
    maxEntries: 30,
} as const;

/** Toast lifecycle defaults (App.tsx top-level toaster). */
export const TOAST = {
    /** Default auto-dismiss duration. */
    defaultDurationMs: 8_000,
    /** Maximum toasts displayed at once. */
    stackLimit: 4,
} as const;

/** Background polling cadences. */
export const POLL_INTERVALS = {
    appNamesMs: 30_000,
    clockTickMs: 60_000,
    /** Automation scheduler tick cadence. */
    automationsMs: 60_000,
} as const;

/** Memory `@memory` mention popover. */
export const MEMORY_MENTIONS = {
    /** Minimum trigger length after `@memory `. */
    minQueryLength: 2,
    debounceMs: 320,
    limit: 5,
} as const;

/** Animated search placeholder cycling. */
export const SEARCH_PLACEHOLDER = {
    displayDurationMs: 3000,
    fadeDurationMs: 400,
} as const;

/** Inline AI summary generation in the search bar. */
export const SEARCH_SUMMARY = {
    /** Delay between submit and summary kickoff (lets the result race settle). */
    delayMs: 600,
    /** Coverage floor for a result to contribute to the summary. */
    coverageFloor: 0.3,
    /** Max cards considered. */
    maxCards: 5,
    /** Max snippets sent to the summarizer. */
    maxSnippets: 10,
    /** Per-card evidence cap. */
    snippetsPerCard: 2,
} as const;

/** SearchBar voice-recording tuning (MediaRecorder + microphone input). */
export const VOICE_RECORDING = {
    /** Audio sample rate suggested to the user agent. */
    sampleRate: 48_000,
    channelCount: 1,
    /** MediaRecorder slice for ondataavailable. */
    timesliceMs: 250,
    /** Reject taps shorter than this — usually accidental presses. */
    minDurationMs: 350,
    /** Bitrate for selected MediaRecorder options. */
    audioBitsPerSecond: 128_000,
    /** Auto-clear status messages after this many ms. */
    statusClearMs: 2000,
} as const;

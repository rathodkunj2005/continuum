// Inspired by CC's BashTool permission gating + command dispatch pattern.
// A global Cmd+K palette that surfaces every action in Continuum with fuzzy search,
// keyboard navigation, and a confirmation gate for destructive operations.
import { useCallback, useEffect, useRef, useState } from "react";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import {
    MemoryCard,
    deleteMemory,
    pauseCapture,
    resumeCapture,
    startAgentTask,
} from "@/shared/ipc/tauri";
import { Icon } from "@/shared/components/atoms";
import "./CommandPalette.css";

// ── Types ─────────────────────────────────────────────────────────────────────

type CommandCategory = "navigate" | "smart" | "memory" | "capture" | "destructive";

interface Command {
    id: string;
    label: string;
    description: string;
    category: CommandCategory;
    keywords?: string[];
    // Requires confirmation before executing (mirrors CC's permission gate)
    requiresConfirm?: boolean;
    confirmMessage?: string;
    // Only shown when a memory is selected
    memoryOnly?: boolean;
    // Shown only when no query is active
    globalOnly?: boolean;
    run: (ctx: CommandContext) => void | Promise<void>;
}

interface CommandContext {
    selectedMemory: MemoryCard | null;
    query: string;
    onOpenPanel: (panel: PanelKey) => void;
    onSearch: (q: string) => void;
    onSearchApp: (app: string) => void;
    onClearSearch: () => void;
    onDeleteMemory: (id: string) => void;
    onResearch: (memory: MemoryCard) => void;
    isCapturing: boolean;
}

export type PanelKey =
    | "memoryCards"
    | "knowledgeGraph"
    | "stats"
    | "todo"
    | "meeting"
    | "dailySummary"
    | "pipeline"
    | "engineMetrics"
    | "glassesImport"
    | "searchHistory"
    | "quickSkills"
    | "focusSession"
    | "agent"
    | "automation"
    | "research"
    | "timeTracking"
    | "focusMode";

// ── Command registry ──────────────────────────────────────────────────────────

const COMMANDS: Command[] = [
    // Navigate
    {
        id: "focus-session",
        label: "Focus Session",
        description: "See what you're working on right now",
        category: "navigate",
        keywords: ["context", "current", "active", "now"],
        run: ({ onOpenPanel }) => onOpenPanel("focusSession"),
    },
    {
        id: "memory-cards",
        label: "Memory Vault",
        description: "Browse all memory cards",
        category: "navigate",
        keywords: ["browse", "all", "cards", "memories", "vault"],
        run: ({ onOpenPanel }) => onOpenPanel("memoryCards"),
    },
    {
        id: "knowledge-graph",
        label: "Knowledge Graph",
        description: "Explore hierarchical memory connections",
        category: "navigate",
        keywords: ["graph", "knowledge", "hierarchy", "connections", "second brain"],
        run: ({ onOpenPanel }) => onOpenPanel("knowledgeGraph"),
    },
    {
        id: "daily-summary",
        label: "Daily Summary",
        description: "Generate an AI summary of today's activity",
        category: "navigate",
        keywords: ["summary", "today", "daily", "recap"],
        run: ({ onOpenPanel }) => onOpenPanel("dailySummary"),
    },
    {
        id: "quick-skills",
        label: "Quick Skills",
        description: "Run a pre-built search shortcut",
        category: "smart",
        keywords: ["skill", "shortcut", "preset", "search"],
        run: ({ onOpenPanel }) => onOpenPanel("quickSkills"),
    },
    {
        id: "search-history",
        label: "Search History",
        description: "Re-run a previous query",
        category: "navigate",
        keywords: ["history", "previous", "past", "recent"],
        run: ({ onOpenPanel }) => onOpenPanel("searchHistory"),
    },
    {
        id: "automation",
        label: "Automation Center",
        description: "Configure scheduled tasks and auto-digests",
        category: "smart",
        keywords: ["schedule", "cron", "auto", "digest", "recurring"],
        run: ({ onOpenPanel }) => onOpenPanel("automation"),
    },
    {
        id: "time-tracking",
        label: "Time Tracking",
        description: "See today's screen-time breakdown by app",
        category: "smart",
        keywords: ["time", "usage", "productivity", "apps", "screen"],
        run: ({ onOpenPanel }) => onOpenPanel("timeTracking"),
    },
    {
        id: "focus-mode",
        label: "Focus Mode",
        description: "Set a focus task — Continuum alerts you when you drift",
        category: "smart",
        keywords: ["focus", "drift", "distraction", "task", "goal"],
        run: ({ onOpenPanel }) => onOpenPanel("focusMode"),
    },
    {
        id: "stats",
        label: "Stats",
        description: "View your capture statistics and activity rhythms",
        category: "navigate",
        keywords: ["statistics", "data", "analytics", "usage"],
        run: ({ onOpenPanel }) => onOpenPanel("stats"),
    },
    {
        id: "todo",
        label: "To-Do List",
        description: "View tasks, reminders and follow-ups",
        category: "navigate",
        keywords: ["task", "todo", "reminder", "followup"],
        run: ({ onOpenPanel }) => onOpenPanel("todo"),
    },
    {
        id: "meetings",
        label: "Meeting Recorder",
        description: "Record and transcribe a meeting",
        category: "navigate",
        keywords: ["record", "transcript", "zoom", "call", "audio"],
        run: ({ onOpenPanel }) => onOpenPanel("meeting"),
    },
    {
        id: "pipeline",
        label: "Pipeline Inspector",
        description: "Debug search ranking and embedding pipeline",
        category: "navigate",
        keywords: ["debug", "pipeline", "inspect", "ranking"],
        run: ({ onOpenPanel }) => onOpenPanel("pipeline"),
    },
    {
        id: "engine-metrics",
        label: "Engine metrics (performance)",
        description: "Live latency, RSS, hybrid search timings — use while tuning performance",
        category: "navigate",
        keywords: ["metrics", "performance", "latency", "cpu", "ram", "rss", "profiler", "timing", "hybrid"],
        run: ({ onOpenPanel }) => onOpenPanel("engineMetrics"),
    },
    {
        id: "glasses-photo-import",
        label: "Import glasses / camera photo",
        description: "Visual semantic extraction + OCR evidence + embeddings (requires Qwen3-VL + mmproj)",
        category: "navigate",
        keywords: ["glasses", "meta", "ray-ban", "photo", "image", "import", "camera", "heic", "jpeg"],
        run: ({ onOpenPanel }) => onOpenPanel("glassesImport"),
    },
    // Smart / search
    {
        id: "search-coding",
        label: "Find: Code sessions",
        description: "Search all coding activity",
        category: "smart",
        keywords: ["code", "editor", "programming", "find"],
        run: ({ onSearch }) => onSearch("function class import export const def"),
    },
    {
        id: "search-errors",
        label: "Find: Errors & bugs",
        description: "Surface recent debugging sessions",
        category: "smart",
        keywords: ["error", "bug", "exception", "debug", "traceback"],
        run: ({ onSearch }) => onSearch("error exception failed traceback stack trace"),
    },
    {
        id: "search-meetings",
        label: "Find: Meeting context",
        description: "Search meeting and call captures",
        category: "smart",
        keywords: ["meeting", "zoom", "call", "agenda", "notes"],
        run: ({ onSearch }) => onSearch("meeting zoom meet teams call agenda"),
    },
    {
        id: "search-reading",
        label: "Find: Articles I read",
        description: "Surface browser reading sessions",
        category: "smart",
        keywords: ["article", "blog", "reading", "browser"],
        run: ({ onSearch }) => onSearch("article blog post read reading research"),
    },
    {
        id: "clear-search",
        label: "Clear search",
        description: "Reset the current query and filters",
        category: "navigate",
        keywords: ["reset", "clear", "home"],
        run: ({ onClearSearch }) => onClearSearch(),
    },
    // Capture controls
    {
        id: "pause-capture",
        label: "Pause capture",
        description: "Temporarily stop screen recording",
        category: "capture",
        keywords: ["pause", "stop", "privacy", "incognito"],
        requiresConfirm: true,
        confirmMessage: "Pause screen capture? Continuum will stop recording until you resume.",
        run: async () => { await pauseCapture(); },
    },
    {
        id: "resume-capture",
        label: "Resume capture",
        description: "Resume screen recording",
        category: "capture",
        keywords: ["resume", "start", "record"],
        run: async () => { await resumeCapture(); },
    },
    {
        id: "import-meta-glasses-photo",
        label: "Import Meta glasses photo (same as sidebar)",
        description: "Open the photo import screen with step-by-step instructions",
        category: "capture",
        keywords: ["glasses", "meta", "ray-ban", "photo", "image", "import", "camera"],
        run: ({ onOpenPanel }) => onOpenPanel("glassesImport"),
    },
    // Memory-specific (only shown when a memory is selected)
    {
        id: "open-url",
        label: "Open URL",
        description: "Open this memory's URL in the browser",
        category: "memory",
        memoryOnly: true,
        keywords: ["open", "browser", "link", "url"],
        run: async ({ selectedMemory }) => {
            if (selectedMemory?.url) {
                await shellOpen(selectedMemory.url);
            }
        },
    },
    {
        id: "research-memory",
        label: "Research this",
        description: "Deep-dive with AI agent using this memory's context",
        category: "smart",
        memoryOnly: true,
        keywords: ["research", "agent", "ai", "deep", "analyze"],
        run: ({ selectedMemory, onResearch }) => {
            if (selectedMemory) onResearch(selectedMemory);
        },
    },
    {
        id: "find-similar",
        label: "Find similar memories",
        description: "Search for memories related to this one",
        category: "memory",
        memoryOnly: true,
        keywords: ["similar", "related", "find"],
        run: ({ selectedMemory, onSearch, onSearchApp }) => {
            if (!selectedMemory) return;
            const terms = selectedMemory.title.split(/\s+/).slice(0, 4).join(" ");
            if (terms) onSearch(terms);
            else onSearchApp(selectedMemory.app_name);
        },
    },
    {
        id: "agent-analyze",
        label: "Analyze with AI agent",
        description: "Run AI agent to extract insights from this memory",
        category: "smart",
        memoryOnly: true,
        keywords: ["agent", "analyze", "ai", "insights"],
        run: async ({ selectedMemory, onOpenPanel }) => {
            if (!selectedMemory) return;
            await startAgentTask(
                `Analyze this memory and extract key insights: "${selectedMemory.title}" in ${selectedMemory.app_name}. Summary: ${selectedMemory.summary}`,
                selectedMemory.url ? [selectedMemory.url] : undefined,
                [selectedMemory.summary]
            );
            onOpenPanel("agent");
        },
    },
    {
        id: "copy-memory-text",
        label: "Copy memory text",
        description: "Copy this memory's content to clipboard",
        category: "memory",
        memoryOnly: true,
        keywords: ["copy", "clipboard", "text"],
        run: async ({ selectedMemory }) => {
            if (selectedMemory) {
                await navigator.clipboard.writeText(selectedMemory.summary);
            }
        },
    },
    {
        id: "delete-memory",
        label: "Delete this memory",
        description: "Permanently remove from Continuum",
        category: "destructive",
        memoryOnly: true,
        requiresConfirm: true,
        confirmMessage: "Delete this memory? This cannot be undone.",
        keywords: ["delete", "remove", "forget"],
        run: async ({ selectedMemory, onDeleteMemory }) => {
            if (!selectedMemory) return;
            const ok = await deleteMemory(selectedMemory.id);
            if (ok) onDeleteMemory(selectedMemory.id);
        },
    },
];

const CATEGORY_ORDER: CommandCategory[] = ["navigate", "smart", "memory", "capture", "destructive"];

const CATEGORY_LABELS: Record<CommandCategory, string> = {
    navigate: "Go to",
    smart: "Smart actions",
    memory: "This memory",
    capture: "Capture",
    destructive: "Danger zone",
};

// ── Fuzzy match ───────────────────────────────────────────────────────────────

function score(command: Command, query: string): number {
    if (!query) return 1;
    const q = query.toLowerCase();
    const label = command.label.toLowerCase();
    const desc = command.description.toLowerCase();
    const kw = (command.keywords ?? []).join(" ").toLowerCase();

    if (label.startsWith(q)) return 100;
    if (label.includes(q)) return 80;
    if (kw.includes(q)) return 60;
    if (desc.includes(q)) return 40;

    // Character subsequence match
    let ci = 0;
    for (const ch of q) {
        const idx = label.indexOf(ch, ci);
        if (idx === -1) return 0;
        ci = idx + 1;
    }
    return 20;
}

// ── Component ─────────────────────────────────────────────────────────────────

interface CommandPaletteProps {
    isOpen: boolean;
    onClose: () => void;
    selectedMemory: MemoryCard | null;
    context: Omit<CommandContext, "selectedMemory">;
}

export function CommandPalette({ isOpen, onClose, selectedMemory, context }: CommandPaletteProps) {
    const [query, setQuery] = useState("");
    const [activeIdx, setActiveIdx] = useState(0);
    const [pendingCommand, setPendingCommand] = useState<Command | null>(null);
    const [running, setRunning] = useState<string | null>(null);
    const [feedback, setFeedback] = useState<string | null>(null);
    const inputRef = useRef<HTMLInputElement>(null);
    const listRef = useRef<HTMLDivElement>(null);

    const ctx: CommandContext = { ...context, selectedMemory };

    // Filter + sort commands
    const visible = COMMANDS.filter((cmd) => {
        if (cmd.memoryOnly && !selectedMemory) return false;
        const s = score(cmd, query);
        return s > 0;
    }).sort((a, b) => score(b, query) - score(a, query));

    // Group by category (only show categories that have results)
    const groups: { category: CommandCategory; commands: Command[] }[] = [];
    for (const cat of CATEGORY_ORDER) {
        const cmds = visible.filter((c) => c.category === cat);
        if (cmds.length > 0) groups.push({ category: cat, commands: cmds });
    }

    const flatVisible = groups.flatMap((g) => g.commands);

    // Reset state on open
    useEffect(() => {
        if (isOpen) {
            setQuery("");
            setActiveIdx(0);
            setPendingCommand(null);
            setFeedback(null);
            setTimeout(() => inputRef.current?.focus(), 30);
        }
    }, [isOpen]);

    // Keep activeIdx in bounds
    useEffect(() => {
        setActiveIdx((prev) => Math.min(prev, Math.max(flatVisible.length - 1, 0)));
    }, [flatVisible.length]);

    // Scroll active item into view
    useEffect(() => {
        const active = listRef.current?.querySelector(`[data-idx="${activeIdx}"]`);
        active?.scrollIntoView({ block: "nearest" });
    }, [activeIdx]);

    const runCommand = useCallback(async (cmd: Command) => {
        setRunning(cmd.id);
        try {
            await cmd.run(ctx);
            setFeedback(`Done: ${cmd.label}`);
            setTimeout(() => {
                setFeedback(null);
                onClose();
            }, 600);
        } catch (err) {
            console.error("Command failed:", err);
            const msg = err instanceof Error ? err.message : String(err);
            setFeedback(`Failed: ${msg}`);
            setTimeout(() => setFeedback(null), 2500);
        } finally {
            setRunning(null);
        }
    }, [ctx, onClose]);

    const executeCommand = useCallback(async (cmd: Command) => {
        if (cmd.requiresConfirm) {
            setPendingCommand(cmd);
            return;
        }
        await runCommand(cmd);
    }, [runCommand]);

    const confirmAndRun = useCallback(async () => {
        if (!pendingCommand) return;
        const cmd = pendingCommand;
        setPendingCommand(null);
        await runCommand(cmd);
    }, [pendingCommand, runCommand]);

    const handleKey = useCallback((e: React.KeyboardEvent) => {
        if (e.key === "Escape") {
            if (pendingCommand) { setPendingCommand(null); return; }
            onClose();
            return;
        }
        if (e.key === "ArrowDown") {
            e.preventDefault();
            setActiveIdx((i) => Math.min(i + 1, flatVisible.length - 1));
        }
        if (e.key === "ArrowUp") {
            e.preventDefault();
            setActiveIdx((i) => Math.max(i - 1, 0));
        }
        if (e.key === "Enter") {
            e.preventDefault();
            if (pendingCommand) { void confirmAndRun(); return; }
            const cmd = flatVisible[activeIdx];
            if (cmd) void executeCommand(cmd);
        }
    }, [activeIdx, flatVisible, pendingCommand, confirmAndRun, executeCommand, onClose]);

    if (!isOpen) return null;

    return (
        <div className="cp-overlay" onClick={onClose}>
            <div
                className="cp-modal"
                onClick={(e) => e.stopPropagation()}
                onKeyDown={handleKey}
            >
                {/* Search bar */}
                <div className="cp-search-row">
                    <span className="cp-search-icon">⌘</span>
                    <input
                        ref={inputRef}
                        className="cp-input"
                        placeholder={selectedMemory ? `Actions for "${selectedMemory.title.slice(0, 40)}…"` : "find a memory"}
                        value={query}
                        onChange={(e) => { setQuery(e.target.value); setActiveIdx(0); }}
                    />
                    <kbd className="cp-esc-hint">ESC</kbd>
                </div>

                {/* Memory context banner */}
                {selectedMemory && (
                    <div className="cp-context-banner">
                        <span className="cp-context-icon"><Icon name="sparkles" size={14} /></span>
                        <span className="cp-context-text">
                            {selectedMemory.app_name} · {selectedMemory.title.slice(0, 55)}{selectedMemory.title.length > 55 ? "…" : ""}
                        </span>
                    </div>
                )}

                {/* Permission confirmation gate (mirrors CC's BashTool confirm flow) */}
                {pendingCommand && (
                    <div className="cp-confirm-gate">
                        <div className="cp-confirm-icon">{pendingCommand.category}</div>
                        <div className="cp-confirm-body">
                            <p className="cp-confirm-message">{pendingCommand.confirmMessage ?? `Run "${pendingCommand.label}"?`}</p>
                            <div className="cp-confirm-actions">
                                <button className="cp-confirm-btn cp-confirm-cancel" onClick={() => setPendingCommand(null)}>
                                    Cancel
                                </button>
                                <button
                                    className={`cp-confirm-btn cp-confirm-run ${pendingCommand.category === "destructive" ? "danger" : ""}`}
                                    onClick={() => void confirmAndRun()}
                                >
                                    {pendingCommand.category === "destructive" ? "Delete" : "Confirm"}
                                </button>
                            </div>
                        </div>
                    </div>
                )}

                {/* Feedback flash */}
                {feedback && (
                    <div className="cp-feedback">{feedback}</div>
                )}

                {/* Results */}
                {!pendingCommand && !feedback && (
                    <div className="cp-results" ref={listRef}>
                        {flatVisible.length === 0 && (
                            <div className="cp-empty">No commands match "{query}"</div>
                        )}
                        {groups.map(({ category, commands }) => (
                            <div key={category} className="cp-group">
                                <div className="cp-group-label">{CATEGORY_LABELS[category]}</div>
                                {commands.map((cmd) => {
                                    const idx = flatVisible.indexOf(cmd);
                                    const isActive = idx === activeIdx;
                                    return (
                                        <button
                                            key={cmd.id}
                                            data-idx={idx}
                                            className={`cp-item ${isActive ? "active" : ""} ${cmd.category === "destructive" ? "danger" : ""} ${running === cmd.id ? "running" : ""}`}
                                            onClick={() => void executeCommand(cmd)}
                                            onMouseEnter={() => setActiveIdx(idx)}
                                        >
                                            <span className="cp-item-body">
                                                <span className="cp-item-label">{cmd.label}</span>
                                                <span className="cp-item-desc">{cmd.description}</span>
                                            </span>
                                            {running === cmd.id && (
                                                <span className="cp-item-spinner" />
                                            )}
                                            {isActive && !running && (
                                                <kbd className="cp-item-enter">↵</kbd>
                                            )}
                                        </button>
                                    );
                                })}
                            </div>
                        ))}
                    </div>
                )}

                <div className="cp-footer">
                    <div className="cp-footer-nav">
                        <span><kbd>↑↓</kbd> navigate</span>
                        <span><kbd>↵</kbd> open</span>
                        <span><kbd>esc</kbd> close</span>
                    </div>
                    <span className="cp-footer-local">Local Only</span>
                </div>
            </div>
        </div>
    );
}

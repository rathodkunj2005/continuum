import type { PanelKey } from "@/domains/command-palette/CommandPalette";

const SIDEBAR_GROUPS = [
    {
        label: "Features",
        items: [
            { key: "memoryCards", text: "Memory Vault" },
            { key: "knowledgeGraph", text: "Knowledge Graph" },
            { key: "engineMetrics", text: "Engine metrics" },
            { key: "glassesImport", text: "Glasses photo import" },
            { key: "stats", text: "Stats" },
            { key: "todo", text: "To Do" },
            { key: "meeting", text: "Meetings" },
            { key: "dailySummary", text: "Daily Summary" },
            { key: "agent", text: "Agent" },
            { key: "pipeline", text: "Pipeline Inspector" },
        ],
    },
    {
        label: "Smart",
        items: [
            { key: "focusSession", text: "Focus Session" },
            { key: "quickSkills", text: "Quick Skills" },
            { key: "searchHistory", text: "Search History" },
            { key: "automation", text: "Automation" },
            { key: "research", text: "Research" },
            { key: "timeTracking", text: "Time Tracking" },
            { key: "focusMode", text: "Focus Mode" },
        ],
    },
] as const satisfies ReadonlyArray<{
    label: string;
    items: ReadonlyArray<{ key: PanelKey; text: string }>;
}>;

interface AppSidebarProps {
    activePanel: PanelKey | null;
    isOpen: boolean;
    onTogglePanel: (panel: PanelKey) => void;
    onOpenCommandPalette: () => void;
    onCloseSidebar?: () => void;
}

export function AppSidebar({
    activePanel,
    isOpen,
    onTogglePanel,
    onOpenCommandPalette,
    onCloseSidebar,
}: AppSidebarProps) {
    return (
        <aside className={`left-sidebar ${isOpen ? "open" : ""}`}>
            <div className="sidebar-brand">
                <button
                    className="sidebar-collapse-btn"
                    onClick={() => onCloseSidebar?.()}
                    aria-label="Close sidebar"
                    title="Close sidebar"
                >
                    ✕
                </button>
            </div>

            {SIDEBAR_GROUPS.map((group) => (
                <div key={group.label} className="sidebar-group sidebar-actions">
                    <div className="sidebar-label">{group.label}</div>
                    {group.items.map(({ key, text }) => (
                        <button
                            key={key}
                            className={`ui-action-btn ${activePanel === key ? "active" : ""}`}
                            onClick={() => onTogglePanel(key)}
                        >
                            {text}
                        </button>
                    ))}
                </div>
            ))}

            <div className="sidebar-group sidebar-actions">
                <div className="sidebar-label">Commands</div>
                <button className="ui-action-btn" onClick={onOpenCommandPalette}>
                    Cmd+K Palette
                </button>
            </div>
        </aside>
    );
}

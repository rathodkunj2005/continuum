import { useState } from "react";
import type { FilterOptions } from "./graph/graphFilterOptions";
import type { GraphFilterState } from "./graph/graphFilters";

export interface KnowledgeGraphTopBarProps {
    options: FilterOptions;
    filters: GraphFilterState;
    onChange: (next: GraphFilterState) => void;
    nodeCount: number;
    edgeCount: number;
}

type MenuKey = "nodeTypes" | "projects" | "topics" | "edgeKinds";

function toggle<T>(set: ReadonlySet<T> | null, value: T): ReadonlySet<T> | null {
    if (!set) return new Set([value]);
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next.size === 0 ? null : next;
}

interface FilterMenuProps {
    label: string;
    open: boolean;
    onOpen: (open: boolean) => void;
    options: readonly string[];
    active: ReadonlySet<string> | null;
    onToggle: (value: string) => void;
}

function FilterMenu({ label, open, onOpen, options, active, onToggle }: FilterMenuProps) {
    const count = active?.size ?? 0;
    return (
        <div className="kg-topbar-menu">
            <button
                type="button"
                className={`kg-topbar-pill${count > 0 ? " kg-topbar-pill-active" : ""}`}
                onClick={() => onOpen(!open)}
                aria-haspopup="listbox"
                aria-expanded={open}
            >
                {label}
                {count > 0 ? ` · ${count}` : ""}
            </button>
            {open && (
                <ul className="kg-topbar-menu-list" role="listbox">
                    {options.map((v) => (
                        <li key={v}>
                            <label className="kg-topbar-menu-item">
                                <input
                                    type="checkbox"
                                    checked={active?.has(v) ?? false}
                                    onChange={() => onToggle(v)}
                                />
                                <span>{v}</span>
                            </label>
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
}

export function KnowledgeGraphTopBar({
    options,
    filters,
    onChange,
    nodeCount,
    edgeCount,
}: KnowledgeGraphTopBarProps) {
    const [openMenu, setOpenMenu] = useState<MenuKey | null>(null);

    const reset = () =>
        onChange({
            nodeTypes: null,
            projects: null,
            topics: null,
            edgeKinds: null,
            minConfidence: 0,
        });
    const activeCount =
        (filters.nodeTypes?.size ?? 0) +
        (filters.projects?.size ?? 0) +
        (filters.topics?.size ?? 0) +
        (filters.edgeKinds?.size ?? 0) +
        (filters.minConfidence > 0 ? 1 : 0);

    const open = (key: MenuKey) => (v: boolean) => setOpenMenu(v ? key : null);

    return (
        <div className="kg-topbar">
            <div className="kg-topbar-left">
                <div className="kg-topbar-title">memory graph</div>
                <div className="kg-topbar-meta">
                    {nodeCount} frames · {edgeCount} threads
                </div>
            </div>
            <div className="kg-topbar-filters">
                {options.nodeTypes.length > 0 && (
                    <FilterMenu
                        label="type"
                        open={openMenu === "nodeTypes"}
                        onOpen={open("nodeTypes")}
                        options={options.nodeTypes}
                        active={filters.nodeTypes}
                        onToggle={(v) =>
                            onChange({ ...filters, nodeTypes: toggle(filters.nodeTypes, v) })
                        }
                    />
                )}
                {options.projects.length > 0 && (
                    <FilterMenu
                        label="project"
                        open={openMenu === "projects"}
                        onOpen={open("projects")}
                        options={options.projects}
                        active={filters.projects}
                        onToggle={(v) =>
                            onChange({ ...filters, projects: toggle(filters.projects, v) })
                        }
                    />
                )}
                {options.topics.length > 0 && (
                    <FilterMenu
                        label="topic"
                        open={openMenu === "topics"}
                        onOpen={open("topics")}
                        options={options.topics}
                        active={filters.topics}
                        onToggle={(v) =>
                            onChange({ ...filters, topics: toggle(filters.topics, v) })
                        }
                    />
                )}
                {options.edgeKinds.length > 0 && (
                    <FilterMenu
                        label="edge"
                        open={openMenu === "edgeKinds"}
                        onOpen={open("edgeKinds")}
                        options={options.edgeKinds}
                        active={filters.edgeKinds}
                        onToggle={(v) =>
                            onChange({ ...filters, edgeKinds: toggle(filters.edgeKinds, v) })
                        }
                    />
                )}
                <label className="kg-topbar-confidence">
                    <span className="kg-topbar-confidence-label">min conf</span>
                    <input
                        type="range"
                        min={0}
                        max={1}
                        step={0.05}
                        value={filters.minConfidence}
                        onChange={(e) =>
                            onChange({ ...filters, minConfidence: parseFloat(e.target.value) })
                        }
                    />
                    <span className="kg-topbar-confidence-value">
                        {filters.minConfidence.toFixed(2)}
                    </span>
                </label>
                {activeCount > 0 && (
                    <button type="button" className="kg-topbar-reset" onClick={reset}>
                        clear · {activeCount}
                    </button>
                )}
            </div>
        </div>
    );
}

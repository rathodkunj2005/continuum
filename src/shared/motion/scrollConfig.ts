/**
 * Section ordering and heights for the immersive scroll experience.
 *
 * `id` matches the section DOM element id and is used by ChapterRail,
 * keyboard nav, and the scroll progress indicator.
 *
 * `height` is the canonical layout height — `"100vh"` for full-viewport
 * sections, `"auto"` for content-fit, or a multiplier of viewport for
 * sticky scroll budgets (e.g. GraphSection at 250vh keeps the canvas
 * pinned for ~1.5 extra viewport-heights of scroll).
 */

export type ImmersiveSectionId =
    | "hero"
    | "capture"
    | "timeline"
    | "search"
    | "graph"
    | "agent"
    | "privacy"
    | "workspace";

export interface ImmersiveSectionConfig {
    id: ImmersiveSectionId;
    /** Short label for ChapterRail tooltip and screen-reader name. */
    label: string;
    /** Canonical CSS height for the section's outer wrapper. */
    height: string;
    /** Extra scroll budget while sticky-pinned (px). 0 = no sticky. */
    stickyBudget?: number;
}

export const IMMERSIVE_SECTIONS: readonly ImmersiveSectionConfig[] = [
    { id: "hero", label: "Hero", height: "100vh" },
    { id: "capture", label: "Capture", height: "100vh" },
    { id: "timeline", label: "Timeline", height: "auto" },
    { id: "search", label: "Search", height: "100vh" },
    { id: "graph", label: "Graph", height: "100vh", stickyBudget: 1200 },
    { id: "agent", label: "Agent", height: "100vh" },
    { id: "privacy", label: "Privacy", height: "60vh" },
    { id: "workspace", label: "Workspace", height: "auto" },
] as const;

/** Map section index (0-based) to its id, used by 1–8 keyboard shortcuts. */
export const SECTION_BY_INDEX: ReadonlyMap<number, ImmersiveSectionId> = new Map(
    IMMERSIVE_SECTIONS.map((s, i) => [i, s.id])
);

/** IntersectionObserver options used by ChapterRail to track the active section. */
export const SECTION_OBSERVER_OPTIONS: IntersectionObserverInit = {
    rootMargin: "-40% 0px -40% 0px",
    threshold: [0, 0.25, 0.5, 0.75, 1],
};

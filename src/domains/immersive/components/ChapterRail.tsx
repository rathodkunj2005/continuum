import { useEffect, useState } from "react";
import {
    IMMERSIVE_SECTIONS,
    SECTION_OBSERVER_OPTIONS,
    type ImmersiveSectionId,
} from "@/shared/motion";
import "./ChapterRail.css";

interface ChapterRailProps {
    onEnterWorkMode: () => void;
}

/**
 * Floating left rail showing active section. Active = amber dot with
 * halation glow. Hover reveals a mono tooltip on the right.
 */
export function ChapterRail({ onEnterWorkMode }: ChapterRailProps) {
    const [activeId, setActiveId] = useState<ImmersiveSectionId>(IMMERSIVE_SECTIONS[0].id);

    useEffect(() => {
        const observer = new IntersectionObserver((entries) => {
            // Pick the section with the largest visible area — handles
            // overlapping observer hits cleanly.
            let best: { id: ImmersiveSectionId; ratio: number } | null = null;
            for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                const id = (entry.target as HTMLElement).dataset.sectionId as ImmersiveSectionId | undefined;
                if (!id) continue;
                if (!best || entry.intersectionRatio > best.ratio) {
                    best = { id, ratio: entry.intersectionRatio };
                }
            }
            if (best) setActiveId(best.id);
        }, SECTION_OBSERVER_OPTIONS);

        const elements: HTMLElement[] = [];
        for (const section of IMMERSIVE_SECTIONS) {
            const el = document.getElementById(`fndr-section-${section.id}`);
            if (el) {
                observer.observe(el);
                elements.push(el);
            }
        }
        return () => {
            for (const el of elements) observer.unobserve(el);
            observer.disconnect();
        };
    }, []);

    const jumpTo = (id: ImmersiveSectionId) => {
        const el = document.getElementById(`fndr-section-${id}`);
        el?.scrollIntoView({ behavior: "smooth", block: "start" });
    };

    return (
        <nav className="fndr-chapter-rail" aria-label="Sections">
            <ol className="fndr-chapter-list">
                {IMMERSIVE_SECTIONS.map((section, idx) => {
                    const active = section.id === activeId;
                    return (
                        <li key={section.id} className="fndr-chapter-item">
                            <button
                                type="button"
                                className={`fndr-chapter-btn ${active ? "is-active" : ""}`}
                                aria-label={`Go to ${section.label} (press ${idx + 1})`}
                                aria-current={active ? "true" : undefined}
                                onClick={() => jumpTo(section.id)}
                            >
                                <span className="fndr-chapter-bar" />
                                <span className="fndr-chapter-tooltip">{section.label}</span>
                            </button>
                        </li>
                    );
                })}
            </ol>

            <button
                type="button"
                className="fndr-chapter-mode-btn"
                aria-label="Enter work mode (Cmd+Period)"
                onClick={onEnterWorkMode}
                title="Enter work mode (⌘.)"
            >
                <span aria-hidden="true">⌘.</span>
            </button>
        </nav>
    );
}

export default ChapterRail;

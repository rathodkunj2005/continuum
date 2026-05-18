import type { CSSProperties, ReactNode } from "react";

interface StickySceneProps {
    /** Extra scroll distance (px) over which children remain sticky-pinned. */
    scrollBudget: number;
    /** Section id used for IntersectionObserver tracking + keyboard jumps. */
    sectionId: string;
    /** Optional className for the inner sticky wrapper. */
    className?: string;
    /** Optional outer wrapper style hooks. */
    style?: CSSProperties;
    children: ReactNode;
}

/**
 * Sticky pinning wrapper. Outer div reserves `100vh + scrollBudget` of
 * scroll height; inner div is `position: sticky` for one viewport. As the
 * user scrolls through the budget, children stay pinned, allowing
 * scroll-progress-driven motion inside them.
 */
export function StickyScene({
    scrollBudget,
    sectionId,
    className,
    style,
    children,
}: StickySceneProps) {
    const totalHeight = `calc(100vh + ${scrollBudget}px)`;

    return (
        <section
            id={`fndr-section-${sectionId}`}
            data-section-id={sectionId}
            className="fndr-sticky-scene"
            style={{ position: "relative", height: totalHeight, ...style }}
        >
            <div
                className={`fndr-sticky-scene-inner ${className ?? ""}`}
                style={{
                    position: "sticky",
                    top: 0,
                    height: "100vh",
                    width: "100%",
                    overflow: "hidden",
                }}
            >
                {children}
            </div>
        </section>
    );
}

export default StickyScene;

import { useEffect, useRef, type ReactNode } from "react";
import { useReducedMotionSafe } from "@/shared/motion";

interface ParallaxLayerProps {
    /**
     * Parallax speed multiplier.
     *   0   = scroll with the page (no transform)
     *   0.5 = move at half page speed
     *   1   = pinned (move opposite to page scroll)
     *
     * Values outside [0, 1] are accepted but rarely look right.
     */
    speed: number;
    /** Optional fixed offset added to the computed translateY. */
    offset?: number;
    /** Scroll container to listen on. Defaults to window. */
    scrollRoot?: HTMLElement | null;
    className?: string;
    children: ReactNode;
}

/**
 * Thin wrapper that translates its child on the Y axis based on the
 * scroll position of either the window or a supplied scroll container.
 * Uses rAF + will-change so multiple instances coexist cheaply.
 */
export function ParallaxLayer({
    speed,
    offset = 0,
    scrollRoot,
    className,
    children,
}: ParallaxLayerProps) {
    const ref = useRef<HTMLDivElement>(null);
    const { reduced } = useReducedMotionSafe();

    useEffect(() => {
        if (reduced) return;
        const node = ref.current;
        if (!node) return;

        const source: HTMLElement | Window = scrollRoot ?? window;
        let raf = 0;

        const compute = () => {
            const sy =
                source instanceof Window
                    ? source.scrollY
                    : (source as HTMLElement).scrollTop;
            const ty = sy * speed + offset;
            node.style.transform = `translate3d(0, ${ty}px, 0)`;
            raf = 0;
        };

        const onScroll = () => {
            if (raf !== 0) return;
            raf = requestAnimationFrame(compute);
        };

        node.style.willChange = "transform";
        compute();
        source.addEventListener("scroll", onScroll as EventListener, { passive: true });

        return () => {
            source.removeEventListener("scroll", onScroll as EventListener);
            node.style.willChange = "";
            if (raf !== 0) cancelAnimationFrame(raf);
        };
    }, [speed, offset, scrollRoot, reduced]);

    return (
        <div ref={ref} className={className} style={{ position: "relative" }}>
            {children}
        </div>
    );
}

export default ParallaxLayer;

import { useEffect, useState, type RefObject } from "react";
import { useReducedMotionSafe } from "@/shared/motion";

interface ScrollProgressIndicatorProps {
    /** Ref to the scroll container the indicator should track. */
    targetRef: RefObject<HTMLElement>;
}

/**
 * 2px top bar showing scroll progress through the immersive container.
 * Reads via rAF to avoid setState on every scroll pixel.
 */
export function ScrollProgressIndicator({ targetRef }: ScrollProgressIndicatorProps) {
    const [progress, setProgress] = useState(0);
    const { reduced } = useReducedMotionSafe();

    useEffect(() => {
        const el = targetRef.current;
        if (!el) return;

        let raf = 0;
        const update = () => {
            const max = el.scrollHeight - el.clientHeight;
            const next = max > 0 ? el.scrollTop / max : 0;
            setProgress(next);
            raf = 0;
        };
        const onScroll = () => {
            if (raf !== 0) return;
            raf = requestAnimationFrame(update);
        };

        el.addEventListener("scroll", onScroll, { passive: true });
        update();
        return () => {
            el.removeEventListener("scroll", onScroll);
            if (raf !== 0) cancelAnimationFrame(raf);
        };
    }, [targetRef]);

    return (
        <div
            className="fndr-scroll-progress"
            role="progressbar"
            aria-label="Scroll progress"
            aria-valuenow={Math.round(progress * 100)}
            aria-valuemin={0}
            aria-valuemax={100}
            style={{
                position: "fixed",
                top: 0,
                left: 0,
                right: 0,
                height: 2,
                background: "var(--hairline)",
                zIndex: 40,
                pointerEvents: "none",
            }}
        >
            <div
                style={{
                    width: `${progress * 100}%`,
                    height: "100%",
                    background: "var(--accent)",
                    boxShadow: reduced ? "none" : "0 0 8px var(--accent)",
                    transition: reduced ? "none" : "width 80ms linear",
                }}
            />
        </div>
    );
}

export default ScrollProgressIndicator;

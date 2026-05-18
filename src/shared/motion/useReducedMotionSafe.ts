import { useReducedMotion } from "framer-motion";

/**
 * Wraps Framer Motion's `useReducedMotion` with helpers that collapse
 * durations and transitions to zero when the user prefers reduced motion.
 *
 * Use at the top of any immersive component that drives motion in JS.
 * CSS-side reduced-motion is handled by the @media query in
 * `film-paper.css` under `.fndr-immersive-root`.
 */
export function useReducedMotionSafe() {
    const reduced = useReducedMotion();

    return {
        reduced: reduced ?? false,
        /** Returns 0 when reduced, otherwise the requested ms. */
        duration: (ms: number): number => (reduced ? 0 : ms),
        /** Wraps a Framer Motion transition; collapses to `{ duration: 0 }` when reduced. */
        transition: <T extends object>(t: T): T | { duration: 0 } =>
            reduced ? { duration: 0 } : t,
        /** Picks one of two values based on reduced-motion. */
        pick: <T>(full: T, fallback: T): T => (reduced ? fallback : full),
    };
}

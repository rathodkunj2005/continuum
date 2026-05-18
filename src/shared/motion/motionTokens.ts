/**
 * Centralised motion values for the immersive scroll experience.
 *
 * Durations are in ms (Framer Motion expects seconds — divide by 1000 at the
 * call site or use the helper `s(ms)` below). Eases mirror the CSS custom
 * properties in `film-paper.css` (`--film-ease-*`) so CSS and JS motion stay
 * visually aligned.
 */

export const motion = {
    ease: {
        // Primary easing for almost all UI transitions
        shutter: [0.22, 1, 0.36, 1] as const,
        // Reveals, page-in
        develop: [0.65, 0, 0.35, 1] as const,
        // Used sparingly — small overshoot
        iris: [0.34, 1.56, 0.64, 1] as const,
    },
    dur: {
        fast: 180,
        base: 320,
        slow: 680,
        cinema: 1200,
    },
    spring: {
        snappy: { type: "spring", stiffness: 400, damping: 30 } as const,
        gentle: { type: "spring", stiffness: 200, damping: 28 } as const,
        reveal: { type: "spring", stiffness: 120, damping: 20 } as const,
    },
} as const;

/** Convert ms to seconds for Framer Motion `transition.duration`. */
export const s = (ms: number): number => ms / 1000;

export type MotionEase = keyof typeof motion.ease;
export type MotionDur = keyof typeof motion.dur;
export type MotionSpring = keyof typeof motion.spring;

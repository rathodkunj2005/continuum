import type { Variants } from "framer-motion";
import { motion, s } from "./motionTokens";

/** Fade in + rise from 16px below. Use for section/headline reveals. */
export const fadeUp: Variants = {
    hidden: { opacity: 0, y: 16 },
    visible: {
        opacity: 1,
        y: 0,
        transition: { duration: s(motion.dur.slow), ease: motion.ease.shutter },
    },
    exit: { opacity: 0, y: -8, transition: { duration: s(motion.dur.base) } },
};

/** Plain opacity fade. */
export const fadeIn: Variants = {
    hidden: { opacity: 0 },
    visible: { opacity: 1, transition: { duration: s(motion.dur.base) } },
};

/** Slide in from the right by 32px. */
export const slideFromRight: Variants = {
    hidden: { opacity: 0, x: 32 },
    visible: {
        opacity: 1,
        x: 0,
        transition: { duration: s(motion.dur.slow), ease: motion.ease.shutter },
    },
};

/** Slide in from the left by 32px. */
export const slideFromLeft: Variants = {
    hidden: { opacity: 0, x: -32 },
    visible: {
        opacity: 1,
        x: 0,
        transition: { duration: s(motion.dur.slow), ease: motion.ease.shutter },
    },
};

/** Iris-style scale reveal — small bounce. Use sparingly. */
export const scaleReveal: Variants = {
    hidden: { opacity: 0, scale: 0.96 },
    visible: {
        opacity: 1,
        scale: 1,
        transition: { duration: s(motion.dur.base), ease: motion.ease.iris },
    },
};

/** Stagger child variants by 80ms each. */
export const staggerContainer: Variants = {
    hidden: {},
    visible: { transition: { staggerChildren: 0.08 } },
};

/** Faster stagger for tight sequences (e.g. mono lines, tool trace rows). */
export const staggerFast: Variants = {
    hidden: {},
    visible: { transition: { staggerChildren: 0.04 } },
};

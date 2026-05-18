import type { Transition } from "framer-motion";
import { motion, s } from "./motionTokens";

/** Quick hover-style transition. */
export const tFast: Transition = {
    duration: s(motion.dur.fast),
    ease: motion.ease.shutter,
};

/** Standard transition for most reveals. */
export const tBase: Transition = {
    duration: s(motion.dur.base),
    ease: motion.ease.shutter,
};

/** Slow, deliberate transition — section reveals, big morphs. */
export const tSlow: Transition = {
    duration: s(motion.dur.slow),
    ease: motion.ease.shutter,
};

/** Cinematic full-page transition. Use sparingly. */
export const tCinema: Transition = {
    duration: s(motion.dur.cinema),
    ease: motion.ease.develop,
};

/** Snappy spring — for chip toggles, button presses. */
export const tSpringSnappy: Transition = motion.spring.snappy;

/** Gentle spring — for cards, panel slides. */
export const tSpringGentle: Transition = motion.spring.gentle;

/** Soft reveal spring — for hero copy, deliberate entrances. */
export const tSpringReveal: Transition = motion.spring.reveal;

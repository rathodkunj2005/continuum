import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { staggerContainer, fadeUp, fadeIn, useReducedMotionSafe, s, motionTokens } from "@/shared/motion";
import { getStatus, type CaptureStatus } from "@/shared/ipc/tauri";
import "./HeroSection.css";

interface HeroSectionProps {
    onEnterReel: () => void;
    onEnterWorkMode: () => void;
}

/**
 * Full-viewport opening section. Title pair "Memory, / developed."
 * with a mono "REEL 0421" label, two CTAs, and a faint frames-today
 * numeral as decoration. Background carries 6 drifting constellation
 * dots at low opacity (CSS keyframes — no Framer Motion).
 */
export function HeroSection({ onEnterReel, onEnterWorkMode }: HeroSectionProps) {
    const { reduced, transition } = useReducedMotionSafe();
    const [status, setStatus] = useState<CaptureStatus | null>(null);

    useEffect(() => {
        let mounted = true;
        getStatus()
            .then((s_) => {
                if (mounted) setStatus(s_);
            })
            .catch(() => {
                // Non-fatal: hero falls back to no stat numeral.
            });
        return () => {
            mounted = false;
        };
    }, []);

    const today = new Date();
    const reelNumber = Math.floor((today.getTime() / 86_400_000) % 10000)
        .toString()
        .padStart(4, "0");
    const frameCount = status?.frames_captured ?? null;

    return (
        <section
            id="fndr-section-hero"
            data-section-id="hero"
            className="fndr-section fndr-hero-section film-grain"
        >
            <HeroConstellation reduced={reduced} />

            <motion.div
                className="fndr-hero-content"
                variants={staggerContainer}
                initial="hidden"
                animate="visible"
                transition={transition({ delayChildren: s(motionTokens.dur.fast) })}
            >
                <motion.p className="fndr-mono-label" variants={fadeUp}>
                    REEL {reelNumber}
                </motion.p>

                <motion.h1 className="fndr-hero-title" variants={fadeUp}>
                    Memory,
                    <br />
                    <em>developed.</em>
                </motion.h1>

                <motion.p className="fndr-hero-subtitle" variants={fadeUp}>
                    {frameCount === null
                        ? "Your work — quietly indexed, locally."
                        : frameCount === 0
                        ? "Nothing here yet. Start working — FNDR is watching the room."
                        : `${frameCount.toLocaleString()} frames from today.`}
                    <br />
                    Nothing left the room.
                </motion.p>

                <motion.div className="fndr-hero-ctas" variants={fadeUp}>
                    <button
                        type="button"
                        className="fndr-btn fndr-btn--primary"
                        onClick={onEnterReel}
                    >
                        Enter the reel
                    </button>
                    <button type="button" className="fndr-btn" onClick={onEnterWorkMode}>
                        Open work mode
                    </button>
                </motion.div>
            </motion.div>

            {frameCount !== null && frameCount > 0 && (
                <motion.div
                    className="fndr-hero-stat"
                    variants={fadeIn}
                    initial="hidden"
                    animate="visible"
                    transition={transition({ delay: s(motionTokens.dur.cinema) })}
                >
                    <div className="fndr-hero-stat-num">{frameCount.toLocaleString()}</div>
                    <div className="fndr-hero-stat-label">FRAMES TODAY</div>
                </motion.div>
            )}
        </section>
    );
}

interface HeroConstellationProps {
    reduced: boolean;
}

/**
 * Six low-opacity nodes drifting in the background. Pure SVG + CSS
 * keyframes — no JS animation, no Framer Motion. When reduced motion
 * is on, the drift animation is suppressed (handled in CSS).
 */
function HeroConstellation({ reduced }: HeroConstellationProps) {
    const nodes: Array<{ cx: number; cy: number; r: number; driftIndex: number }> = [
        { cx: 18, cy: 22, r: 4.5, driftIndex: 0 },
        { cx: 32, cy: 64, r: 3.5, driftIndex: 1 },
        { cx: 58, cy: 18, r: 5.0, driftIndex: 2 },
        { cx: 72, cy: 46, r: 3.0, driftIndex: 3 },
        { cx: 82, cy: 78, r: 4.0, driftIndex: 4 },
        { cx: 46, cy: 88, r: 3.5, driftIndex: 0 },
    ];

    const edges: Array<[number, number]> = [
        [0, 2],
        [2, 3],
        [3, 4],
        [1, 5],
        [0, 1],
    ];

    return (
        <svg
            className={`fndr-hero-constellation ${reduced ? "is-reduced" : ""}`}
            viewBox="0 0 100 100"
            preserveAspectRatio="xMidYMid slice"
            aria-hidden="true"
        >
            {edges.map(([a, b], i) => (
                <line
                    key={i}
                    x1={nodes[a].cx}
                    y1={nodes[a].cy}
                    x2={nodes[b].cx}
                    y2={nodes[b].cy}
                    stroke="rgba(196, 168, 120, 0.18)"
                    strokeWidth={0.18}
                />
            ))}
            {nodes.map((n, i) => (
                <g key={i} className={`fndr-constellation-node drift-${n.driftIndex}`}>
                    <circle
                        cx={n.cx}
                        cy={n.cy}
                        r={n.r + 1.5}
                        fill="rgba(212, 160, 74, 0.06)"
                    />
                    <circle cx={n.cx} cy={n.cy} r={n.r} fill="rgba(212, 160, 74, 0.18)" />
                    <circle
                        cx={n.cx}
                        cy={n.cy}
                        r={n.r * 0.45}
                        fill="rgba(232, 223, 200, 0.55)"
                    />
                </g>
            ))}
        </svg>
    );
}

export default HeroSection;

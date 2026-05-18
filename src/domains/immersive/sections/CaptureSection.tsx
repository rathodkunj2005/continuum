import { useEffect, useRef, useState } from "react";
import { motion, useInView } from "framer-motion";
import { fadeUp, staggerContainer, useReducedMotionSafe, motionTokens, s } from "@/shared/motion";
import { MorphMemoryCard, type MorphMemoryCardData } from "../components/MorphMemoryCard";
import "./CaptureSection.css";

interface PipelineStep {
    n: string;
    label: string;
    desc: string;
}

const STEPS: PipelineStep[] = [
    { n: "01", label: "Capture", desc: "Screen context recorded" },
    { n: "02", label: "OCR", desc: "Text extracted, entities tagged" },
    { n: "03", label: "Embedding", desc: "Semantic vector computed" },
    { n: "04", label: "Memory event", desc: "Frame developed, indexed" },
    { n: "05", label: "Searchable", desc: "Available in graph + search" },
];

/** Step activation lag (ms apart). Reduced to 0 when prefers-reduced-motion. */
const STEP_INTERVAL_MS = 600;

/**
 * Visualises the capture pipeline. Left column: 5 stacked steps that
 * light up in sequence when the section enters the viewport. Right
 * column: a raw "capture" card morphs into a developed "preview" card.
 */
export function CaptureSection() {
    const ref = useRef<HTMLDivElement>(null);
    const inView = useInView(ref, { once: true, amount: 0.4 });
    const { reduced, transition } = useReducedMotionSafe();
    const [activeStep, setActiveStep] = useState(-1);
    const [developed, setDeveloped] = useState(false);

    useEffect(() => {
        if (!inView) return;

        if (reduced) {
            setActiveStep(STEPS.length - 1);
            setDeveloped(true);
            return;
        }

        const timers: number[] = [];
        STEPS.forEach((_, i) => {
            timers.push(
                window.setTimeout(() => setActiveStep(i), STEP_INTERVAL_MS * (i + 1))
            );
        });
        // Morph at the last step
        timers.push(
            window.setTimeout(
                () => setDeveloped(true),
                STEP_INTERVAL_MS * STEPS.length + 100
            )
        );
        return () => {
            for (const t of timers) window.clearTimeout(t);
        };
    }, [inView, reduced]);

    const cardData: MorphMemoryCardData = developed
        ? {
              id: "capture-demo",
              frameNumber: "0412 / 412",
              source: "Notes.app",
              timestamp: "14:32",
              title: "Light leak, late afternoon",
              preview:
                  "The room held the same gold for the last twenty minutes. Worth noting.",
              threads: ["cinema", "light", "archive"],
              stamp: "developed",
          }
        : {
              id: "capture-demo",
              frameNumber: "0412 / 412",
              source: "Notes.app",
              timestamp: "14:32",
              stamp: "raw",
          };

    return (
        <section
            id="fndr-section-capture"
            data-section-id="capture"
            className="fndr-section fndr-capture-section"
            ref={ref}
        >
            <div className="fndr-capture-grid">
                <motion.div
                    className="fndr-capture-left"
                    variants={staggerContainer}
                    initial="hidden"
                    animate={inView ? "visible" : "hidden"}
                >
                    <motion.p className="fndr-mono-label" variants={fadeUp}>
                        CAPTURE PIPELINE
                    </motion.p>

                    <motion.h2 className="fndr-section-title" variants={fadeUp}>
                        Raw context,
                        <br />
                        <em>developed into memory.</em>
                    </motion.h2>

                    <ol
                        className="fndr-pipeline-steps"
                        aria-label="Capture pipeline stages"
                    >
                        {STEPS.map((step, i) => {
                            const state =
                                i < activeStep
                                    ? "complete"
                                    : i === activeStep
                                    ? "active"
                                    : "pending";
                            return (
                                <motion.li
                                    key={step.n}
                                    className={`fndr-pipeline-step is-${state}`}
                                    variants={fadeUp}
                                    transition={transition({
                                        duration: s(motionTokens.dur.base),
                                    })}
                                >
                                    <span className="fndr-pipeline-step-num">{step.n}</span>
                                    <span className="fndr-pipeline-step-label">
                                        {step.label}
                                    </span>
                                    <span className="fndr-pipeline-step-line" aria-hidden="true" />
                                    <span className="fndr-pipeline-step-desc">{step.desc}</span>
                                </motion.li>
                            );
                        })}
                    </ol>
                </motion.div>

                <div className="fndr-capture-right">
                    <div className="fndr-capture-card-stage">
                        <MorphMemoryCard
                            data={cardData}
                            variant={developed ? "preview" : "capture"}
                        />
                    </div>
                </div>
            </div>
        </section>
    );
}

export default CaptureSection;

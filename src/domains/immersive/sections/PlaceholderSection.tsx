import { motion } from "framer-motion";
import { fadeUp, staggerContainer } from "@/shared/motion";
import type { ImmersiveSectionId } from "@/shared/motion";

interface PlaceholderSectionProps {
    id: ImmersiveSectionId;
    label: string;
    subtitle: string;
    height?: string;
    showEnterWorkMode?: boolean;
    onEnterWorkMode?: () => void;
}

/**
 * Stub used for sections that have not been built yet (Timeline, Search,
 * Graph, Agent, Privacy, Workspace as of Slice 3). Keeps section ids in
 * the DOM so ChapterRail, scroll progress, and 1–8 keyboard nav all work.
 */
export function PlaceholderSection({
    id,
    label,
    subtitle,
    height,
    showEnterWorkMode,
    onEnterWorkMode,
}: PlaceholderSectionProps) {
    return (
        <section
            id={`fndr-section-${id}`}
            data-section-id={id}
            className={`fndr-section fndr-section-placeholder ${
                height === "60vh" ? "fndr-section--short" : ""
            }`}
            style={height ? { minHeight: height } : undefined}
        >
            <motion.div
                className="fndr-section-placeholder-content"
                variants={staggerContainer}
                initial="hidden"
                whileInView="visible"
                viewport={{ once: true, amount: 0.4 }}
            >
                <motion.p className="fndr-mono-label" variants={fadeUp}>
                    {label}
                </motion.p>
                <motion.div className="fndr-section-placeholder-card" variants={fadeUp}>
                    {subtitle}
                </motion.div>
                {showEnterWorkMode && onEnterWorkMode && (
                    <motion.div
                        style={{ marginTop: "var(--s-5)" }}
                        variants={fadeUp}
                    >
                        <button
                            type="button"
                            className="fndr-btn fndr-btn--primary"
                            onClick={onEnterWorkMode}
                        >
                            Enter work mode →
                        </button>
                    </motion.div>
                )}
            </motion.div>
        </section>
    );
}

export default PlaceholderSection;

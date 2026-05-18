import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { StickyScene } from "@/domains/immersive/components/StickyScene";
import { useSetWallpaperPage } from "@/shared/hooks/useImmersiveWallpaper";
import { getBlocklist } from "@/shared/ipc/tauri";
import "./PrivacySection.css";

const SECTION_ID = "privacy";

const STAYS_LOCAL_ITEMS = [
    "All captures stored on your Mac",
    "No cloud upload, ever",
    "Embeddings computed locally",
    "Graph reasoning runs on-device",
    "Full-disk encryption at rest",
];

const SEARCHABLE_ITEMS = [
    "Titles and summaries",
    "App and window context",
    "Timeline actions",
    "Thread connections",
    "Topic categories",
];

interface ColumnProps {
    title: string;
    stamp: string;
    stampTone: "developed" | "alarm" | "amber";
    items: string[];
    delay?: number;
}

function PrivacyColumn({ title, stamp, stampTone, items, delay = 0 }: ColumnProps) {
    return (
        <motion.div
            className={`fndr-privacy-col fndr-privacy-col--${stampTone}`}
            initial={{ opacity: 0, y: 16 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ duration: 0.5, delay }}
        >
            <div className={`fndr-privacy-stamp fndr-privacy-stamp--${stampTone}`}>
                {stamp}
            </div>
            <h3 className="fndr-privacy-col-title">{title}</h3>
            <ul className="fndr-privacy-list">
                {items.map((item) => (
                    <li key={item}>{item}</li>
                ))}
            </ul>
        </motion.div>
    );
}

/**
 * Slice 7b — Privacy scene.
 * 3-column grid: STAYS LOCAL / DISCARDED / SEARCHABLE.
 * Blocklist items populate the DISCARDED column.
 */
export function PrivacySection() {
    useSetWallpaperPage("darkroom", SECTION_ID);

    const [blocklist, setBlocklist] = useState<string[]>([]);

    useEffect(() => {
        let cancelled = false;
        void getBlocklist().then((list) => {
            if (!cancelled) setBlocklist(list);
        });
        return () => { cancelled = true; };
    }, []);

    const discardedItems =
        blocklist.length > 0
            ? blocklist.slice(0, 5)
            : ["No apps blocked yet", "Add apps in Privacy settings"];

    return (
        <StickyScene sectionId={SECTION_ID} scrollBudget={600}>
            <div className="fndr-privacy-scene">
                <div className="fndr-privacy-scene-inner">
                    <motion.p
                        className="fndr-privacy-label"
                        initial={{ opacity: 0, y: 8 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5 }}
                    >
                        Your data, your machine
                    </motion.p>

                    <motion.h2
                        className="fndr-privacy-heading"
                        initial={{ opacity: 0, y: 14 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.6, delay: 0.08 }}
                    >
                        private by design
                    </motion.h2>

                    <div className="fndr-privacy-grid">
                        <PrivacyColumn
                            title="Stays Local"
                            stamp="LOCAL"
                            stampTone="developed"
                            items={STAYS_LOCAL_ITEMS}
                            delay={0.15}
                        />
                        <PrivacyColumn
                            title="Discarded"
                            stamp="BLOCKED"
                            stampTone="alarm"
                            items={discardedItems}
                            delay={0.22}
                        />
                        <PrivacyColumn
                            title="Searchable"
                            stamp="INDEXED"
                            stampTone="amber"
                            items={SEARCHABLE_ITEMS}
                            delay={0.29}
                        />
                    </div>
                </div>
            </div>
        </StickyScene>
    );
}

export default PrivacySection;

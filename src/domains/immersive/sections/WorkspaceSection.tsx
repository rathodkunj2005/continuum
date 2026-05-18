import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { StickyScene } from "@/domains/immersive/components/StickyScene";
import { useSetWallpaperPage } from "@/shared/hooks/useImmersiveWallpaper";
import { useAppShell } from "@/app/AppShell";
import { MemoryCard } from "@/domains/memory-vault/MemoryCard";
import { listMemoryCards, listRecentContextPacks, type MemoryCard as MemoryCardData, type ContextPack } from "@/shared/ipc/tauri";
import "./WorkspaceSection.css";

const SECTION_ID = "workspace";

interface GridCell {
    label: string;
    mono: string;
    children: React.ReactNode;
}

function WorkspaceGrid({ cells }: { cells: GridCell[] }) {
    return (
        <div className="fndr-ws-grid">
            {cells.map((cell, i) => (
                <motion.div
                    key={cell.label}
                    className="fndr-ws-cell"
                    initial={{ opacity: 0, y: 10 }}
                    whileInView={{ opacity: 1, y: 0 }}
                    viewport={{ once: true }}
                    transition={{ duration: 0.45, delay: 0.1 + i * 0.07 }}
                >
                    <div className="fndr-ws-cell-header">
                        <span className="fndr-ws-cell-mono">{cell.mono}</span>
                        <span className="fndr-ws-cell-label">{cell.label}</span>
                    </div>
                    <div className="fndr-ws-cell-body">{cell.children}</div>
                </motion.div>
            ))}
        </div>
    );
}

function EmptyState({ text }: { text: string }) {
    return <p className="fndr-ws-empty">{text}</p>;
}

/**
 * Slice 7c — Workspace scene.
 * 2×2 grid: recent memories · context packs (+ CTA to enter work mode).
 */
export function WorkspaceSection() {
    useSetWallpaperPage("pinned", SECTION_ID);

    const { setMode } = useAppShell();
    const [recentCards, setRecentCards] = useState<MemoryCardData[]>([]);
    const [contextPacks, setContextPacks] = useState<ContextPack[]>([]);

    useEffect(() => {
        let cancelled = false;
        void listMemoryCards(4).then((cards) => {
            if (!cancelled) setRecentCards(cards);
        });
        void listRecentContextPacks(4).then((packs) => {
            if (!cancelled) setContextPacks(packs);
        });
        return () => { cancelled = true; };
    }, []);

    const cells: GridCell[] = [
        {
            label: "Recent memories",
            mono: "01",
            children: recentCards.length > 0 ? (
                <div className="fndr-ws-cards">
                    {recentCards.map((card) => (
                        <MemoryCard key={card.id} card={card} variant="compact" />
                    ))}
                </div>
            ) : <EmptyState text="Memories appear here as you work" />,
        },
        {
            label: "Context packs",
            mono: "02",
            children: contextPacks.length > 0 ? (
                <ul className="fndr-ws-pack-list">
                    {contextPacks.map((pack) => (
                        <li key={pack.id} className="fndr-ws-pack-item">
                            <span className="fndr-ws-pack-title">
                                {pack.project ?? pack.active_goal ?? pack.query ?? "Context pack"}
                            </span>
                            <span className="fndr-ws-pack-count">
                                {pack.included.length} items
                            </span>
                        </li>
                    ))}
                </ul>
            ) : <EmptyState text="Context packs created in work mode" />,
        },
    ];

    return (
        <StickyScene sectionId={SECTION_ID} scrollBudget={600}>
            <div className="fndr-ws-scene">
                <div className="fndr-ws-scene-inner">
                    <motion.p
                        className="fndr-ws-label"
                        initial={{ opacity: 0, y: 8 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5 }}
                    >
                        Memory becomes action
                    </motion.p>

                    <motion.h2
                        className="fndr-ws-heading"
                        initial={{ opacity: 0, y: 14 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.6, delay: 0.08 }}
                    >
                        enter work mode
                    </motion.h2>

                    <WorkspaceGrid cells={cells} />

                    <motion.div
                        className="fndr-ws-cta-row"
                        initial={{ opacity: 0, y: 10 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5, delay: 0.4 }}
                    >
                        <button
                            type="button"
                            className="fndr-ws-cta"
                            onClick={() => setMode("work")}
                        >
                            Enter work mode →
                        </button>
                    </motion.div>
                </div>
            </div>
        </StickyScene>
    );
}

export default WorkspaceSection;

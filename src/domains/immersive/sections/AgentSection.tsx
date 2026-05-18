import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { StickyScene } from "@/domains/immersive/components/StickyScene";
import { useSetWallpaperPage } from "@/shared/hooks/useImmersiveWallpaper";
import { MemoryCard } from "@/domains/memory-vault/MemoryCard";
import {
    getAgentStatus,
    type AgentStatus,
    type MemoryCard as MemoryCardData,
    searchMemoryCards,
} from "@/shared/ipc/tauri";
import "./AgentSection.css";

const SECTION_ID = "agent";

function useLiveAgentStatus() {
    const [status, setStatus] = useState<AgentStatus | null>(null);
    const [recentCards, setRecentCards] = useState<MemoryCardData[]>([]);

    useEffect(() => {
        let cancelled = false;
        void getAgentStatus().then((s) => {
            if (!cancelled) setStatus(s);
        });
        // Surface the last agent query's retrieved memories using task_title as hint
        void searchMemoryCards("", undefined, undefined, 3).then((cards) => {
            if (!cancelled) setRecentCards(cards);
        });
        return () => { cancelled = true; };
    }, []);

    return { status, recentCards };
}

/** Fake skeleton cards while loading */
function SkeletonCard() {
    return <div className="fndr-agent-skeleton-card" aria-hidden />;
}

/**
 * Slice 7a — Agent context scene.
 * Shows the current agent query, retrieved memory cards, and MCP tool trace.
 */
export function AgentSection() {
    useSetWallpaperPage("smart", SECTION_ID);

    const { status: agentStatus, recentCards: cards } = useLiveAgentStatus();
    const toolTrace: string[] = [];
    const currentQuery: string = agentStatus?.task_title ?? "";
    const isLoading = agentStatus === null;

    return (
        <StickyScene sectionId={SECTION_ID} scrollBudget={600}>
            <div className="fndr-agent-scene">
                <div className="fndr-agent-scene-inner">
                    <motion.p
                        className="fndr-agent-label"
                        initial={{ opacity: 0, y: 8 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5 }}
                    >
                        Agent context
                    </motion.p>

                    <motion.h2
                        className="fndr-agent-heading"
                        initial={{ opacity: 0, y: 14 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.6, delay: 0.08 }}
                    >
                        {currentQuery || "memory as context"}
                    </motion.h2>

                    {/* Retrieved cards */}
                    <motion.div
                        className="fndr-agent-cards"
                        initial={{ opacity: 0 }}
                        whileInView={{ opacity: 1 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5, delay: 0.18 }}
                    >
                        {isLoading ? (
                            <>
                                <SkeletonCard />
                                <SkeletonCard />
                                <SkeletonCard />
                            </>
                        ) : cards.length > 0 ? (
                            cards.slice(0, 3).map((card, i) => (
                                <motion.div
                                    key={card.id}
                                    initial={{ opacity: 0, x: -8 }}
                                    animate={{ opacity: 1, x: 0 }}
                                    transition={{ delay: i * 0.08 }}
                                >
                                    <MemoryCard card={card} variant="compact" />
                                </motion.div>
                            ))
                        ) : (
                            <p className="fndr-agent-empty">
                                Agent is idle — memories surface when active
                            </p>
                        )}
                    </motion.div>

                    {/* MCP tool trace */}
                    {toolTrace.length > 0 && (
                        <motion.div
                            className="fndr-agent-trace"
                            initial={{ opacity: 0 }}
                            whileInView={{ opacity: 1 }}
                            viewport={{ once: true }}
                            transition={{ delay: 0.3 }}
                        >
                            <p className="fndr-agent-trace-label">Tool trace</p>
                            {toolTrace.map((line, i) => (
                                <motion.p
                                    key={i}
                                    className="fndr-agent-trace-line"
                                    initial={{ opacity: 0 }}
                                    animate={{ opacity: 1 }}
                                    transition={{ delay: 0.3 + i * 0.08 }}
                                >
                                    {line}
                                </motion.p>
                            ))}
                        </motion.div>
                    )}
                </div>
            </div>
        </StickyScene>
    );
}

export default AgentSection;

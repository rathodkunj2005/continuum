import { useMemo, type ReactNode } from "react";
import { motion } from "framer-motion";
import type { MemoryCard as MemoryCardData } from "@/shared/ipc/tauri";
import { DossierCorners, Stamp, Pill, Button } from "@/shared/components/atoms";
import { MemoryProvenanceStrip } from "./MemoryProvenanceStrip";
import "./MemoryCard.css";

export type MemoryCardVariant = "compact" | "preview" | "expanded" | "immersive";

interface MemoryCardProps {
    card: MemoryCardData;
    variant: MemoryCardVariant;
    /** Open the expanded inspector (e.g. clicking a compact row). */
    onOpen?: (card: MemoryCardData) => void;
    onDelete?: (id: string) => void;
    onResearch?: (card: MemoryCardData) => void;
    onOpenInGraph?: (card: MemoryCardData) => void;
    /** Reopen the captured source target (file/URL). */
    onReopen?: (card: MemoryCardData) => void;
    /** Slot for InsightLayers in expanded variant. */
    insightsSlot?: ReactNode;
    /** Slot for evidence pack in expanded variant. */
    evidenceSlot?: ReactNode;
    /** Slot for related-memories list in expanded variant. */
    relatedSlot?: ReactNode;
    /** Slot for extras (e.g. surfacing-reason explainer). */
    headerSlot?: ReactNode;
    /** Slot for footer (e.g. debug/similar drawer toggles). */
    footerSlot?: ReactNode;
    /** Shared-layout id for Framer Motion morph transitions. */
    layoutId?: string;
    /** Show DEVELOPED stamp (defaults true when synthesis_branch is set). */
    developed?: boolean;
    /** Show CONFIDENTIAL alarm stamp. */
    confidential?: boolean;
    className?: string;
    /** Compact-variant override when card has no threads in the topic_categories array. */
    threadCountHint?: number;
}

/**
 * Canonical FNDR memory card. Four variants mapped onto a single component
 * so the immersive scroll, the vault list, the expanded modal, and the hero
 * card all share one surface.
 *
 * Field gating: only `title`, `app_name`, `timestamp` are always-on; every
 * other field hides when undefined/empty. No placeholder text, no fake data.
 */
export function MemoryCard({
    card,
    variant,
    onOpen,
    onDelete,
    onResearch,
    onOpenInGraph,
    onReopen,
    insightsSlot,
    evidenceSlot,
    relatedSlot,
    headerSlot,
    footerSlot,
    layoutId,
    developed,
    confidential,
    className,
    threadCountHint,
}: MemoryCardProps) {
    const frameId = useMemo(() => deriveFrameId(card.id), [card.id]);
    const previewText = pickPreviewText(card);
    const threads = useMemo(() => deriveThreads(card), [card]);
    const timeLabel = useMemo(() => formatTime(card.timestamp), [card.timestamp]);
    const dayLabel = useMemo(() => formatDay(card.timestamp), [card.timestamp]);
    const isDeveloped = developed ?? (typeof card.synthesis_branch === "string" && card.synthesis_branch.length > 0);

    const cls = [
        "fndr-mc",
        `fndr-mc--${variant}`,
        className ?? "",
    ]
        .filter(Boolean)
        .join(" ");

    // Compact — single row, no preview, click to expand.
    if (variant === "compact") {
        return (
            <motion.article
                layoutId={layoutId}
                data-testid="memory-card"
                data-card-id={card.id}
                id={`memory-card-${card.id}`}
                className={cls}
                onClick={onOpen ? () => onOpen(card) : undefined}
                role={onOpen ? "button" : undefined}
                tabIndex={onOpen ? 0 : undefined}
                onKeyDown={(e) => {
                    if (!onOpen) return;
                    if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        onOpen(card);
                    }
                }}
            >
                <span className="fndr-mc-bar" aria-hidden="true" />
                <span className="fndr-mc-c-frame">FRAME {frameId}</span>
                <span className="fndr-mc-c-title">{card.title}</span>
                <span className="fndr-mc-c-source">
                    <em>{card.app_name}</em>
                </span>
                <span className="fndr-mc-c-time">{timeLabel}</span>
                <span className="fndr-mc-c-threads">
                    {threads.length > 0
                        ? `${threads.length}`
                        : threadCountHint !== undefined
                        ? `${threadCountHint}`
                        : ""}
                </span>
            </motion.article>
        );
    }

    // Preview / Expanded / Immersive share the dossier-framed card structure.
    return (
        <motion.article
            layoutId={layoutId}
            data-testid="memory-card"
            data-card-id={card.id}
            id={`memory-card-${card.id}`}
            className={cls}
            onClick={variant === "preview" && onOpen ? () => onOpen(card) : undefined}
            role={variant === "preview" && onOpen ? "button" : undefined}
            tabIndex={variant === "preview" && onOpen ? 0 : undefined}
        >
            <DossierCorners />

            <header className="fndr-mc-strip">
                {isDeveloped && (
                    <Stamp tone="developed" rotate={-1}>
                        DEVELOPED
                    </Stamp>
                )}
                {confidential && (
                    <Stamp tone="alarm" rotate={2}>
                        CONFIDENTIAL
                    </Stamp>
                )}
                <span className="fndr-mc-frame-no">FRAME {frameId}</span>
                <span className="fndr-mc-ts">
                    {dayLabel} · {timeLabel}
                </span>
            </header>

            {headerSlot}

            <h3 className="fndr-mc-title">{card.title}</h3>

            {previewText && (
                <p className="fndr-mc-preview">
                    &ldquo;{previewText}&rdquo;
                </p>
            )}

            <div className="fndr-mc-source">
                {card.app_name}
                {card.window_title && variant === "expanded" ? ` · ${card.window_title}` : null}
            </div>

            {threads.length > 0 && (
                <ul className="fndr-mc-threads">
                    {threads.map((t) => (
                        <li key={t}>{t}</li>
                    ))}
                </ul>
            )}

            {variant === "expanded" && (
                <>
                    <MemoryProvenanceStrip card={card} />

                    {(card.session_duration_mins ?? 0) > 0 ||
                    card.timeline_action_class ||
                    card.source_count > 1 ? (
                        <div className="fndr-mc-meta-row">
                            {card.source_count > 1 && (
                                <Pill tone="muted" noDot>
                                    {card.source_count} captures
                                </Pill>
                            )}
                            {(card.session_duration_mins ?? 0) > 0 && (
                                <Pill tone="muted" noDot>
                                    {card.session_duration_mins}m
                                </Pill>
                            )}
                            {card.timeline_action_class &&
                                card.timeline_action_class !== "other" && (
                                    <Pill tone="live" noDot>
                                        {card.timeline_action_class}
                                    </Pill>
                                )}
                        </div>
                    ) : null}

                    {insightsSlot && (
                        <div className="fndr-mc-slot fndr-mc-slot--insights">
                            {insightsSlot}
                        </div>
                    )}
                    {evidenceSlot && (
                        <div className="fndr-mc-slot fndr-mc-slot--evidence">
                            {evidenceSlot}
                        </div>
                    )}
                    {relatedSlot && (
                        <div className="fndr-mc-slot fndr-mc-slot--related">
                            {relatedSlot}
                        </div>
                    )}

                    <div className="fndr-mc-actions">
                        {onOpenInGraph && (
                            <Button mono variant="secondary" onClick={() => onOpenInGraph(card)}>
                                See in graph
                            </Button>
                        )}
                        {onReopen && card.reopen_target && (
                            <Button mono variant="secondary" onClick={() => onReopen(card)}>
                                Open source
                            </Button>
                        )}
                        {onResearch && (
                            <Button mono variant="ghost" onClick={() => onResearch(card)}>
                                Research
                            </Button>
                        )}
                        {onDelete && (
                            <Button mono variant="alarm" onClick={() => onDelete(card.id)}>
                                Delete
                            </Button>
                        )}
                    </div>
                </>
            )}

            {footerSlot}
        </motion.article>
    );
}

/* ── helpers ──────────────────────────────────────────────── */

function deriveFrameId(id: string): string {
    // Stable 4-char uppercase from the trailing characters of the id.
    const trimmed = id.replace(/[^a-zA-Z0-9]/g, "");
    return trimmed.slice(-4).toUpperCase().padStart(4, "0");
}

function pickPreviewText(card: MemoryCardData): string {
    return (
        card.display_summary?.trim() ||
        card.summary?.trim() ||
        card.internal_context?.trim() ||
        ""
    );
}

function deriveThreads(card: MemoryCardData): string[] {
    const out: string[] = [];
    if (Array.isArray(card.topic_categories)) {
        for (const t of card.topic_categories) {
            const trimmed = t?.trim();
            if (trimmed && !out.includes(trimmed)) out.push(trimmed);
        }
    }
    const ctxThread = card.insight_context_thread?.trim();
    if (ctxThread && !out.includes(ctxThread)) out.push(ctxThread);
    return out.slice(0, 5);
}

function formatTime(timestamp: number): string {
    const d = new Date(timestamp);
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function formatDay(timestamp: number): string {
    const d = new Date(timestamp);
    const now = new Date();
    const isToday =
        d.getFullYear() === now.getFullYear() &&
        d.getMonth() === now.getMonth() &&
        d.getDate() === now.getDate();
    if (isToday) return "TODAY";
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }).toUpperCase();
}

export default MemoryCard;

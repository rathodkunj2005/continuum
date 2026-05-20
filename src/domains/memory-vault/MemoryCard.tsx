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

    // Compact — single row (wide) / two rows (narrow container), click to expand.
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
                <div className="fndr-mc-c-main">
                    <span className="fndr-mc-c-title">{card.title}</span>
                    {previewText ? (
                        <span className="fndr-mc-c-preview" title={previewText}>
                            {previewText}
                        </span>
                    ) : null}
                </div>
                {/* source area: app name + activity/files chips */}
                <div className="fndr-mc-c-source" aria-label="app and context">
                    <em className="fndr-mc-c-source-app">{card.app_name}</em>
                    {card.activity_type && card.activity_type !== "other" && (
                        <span className="fndr-mc-c-chip fndr-mc-c-chip--activity" aria-label={`activity: ${card.activity_type}`}>
                            {card.activity_type}
                        </span>
                    )}
                    {Array.isArray(card.files_touched) && card.files_touched.length > 0 && (
                        <span
                            className="fndr-mc-c-chip fndr-mc-c-chip--files"
                            title={card.files_touched.join("\n")}
                            aria-label={`${card.files_touched.length} file${card.files_touched.length !== 1 ? "s" : ""}`}
                        >
                            {card.files_touched.length === 1
                                ? card.files_touched[0].split("/").pop() ?? card.files_touched[0]
                                : `${card.files_touched.length} files`}
                        </span>
                    )}
                </div>
                <span className="fndr-mc-c-time">
                    <span className="fndr-mc-c-day">{dayLabel}</span>
                    <span className="fndr-mc-c-clock">{timeLabel}</span>
                </span>
                <CompactSurfacingGlyph card={card} />
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
                {isDeveloped ? (
                    <Stamp tone="developed" rotate={-1}>
                        DEVELOPED
                    </Stamp>
                ) : (
                    <Stamp tone="muted" rotate={-1}>
                        RAW
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
                            <div className="fndr-mc-actions-danger">
                                <Button
                                    mono
                                    variant="ghost"
                                    onClick={() => {
                                        const ok =
                                            typeof window === "undefined"
                                                ? true
                                                : window.confirm(
                                                      `Delete this memory? It will be removed permanently.\n\n"${card.title}"`,
                                                  );
                                        if (ok) onDelete(card.id);
                                    }}
                                >
                                    Delete
                                </Button>
                            </div>
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
    const fromFields =
        card.display_summary?.trim() ||
        card.summary?.trim() ||
        card.internal_context?.trim() ||
        "";
    if (fromFields) {
        return fromFields;
    }
    if (Array.isArray(card.context)) {
        const joined = card.context
            .map((line) => line?.trim())
            .filter((line): line is string => Boolean(line))
            .join(" · ");
        if (joined) {
            return joined;
        }
    }
    return "";
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
    // Compare calendar-day distance, not raw 24-hour buckets — a capture
    // from yesterday at 23:00 should read "YESTERDAY", not "TODAY", even if
    // it's less than 24 hours old.
    const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const dayDiff = Math.round((startOf(now) - startOf(d)) / (24 * 60 * 60 * 1000));
    if (dayDiff === 0) return "TODAY";
    if (dayDiff === 1) return "YESTERDAY";
    if (dayDiff > 1 && dayDiff < 7) return `${dayDiff}D AGO`;
    if (dayDiff >= 7 && dayDiff < 14) {
        const wd = d.toLocaleDateString(undefined, { weekday: "short" }).toUpperCase();
        return `LAST ${wd}`;
    }
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }).toUpperCase();
}

/** Maps a surfacing-reason route to a single-letter glyph for the compact row.
 *  Keeps the contract that *every place a memory surfaces, the why is shown*
 *  without bloating the row layout. */
const ROUTE_GLYPHS: Record<string, string> = {
    BM25: "B",
    bm25: "B",
    semantic: "S",
    entity_link: "E",
    entity: "E",
    recency: "R",
    synthesis: "Y",
};

function CompactSurfacingGlyph({ card }: { card: MemoryCardData }) {
    const reason = card.surfacing_reason;
    if (!reason || !reason.routes || reason.routes.length === 0) {
        return <span className="fndr-mc-c-route" aria-hidden="true" />;
    }
    const primary = reason.routes[0];
    const glyph = ROUTE_GLYPHS[primary] ?? primary.slice(0, 1).toUpperCase();
    const tooltipLines = [
        reason.headline,
        `Routes: ${reason.routes.join(", ")}`,
        reason.anchor_terms_hit && reason.anchor_terms_hit.length > 0
            ? `Anchors: ${reason.anchor_terms_hit.join(", ")}`
            : null,
    ]
        .filter(Boolean)
        .join("\n");
    return (
        <span className="fndr-mc-c-route" title={tooltipLines} data-route={primary}>
            {glyph}
        </span>
    );
}

export default MemoryCard;

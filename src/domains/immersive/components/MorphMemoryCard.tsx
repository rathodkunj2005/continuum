import type { MemoryCard as MemoryCardData } from "@/shared/ipc/tauri";
import { MemoryCard } from "@/domains/memory-vault/MemoryCard";

/**
 * Variants of a memory card used inside the immersive scroll experience.
 * "capture" renders the raw / undeveloped look (compact variant); "preview"
 * renders the developed memory card.
 *
 * This is a thin wrapper around the canonical `<MemoryCard>` that
 * preserves the `layoutId` shared-layout morph used by CaptureSection
 * (raw frame → developed memory). The original ad-hoc styles in
 * MorphMemoryCard.css were lifted into MemoryCard.css + atoms.css.
 */
export type MorphMemoryCardVariant = "capture" | "preview";

export interface MorphMemoryCardData {
    /** Stable id used for Framer Motion shared-layout transitions. */
    id: string;
    frameNumber?: string;
    source?: string;
    timestamp?: string;
    title?: string;
    preview?: string;
    threads?: string[];
    stamp?: "developed" | "raw";
}

interface MorphMemoryCardProps {
    data: MorphMemoryCardData;
    variant: MorphMemoryCardVariant;
    className?: string;
}

export function MorphMemoryCard({ data, variant, className }: MorphMemoryCardProps) {
    const card = toMemoryCardData(data);
    return (
        <MemoryCard
            card={card}
            variant={variant === "capture" ? "compact" : "preview"}
            layoutId={`memory-card-${data.id}`}
            developed={data.stamp === "developed"}
            className={className}
        />
    );
}

/** Adapter — map the slim MorphMemoryCardData onto the real Tauri MemoryCard
 *  shape so the canonical component can render it. Fields not provided by
 *  the demo data resolve to empty strings (the card hides them via gating). */
function toMemoryCardData(data: MorphMemoryCardData): MemoryCardData {
    const ts = data.timestamp ? parseTimestamp(data.timestamp) : Date.now();
    return {
        id: data.id,
        title: data.title ?? "",
        summary: data.preview ?? "",
        action: "",
        context: [],
        timestamp: ts,
        app_name: data.source ?? "",
        window_title: "",
        score: 0,
        source_count: 1,
        raw_snippets: [],
        topic_categories: data.threads,
        synthesis_branch: data.stamp === "developed" ? "synthesis" : undefined,
    };
}

function parseTimestamp(s: string): number {
    // Demo data uses "HH:MM" — fold onto today.
    const m = s.match(/^(\d{1,2}):(\d{2})$/);
    if (m) {
        const d = new Date();
        d.setHours(Number(m[1]), Number(m[2]), 0, 0);
        return d.getTime();
    }
    const parsed = Date.parse(s);
    return Number.isNaN(parsed) ? Date.now() : parsed;
}

export default MorphMemoryCard;

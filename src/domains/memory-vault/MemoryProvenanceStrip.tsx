import type { MemoryCard as MemoryCardData } from "@/shared/ipc/tauri";
import "./MemoryProvenanceStrip.css";

interface Props {
    card: MemoryCardData;
}

/** Tabular provenance strip — mono caps. Used in expanded variant. */
export function MemoryProvenanceStrip({ card }: Props) {
    const d = new Date(card.timestamp);
    const stamp = card.synthesis_branch ? "DEVELOPED" : "RAW";
    return (
        <dl className="fndr-mc-provenance">
            <div>
                <dt>Captured</dt>
                <dd>
                    {d.toLocaleDateString(undefined, {
                        month: "short",
                        day: "numeric",
                        year: "numeric",
                    })}
                    {" · "}
                    {d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                </dd>
            </div>
            <div>
                <dt>Source</dt>
                <dd>{card.app_name}</dd>
            </div>
            {card.window_title && (
                <div>
                    <dt>Window</dt>
                    <dd>{card.window_title}</dd>
                </div>
            )}
            {typeof card.confidence === "number" && (
                <div>
                    <dt>Confidence</dt>
                    <dd>{Math.round(card.confidence * 100)}%</dd>
                </div>
            )}
            {typeof card.anchor_coverage_score === "number" && (
                <div>
                    <dt>Anchor</dt>
                    <dd>{Math.round(card.anchor_coverage_score * 100)}%</dd>
                </div>
            )}
            {card.project && (
                <div>
                    <dt>Project</dt>
                    <dd>{card.project}</dd>
                </div>
            )}
            <div>
                <dt>Status</dt>
                <dd>{stamp}</dd>
            </div>
        </dl>
    );
}

export default MemoryProvenanceStrip;

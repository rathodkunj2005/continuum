import type { MemoryCard } from "@/shared/ipc/tauri";
import "./InsightLayers.css";

function hasInsight(card: MemoryCard): boolean {
    return Boolean(
        card.insight_what_happened?.trim() ||
            card.insight_why_mattered?.trim() ||
            card.insight_what_changed?.trim() ||
            card.insight_context_thread?.trim(),
    );
}

/** Renders persisted insight rows when present; optional eval debug for span JSON. */
export function InsightLayers({ card, evalUi = false }: { card: MemoryCard; evalUi?: boolean }) {
    const ic = card.insight_card_confidence ?? 0;
    const low = ic > 0 && ic < 0.4;
    if (!hasInsight(card) && !low) {
        return null;
    }

    const rows: { label: string; value: string }[] = [];
    if (card.insight_what_happened?.trim()) {
        rows.push({ label: "What happened", value: card.insight_what_happened.trim() });
    }
    if (card.insight_why_mattered?.trim()) {
        rows.push({ label: "Why it mattered", value: card.insight_why_mattered.trim() });
    }
    if (card.insight_what_changed?.trim()) {
        rows.push({ label: "What changed", value: card.insight_what_changed.trim() });
    }
    if (card.insight_context_thread?.trim()) {
        rows.push({ label: "Thread", value: card.insight_context_thread.trim() });
    }

    const categories = card.topic_categories?.filter((c) => c.trim()) ?? [];

    return (
        <div className={`insight-layers${low ? " insight-layers--low-conf" : ""}`}>
            <div className="insight-meta-row">
                {low && <div className="insight-low-badge">Low insight confidence</div>}
                {card.synthesis_branch && card.synthesis_branch !== "" && (
                    <span
                        className="insight-branch-chip"
                        title={`Synthesized via: ${card.synthesis_branch}`}
                    >
                        {card.synthesis_branch}
                    </span>
                )}
            </div>
            {rows.map((row) => (
                <div className="insight-row" key={row.label}>
                    <span className="insight-label">{row.label}</span>
                    <p className="insight-value">{row.value}</p>
                </div>
            ))}
            {categories.length > 0 && (
                <div className="insight-categories">
                    {categories.slice(0, 6).map((c) => (
                        <span className="insight-category-chip" key={c}>
                            {c}
                        </span>
                    ))}
                </div>
            )}
            {evalUi && card.insight_spans_json?.trim() && (
                <details className="insight-spans-debug" onClick={(e) => e.stopPropagation()}>
                    <summary>Salience spans (debug)</summary>
                    <pre className="insight-spans-pre">{card.insight_spans_json}</pre>
                </details>
            )}
        </div>
    );
}

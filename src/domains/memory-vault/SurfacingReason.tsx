import { useState } from "react";
import type { SurfacingReason as SurfacingReasonType } from "../../shared/ipc/tauri";

interface Props {
    reason: SurfacingReasonType;
}

/**
 * Phase 5 — "Why this surfaced" chip rendered under a MemoryCard title.
 * Single-line headline with a click/hover tooltip that exposes the route
 * mix, the graph path (when present), and the anchor terms that matched.
 *
 * Styled to match the warm palette used throughout the memory vault.
 */
export function SurfacingReason({ reason }: Props) {
    const [open, setOpen] = useState(false);
    if (!reason || !reason.headline) {
        return null;
    }

    return (
        <span
            className="continuum-surfacing-reason"
            style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "2px 8px",
                marginTop: 4,
                borderRadius: 999,
                fontSize: 11,
                lineHeight: "16px",
                color: "#3E2723",
                background: "#FAF9F6",
                border: "1px solid rgba(62, 39, 35, 0.16)",
                cursor: reason.routes.length > 0 ? "help" : "default",
                position: "relative",
            }}
            title={[
                `Routes: ${reason.routes.join(", ") || "—"}`,
                reason.graph_path && reason.graph_path.length > 0
                    ? `Path: ${reason.graph_path
                          .map((s) => `${s.from_label} —${s.edge}→ ${s.to_label}`)
                          .join(" / ")}`
                    : null,
                reason.anchor_terms_hit && reason.anchor_terms_hit.length > 0
                    ? `Anchors: ${reason.anchor_terms_hit.join(", ")}`
                    : null,
            ]
                .filter(Boolean)
                .join("\n")}
            onMouseEnter={() => setOpen(true)}
            onMouseLeave={() => setOpen(false)}
            onClick={() => setOpen((v) => !v)}
            data-testid="continuum-surfacing-reason"
        >
            <span
                style={{
                    width: 6,
                    height: 6,
                    borderRadius: 999,
                    background: "#E65100",
                    display: "inline-block",
                }}
            />
            <span>{reason.headline}</span>
            {open && reason.routes.length > 0 && (
                <span
                    role="tooltip"
                    style={{
                        position: "absolute",
                        top: "100%",
                        left: 0,
                        marginTop: 6,
                        padding: "6px 10px",
                        background: "#3E2723",
                        color: "#FAF9F6",
                        borderRadius: 8,
                        fontSize: 11,
                        whiteSpace: "nowrap",
                        zIndex: 50,
                        boxShadow: "0 8px 24px rgba(0,0,0,0.25)",
                    }}
                >
                    {reason.routes.join(" + ")}
                </span>
            )}
        </span>
    );
}

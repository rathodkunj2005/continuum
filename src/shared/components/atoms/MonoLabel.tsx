import type { ReactNode } from "react";

interface MonoLabelProps {
    tone?: "accent" | "fg-2" | "fg-3";
    children: ReactNode;
    className?: string;
    as?: "p" | "span" | "div";
}

/** Caps mono label — "REEL 0421", "CAPTURE PIPELINE", section headers. */
export function MonoLabel({ tone = "accent", children, className, as: As = "p" }: MonoLabelProps) {
    const toneCls = tone === "fg-2" ? " fndr-mono-label-atom--fg-2" : tone === "accent" ? " fndr-mono-label-atom--accent" : "";
    return (
        <As className={`fndr-mono-label-atom${toneCls}${className ? ` ${className}` : ""}`}>
            {children}
        </As>
    );
}

export default MonoLabel;

import type { ReactNode } from "react";

export type PillTone = "neutral" | "live" | "alarm" | "muted" | "solid";

interface PillProps {
    tone?: PillTone;
    /** Hide the leading dot. */
    noDot?: boolean;
    children: ReactNode;
    className?: string;
    onClick?: () => void;
    title?: string;
}

/** Mono caps tag — used for threads, filter chips, status capsules. */
export function Pill({ tone = "neutral", noDot = false, children, className, onClick, title }: PillProps) {
    const cls = `continuum-pill continuum-pill--${tone}${className ? ` ${className}` : ""}`;
    const props = {
        className: cls,
        "data-no-dot": noDot ? "true" : undefined,
        onClick,
        title,
    };
    if (onClick) {
        return (
            <button type="button" {...props} style={{ cursor: "pointer", font: "inherit" }}>
                {children}
            </button>
        );
    }
    return <span {...props}>{children}</span>;
}

export default Pill;

import type { HTMLAttributes, ReactNode } from "react";

export type StampTone = "alarm" | "developed" | "amber" | "muted";

interface StampProps extends HTMLAttributes<HTMLSpanElement> {
    tone?: StampTone;
    /** Rotation in degrees. Default chosen per tone. */
    rotate?: number;
    children: ReactNode;
}

/** Rotated archival stamp ("DEVELOPED", "CONFIDENTIAL", "RAW", …). */
export function Stamp({
    tone = "alarm",
    rotate,
    children,
    className,
    style,
    ...rest
}: StampProps) {
    const cls = `fndr-stamp fndr-stamp--${tone}${className ? ` ${className}` : ""}`;
    const mergedStyle = rotate !== undefined ? { transform: `rotate(${rotate}deg)`, ...style } : style;
    return (
        <span className={cls} style={mergedStyle} {...rest}>
            {children}
        </span>
    );
}

export default Stamp;

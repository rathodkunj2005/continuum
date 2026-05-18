interface PinProps {
    on?: boolean;
    size?: number;
    className?: string;
}

/** Amber dot when on; hollow hairline ring when off. */
export function Pin({ on = true, size = 8, className }: PinProps) {
    return (
        <span
            className={`fndr-pin fndr-pin--${on ? "on" : "off"}${className ? ` ${className}` : ""}`}
            style={{ width: size, height: size }}
            aria-hidden="true"
        />
    );
}

export default Pin;

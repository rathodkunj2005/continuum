interface DividerProps {
    vertical?: boolean;
    className?: string;
}

export function Divider({ vertical = false, className }: DividerProps) {
    return (
        <hr
            className={`fndr-divider${vertical ? " fndr-divider--vertical" : ""}${className ? ` ${className}` : ""}`}
            aria-hidden="true"
        />
    );
}

export default Divider;

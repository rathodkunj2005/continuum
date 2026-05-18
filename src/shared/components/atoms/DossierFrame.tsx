import type { CSSProperties, ReactNode } from "react";
import { DossierCorners } from "./DossierCorner";

interface DossierFrameProps {
    children: ReactNode;
    className?: string;
    style?: CSSProperties;
    padding?: number;
}

/** Card surface with 4-corner brackets, hairline border, raised shadow. */
export function DossierFrame({ children, className, style, padding }: DossierFrameProps) {
    return (
        <div
            className={`fndr-dossier-frame${className ? ` ${className}` : ""}`}
            style={padding !== undefined ? { padding, ...style } : style}
        >
            <DossierCorners />
            {children}
        </div>
    );
}

export default DossierFrame;

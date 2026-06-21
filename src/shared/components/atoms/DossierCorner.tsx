export type DossierCornerPosition = "tl" | "tr" | "bl" | "br";

interface DossierCornerProps {
    position: DossierCornerPosition;
}

/** Single L-shaped corner bracket — used four-up to frame a memory card. */
export function DossierCorner({ position }: DossierCornerProps) {
    return (
        <span
            className={`continuum-dossier-corner continuum-dossier-corner--${position}`}
            aria-hidden="true"
        />
    );
}

/** Convenience: render all four corners. */
export function DossierCorners() {
    return (
        <>
            <DossierCorner position="tl" />
            <DossierCorner position="tr" />
            <DossierCorner position="bl" />
            <DossierCorner position="br" />
        </>
    );
}

export default DossierCorner;

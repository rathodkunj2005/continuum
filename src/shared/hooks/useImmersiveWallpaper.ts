/**
 * Context + hook for immersive sections to update the Aurora wallpaper page
 * as the user scrolls through the immersive experience.
 *
 * Usage:
 *   // In the scroll shell — provide a setter:
 *   const [immersivePage, setImmersivePage] = useState<AuroraPageId>("home");
 *   <ImmersiveWallpaperContext.Provider value={setImmersivePage}>
 *     ...
 *   </ImmersiveWallpaperContext.Provider>
 *
 *   // In any section component — call when in view:
 *   useSetWallpaperPage("search");
 */

import { createContext, useContext, useEffect, useRef } from "react";
import type { AuroraPageId } from "@/shared/components/AuroraWallpaper";

export const ImmersiveWallpaperContext = createContext<
    ((page: AuroraPageId) => void) | null
>(null);

/**
 * Called by an immersive section to declare which wallpaper page should show
 * when that section is in the viewport. Uses IntersectionObserver on the
 * provided `sectionRef` (or defaults to the document body).
 */
export function useSetWallpaperPage(
    page: AuroraPageId,
    sectionId: string,
): void {
    const setter = useContext(ImmersiveWallpaperContext);
    const setterRef = useRef(setter);
    setterRef.current = setter;

    useEffect(() => {
        const el = document.getElementById(`fndr-section-${sectionId}`);
        if (!el || !setterRef.current) return;

        const obs = new IntersectionObserver(
            ([entry]) => {
                if (entry && entry.isIntersecting && setterRef.current) {
                    setterRef.current(page);
                }
            },
            { threshold: 0.25 },
        );
        obs.observe(el);
        return () => obs.disconnect();
    }, [page, sectionId]);
}

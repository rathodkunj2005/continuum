import { useEffect, useState } from "react";
import { DEFAULT_WALLPAPER, isWallpaperId, type WallpaperId } from "@/shared/wallpaper/wallpaper-registry";
import { STORAGE_KEYS } from "@/shared/utils/config";

/** Persisted motion-background selection from Appearance settings. */
export function useActiveWallpaper() {
    const [wallpaperId, setWallpaperId] = useState<WallpaperId>(() => {
        const stored = localStorage.getItem(STORAGE_KEYS.wallpaper);
        return isWallpaperId(stored) ? stored : DEFAULT_WALLPAPER;
    });

    useEffect(() => {
        const handler = (e: Event) => {
            const detail = (e as CustomEvent<{ wallpaper?: WallpaperId }>).detail;
            if (detail?.wallpaper && isWallpaperId(detail.wallpaper)) {
                setWallpaperId(detail.wallpaper);
            }
        };
        window.addEventListener("continuum-appearance-changed", handler);
        return () => window.removeEventListener("continuum-appearance-changed", handler);
    }, []);

    return wallpaperId;
}

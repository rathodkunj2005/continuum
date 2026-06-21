import { useEffect, useState } from "react";
import {
    getWallpaperAuroraColors,
    isPaletteKey,
    type PaletteKey,
    type PaletteMode,
} from "@/shared/theme/cinematic-palettes";
import { STORAGE_KEYS } from "@/shared/utils/config";

/** Active cinematic palette + aurora triple (bg/mid/acc) for wallpaper shaders. */
export function useActiveCinematicPalette() {
    const [paletteKey, setPaletteKey] = useState<PaletteKey>(() => {
        const stored = localStorage.getItem(STORAGE_KEYS.palette);
        return isPaletteKey(stored) ? stored : "matrix";
    });
    const [mode, setMode] = useState<PaletteMode>(() =>
        localStorage.getItem(STORAGE_KEYS.theme) === "light" ? "light" : "dark"
    );

    useEffect(() => {
        const handler = (e: Event) => {
            const detail = (e as CustomEvent<{ palette?: PaletteKey; mode?: PaletteMode }>).detail;
            if (detail?.palette && isPaletteKey(detail.palette)) setPaletteKey(detail.palette);
            if (detail?.mode === "light" || detail?.mode === "dark") setMode(detail.mode);
        };
        window.addEventListener("continuum-appearance-changed", handler);
        return () => window.removeEventListener("continuum-appearance-changed", handler);
    }, []);

    return { paletteKey, mode, aurora: getWallpaperAuroraColors(paletteKey, mode) };
}

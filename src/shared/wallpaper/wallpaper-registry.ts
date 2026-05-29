/** Interactive motion backgrounds for the global wallpaper layer. */

export const WALLPAPERS = {
    aurora: {
        name: "Aurora",
        description: "Flowing northern lights that bend toward your cursor and ripple on click.",
        preview: "linear-gradient(160deg, var(--cp-wall-bg) 0%, var(--cp-wall-mid) 58%, var(--cp-wall-acc) 100%)",
    },
    nebula: {
        name: "Nebula Drift",
        description: "Soft cosmic clouds pulled by the pointer with luminous click bursts.",
        preview: "radial-gradient(circle at 30% 38%, var(--cp-wall-acc) 0%, var(--cp-wall-mid) 36%, var(--cp-wall-bg) 76%)",
    },
    plasma: {
        name: "Plasma Pulse",
        description: "Electric color waves that speed up near the mouse and flash when you tap.",
        preview: "linear-gradient(135deg, var(--cp-wall-bg) 0%, var(--cp-wall-mid) 42%, var(--cp-wall-acc) 100%)",
    },
    warpGrid: {
        name: "Warp Grid",
        description: "A perspective lattice that warps and glows around your pointer.",
        preview: "linear-gradient(180deg, var(--cp-wall-bg) 0%, var(--cp-wall-mid) 54%, var(--cp-wall-acc) 100%)",
    },
    liquid: {
        name: "Liquid Glass",
        description: "Metaball fluid that follows the cursor and splashes on every click.",
        preview: "radial-gradient(ellipse at 50% 78%, var(--cp-wall-mid) 0%, var(--cp-wall-acc) 36%, var(--cp-wall-bg) 76%)",
    },
} as const;

export type WallpaperId = keyof typeof WALLPAPERS;

export const DEFAULT_WALLPAPER: WallpaperId = "aurora";

export function isWallpaperId(value: string | null | undefined): value is WallpaperId {
    return Boolean(value && value in WALLPAPERS);
}

export function listWallpapers(): WallpaperId[] {
    return Object.keys(WALLPAPERS) as WallpaperId[];
}

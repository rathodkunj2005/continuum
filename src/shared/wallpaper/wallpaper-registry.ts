/** Interactive motion backgrounds for the global wallpaper layer. */

export const WALLPAPERS = {
    aurora: {
        name: "Aurora",
        description: "Flowing northern lights that bend toward your cursor and ripple on click.",
        preview: "linear-gradient(160deg, #0a1408 0%, #1a4020 45%, #3dff6a 100%)",
    },
    nebula: {
        name: "Nebula Drift",
        description: "Soft cosmic clouds pulled by the pointer with luminous click bursts.",
        preview: "radial-gradient(circle at 30% 40%, #4a2080 0%, #0a0818 55%, #120820 100%)",
    },
    plasma: {
        name: "Plasma Pulse",
        description: "Electric color waves that speed up near the mouse and flash when you tap.",
        preview: "linear-gradient(135deg, #1a0530 0%, #6020a0 40%, #ff4080 100%)",
    },
    warpGrid: {
        name: "Warp Grid",
        description: "A perspective lattice that warps and glows around your pointer.",
        preview: "linear-gradient(180deg, #020408 0%, #0a1830 50%, #00ffaa33 100%)",
    },
    liquid: {
        name: "Liquid Glass",
        description: "Metaball fluid that follows the cursor and splashes on every click.",
        preview: "radial-gradient(ellipse at 50% 80%, #1a4038 0%, #050a10 70%)",
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

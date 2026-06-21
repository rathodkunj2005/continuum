import { describe, expect, it } from "vitest";
import {
    getWallpaperAuroraColors,
    getWallpaperFieldColors,
    listPalettes,
    rgbToHex,
    relativeLuminance,
} from "../cinematic-palettes";

describe("getWallpaperAuroraColors", () => {
    it("uses 60/30/10 palette roles for the dark wallpaper field", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("bladeRunner2049", "dark");

        expect(rgbToHex(bg)).toBe("#111929");
        expect(rgbToHex(mid)).toBe("#a87f18");
        expect(rgbToHex(acc)).toBe("#f5a623");
        expect(acc[0]).toBeGreaterThan(mid[0]);
    });

    it("matrix accent is neon green", () => {
        const { acc } = getWallpaperAuroraColors("matrix", "dark");
        expect(acc).toEqual([0, 1, 0.2549019607843137]);
        expect(acc[1]).toBeCloseTo(1, 1);
    });

    it("light mode keeps palette depth instead of a near-white void", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("film", "light");

        expect(rgbToHex(bg)).toBe("#857b6f");
        expect(rgbToHex(mid)).toBe("#ccaf7a");
        expect(relativeLuminance(bg)).toBeLessThan(0.42);
        expect(relativeLuminance(mid)).toBeGreaterThan(relativeLuminance(bg));
        expect(acc).toEqual([0.6392156862745098, 0.35294117647058826, 0.11764705882352941]);
    });

    it("every cinematic light animated wallpaper field has three distinct, non-white palette colors", () => {
        for (const paletteKey of listPalettes()) {
            if (paletteKey === "continuumLight") continue;
            const { bg, mid, acc } = getWallpaperAuroraColors(paletteKey, "light");

            expect(relativeLuminance(bg), `${paletteKey} bg`).toBeLessThan(0.72);
            expect(bg, `${paletteKey} bg/mid`).not.toEqual(mid);
            expect(mid, `${paletteKey} mid/acc`).not.toEqual(acc);
            expect(bg, `${paletteKey} bg/acc`).not.toEqual(acc);
        }
    });
});

describe("getWallpaperFieldColors", () => {
    it("delegates to getWallpaperAuroraColors", () => {
        expect(getWallpaperFieldColors("matrix", "dark")).toEqual(
            getWallpaperAuroraColors("matrix", "dark")
        );
    });
});

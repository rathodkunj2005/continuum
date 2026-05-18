import { describe, expect, it } from "vitest";
import {
    getPaletteShadeRgb,
    getWallpaperAuroraColors,
    getWallpaperFieldColors,
    hexToRgb,
    linearRgbMix3,
    PALETTES,
} from "../cinematic-palettes";

describe("getWallpaperAuroraColors", () => {
    it("uses exact cinematic swatches: void d1–d3, fog d4, pop accent", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("bladeRunner2049", "dark");
        const [d1, d2, d3, d4, , , accent] = PALETTES.bladeRunner2049.shades;

        expect(bg).toEqual(linearRgbMix3(hexToRgb(d1), hexToRgb(d2), hexToRgb(d3)));
        expect(mid).toEqual(hexToRgb(d4));
        expect(acc).toEqual(hexToRgb(accent));
        expect(mid[2]).toBeGreaterThan(mid[0] * 1.4);
        expect(acc[0]).toBeGreaterThan(mid[0]);
    });

    it("matrix accent is neon green swatch #7", () => {
        const { acc } = getWallpaperAuroraColors("matrix", "dark");
        expect(acc).toEqual(getPaletteShadeRgb("matrix", 6));
        expect(acc[1]).toBeCloseTo(1, 1);
    });

    it("light mode keeps paper void with same cinematic mid and accent swatches", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("film", "light");
        expect(bg).toEqual(hexToRgb(PALETTES.film.light.bg));
        expect(mid).toEqual(hexToRgb(PALETTES.film.shades[3]));
        expect(acc).toEqual(hexToRgb(PALETTES.film.shades[6]));
    });
});

describe("getWallpaperFieldColors", () => {
    it("delegates to getWallpaperAuroraColors", () => {
        expect(getWallpaperFieldColors("matrix", "dark")).toEqual(
            getWallpaperAuroraColors("matrix", "dark")
        );
    });
});

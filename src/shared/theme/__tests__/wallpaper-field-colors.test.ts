import { describe, expect, it } from "vitest";
import {
    getWallpaperAuroraColors,
    getWallpaperFieldColors,
    PALETTES,
} from "../cinematic-palettes";

describe("getWallpaperAuroraColors", () => {
    it("returns the palette's hand-tuned dark aurora triple", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("bladeRunner2049", "dark");
        const aurora = PALETTES.bladeRunner2049.aurora.dark;

        expect(bg).toEqual(aurora.bg);
        expect(mid).toEqual(aurora.mid);
        expect(acc).toEqual(aurora.acc);
        expect(acc[0]).toBeGreaterThan(mid[0]);
    });

    it("matrix accent is neon green", () => {
        const { acc } = getWallpaperAuroraColors("matrix", "dark");
        expect(acc).toEqual(PALETTES.matrix.aurora.dark.acc);
        expect(acc[1]).toBeCloseTo(1, 1);
    });

    it("light mode returns the palette's hand-tuned light aurora triple", () => {
        const { bg, mid, acc } = getWallpaperAuroraColors("film", "light");
        const aurora = PALETTES.film.aurora.light;
        expect(bg).toEqual(aurora.bg);
        expect(mid).toEqual(aurora.mid);
        expect(acc).toEqual(aurora.acc);
    });
});

describe("getWallpaperFieldColors", () => {
    it("delegates to getWallpaperAuroraColors", () => {
        expect(getWallpaperFieldColors("matrix", "dark")).toEqual(
            getWallpaperAuroraColors("matrix", "dark")
        );
    });
});

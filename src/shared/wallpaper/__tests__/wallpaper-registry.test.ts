import { describe, expect, it } from "vitest";
import { DEFAULT_WALLPAPER, WALLPAPERS, isWallpaperId, listWallpapers } from "../wallpaper-registry";

describe("wallpaper-registry", () => {
    it("lists five motion backgrounds", () => {
        expect(listWallpapers()).toHaveLength(5);
        expect(listWallpapers()).toEqual(
            expect.arrayContaining(["aurora", "nebula", "plasma", "warpGrid", "liquid"])
        );
    });

    it("validates wallpaper ids", () => {
        expect(isWallpaperId("aurora")).toBe(true);
        expect(isWallpaperId("invalid")).toBe(false);
        expect(isWallpaperId(null)).toBe(false);
    });

    it("defaults to aurora", () => {
        expect(DEFAULT_WALLPAPER).toBe("aurora");
        expect(WALLPAPERS.aurora.name).toBe("Aurora");
    });
});

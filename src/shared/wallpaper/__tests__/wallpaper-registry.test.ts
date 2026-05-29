import { describe, expect, it } from "vitest";
import { SHADER_SOURCES } from "../shader-sources";
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

    it("grades every motion shader through the shared cinematic palette base", () => {
        for (const wallpaperId of listWallpapers()) {
            expect(SHADER_SOURCES[wallpaperId], wallpaperId).toContain("cinematicBase(uv)");
        }
    });

    it("keeps motion previews tied to active cinematic palette variables", () => {
        for (const wallpaperId of listWallpapers()) {
            expect(WALLPAPERS[wallpaperId].preview, wallpaperId).toContain("--cp-wall-bg");
            expect(WALLPAPERS[wallpaperId].preview, wallpaperId).toContain("--cp-wall-mid");
            expect(WALLPAPERS[wallpaperId].preview, wallpaperId).toContain("--cp-wall-acc");
        }
    });
});

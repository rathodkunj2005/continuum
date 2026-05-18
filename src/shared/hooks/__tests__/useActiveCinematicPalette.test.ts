import { describe, expect, it } from "vitest";
import { getWallpaperFieldColors } from "@/shared/theme/cinematic-palettes";

describe("wallpaper field palette triples", () => {
    it("derives 60-30-10 bg/mid/acc from cinematic shades", () => {
        const blade = getWallpaperFieldColors("bladeRunner2049");
        const bgLum = blade.bg[0] * 0.2126 + blade.bg[1] * 0.7152 + blade.bg[2] * 0.0722;
        const midLum = blade.mid[0] * 0.2126 + blade.mid[1] * 0.7152 + blade.mid[2] * 0.0722;
        expect(bgLum).toBeLessThan(midLum + 0.2);
        expect(blade.acc[0]).toBeGreaterThan(blade.bg[0]);
    });
});

import { afterEach, describe, expect, it } from "vitest";
import {
    applyPalette,
    getWallpaperInkColors,
    PALETTES,
    removePalette,
    rgbToHex,
} from "../cinematic-palettes";

describe("applyPalette", () => {
    afterEach(() => {
        removePalette();
    });

    it("injects --cp-bg from the selected palette onto :root", () => {
        applyPalette("bladeRunner2049", "dark");

        const style = document.getElementById("cinematic-palette-vars");
        expect(style?.textContent).toContain('--cp-bg: #000000');
        expect(style?.textContent).toContain('--cp-active-palette: "bladeRunner2049"');
    });

    it("updates tokens when switching palettes", () => {
        applyPalette("film", "dark");
        applyPalette("matrix", "dark");

        const style = document.getElementById("cinematic-palette-vars");
        expect(style?.textContent).toContain('--cp-bg: #000000');
        expect(style?.textContent).toContain('--cp-accent: #00ff41');
    });

    it("injects wall ink from the active theme tokens", () => {
        applyPalette("matrix", "dark");

        const style = document.getElementById("cinematic-palette-vars");
        const ink = getWallpaperInkColors("matrix", "dark");
        expect(ink.primary).toBe(PALETTES.matrix.dark.textPrimary);
        expect(style?.textContent).toContain(`--cp-wall-text-primary: ${ink.primary}`);
    });

    it("injects exact wall swatch hex from aurora table for CSS fallback", () => {
        applyPalette("bladeRunner2049", "dark");

        const style = document.getElementById("cinematic-palette-vars");
        const aurora = PALETTES.bladeRunner2049.aurora.dark;
        expect(style?.textContent).toContain(`--cp-wall-bg: ${rgbToHex(aurora.bg)}`);
        expect(style?.textContent).toContain(`--cp-wall-acc: ${rgbToHex(aurora.acc)}`);
    });
});

describe("getWallpaperInkColors", () => {
    it("uses dark-mode text on black wallpaper void", () => {
        const ink = getWallpaperInkColors("bladeRunner2049", "dark");
        expect(ink.primary).toBe(PALETTES.bladeRunner2049.dark.textPrimary);
    });

    it("uses light-mode text on paper wallpaper void", () => {
        const ink = getWallpaperInkColors("grandBudapestHotel", "light");
        expect(ink.primary).toBe(PALETTES.grandBudapestHotel.light.textPrimary);
    });

    it("uses light-mode UI ink when theme is light", () => {
        const ink = getWallpaperInkColors("film", "light");
        expect(ink.primary).toBe(PALETTES.film.light.textPrimary);
    });
});

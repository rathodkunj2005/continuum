export type PaletteMode = "dark" | "light";

export interface CinematicPalette {
    name: string;
    year: number;
    director: string;
    description: string;
    shades: [string, string, string, string, string, string, string];
    dark: PaletteTokens;
    light: PaletteTokens;
    aurora: {
        dark:  { bg: [number, number, number]; mid: [number, number, number]; acc: [number, number, number] };
        light: { bg: [number, number, number]; mid: [number, number, number]; acc: [number, number, number] };
    };
}

interface PaletteTokens {
    bg: string;
    surface: string;
    surfaceRaised: string;
    border: string;
    borderStrong: string;
    textPrimary: string;
    textSecondary: string;
    textInverse: string;
    accent: string;
    accentMuted: string;
    accentSubtle: string;
}

export const PALETTES = {
    film: {
        name: "Old Film",
        year: 2026,
        director: "FNDR",
        description: "Personal memory, processed like film. Amber halation over deep umber.",
        shades: ["#1a1410", "#221915", "#2a2018", "#352a20", "#a37a30", "#d4a04a", "#e8b85a"],
        dark: {
            bg: "#1a1410",
            surface: "#221915",
            surfaceRaised: "#2a2018",
            border: "rgba(232, 223, 200, 0.08)",
            borderStrong: "rgba(232, 223, 200, 0.22)",
            textPrimary: "#e8dfc8",
            textSecondary: "#c4a878",
            textInverse: "#1a1410",
            accent: "#d4a04a",
            accentMuted: "#a37a30",
            accentSubtle: "#2a2018",
        },
        light: {
            bg: "#f2ead8",
            surface: "#e8dfc8",
            surfaceRaised: "#ddd3bc",
            border: "rgba(42, 31, 26, 0.10)",
            borderStrong: "rgba(42, 31, 26, 0.30)",
            textPrimary: "#2a1f1a",
            textSecondary: "#5a4a3a",
            textInverse: "#f2ead8",
            accent: "#a35a1e",
            accentMuted: "#c4621e",
            accentSubtle: "#e8dfc8",
        },
        aurora: {
            dark:  { bg: [0.102, 0.078, 0.063], mid: [0.769, 0.659, 0.471], acc: [0.831, 0.627, 0.290] },
            light: { bg: [0.949, 0.918, 0.847], mid: [0.353, 0.290, 0.227], acc: [0.639, 0.353, 0.118] },
        },
    },
    matrix: {
        name: "The Matrix",
        year: 1999,
        director: "The Wachowskis",
        description: "Phosphor-green tones swimming in the digital void",
        shades: ["#050f05", "#0d1f0d", "#163016", "#1f4a1f", "#336633", "#4d9a4d", "#00ff41"],
        dark: {
            bg: "#000000",
            surface: "#050f05",
            surfaceRaised: "#0d1f0d",
            border: "#163016",
            borderStrong: "#1f4a1f",
            textPrimary: "#a8e6a8",
            textSecondary: "#4d9a4d",
            textInverse: "#000000",
            accent: "#00ff41",
            accentMuted: "#4d9a4d",
            accentSubtle: "#0d1f0d",
        },
        light: {
            bg: "#ffffff",
            surface: "#f0faf0",
            surfaceRaised: "#e0f5e0",
            border: "#a8d8a8",
            borderStrong: "#4d9a4d",
            textPrimary: "#0d2a0d",
            textSecondary: "#336633",
            textInverse: "#ffffff",
            accent: "#1a7a1a",
            accentMuted: "#2d9a2d",
            accentSubtle: "#e8f8e8",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.302, 0.604, 0.302], acc: [0.000, 1.000, 0.255] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.200, 0.400, 0.200], acc: [0.102, 0.478, 0.102] },
        },
    },
    bladeRunner2049: {
        name: "Blade Runner 2049",
        year: 2017,
        director: "Denis Villeneuve",
        description: "Amber neon bleeding through indigo fog",
        shades: ["#080c14", "#101828", "#1a2840", "#253858", "#8b6914", "#c4941d", "#f5a623"],
        dark: {
            bg: "#000000",
            surface: "#080c14",
            surfaceRaised: "#101828",
            border: "#1a2840",
            borderStrong: "#253858",
            textPrimary: "#d4b896",
            textSecondary: "#8b6914",
            textInverse: "#080c14",
            accent: "#f5a623",
            accentMuted: "#c4941d",
            accentSubtle: "#1a1808",
        },
        light: {
            bg: "#ffffff",
            surface: "#faf8f4",
            surfaceRaised: "#f0ebe0",
            border: "#d4c4a0",
            borderStrong: "#8b6914",
            textPrimary: "#1a1408",
            textSecondary: "#6b5010",
            textInverse: "#ffffff",
            accent: "#c47a10",
            accentMuted: "#d48e20",
            accentSubtle: "#fdf5e6",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.545, 0.412, 0.078], acc: [0.961, 0.651, 0.137] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.420, 0.314, 0.063], acc: [0.769, 0.478, 0.063] },
        },
    },
    madMaxFuryRoad: {
        name: "Mad Max: Fury Road",
        year: 2015,
        director: "George Miller",
        description: "Scorched copper earth beneath a chrome and teal sky",
        shades: ["#1a0e05", "#2e1a08", "#4a2c0f", "#6b3f15", "#1a4a5c", "#2a7a9a", "#e8f4f8"],
        dark: {
            bg: "#000000",
            surface: "#1a0e05",
            surfaceRaised: "#2e1a08",
            border: "#4a2c0f",
            borderStrong: "#6b3f15",
            textPrimary: "#e8c8a0",
            textSecondary: "#a87840",
            textInverse: "#1a0e05",
            accent: "#2a9abf",
            accentMuted: "#1a7a9a",
            accentSubtle: "#0a1a20",
        },
        light: {
            bg: "#ffffff",
            surface: "#fdf8f2",
            surfaceRaised: "#f5ece0",
            border: "#d4b090",
            borderStrong: "#8b5a28",
            textPrimary: "#2e1a08",
            textSecondary: "#6b3f15",
            textInverse: "#ffffff",
            accent: "#1a6a8a",
            accentMuted: "#2a7a9a",
            accentSubtle: "#e8f4f8",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.659, 0.471, 0.251], acc: [0.165, 0.604, 0.749] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.420, 0.247, 0.082], acc: [0.102, 0.416, 0.541] },
        },
    },
    her: {
        name: "Her",
        year: 2013,
        director: "Spike Jonze",
        description: "Warm ember light and dusty coral in a future that feels like home",
        shades: ["#1a0c08", "#2e1810", "#4a2a1e", "#6b3f2e", "#8c3a2e", "#b54c3c", "#e8533f"],
        dark: {
            bg: "#000000",
            surface: "#1a0c08",
            surfaceRaised: "#2e1810",
            border: "#4a2a1e",
            borderStrong: "#6b3f2e",
            textPrimary: "#e8c4b0",
            textSecondary: "#a87860",
            textInverse: "#1a0c08",
            accent: "#e8533f",
            accentMuted: "#b54c3c",
            accentSubtle: "#2a1008",
        },
        light: {
            bg: "#ffffff",
            surface: "#fdf8f5",
            surfaceRaised: "#f8ece5",
            border: "#e0c4b4",
            borderStrong: "#b57060",
            textPrimary: "#2e1810",
            textSecondary: "#6b3f2e",
            textInverse: "#ffffff",
            accent: "#c03828",
            accentMuted: "#d84838",
            accentSubtle: "#fdf0ec",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.659, 0.471, 0.376], acc: [0.910, 0.325, 0.247] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.420, 0.247, 0.180], acc: [0.753, 0.220, 0.157] },
        },
    },
    moonlight: {
        name: "Moonlight",
        year: 2016,
        director: "Barry Jenkins",
        description: "Deep ocean midnight illuminated by bioluminescent gold",
        shades: ["#050810", "#0a1020", "#101830", "#182440", "#1a4a5a", "#257a8a", "#e8a040"],
        dark: {
            bg: "#000000",
            surface: "#050810",
            surfaceRaised: "#0a1020",
            border: "#101830",
            borderStrong: "#182440",
            textPrimary: "#b0d4e0",
            textSecondary: "#257a8a",
            textInverse: "#050810",
            accent: "#e8a040",
            accentMuted: "#c07820",
            accentSubtle: "#1a1205",
        },
        light: {
            bg: "#ffffff",
            surface: "#f2f8fa",
            surfaceRaised: "#e4f0f5",
            border: "#a0c8d8",
            borderStrong: "#257a8a",
            textPrimary: "#0a1820",
            textSecondary: "#1a4a5a",
            textInverse: "#ffffff",
            accent: "#c07820",
            accentMuted: "#d08830",
            accentSubtle: "#fdf8ef",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.145, 0.478, 0.541], acc: [0.910, 0.627, 0.251] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.102, 0.290, 0.353], acc: [0.753, 0.471, 0.125] },
        },
    },
    grandBudapestHotel: {
        name: "The Grand Budapest Hotel",
        year: 2014,
        director: "Wes Anderson",
        description: "Powdery mauve pastels, deep crimson velvet, and Mendl's gold",
        shades: ["#1a0a14", "#2e1424", "#4a2038", "#6b304e", "#8c1a40", "#b5245c", "#f5c842"],
        dark: {
            bg: "#000000",
            surface: "#1a0a14",
            surfaceRaised: "#2e1424",
            border: "#4a2038",
            borderStrong: "#6b304e",
            textPrimary: "#e8c4d0",
            textSecondary: "#b5245c",
            textInverse: "#1a0a14",
            accent: "#f5c842",
            accentMuted: "#d4a820",
            accentSubtle: "#1a1505",
        },
        light: {
            bg: "#ffffff",
            surface: "#fdf5f8",
            surfaceRaised: "#f8e8f0",
            border: "#e0b0c8",
            borderStrong: "#8c1a40",
            textPrimary: "#2e1424",
            textSecondary: "#6b304e",
            textInverse: "#ffffff",
            accent: "#b89010",
            accentMuted: "#c8a020",
            accentSubtle: "#fdfaed",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.710, 0.141, 0.361], acc: [0.961, 0.784, 0.259] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.420, 0.188, 0.306], acc: [0.722, 0.565, 0.063] },
        },
    },
    drive: {
        name: "Drive",
        year: 2011,
        director: "Nicolas Winding Refn",
        description: "Neon magenta bleeding over deep navy and blood red",
        shades: ["#050510", "#0a0a20", "#101030", "#181840", "#5a0a1a", "#8c1030", "#ff2d6b"],
        dark: {
            bg: "#000000",
            surface: "#050510",
            surfaceRaised: "#0a0a20",
            border: "#101030",
            borderStrong: "#181840",
            textPrimary: "#e0d0f0",
            textSecondary: "#8c1030",
            textInverse: "#050510",
            accent: "#ff2d6b",
            accentMuted: "#cc2058",
            accentSubtle: "#1a0510",
        },
        light: {
            bg: "#ffffff",
            surface: "#f8f5ff",
            surfaceRaised: "#f0ebff",
            border: "#c8b0e0",
            borderStrong: "#8c1030",
            textPrimary: "#100a20",
            textSecondary: "#5a0a1a",
            textInverse: "#ffffff",
            accent: "#cc1058",
            accentMuted: "#e01a6a",
            accentSubtle: "#fff0f5",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.549, 0.063, 0.188], acc: [1.000, 0.176, 0.420] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.353, 0.039, 0.102], acc: [0.800, 0.063, 0.345] },
        },
    },
    amelie: {
        name: "Amélie",
        year: 2001,
        director: "Jean-Pierre Jeunet",
        description: "Vivid Montmartre greens and saturated Amélie reds with golden warmth",
        shades: ["#050f08", "#0a1e10", "#102e18", "#184523", "#8c1a14", "#b5241c", "#f5c028"],
        dark: {
            bg: "#000000",
            surface: "#050f08",
            surfaceRaised: "#0a1e10",
            border: "#102e18",
            borderStrong: "#184523",
            textPrimary: "#d4e8c0",
            textSecondary: "#4d8a4d",
            textInverse: "#050f08",
            accent: "#f5c028",
            accentMuted: "#c89c18",
            accentSubtle: "#181205",
        },
        light: {
            bg: "#ffffff",
            surface: "#f4faf5",
            surfaceRaised: "#e8f5ea",
            border: "#a8d0b0",
            borderStrong: "#184523",
            textPrimary: "#0a1e10",
            textSecondary: "#2e5a30",
            textInverse: "#ffffff",
            accent: "#c89c18",
            accentMuted: "#d8ac20",
            accentSubtle: "#fdf8e8",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.302, 0.541, 0.302], acc: [0.961, 0.753, 0.157] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.180, 0.353, 0.188], acc: [0.784, 0.612, 0.094] },
        },
    },
    noCountryForOldMen: {
        name: "No Country for Old Men",
        year: 2007,
        director: "The Coen Brothers",
        description: "Sun-bleached West Texas dust and rust under a merciless sky",
        shades: ["#0f0c08", "#1e1810", "#32271a", "#4a3a26", "#6b3a1a", "#8c4e24", "#c42020"],
        dark: {
            bg: "#000000",
            surface: "#0f0c08",
            surfaceRaised: "#1e1810",
            border: "#32271a",
            borderStrong: "#4a3a26",
            textPrimary: "#d4c4a8",
            textSecondary: "#8c7858",
            textInverse: "#0f0c08",
            accent: "#c42020",
            accentMuted: "#8c1818",
            accentSubtle: "#1a0808",
        },
        light: {
            bg: "#ffffff",
            surface: "#faf8f4",
            surfaceRaised: "#f5eedf",
            border: "#d4c4a0",
            borderStrong: "#8c7858",
            textPrimary: "#1e1810",
            textSecondary: "#4a3a26",
            textInverse: "#ffffff",
            accent: "#a01818",
            accentMuted: "#b82020",
            accentSubtle: "#fdf4f4",
        },
        aurora: {
            dark:  { bg: [0.000, 0.000, 0.000], mid: [0.549, 0.471, 0.345], acc: [0.769, 0.125, 0.125] },
            light: { bg: [1.000, 1.000, 1.000], mid: [0.290, 0.227, 0.149], acc: [0.627, 0.094, 0.094] },
        },
    },
    fndrDark: {
        name: "Nocturne",
        year: 2026,
        director: "Anurup",
        description: "The original quiet dark FNDR interface palette",
        shades: ["#0a0a0a", "#111111", "#151515", "#232323", "#6b7280", "#a1a1aa", "#f3f3f1"],
        dark: {
            bg: "#0a0a0a",
            surface: "#111111",
            surfaceRaised: "#151515",
            border: "#232323",
            borderStrong: "#3a3a40",
            textPrimary: "#f3f3f1",
            textSecondary: "#a1a1aa",
            textInverse: "#0a0a0a",
            accent: "#f3f3f1",
            accentMuted: "#ffffff",
            accentSubtle: "rgba(255, 255, 255, 0.1)",
        },
        light: {
            bg: "#0a0a0a",
            surface: "#111111",
            surfaceRaised: "#151515",
            border: "#232323",
            borderStrong: "#3a3a40",
            textPrimary: "#f3f3f1",
            textSecondary: "#a1a1aa",
            textInverse: "#0a0a0a",
            accent: "#f3f3f1",
            accentMuted: "#ffffff",
            accentSubtle: "rgba(255, 255, 255, 0.1)",
        },
        aurora: {
            dark:  { bg: [0.039, 0.039, 0.039], mid: [0.471, 0.471, 0.510], acc: [0.953, 0.953, 0.945] },
            light: { bg: [0.039, 0.039, 0.039], mid: [0.471, 0.471, 0.510], acc: [0.953, 0.953, 0.945] },
        },
    },
    fndrLight: {
        name: "Lumen",
        year: 2026,
        director: "Anurup",
        description: "The original clean light FNDR interface palette",
        shades: ["#f5f5f7", "#ffffff", "#f0f0f2", "#d8d8dc", "#8e8e93", "#555560", "#1a1a1a"],
        dark: {
            bg: "#f5f5f7",
            surface: "#ffffff",
            surfaceRaised: "#f0f0f2",
            border: "#d8d8dc",
            borderStrong: "#b8b8c0",
            textPrimary: "#1a1a1a",
            textSecondary: "#555560",
            textInverse: "#ffffff",
            accent: "#1a1a1a",
            accentMuted: "#000000",
            accentSubtle: "rgba(0, 0, 0, 0.06)",
        },
        light: {
            bg: "#f5f5f7",
            surface: "#ffffff",
            surfaceRaised: "#f0f0f2",
            border: "#d8d8dc",
            borderStrong: "#b8b8c0",
            textPrimary: "#1a1a1a",
            textSecondary: "#555560",
            textInverse: "#ffffff",
            accent: "#1a1a1a",
            accentMuted: "#000000",
            accentSubtle: "rgba(0, 0, 0, 0.06)",
        },
        aurora: {
            dark:  { bg: [0.961, 0.961, 0.969], mid: [0.333, 0.333, 0.376], acc: [0.102, 0.102, 0.102] },
            light: { bg: [0.961, 0.961, 0.969], mid: [0.333, 0.333, 0.376], acc: [0.102, 0.102, 0.102] },
        },
    },
} as const satisfies Record<string, CinematicPalette>;

export type PaletteKey = keyof typeof PALETTES;

const STYLE_TAG_ID = "cinematic-palette-vars";

type RgbTriple = [number, number, number];

/** Parse #rrggbb to linear 0–1 RGB for WebGL / CSS injection. */
export function hexToRgb(hex: string): RgbTriple {
    const h = hex.trim().replace("#", "");
    const n = Number.parseInt(h, 16);
    if (Number.isNaN(n) || h.length < 6) {
        return [0, 0, 0];
    }
    return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** Convert 0–1 linear RGB triple back to a #rrggbb hex string. */
export function rgbToHex(rgb: RgbTriple): string {
    const r = Math.round(Math.max(0, Math.min(1, rgb[0])) * 255);
    const g = Math.round(Math.max(0, Math.min(1, rgb[1])) * 255);
    const b = Math.round(Math.max(0, Math.min(1, rgb[2])) * 255);
    return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

/** WCAG relative luminance for linear 0–1 RGB. */
export function relativeLuminance(rgb: RgbTriple): number {
    const lin = rgb.map((c) =>
        c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)
    ) as RgbTriple;
    return lin[0] * 0.2126 + lin[1] * 0.7152 + lin[2] * 0.0722;
}

/** Mean of three linear RGB triples (cinematic swatches 1–3 void). */
export function linearRgbMix3(a: RgbTriple, b: RgbTriple, c: RgbTriple): RgbTriple {
    return [(a[0] + b[0] + c[0]) / 3, (a[1] + b[1] + c[1]) / 3, (a[2] + b[2] + c[2]) / 3];
}

/** Exact cinematic swatch by index (0–6), same hex as the palette picker. */
export function getPaletteShadeRgb(paletteKey: PaletteKey, index: number): RgbTriple {
    const shades = PALETTES[paletteKey].shades;
    const hex = shades[Math.max(0, Math.min(shades.length - 1, index))];
    return hexToRgb(hex);
}

/** Readable ink for hero copy and OS chrome on the motion wallpaper. */
export function getWallpaperInkColors(
    paletteKey: PaletteKey,
    mode: PaletteMode = "dark"
): {
    primary: string;
    secondary: string;
    fieldLuminance: number;
} {
    const palette = PALETTES[paletteKey];
    const tokens = palette[mode];
    const { bg } = getWallpaperAuroraColors(paletteKey, mode);
    const anchorLum = relativeLuminance(bg);
    return {
        primary: tokens.textPrimary,
        secondary: tokens.textSecondary,
        fieldLuminance: anchorLum,
    };
}

/** Wallpaper field triple: void + saturated primary glow. */
export function getWallpaperFieldColors(paletteKey: PaletteKey, mode: PaletteMode = "dark") {
    return getWallpaperAuroraColors(paletteKey, mode);
}

export function isPaletteKey(value: string | null): value is PaletteKey {
    return Boolean(value && value in PALETTES);
}

export function applyPalette(paletteKey: PaletteKey, mode: PaletteMode = "dark", selector = ":root") {
    const palette = PALETTES[paletteKey];
    const tokens = palette[mode];
    const [d1, d2, d3, d4, s1, s2, accent] = palette.shades;
    const aurWall = getWallpaperAuroraColors(paletteKey, mode);
    const wallInk = getWallpaperInkColors(paletteKey, mode);

    const css = `
${selector} {
  --cp-bg: ${tokens.bg};
  --cp-surface: ${tokens.surface};
  --cp-surface-raised: ${tokens.surfaceRaised};
  --cp-border: ${tokens.border};
  --cp-border-strong: ${tokens.borderStrong};
  --cp-text-primary: ${tokens.textPrimary};
  --cp-text-secondary: ${tokens.textSecondary};
  --cp-text-inverse: ${tokens.textInverse};
  --cp-accent: ${tokens.accent};
  --cp-accent-muted: ${tokens.accentMuted};
  --cp-accent-subtle: ${tokens.accentSubtle};
  --cp-dominant-1: ${d1};
  --cp-dominant-2: ${d2};
  --cp-dominant-3: ${d3};
  --cp-dominant-4: ${d4};
  --cp-secondary-1: ${s1};
  --cp-secondary-2: ${s2};
  --cp-accent-raw: ${accent};
  --cp-active-palette: "${paletteKey}";
  --cp-active-mode: "${mode}";
  --cp-wall-text-primary: ${wallInk.primary};
  --cp-wall-text-secondary: ${wallInk.secondary};
  --cp-wall-bg: ${rgbToHex(aurWall.bg)};
  --cp-wall-mid: ${rgbToHex(aurWall.mid)};
  --cp-wall-acc: ${rgbToHex(aurWall.acc)};
  --cp-aurora-bg-r: ${aurWall.bg[0]};
  --cp-aurora-bg-g: ${aurWall.bg[1]};
  --cp-aurora-bg-b: ${aurWall.bg[2]};
  --cp-aurora-mid-r: ${aurWall.mid[0]};
  --cp-aurora-mid-g: ${aurWall.mid[1]};
  --cp-aurora-mid-b: ${aurWall.mid[2]};
  --cp-aurora-acc-r: ${aurWall.acc[0]};
  --cp-aurora-acc-g: ${aurWall.acc[1]};
  --cp-aurora-acc-b: ${aurWall.acc[2]};
}`.trim();

    let tag = document.getElementById(STYLE_TAG_ID);
    if (!tag) {
        tag = document.createElement("style");
        tag.id = STYLE_TAG_ID;
        document.head.appendChild(tag);
    }
    tag.textContent = css;
}

export function removePalette() {
    document.getElementById(STYLE_TAG_ID)?.remove();
}

export function getPaletteTokens(paletteKey: PaletteKey, mode: PaletteMode = "dark") {
    const palette = PALETTES[paletteKey];
    const tokens = palette[mode];
    const [d1, d2, d3, d4, s1, s2, accent] = palette.shades;
    return {
        "--cp-bg": tokens.bg,
        "--cp-surface": tokens.surface,
        "--cp-surface-raised": tokens.surfaceRaised,
        "--cp-border": tokens.border,
        "--cp-border-strong": tokens.borderStrong,
        "--cp-text-primary": tokens.textPrimary,
        "--cp-text-secondary": tokens.textSecondary,
        "--cp-text-inverse": tokens.textInverse,
        "--cp-accent": tokens.accent,
        "--cp-accent-muted": tokens.accentMuted,
        "--cp-accent-subtle": tokens.accentSubtle,
        "--cp-dominant-1": d1,
        "--cp-dominant-2": d2,
        "--cp-dominant-3": d3,
        "--cp-dominant-4": d4,
        "--cp-secondary-1": s1,
        "--cp-secondary-2": s2,
        "--cp-accent-raw": accent,
    };
}

export function getAuroraColors(
    paletteKey: PaletteKey,
    mode: PaletteMode
): { bg: [number, number, number]; mid: [number, number, number]; acc: [number, number, number] } {
    return PALETTES[paletteKey].aurora[mode];
}

/**
 * Wallpaper shader triple — cinematic aurora values tuned per palette/mode.
 * Delegates to the palette's aurora table so dark and light modes both render
 * correctly against the active background.
 */
export function getWallpaperAuroraColors(paletteKey: PaletteKey, mode: PaletteMode = "dark") {
    return PALETTES[paletteKey].aurora[mode];
}

export function listPalettes() {
    return Object.keys(PALETTES) as PaletteKey[];
}

export default PALETTES;

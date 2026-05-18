# Hero Parallax & Cinematic Background Design
*2026-05-18*

## Context

The FNDR home screen hero currently shows a static "Memory, developed." heading with two CTAs over a radial-gradient background and a drifting constellation SVG. The AuroraWallpaper WebGL background already tracks mouse position and click ripples, but:

1. Hero DOM elements do not respond to mouse — there is no layered parallax on the text.
2. Aurora colors are hardcoded to two themes (`film` / `paper`) independent of the cinematic palette system.
3. The hero layout does not match the target design: no time-based greeting, no date chip, no inline search bar with voice input, no scroll indicator.

## Goals

- Match the screenshot layout (date chip → time-based greeting → subtitle → search pill → CTAs → scroll indicator).
- Keep existing CTAs ("Enter the reel" / "Open work mode") below the search bar.
- Add dramatic multi-depth mouse parallax (20–30px) on hero elements.
- Wire aurora colors to active cinematic palette so background changes with appearance.
- Speak button triggers existing voice-transcription flow from SearchBar.tsx.
- Support both dark and light modes.

## Architecture

Four areas of change:

```
cinematic-palettes.ts      → add aurora: {bg,mid,acc} token per palette
AuroraWallpaper.tsx        → accept auroraBg/aurMid/aurAcc props; drop hardcoded PAL
src/app/ScrollModeShell.tsx    → greeting/date state; pass aurora props + scroll callback
HeroSection.tsx / .css         → new layout + mouse parallax hook
```

Note: `ScrollModeShell.tsx` (not `App.tsx`) hosts `HeroSection` and `AuroraWallpaper`.
`getFunGreeting(displayName)` and `getOnboardingState()` are importable from existing modules.
`formatHomeDate` moves from `App.tsx` to `@/shared/utils/dateFormat` as a shared export.

No new packages required (Framer Motion already present).

## Palette Token Extension

Each palette in `cinematic-palettes.ts` gets an `aurora` object:

```ts
aurora: {
  dark:  { bg: [r,g,b], mid: [r,g,b], acc: [r,g,b] },
  light: { bg: [r,g,b], mid: [r,g,b], acc: [r,g,b] },
}
```

Values are `[0..1]` RGB (matching the existing shader uniform format).  
`getPaletteTokens(key, mode)` returns these alongside other tokens.  
`listPalettes()` already exports all palettes — aurora fields are added inline.

## AuroraWallpaper Changes

Accept three new optional props:

```ts
interface AuroraWallpaper {
  ...existing...
  auroraBg?:  [number, number, number];
  aurMid?:    [number, number, number];
  aurAcc?:    [number, number, number];
}
```

Inside the rAF loop, replace `PAL[theme]` lookup with prop values when present. The existing lerp (lk=0.028) already handles smooth color transitions — palette changes will fade in automatically.

## App.tsx Changes

- Read `activeKey` + `activeMode` from localStorage on mount (already done for applyPalette).
- Derive `auroraBg/aurMid/aurAcc` from `getPaletteTokens(activeKey, activeMode).aurora`.
- Pass as props to `<AuroraWallpaper>`.
- Re-derive on `selectAppearance` callback (already fires on palette/theme change in ControlPanel).

## HeroSection Layout

```
                [date chip: SUNDAY • MAY 17]
         [h1: Good Night, Anurup!]          ← depth 22px
         [p: Let's dive into your memories.] ← depth 16px

  [🔍  What shall we uncover tonight?  ⏸ Speak  →]  ← depth 10px

         [Enter the reel]  [Open work mode]   ← depth 6px

                  SCROLL TO EXPLORE
                       │  (animated dot)      ← depth 4px
```

- Content is centered horizontally and vertically.
- The REEL-number label and "Memory, developed." heading are removed from the hero section; the time-based greeting replaces them.
- The frame-count decoration numeral moves to a lower-opacity overlay or is dropped.

### Greeting Logic

```ts
function getGreeting(name: string): { salutation: string; subtitle: string } {
  const h = new Date().getHours();
  const salutation =
    h < 12 ? `Good Morning, ${name}!`
    : h < 17 ? `Good Afternoon, ${name}!`
    : h < 21 ? `Good Evening, ${name}!`
    : `Good Night, ${name}!`;
  const subtitle =
    h < 12 ? "Let's see what the morning holds."
    : h < 17 ? "Let's pick up where you left off."
    : h < 21 ? "Let's revisit your day."
    : "Let's dive into your memories.";
  return { salutation, subtitle };
}
```

Name comes from existing `getStatus()` → `status.user_name` or falls back to "Anurup".

### Date Chip

```ts
const dateLabel = today.toLocaleDateString("en-US", {
  weekday: "long", month: "long", day: "numeric"
}).toUpperCase();
// → "SUNDAY • MAY 17"  (format as WEEKDAY • MONTH DAY)
```

### Mouse Parallax

```tsx
const mx = useMotionValue(0);  // normalized -1..1
const my = useMotionValue(0);
const sx = useSpring(mx, { stiffness: 80, damping: 22, mass: 1 });
const sy = useSpring(my, { stiffness: 80, damping: 22, mass: 1 });

function onMouseMove(e: React.MouseEvent) {
  const rect = e.currentTarget.getBoundingClientRect();
  mx.set((e.clientX - rect.left) / rect.width * 2 - 1);
  my.set((e.clientY - rect.top)  / rect.height * 2 - 1);
}

// Per layer (maxPx = 30 for title):
const tx = useTransform(sx, [-1, 1], [-maxPx, maxPx]);
const ty = useTransform(sy, [-1, 1], [-maxPx, maxPx]);
```

Depth coefficients: date=0.2, title=1.0, subtitle=0.55, search=0.35, ctas=0.2, scroll-hint=0.13.
Reduced-motion guard: skip parallax entirely when `useReducedMotionSafe().reduced`.

### Search Bar

A pill-shaped input (height 56px, border-radius 28px) using existing `--cp-surface` + `backdrop-blur: 16px`.

- Left: `<SearchIcon>` 20px + `<input>` placeholder cycling through time-appropriate phrases.
- Right: `HeroVoiceButton` + arrow submit button.

On submit → `onScrollToSearch(query)` callback (new prop on HeroSection) which:
1. Sets the query in the SearchSection state (via lifted state or event).
2. Smooth-scrolls to `#fndr-section-search`.

### Voice Button (HeroVoiceButton)

Extracts the `MediaRecorder` + `transcribeVoiceInput` pattern from `SearchBar.tsx` into a local hook `useHeroVoice(onTranscript: (text: string) => void)`.

- Idle: microphone-bars icon + "Speak" label.
- Recording: pulsing waveform animation + "Stop" label.
- On transcript: sets search input value.

### Scroll Indicator

```
SCROLL TO EXPLORE (Cutive Mono, 9px, 0.24em tracking, opacity 0.45)
│  (48px vertical line)
●  (4px dot, animated translateY 0→40px with 1.8s ease-in-out infinite)
```

## HeroSection Props (updated)

```ts
interface HeroSectionProps {
  onEnterReel: () => void;
  onEnterWorkMode: () => void;
  onScrollToSearch: (query: string) => void;  // new
  userName?: string;                           // new, optional fallback
}
```

## Critical Files

| File | Change |
|------|--------|
| `src/shared/theme/cinematic-palettes.ts` | Add `aurora` token to all 11 palettes |
| `src/shared/components/AuroraWallpaper.tsx` | Accept aurora RGB props; replace PAL lookup |
| `src/app/ScrollModeShell.tsx` | Greeting/date state, aurora props, `onScrollToSearch` callback, active theme forwarding |
| `src/shared/utils/dateFormat.ts` | Extract `formatHomeDate` from `App.tsx` as shared util |
| `src/domains/immersive/sections/HeroSection.tsx` | Full redesign per layout above |
| `src/domains/immersive/sections/HeroSection.css` | New CSS for all new elements |

## Verification

1. Launch dev server (`npm run dev` / `tauri dev`).
2. Confirm greeting changes based on system time.
3. Move mouse across hero — verify all layers shift with different depths.
4. Click aurora background — confirm ripple effect fires.
5. Type in hero search bar → Enter or click arrow → verify smooth-scroll to SearchSection with pre-filled query.
6. Click Speak → grant mic → speak → confirm transcript appears in search field.
7. Open appearance panel → switch palettes → confirm aurora colors transition.
8. Toggle dark/light mode → verify hero typography and search bar adapt via `--cp-*` vars.
9. Enable "Reduce motion" in OS → confirm parallax disables but layout renders normally.

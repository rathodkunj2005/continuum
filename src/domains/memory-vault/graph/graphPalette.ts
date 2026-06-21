/**
 * Deterministic, palette-aware community coloring.
 *
 * Hues are picked from an amber-leaning slice of the wheel (15–60° + wrap)
 * so colors stay in the Continuum brand neighborhood regardless of community id.
 * Saturation and lightness are constants tuned to read on both Old Film
 * (dark) and Archival Paper (light) backgrounds.
 */
const HUE_STRIDE = 47; // coprime with 360 -> good spread for small N
const SATURATION = 58;
const LIGHTNESS = 52;
const BASE_HUE = 30; // amber centerpoint

export function colorForCommunity(communityId: number | null): string {
    if (communityId === null) {
        return "var(--cp-accent-muted)";
    }
    const hue = (Math.abs(communityId) * HUE_STRIDE + BASE_HUE) % 360;
    return `hsl(${hue} ${SATURATION}% ${LIGHTNESS}%)`;
}

export function assignCommunityColors(ids: ReadonlyArray<number>): Record<number, string> {
    const out: Record<number, string> = {};
    for (const id of ids) {
        out[id] = colorForCommunity(id);
    }
    return out;
}

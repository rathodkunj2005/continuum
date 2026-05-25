/**
 * MemoryCard lifecycle presentation — Subagent 10.
 *
 * Covers the vault's reviewed-memory surface: the lifecycle status chip
 * (DEVELOPED / PENDING / RAW / REVIEW_FAILED / VISUAL_FAILED), the compact-card
 * preview-text priority (insight_what_happened > reviewed display_summary >
 * memory_context excerpt > safe fallback), and meta-OCR narration cleanup.
 */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { MemoryCard as MemoryCardData } from "@/shared/ipc/tauri";
import {
    MemoryCard,
    deriveLifecycleStatus,
    isMetaOcrNarration,
} from "../MemoryCard";

afterEach(() => {
    cleanup();
});

function makeCard(overrides: Partial<MemoryCardData> = {}): MemoryCardData {
    return {
        id: "test-card-abcd-1234",
        title: "Lifecycle stress test",
        summary: "Captured activity in VS Code.",
        action: "Reviewed key details",
        context: ["FNDR"],
        timestamp: new Date("2026-05-21T10:30:00Z").getTime(),
        app_name: "VS Code",
        window_title: "src/retrieval/hybrid.rs",
        score: 0.82,
        source_count: 1,
        raw_snippets: [],
        ...overrides,
    };
}

describe("deriveLifecycleStatus", () => {
    it("returns DEVELOPED for reviewed_local", () => {
        expect(deriveLifecycleStatus(makeCard({ enrichment_status: "reviewed_local" }))).toBe(
            "DEVELOPED",
        );
    });
    it("returns DEVELOPED for reviewed_daily", () => {
        expect(deriveLifecycleStatus(makeCard({ enrichment_status: "reviewed_daily" }))).toBe(
            "DEVELOPED",
        );
    });
    it("returns PENDING for pending", () => {
        expect(deriveLifecycleStatus(makeCard({ enrichment_status: "pending" }))).toBe("PENDING");
    });
    it("returns REVIEW_FAILED for review_failed", () => {
        expect(deriveLifecycleStatus(makeCard({ enrichment_status: "review_failed" }))).toBe(
            "REVIEW_FAILED",
        );
    });
    it("returns RAW when enrichment_status is empty", () => {
        expect(deriveLifecycleStatus(makeCard())).toBe("RAW");
    });
    it("returns VISUAL_FAILED when storage_outcome is visual_semantics_failed", () => {
        // VISUAL_FAILED overrides anything enrichment_status might claim — a
        // failed visual ingest must never look like a reviewed memory.
        expect(
            deriveLifecycleStatus(
                makeCard({
                    enrichment_status: "reviewed_local",
                    storage_outcome: "visual_semantics_failed",
                }),
            ),
        ).toBe("VISUAL_FAILED");
    });
});

describe("MemoryCard — lifecycle chip rendering (expanded variant)", () => {
    it("reviewed_local renders the DEVELOPED stamp", () => {
        render(
            <MemoryCard
                variant="expanded"
                card={makeCard({ enrichment_status: "reviewed_local" })}
            />,
        );
        const stamp = screen.getByLabelText("memory status: DEVELOPED");
        expect(stamp).toBeTruthy();
        expect(stamp.textContent).toBe("DEVELOPED");
    });

    it("pending renders the PENDING stamp", () => {
        render(
            <MemoryCard
                variant="expanded"
                card={makeCard({ enrichment_status: "pending" })}
            />,
        );
        const stamp = screen.getByLabelText("memory status: PENDING");
        expect(stamp).toBeTruthy();
        expect(stamp.textContent).toBe("PENDING");
    });

    it("review_failed renders the REVIEW FAILED stamp", () => {
        render(
            <MemoryCard
                variant="expanded"
                card={makeCard({ enrichment_status: "review_failed" })}
            />,
        );
        const stamp = screen.getByLabelText("memory status: REVIEW_FAILED");
        expect(stamp).toBeTruthy();
        expect(stamp.textContent).toBe("REVIEW FAILED");
    });

    it("visual_semantics_failed renders VISUAL FAILED — does not look like a good memory", () => {
        render(
            <MemoryCard
                variant="expanded"
                card={makeCard({
                    enrichment_status: "reviewed_local",
                    storage_outcome: "visual_semantics_failed",
                    insight_what_happened: "",
                    display_summary: "",
                })}
            />,
        );
        const stamp = screen.getByLabelText("memory status: VISUAL_FAILED");
        expect(stamp).toBeTruthy();
        expect(stamp.textContent).toBe("VISUAL FAILED");
        // The "DEVELOPED" label must not be present for a failed visual ingest.
        expect(screen.queryByLabelText("memory status: DEVELOPED")).toBeNull();
    });

    it("absent lifecycle fields fall back to RAW", () => {
        render(<MemoryCard variant="expanded" card={makeCard()} />);
        const stamp = screen.getByLabelText("memory status: RAW");
        expect(stamp).toBeTruthy();
    });
});

describe("MemoryCard — compact preview priority", () => {
    it("reviewed summary wins over raw OCR / window title", () => {
        render(
            <MemoryCard
                variant="compact"
                card={makeCard({
                    enrichment_status: "reviewed_local",
                    display_summary:
                        "Re-ranked hybrid retrieval results and tuned the chunk-first router.",
                    summary: "raw OCR junk that should never surface",
                })}
            />,
        );
        // The reviewed display_summary is used; the noisy `summary` should be hidden.
        expect(
            screen.getByTitle(
                "Re-ranked hybrid retrieval results and tuned the chunk-first router.",
            ),
        ).toBeTruthy();
        expect(screen.queryByTitle("raw OCR junk that should never surface")).toBeNull();
    });

    it("insight_what_happened wins over reviewed display_summary", () => {
        render(
            <MemoryCard
                variant="compact"
                card={makeCard({
                    enrichment_status: "reviewed_local",
                    insight_what_happened:
                        "User finalised the chunk-first retrieval router design.",
                    display_summary: "Reviewed retrieval design",
                })}
            />,
        );
        expect(
            screen.getByTitle("User finalised the chunk-first retrieval router design."),
        ).toBeTruthy();
    });

    it("meta OCR narration is hidden / cleaned from the preview", () => {
        render(
            <MemoryCard
                variant="compact"
                card={makeCard({
                    enrichment_status: "reviewed_local",
                    display_summary:
                        "The OCR text indicates the user is on a settings page with toggles.",
                    internal_context: "User adjusted memory-review settings to enable local review.",
                })}
            />,
        );
        // Meta narration is stripped — the cleaner internal_context surfaces instead.
        expect(
            screen.getByTitle(
                "User adjusted memory-review settings to enable local review.",
            ),
        ).toBeTruthy();
        expect(
            screen.queryByText(/The OCR text indicates/i),
        ).toBeNull();
    });

    it("never exposes raw clean_text-style meta narration as preview", () => {
        render(
            <MemoryCard
                variant="compact"
                card={makeCard({
                    summary: "The screen shows a New Tab page with toolbar buttons.",
                    window_title: "Settings — Privacy",
                })}
            />,
        );
        // Meta-OCR `summary` is rejected; the safe fallback (window_title) is used.
        expect(
            screen.queryByTitle("The screen shows a New Tab page with toolbar buttons."),
        ).toBeNull();
    });
});

describe("isMetaOcrNarration", () => {
    it("flags classic OCR-narration prefixes", () => {
        expect(isMetaOcrNarration("The OCR text indicates the user is browsing.")).toBe(true);
        expect(isMetaOcrNarration("The screen shows a settings panel.")).toBe(true);
        expect(isMetaOcrNarration("Based on the OCR, the user opened a PR.")).toBe(true);
        expect(isMetaOcrNarration("I can see a list of memory records.")).toBe(true);
    });
    it("leaves clean reviewer-grade summaries alone", () => {
        expect(
            isMetaOcrNarration("User finalised the chunk-first retrieval router design."),
        ).toBe(false);
        expect(isMetaOcrNarration("")).toBe(false);
    });
});

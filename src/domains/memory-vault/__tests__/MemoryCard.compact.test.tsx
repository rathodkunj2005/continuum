/**
 * MemoryCard compact variant — container-query layout tests.
 *
 * These tests mount a compact MemoryCard inside a fixed-width wrapper and
 * assert that every critical UI cell renders and is not hidden.  Because
 * jsdom does not execute CSS (and therefore container queries cannot be
 * evaluated at runtime), we test the React output rather than computed
 * styles.  Layout correctness at pixel level is covered by the visual
 * snapshot in the design review; here we guard against regressions where
 * elements are accidentally omitted or receive `display: none` via inline
 * styles.
 */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import type { MemoryCard as MemoryCardData } from "@/shared/ipc/tauri";
import { MemoryCard } from "../MemoryCard";

afterEach(() => {
    cleanup();
});

/** Minimal valid MemoryCard fixture */
function makeCard(overrides: Partial<MemoryCardData> = {}): MemoryCardData {
    return {
        id: "test-card-abcd-1234",
        title: "Compact layout stress test",
        summary: "Why we replaced BM25-only retrieval with hybrid semantic search.",
        action: "Reviewed retrieval design",
        context: ["Continuum"],
        timestamp: new Date("2026-05-20T10:30:00Z").getTime(),
        app_name: "VS Code",
        window_title: "src/retrieval/hybrid.rs",
        score: 0.95,
        source_count: 1,
        raw_snippets: [],
        ...overrides,
    };
}

/**
 * Mount the card inside a wrapper that mimics a ~480px container (the
 * narrow-window pain-point described in the task spec).  We set an explicit
 * width so any future layout code that reads offsetWidth/clientWidth sees
 * a realistic value.
 */
function renderCompact(card: MemoryCardData) {
    return render(
        <div
            data-testid="card-wrapper"
            style={{ width: "480px", position: "relative" }}
        >
            <MemoryCard variant="compact" card={card} />
        </div>,
    );
}

describe("MemoryCard — compact variant (narrow container)", () => {
    it("renders the article element with the correct test-id", () => {
        renderCompact(makeCard());
        expect(screen.getByTestId("memory-card")).toBeTruthy();
    });

    it("renders the card title", () => {
        renderCompact(makeCard());
        expect(screen.getByText("Compact layout stress test")).toBeTruthy();
    });

    it("renders the day and time cells", () => {
        // The card timestamp is 2026-05-20 10:30 UTC — formatDay returns "TODAY"
        // only when running exactly that day; use a loose text match instead.
        renderCompact(makeCard());
        const card = screen.getByTestId("memory-card");
        // time element should contain *some* text (day + clock)
        const timeEl = card.querySelector(".continuum-mc-c-time");
        expect(timeEl).not.toBeNull();
        expect((timeEl as HTMLElement).textContent?.trim().length).toBeGreaterThan(0);
    });

    it("renders the app name cell", () => {
        renderCompact(makeCard());
        const sourceEl = screen.getByLabelText("app and context");
        expect(sourceEl).toBeTruthy();
        expect(within(sourceEl).getByText("VS Code")).toBeTruthy();
    });

    it("renders the frame id cell", () => {
        renderCompact(makeCard());
        // deriveFrameId trims the id and takes the last 4 chars
        expect(screen.getByText(/FRAME/)).toBeTruthy();
    });

    it("renders the preview text when summary is provided", () => {
        const card = makeCard({ summary: "Hybrid retrieval summary text." });
        renderCompact(card);
        const previewEl = screen.getByTitle("Hybrid retrieval summary text.");
        expect(previewEl).toBeTruthy();
    });

    it("does NOT hide critical elements via inline styles", () => {
        const { container } = renderCompact(makeCard());
        const article = container.querySelector("[data-testid='memory-card']") as HTMLElement;
        // Verify none of the key cells have display:none via inline style
        const criticalSelectors = [
            ".continuum-mc-c-frame",
            ".continuum-mc-c-main",
            ".continuum-mc-c-time",
        ];
        for (const selector of criticalSelectors) {
            const el = article.querySelector(selector) as HTMLElement | null;
            expect(el, `element ${selector} should exist`).not.toBeNull();
            expect(
                el!.style.display,
                `element ${selector} should not have inline display:none`,
            ).not.toBe("none");
        }
    });

    it("renders activity_type chip when provided (non-other)", () => {
        const card = makeCard({ activity_type: "coding" });
        renderCompact(card);
        const chip = screen.getByLabelText("activity: coding");
        expect(chip).toBeTruthy();
        expect(chip.textContent).toBe("coding");
    });

    it("does NOT render activity_type chip when value is 'other'", () => {
        const card = makeCard({ activity_type: "other" });
        renderCompact(card);
        // No aria-label matching activity: other
        const chips = document.querySelectorAll("[aria-label^='activity:']");
        expect(chips.length).toBe(0);
    });

    it("does NOT render activity_type chip when field is absent", () => {
        renderCompact(makeCard());
        const chips = document.querySelectorAll("[aria-label^='activity:']");
        expect(chips.length).toBe(0);
    });

    it("renders single file name chip (basename only) for one file", () => {
        const card = makeCard({ files_touched: ["/Users/dev/continuum/src/retrieval/hybrid.rs"] });
        renderCompact(card);
        // The chip shows just the basename
        expect(screen.getByText("hybrid.rs")).toBeTruthy();
        expect(screen.getByLabelText("1 file")).toBeTruthy();
    });

    it("renders files count chip when multiple files are touched", () => {
        const card = makeCard({
            files_touched: ["src/a.rs", "src/b.rs", "src/c.rs"],
        });
        renderCompact(card);
        expect(screen.getByText("3 files")).toBeTruthy();
        expect(screen.getByLabelText("3 files")).toBeTruthy();
    });

    it("does NOT render files chip when files_touched is empty", () => {
        const card = makeCard({ files_touched: [] });
        renderCompact(card);
        const chips = document.querySelectorAll(".continuum-mc-c-chip--files");
        expect(chips.length).toBe(0);
    });

    it("renders both activity and files chips when both are present", () => {
        const card = makeCard({
            activity_type: "coding",
            files_touched: ["src/main.rs", "src/lib.rs"],
        });
        renderCompact(card);
        expect(screen.getByLabelText("activity: coding")).toBeTruthy();
        expect(screen.getByLabelText("2 files")).toBeTruthy();
    });

    it("card is still interactive (has role=button and tabIndex) when onOpen is provided", () => {
        renderCompact(makeCard());
        // renderCompact doesn't pass onOpen, so no role=button
        const article = screen.getByTestId("memory-card");
        expect(article.getAttribute("role")).toBeNull();

        const { unmount } = render(
            <MemoryCard
                variant="compact"
                card={makeCard()}
                onOpen={() => {}}
            />,
        );
        const allCards = screen.getAllByTestId("memory-card");
        const clickable = allCards[allCards.length - 1] as HTMLElement;
        expect(clickable.getAttribute("role")).toBe("button");
        expect(clickable.getAttribute("tabindex")).toBe("0");
        unmount();
    });
});

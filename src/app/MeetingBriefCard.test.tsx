import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MeetingBrief } from "@/shared/ipc/tauri";
import { MeetingBriefCard, startsLabel } from "./MeetingBriefCard";

const ipc = vi.hoisted(() => ({
    nextMeetingBrief: vi.fn(),
    setCalendarMeetingPrep: vi.fn(),
    calendarMeetingPrepState: vi.fn(),
    openCalendarPrivacySettings: vi.fn(),
    resolveWorkSet: vi.fn(),
}));
vi.mock("@/shared/ipc/tauri", () => ipc);

const ready = (overrides: Partial<MeetingBrief> = {}): MeetingBrief => ({
    state: "ready",
    title: "FNDR demo walkthrough",
    startsAt: Date.now() + 10 * 60_000,
    strongMatch: true,
    thread: { key: "FNDR", title: "FNDR" },
    memories: [
        { id: "m1", title: "Voiceover script", appName: "Terminal", timestamp: Date.now() - 5 * 60_000, snippet: "Script for the demo" },
    ],
    ...overrides,
});

describe("MeetingBriefCard", () => {
    beforeEach(() => vi.clearAllMocks());
    afterEach(cleanup);

    it("labels how soon a meeting starts", () => {
        const now = 1_000_000_000;
        expect(startsLabel(now + 30_000, now)).toBe("Starting now");
        expect(startsLabel(now + 12 * 60_000, now)).toBe("In 12 min");
        expect(startsLabel(now + 90 * 60_000, now)).toBe("In 1 h 30 min");
        expect(startsLabel(now - 4 * 60_000, now)).toBe("Started 4 min ago");
    });

    it("shows the meeting and opens a related memory", async () => {
        ipc.nextMeetingBrief.mockResolvedValue(ready());
        const onOpenMemory = vi.fn();
        render(<MeetingBriefCard onOpenMemory={onOpenMemory} />);
        expect(await screen.findByRole("heading", { name: "FNDR demo walkthrough" })).toBeTruthy();
        fireEvent.click(screen.getByRole("button", { name: "Open memory: Voiceover script" }));
        expect(onOpenMemory).toHaveBeenCalledWith("m1");
    });

    it("says so when nothing relates to the meeting", async () => {
        ipc.nextMeetingBrief.mockResolvedValue(ready({ memories: [], thread: null, strongMatch: false }));
        render(<MeetingBriefCard onOpenMemory={vi.fn()} />);
        expect(await screen.findByText(/No related work found/)).toBeTruthy();
    });

    it("offers the switch when the calendar is off, and turns it on", async () => {
        ipc.nextMeetingBrief
            .mockResolvedValueOnce({ state: "off", title: null, startsAt: null, strongMatch: false, thread: null, memories: [] })
            .mockResolvedValue(ready());
        ipc.setCalendarMeetingPrep.mockResolvedValue({ enabled: true, status: "granted" });
        render(<MeetingBriefCard onOpenMemory={vi.fn()} />);
        fireEvent.click(await screen.findByRole("button", { name: "Turn on meeting prep" }));
        expect(ipc.setCalendarMeetingPrep).toHaveBeenCalledWith(true);
        await waitFor(() => expect(screen.getByRole("heading", { name: "FNDR demo walkthrough" })).toBeTruthy());
    });

    it("renders nothing without a meeting", async () => {
        ipc.nextMeetingBrief.mockResolvedValue({ state: "no_meeting", title: null, startsAt: null, strongMatch: false, thread: null, memories: [] });
        const { container } = render(<MeetingBriefCard onOpenMemory={vi.fn()} />);
        await waitFor(() => expect(ipc.nextMeetingBrief).toHaveBeenCalled());
        expect(container.firstChild).toBeNull();
    });
});

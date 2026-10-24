import { useCallback, useEffect, useState } from "react";
import {
    calendarMeetingPrepState,
    nextMeetingBrief,
    openCalendarPrivacySettings,
    resolveWorkSet,
    setCalendarMeetingPrep,
    type MeetingBrief,
} from "@/shared/ipc/tauri";
import { pickSet } from "./homeWorkSet";
import { WorkSetOpener, type WorkSetLoad } from "./WorkSetOpener";
import "./MeetingBriefCard.css";

interface MeetingBriefCardProps {
    onOpenMemory: (memoryId: string) => void;
}

const REFRESH_MS = 5 * 60_000;
const TICK_MS = 30_000;

/** How soon the meeting starts, in the words a person would use. */
export function startsLabel(startsAt: number, now: number): string {
    const diff = startsAt - now;
    if (Math.abs(diff) < 60_000) return "Starting now";
    if (diff < 0) return `Started ${Math.round(-diff / 60_000)} min ago`;
    const minutes = Math.round(diff / 60_000);
    if (minutes < 60) return `In ${minutes} min`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest === 0 ? `In ${hours} h` : `In ${hours} h ${rest} min`;
}

function clock(ms: number): string {
    return new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

function age(ms: number, now: number): string {
    const minutes = Math.max(0, Math.round((now - ms) / 60_000));
    if (minutes < 1) return "just now";
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    return hours < 24 ? `${hours}h ago` : `${Math.floor(hours / 24)}d ago`;
}

/**
 * Home's briefing for the next calendar meeting: when it starts and the
 * memories FNDR holds that bear on it, ready to open. Everything is read on
 * this Mac; with the calendar switch off it offers the switch, and with no
 * meeting in the next day it stays out of the way.
 */
export function MeetingBriefCard({ onOpenMemory }: MeetingBriefCardProps) {
    const [brief, setBrief] = useState<MeetingBrief | null>(null);
    const [now, setNow] = useState(() => Date.now());
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const load = useCallback(async () => {
        try {
            setBrief(await nextMeetingBrief());
            setError(null);
        } catch {
            setError("The meeting brief couldn’t be loaded.");
        }
    }, []);

    useEffect(() => {
        void load();
        const refresh = window.setInterval(() => void load(), REFRESH_MS);
        const tick = window.setInterval(() => setNow(Date.now()), TICK_MS);
        return () => {
            window.clearInterval(refresh);
            window.clearInterval(tick);
        };
    }, [load]);

    const turnOn = async () => {
        setBusy(true);
        setError(null);
        try {
            const next = await setCalendarMeetingPrep(true);
            if (!next.enabled) {
                setError("Calendar access wasn’t granted. Allow FNDR in System Settings, then try again.");
            }
            await load();
        } catch (reason) {
            setError(reason instanceof Error ? reason.message : String(reason));
        } finally {
            setBusy(false);
        }
    };

    const openSettings = async () => {
        await openCalendarPrivacySettings().catch(() => undefined);
        const state = await calendarMeetingPrepState().catch(() => null);
        if (state?.status === "granted") await load();
    };

    if (!brief) return null;
    if (brief.state === "private" || brief.state === "no_meeting") return null;

    if (brief.state === "off" || brief.state === "no_access") {
        const off = brief.state === "off";
        return (
            <section className="meeting-brief is-offer" aria-labelledby="meeting-brief-title">
                <p className="meeting-brief-eyebrow">Meeting prep</p>
                <h2 id="meeting-brief-title">{off ? "Walk into meetings prepared" : "FNDR can’t read your calendar yet"}</h2>
                <p className="meeting-brief-note">
                    {off
                        ? "FNDR reads your calendar on this Mac and gathers what you’ve worked on that relates to your next meeting. Nothing leaves your device."
                        : "Allow Calendars for FNDR in System Settings so it can prepare you for your next meeting."}
                </p>
                <div className="meeting-brief-actions">
                    <button type="button" className="is-primary" disabled={busy} onClick={() => void (off ? turnOn() : openSettings())}>
                        {off ? "Turn on meeting prep" : "Open Calendar settings"}
                    </button>
                </div>
                {error && <p role="alert" className="meeting-brief-error">{error}</p>}
            </section>
        );
    }

    const title = brief.title ?? "Upcoming meeting";
    const startsAt = brief.startsAt ?? now;
    const soon = startsAt - now <= 15 * 60_000;
    const memoryIds = brief.memories.map((memory) => memory.id);
    const openAll: WorkSetLoad = async () => {
        const resolution = await resolveWorkSet(title);
        const set = pickSet(resolution, memoryIds);
        if (set) return set.items;
        return { why: resolution.kind === "none" ? resolution.value.why : "FNDR found no places to reopen for this meeting." };
    };

    return (
        <section className={`meeting-brief${soon ? " is-soon" : ""}`} aria-labelledby="meeting-brief-title">
            <div className="meeting-brief-head">
                <div>
                    <p className="meeting-brief-eyebrow">Next meeting</p>
                    <h2 id="meeting-brief-title">{title}</h2>
                    <p className="meeting-brief-time">{clock(startsAt)}</p>
                </div>
                <span className="meeting-brief-when" role="status">{startsLabel(startsAt, now)}</span>
            </div>

            {brief.memories.length > 0 ? (
                <>
                    <p className="meeting-brief-label">
                        {brief.thread ? <>Related work · <strong>{brief.thread.title}</strong></> : "Related work"}
                    </p>
                    <ul className="meeting-brief-list">
                        {brief.memories.map((memory) => (
                            <li key={memory.id}>
                                <button type="button" onClick={() => onOpenMemory(memory.id)} aria-label={`Open memory: ${memory.title}`}>
                                    <span className="meeting-brief-row-top">
                                        <span className="meeting-brief-app">{memory.appName}</span>
                                        <span className="meeting-brief-age">{age(memory.timestamp, now)}</span>
                                    </span>
                                    <span className="meeting-brief-row-title">{memory.title}</span>
                                    {memory.snippet && <span className="meeting-brief-snippet">{memory.snippet}</span>}
                                </button>
                            </li>
                        ))}
                    </ul>
                    <div className="meeting-brief-actions">
                        <WorkSetOpener name={title} load={openAll} label="Open all for this meeting" />
                    </div>
                </>
            ) : (
                <p className="meeting-brief-note">
                    No related work found for this meeting yet. FNDR matches on the event’s title, so a more specific title helps.
                </p>
            )}
            {error && <p role="alert" className="meeting-brief-error">{error}</p>}
        </section>
    );
}

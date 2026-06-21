import { useEffect, useMemo, useRef, useState } from "react";
import {
    MeetingRecorderStatus,
    MeetingSession,
    MeetingTranscript,
    deleteMeeting,
    getMeetingStatus,
    getMeetingTranscript,
    listMeetings,
    onMeetingStatus,
    retranscribeMeeting,
    startMeetingRecording,
    stopMeetingRecording,
} from "@/shared/ipc/tauri";
import "./MeetingRecorderPanel.css";

interface MeetingRecorderPanelProps {
    isVisible: boolean;
    onClose: () => void;
}

export function MeetingRecorderPanel({ isVisible, onClose }: MeetingRecorderPanelProps) {
    const [status, setStatus] = useState<MeetingRecorderStatus | null>(null);
    const [meetings, setMeetings] = useState<MeetingSession[]>([]);
    const [selectedMeetingId, setSelectedMeetingId] = useState<string | null>(null);
    const userSelectedId = useRef<string | null>(null);
    const [transcript, setTranscript] = useState<MeetingTranscript | null>(null);
    const [titleInput, setTitleInput] = useState("");
    const [starting, setStarting] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [retranscribing, setRetranscribing] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const selectedMeeting = useMemo(
        () => meetings.find((m) => m.id === selectedMeetingId) ?? null,
        [meetings, selectedMeetingId]
    );
    const activeBreakdown = useMemo(() => {
        if (!selectedMeeting) return null;
        if (transcript?.meeting.id === selectedMeeting.id && transcript.meeting.breakdown) {
            return transcript.meeting.breakdown;
        }
        return selectedMeeting.breakdown ?? null;
    }, [selectedMeeting, transcript]);

    const refresh = async (autoSelect = false) => {
        if (!isVisible) return;
        try {
            const [meetingStatus, meetingList] = await Promise.all([
                getMeetingStatus(),
                listMeetings(),
            ]);

            setStatus(meetingStatus);
            setMeetings(meetingList);

            // Determine which meeting tab to show.
            // If the user has explicitly clicked a tab, preserve it.
            // Only auto-switch when: first mount, or autoSelect is forced
            // (e.g. after starting/stopping a recording).
            let nextId: string | null = null;

            if (userSelectedId.current) {
                // User explicitly selected a tab — keep it if it still exists.
                const stillExists = meetingList.some(m => m.id === userSelectedId.current);
                nextId = stillExists ? userSelectedId.current : (meetingList[0]?.id ?? null);
                if (!stillExists) {
                    userSelectedId.current = null;
                }
            } else if (autoSelect) {
                // Auto-select: prefer current recording, else first in list.
                nextId = meetingStatus.current_meeting_id ?? meetingList[0]?.id ?? null;
            } else {
                // Background refresh with no user selection yet — keep current or pick first.
                nextId = selectedMeetingId ?? meetingList[0]?.id ?? null;
            }

            setSelectedMeetingId(nextId);
            if (nextId) {
                const data = await getMeetingTranscript(nextId);
                setTranscript(data);
            } else {
                setTranscript(null);
            }
            setError(null);
        } catch (err) {
            setError(String(err));
        }
    };

    useEffect(() => {
        if (!isVisible) return;
        refresh(true);

        const unlistenPromise = onMeetingStatus((nextStatus) => {
            setStatus(nextStatus);
            if (!nextStatus.is_recording) {
                // Recording just stopped — auto-select the finished meeting.
                userSelectedId.current = null;
                refresh(true);
            }
        });

        // Poll every 5 seconds while panel is open to catch async transcription updates.
        const pollInterval = setInterval(() => void refresh(false), 5000);

        return () => {
            unlistenPromise.then(unlisten => unlisten());
            clearInterval(pollInterval);
        };
    }, [isVisible]);

    const handleStart = async () => {
        if (starting || status?.is_recording) return;
        setStarting(true);
        setError(null);
        try {
            const title = titleInput.trim() || `Meeting ${new Date().toLocaleString()}`;
            await startMeetingRecording(title, []);
            setTitleInput("");
            await refresh(true);
        } catch (err) {
            setError(String(err));
        } finally {
            setStarting(false);
        }
    };

    const handleStop = async () => {
        if (stopping || !status?.is_recording) return;
        setStopping(true);
        setError(null);
        try {
            await stopMeetingRecording();
            await refresh(true);
        } catch (err) {
            setError(String(err));
        } finally {
            setStopping(false);
        }
    };

    const handleSelectMeeting = async (meetingId: string) => {
        userSelectedId.current = meetingId;
        setSelectedMeetingId(meetingId);
        try {
            const data = await getMeetingTranscript(meetingId);
            setTranscript(data);
        } catch (err) {
            setError(String(err));
        }
    };

    const handleDelete = async (meetingId: string) => {
        try {
            await deleteMeeting(meetingId);
            await refresh(false);
        } catch (err) {
            setError(String(err));
        }
    };

    const handleRetranscribe = async (meetingId: string) => {
        setRetranscribing(true);
        setError(null);
        try {
            await retranscribeMeeting(meetingId);
            await refresh(true);
        } catch (err) {
            setError(String(err));
        } finally {
            setRetranscribing(false);
        }
    };

    if (!isVisible) return null;

    return (
        <div className="meeting-panel simplified">
            <header className="meeting-panel-header">
                <div className="meeting-headline">
                    <h2>Meeting Intelligence</h2>
                    <p>Audio recording and post-meeting analysis.</p>
                </div>
                <button className="ui-action-btn meeting-btn meeting-close-btn" onClick={onClose}>X</button>
            </header>

            <div className="meeting-panel-content">
                {/* 1. Main Recording Control */}
                <section className="meeting-main-ctrl">
                    {!status?.is_recording && !stopping && (
                        <div className="meeting-start-form">
                            <input
                                className="meeting-title-input"
                                value={titleInput}
                                onChange={(e) => setTitleInput(e.target.value)}
                                placeholder="Enter meeting title..."
                            />
                            <button 
                                className="ui-action-btn meeting-btn meeting-hero-btn"
                                onClick={handleStart}
                                disabled={starting || !status?.ffmpeg_available}
                            >
                                {starting ? "Starting..." : "● START RECORDING"}
                            </button>
                        </div>
                    )}

                    {status?.is_recording && (
                        <div className="meeting-recording-state">
                            <div className="recording-pulse">
                                <span className="pulse-dot" />
                                <span>Recording: {status.current_title}</span>
                            </div>
                            <button className="ui-action-btn meeting-btn meeting-hero-btn" onClick={handleStop} disabled={stopping}>
                                {stopping ? "Analyzing..." : "■ STOP & ANALYZE"}
                            </button>
                        </div>
                    )}

                    {stopping && (
                        <div className="meeting-analyzing-state">
                            <div className="spinner" />
                            <p>Transcribing and extracting action items...</p>
                        </div>
                    )}
                </section>

                {/* 2. Breakdown Results */}
                {selectedMeeting && !status?.is_recording && !stopping && (
                    <section className="meeting-breakdown">
                        <div className="breakdown-header">
                            <h3>{displayTitle(selectedMeeting)}</h3>
                            <span className="breakdown-meta">
                                {new Date(selectedMeeting.start_timestamp).toLocaleDateString()} • {Math.round(selectedMeeting.duration_seconds / 60)} min
                            </span>
                            <button
                                className="ui-action-btn meeting-btn"
                                onClick={() => handleRetranscribe(selectedMeeting.id)}
                                disabled={retranscribing}
                                title="Re-run Whisper transcription on this meeting"
                            >
                                {retranscribing ? "Transcribing..." : "Re-transcribe"}
                            </button>
                            <button
                                className="ui-action-btn meeting-btn delete-session-btn"
                                onClick={() => handleDelete(selectedMeeting.id)}
                            >
                                Delete
                            </button>
                        </div>

                        {retranscribing && (
                            <div className="meeting-analyzing-state">
                                <div className="spinner" />
                                <p>Running Whisper transcription...</p>
                            </div>
                        )}

                        {!retranscribing && activeBreakdown ? (
                            <div className="breakdown-grids">
                                {activeBreakdown.summary && (
                                    <div className="breakdown-item summary-box">
                                        <h4>Summary</h4>
                                        <p>{activeBreakdown.summary}</p>
                                    </div>
                                )}
                                
                                <div className="breakdown-grid-row">
                                    <div className="breakdown-item todo-box">
                                        <h4>To-dos</h4>
                                        {activeBreakdown.todos.length > 0 ? (
                                            <ul>{activeBreakdown.todos.map((item, i) => <li key={i}>{item}</li>)}</ul>
                                        ) : <p className="empty-sub">No tasks detected.</p>}
                                    </div>
                                    <div className="breakdown-item reminder-box">
                                        <h4>Reminders</h4>
                                        {activeBreakdown.reminders.length > 0 ? (
                                            <ul>{activeBreakdown.reminders.map((item, i) => <li key={i}>{item}</li>)}</ul>
                                        ) : <p className="empty-sub">No reminders detected.</p>}
                                    </div>
                                    <div className="breakdown-item followup-box">
                                        <h4>Follow-ups</h4>
                                        {activeBreakdown.followups.length > 0 ? (
                                            <ul>{activeBreakdown.followups.map((item, i) => <li key={i}>{item}</li>)}</ul>
                                        ) : <p className="empty-sub">No follow-ups detected.</p>}
                                    </div>
                                </div>
                            </div>
                        ) : !retranscribing ? (
                            <p className="empty-results">No transcript yet. Click "Re-transcribe" to run Whisper on this recording.</p>
                        ) : null}

                        <details className="raw-transcript-details">
                            <summary>View Full Transcript</summary>
                            <div className="transcript-text">
                                {transcript?.full_text || "No transcript available. Click \"Re-transcribe\" above to run Whisper on this recording."}
                            </div>
                        </details>
                    </section>
                )}

                {!selectedMeeting && !status?.is_recording && !stopping && (
                    <div className="meeting-empty-state">
                        <p>No meeting active. Start recording to capture one.</p>
                    </div>
                )}
            </div>

            {/* 3. Minimalist History Row */}
            {meetings.length > 1 && !status?.is_recording && !stopping && (
                <footer className="meeting-history-footer">
                    <span>Recent:</span>
                    <div className="history-pills">
                        {meetings.slice(0, 5).map(m => (
                            <button 
                                key={m.id} 
                                className={`ui-action-btn meeting-btn history-pill ${selectedMeetingId === m.id ? "active" : ""}`}
                                onClick={() => handleSelectMeeting(m.id)}
                            >
                                {displayTitle(m)}
                            </button>
                        ))}
                    </div>
                </footer>
            )}

            {error && <div className="meeting-error">{error}</div>}
        </div>
    );
}

/** Weak/generic titles that should be replaced with a more meaningful label. */
const WEAK_TITLES = new Set([
    "join meeting",
    "login",
    "detected meeting",
    "continuum",
    "meeting",
    "untitled meeting",
    "untitled",
    "",
]);

function displayTitle(meeting: MeetingSession): string {
    const raw = (meeting.title ?? "").trim();
    if (raw.length <= 1 || WEAK_TITLES.has(raw.toLowerCase())) {
        const date = new Date(meeting.start_timestamp);
        const formatted = date.toLocaleDateString(undefined, {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
        });
        return `Meeting — ${formatted}`;
    }
    return raw;
}

import { useMemo } from "react";
import type { CaptureStatus } from "@/shared/ipc/tauri";
import "./StatusBar.css";

interface StatusBarProps {
    status: CaptureStatus | null;
}

/**
 * Always-on bottom bar — 26px tall, mono 10px, archival film aesthetic.
 *
 * Left: pulsing amber dot (when indexing) · reel date · frame count
 * Right: LOCAL ONLY badge (amber, fixed)
 *
 * Bound to real Tauri capture status — pulses when `is_capturing` is
 * true and `is_paused` is false. The reel date is derived from now()
 * since the capture pipeline runs continuously.
 */
export function StatusBar({ status }: StatusBarProps) {
    const reelDate = useMemo(() => formatReelDate(new Date()), []);
    const frameCount = status?.frames_captured ?? 0;
    const indexing = (status?.is_capturing ?? false) && !(status?.is_paused ?? false);

    return (
        <footer className="fndr-status-bar" role="status" aria-live="polite">
            <div className="fndr-status-left">
                <span
                    className={`fndr-status-dot ${indexing ? "is-active" : "is-idle"}`}
                    aria-hidden="true"
                />
                <span className="fndr-status-text">
                    {indexing ? "INDEXING" : status?.is_paused ? "PAUSED" : "IDLE"}
                </span>
                <span className="fndr-status-sep" aria-hidden="true">
                    ·
                </span>
                <span className="fndr-status-text">REEL {reelDate}</span>
                <span className="fndr-status-sep" aria-hidden="true">
                    ·
                </span>
                <span className="fndr-status-text">
                    {frameCount.toLocaleString()} FRAMES
                </span>
                {status && status.frames_dropped > 0 && (
                    <>
                        <span className="fndr-status-sep" aria-hidden="true">
                            ·
                        </span>
                        <span className="fndr-status-text fndr-status-text-muted">
                            {status.frames_dropped.toLocaleString()} DROPPED
                        </span>
                    </>
                )}
            </div>
            <div className="fndr-status-right">
                <span className="fndr-status-local">LOCAL ONLY</span>
            </div>
        </footer>
    );
}

function formatReelDate(d: Date): string {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${y}-${m}-${day}`;
}

export default StatusBar;

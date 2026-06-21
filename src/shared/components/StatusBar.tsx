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
        <footer className="continuum-status-bar" role="status" aria-live="polite">
            <div className="continuum-status-left">
                <span
                    className={`continuum-status-dot ${indexing ? "is-active" : "is-idle"}`}
                    aria-hidden="true"
                />
                <span className="continuum-status-text">
                    {indexing ? "INDEXING" : status?.is_paused ? "PAUSED" : "IDLE"}
                </span>
                <span className="continuum-status-sep" aria-hidden="true">
                    ·
                </span>
                <span className="continuum-status-text">REEL {reelDate}</span>
                <span className="continuum-status-sep" aria-hidden="true">
                    ·
                </span>
                <span className="continuum-status-text">
                    {frameCount.toLocaleString()} FRAMES
                </span>
                {status && status.frames_dropped > 0 && (
                    <>
                        <span className="continuum-status-sep" aria-hidden="true">
                            ·
                        </span>
                        <span className="continuum-status-text continuum-status-text-muted">
                            {status.frames_dropped.toLocaleString()} DROPPED
                        </span>
                    </>
                )}
            </div>
            <div className="continuum-status-right">
                <span className="continuum-status-local">LOCAL ONLY</span>
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

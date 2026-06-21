/**
 * Settings UI for the iPhone / Apple Watch Companion API:
 *  - Server status (host/port/TLS, written to ~/.continuum/companion.json)
 *  - "Pair a device" flow — issues a short-lived 6-digit code + QR payload
 *  - List of paired devices with revoke
 *
 * Intentionally minimal for slice 1. The QR pixel rendering and a polished
 * design pass arrive in slice 2 alongside the iOS app that consumes it.
 */

import { useCallback, useEffect, useState } from "react";
import {
    CompanionDeviceListEntry,
    CompanionPairStartResponse,
    CompanionStatusPayload,
    companionGetStatus,
    companionListDevices,
    companionRevokeDevice,
    companionStartPairing,
} from "@/shared/ipc/tauri";

interface PendingPairing {
    pairing_code: string;
    qr_payload: string;
    expires_at_ms: number;
}

export interface CompanionDevicesPanelProps {
    /** Override the auto-poll cadence for tests. */
    pollIntervalMs?: number;
}

export function CompanionDevicesPanel({ pollIntervalMs = 5000 }: CompanionDevicesPanelProps = {}) {
    const [status, setStatus] = useState<CompanionStatusPayload | null>(null);
    const [devices, setDevices] = useState<CompanionDeviceListEntry[]>([]);
    const [pending, setPending] = useState<PendingPairing | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [pairingInFlight, setPairingInFlight] = useState(false);

    const refresh = useCallback(async () => {
        try {
            const [s, d] = await Promise.all([
                companionGetStatus(),
                companionListDevices().catch(() => [] as CompanionDeviceListEntry[]),
            ]);
            setStatus(s);
            setDevices(d);
            setError(null);
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
        }
    }, []);

    useEffect(() => {
        void refresh();
        if (pollIntervalMs <= 0) return;
        const handle = window.setInterval(refresh, pollIntervalMs);
        return () => window.clearInterval(handle);
    }, [refresh, pollIntervalMs]);

    const handleStartPairing = useCallback(async () => {
        setPairingInFlight(true);
        try {
            const resp: CompanionPairStartResponse = await companionStartPairing();
            setPending({
                pairing_code: resp.pairing_code,
                qr_payload: resp.qr_payload,
                expires_at_ms: resp.expires_at_ms,
            });
            setError(null);
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
        } finally {
            setPairingInFlight(false);
        }
    }, []);

    const handleRevoke = useCallback(
        async (deviceId: string) => {
            try {
                await companionRevokeDevice(deviceId);
                await refresh();
            } catch (e) {
                setError(e instanceof Error ? e.message : String(e));
            }
        },
        [refresh],
    );

    return (
        <section className="companion-devices-panel" aria-label="Paired devices">
            <header>
                <h2>iPhone &amp; Apple Watch</h2>
                <p className="muted">
                    Pair your iPhone or Apple Watch to ask Continuum, search memories,
                    and capture notes from your phone. Mac stays the brain;
                    nothing leaves the local network.
                </p>
            </header>

            <div className="server-status" role="status">
                {status ? (
                    <>
                        <span className={status.running ? "dot dot-on" : "dot dot-off"} />
                        <span>
                            {status.running
                                ? `Listening on ${status.host}:${status.port}${status.tls ? " (TLS)" : ""}`
                                : "Companion API is offline"}
                        </span>
                        {status.last_error ? (
                            <span className="server-status-error" role="alert">
                                {status.last_error}
                            </span>
                        ) : null}
                    </>
                ) : (
                    <span>Loading…</span>
                )}
            </div>

            <div className="pair-controls">
                <button
                    type="button"
                    onClick={handleStartPairing}
                    disabled={pairingInFlight || !status?.running}
                >
                    {pairingInFlight ? "Generating…" : "Pair a device"}
                </button>
                {pending ? (
                    <div className="pending-pair" data-testid="pending-pair">
                        <p>
                            On your iPhone or Apple Watch, open Continuum and enter
                            this code:
                        </p>
                        <p className="pair-code" aria-label="pairing code">
                            {formatPairCode(pending.pairing_code)}
                        </p>
                        <p className="muted">
                            Expires {formatExpires(pending.expires_at_ms)}.
                        </p>
                        <details>
                            <summary>QR payload (debug)</summary>
                            <pre>{pending.qr_payload}</pre>
                        </details>
                    </div>
                ) : null}
            </div>

            {error ? (
                <p className="error" role="alert" data-testid="companion-error">
                    {error}
                </p>
            ) : null}

            <h3>Paired devices</h3>
            {devices.length === 0 ? (
                <p className="muted">No devices paired yet.</p>
            ) : (
                <ul className="device-list">
                    {devices.map((d) => (
                        <li key={d.device_id} className={d.revoked_at_ms ? "revoked" : undefined}>
                            <div>
                                <strong>{d.device_name}</strong>
                                <span className="device-type">{d.device_type}</span>
                                {d.revoked_at_ms ? <span className="badge">revoked</span> : null}
                            </div>
                            <div className="muted">
                                Last seen {formatRelative(d.last_seen_at_ms)} ·
                                paired {formatRelative(d.paired_at_ms)}
                                {d.app_version ? ` · v${d.app_version}` : ""}
                            </div>
                            {!d.revoked_at_ms ? (
                                <button
                                    type="button"
                                    onClick={() => void handleRevoke(d.device_id)}
                                    aria-label={`Revoke ${d.device_name}`}
                                >
                                    Revoke
                                </button>
                            ) : null}
                        </li>
                    ))}
                </ul>
            )}
        </section>
    );
}

function formatPairCode(code: string): string {
    return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

function formatExpires(ms: number): string {
    const remaining = ms - Date.now();
    if (remaining <= 0) return "now";
    const seconds = Math.ceil(remaining / 1000);
    if (seconds < 60) return `in ${seconds}s`;
    const minutes = Math.ceil(seconds / 60);
    return `in ${minutes}m`;
}

function formatRelative(ms: number): string {
    if (!ms) return "just now";
    const diff = Date.now() - ms;
    if (diff < 60_000) return "just now";
    if (diff < 3_600_000) return `${Math.round(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.round(diff / 3_600_000)}h ago`;
    return `${Math.round(diff / 86_400_000)}d ago`;
}

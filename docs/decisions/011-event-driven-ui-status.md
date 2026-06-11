# ADR 011: Event-Driven UI Status Over Fixed-Interval Polling

## Status

Accepted

## Context

The renderer polled the backend on fixed intervals regardless of whether anything changed. Three pollers ran unconditionally for the app's lifetime:

- `App.tsx` fetched `get_status` every 2 s (capture counters, pipeline breakdown, embedding status).
- `PrivacyPanel.tsx` fetched `get_privacy_alerts` every 2 s.
- `ControlPanel.tsx` fetched `get_privacy_alerts` every 3 s to drive the auto-open badge.

Each `get_status` call serializes the full `CaptureStatus` payload including the per-reason pipeline breakdown, so an idle app still produced ~90 IPC round-trips per minute. The backend already pushes events for meetings (`meeting://status`), model downloads, and proactive suggestions, so a push channel existed but status did not use it.

## Decision

The backend emits state to the renderer when it changes; the renderer subscribes instead of polling.

- `capture://status` carries the full `CaptureStatus` payload. The capture loop (`src-tauri/src/capture/mod.rs`) snapshots a fingerprint of its counters and toggles at the top of each tick and emits only when the fingerprint changes — at most one event per loop tick, zero while idle. `pause_capture` / `resume_capture` and the companion capture-control handler emit directly so toggles reflect immediately rather than on the next tick.
- `privacy://alerts` carries the full pending-alert list, emitted when an alert is pushed (capture loop), dismissed, or converted to a blocklist entry (`src-tauri/src/ipc/commands/privacy.rs`).
- Renderer side, `src/shared/hooks/useTauriEvent.ts` wraps `listen()` with a handler ref so callers can pass inline closures. Components do one initial fetch on mount (events only describe changes after subscription) and then update from events. Event names are exported from `src/shared/ipc/tauri.ts` (`CAPTURE_STATUS_EVENT`, `PRIVACY_ALERTS_EVENT`).
- Emit helpers (`emit_capture_status`, `emit_privacy_alerts`) read `AppState.app_handle` and no-op before setup registers it. There is no polling fallback: if event registration fails outside a Tauri runtime (vitest, plain browser), the initial fetch result simply stays on screen.

Pollers deliberately left alone:

- Repair/reclaim progress (1 s / 850 ms) only run while a maintenance operation is in flight; converting them means threading an emitter through ~25 `persist_*_progress` call sites for an operation that runs rarely.
- AgentPanel (4 s) batches five different IPC reads and is gated on panel visibility; no single backend change-point exists to replace it.
- Stats, storage health, clock, app names, time tracking poll at 15 s–5 min and are visibility- or panel-gated.

## Consequences

- Idle app does zero status/alert IPC; pre-change behavior was ~90 calls/minute.
- Status reaction time improves: pause/resume and incognito reflect immediately (direct emit) instead of up to 2 s later; loop-driven changes reflect within one capture tick (500 ms when paused).
- The capture-loop fingerprint must include any field whose change should reach the UI; today that is the frame/pipeline counters, pause/incognito flags, and `ai_model_loaded`. Embedding backend changes only reach the UI on the next counter change — acceptable because backend swaps coincide with capture activity.
- New always-on UI state should follow this pattern (emit at change points) rather than adding pollers.

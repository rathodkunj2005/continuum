import { useCallback, useMemo, useState } from "react";
import {
    getMemoryReviewStatus,
    getRuntimeMetrics,
    type MemoryReviewWorkerStatus,
    type RuntimeMetricsSnapshot,
} from "@/shared/ipc/tauri";
import { usePolling } from "@/shared/hooks/usePolling";
import "./PipelineInspectorPanel.css";

const BYTES_PER_MIB = 1024 * 1024;
const BYTES_PER_GIB = 1024 * 1024 * 1024;

function formatBytes(bytes: number | null | undefined): string {
    if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return "—";
    if (bytes >= BYTES_PER_GIB) return `${(bytes / BYTES_PER_GIB).toFixed(2)} GiB`;
    if (bytes >= BYTES_PER_MIB) return `${(bytes / BYTES_PER_MIB).toFixed(0)} MiB`;
    return `${(bytes / 1024).toFixed(0)} KiB`;
}

function formatRate(bps: number): string {
    if (bps <= 0) return "0 B/s";
    if (bps >= BYTES_PER_MIB) return `${(bps / BYTES_PER_MIB).toFixed(2)} MiB/s`;
    if (bps >= 1024) return `${(bps / 1024).toFixed(1)} KiB/s`;
    return `${bps.toFixed(0)} B/s`;
}

function pressureClass(label: string): string {
    if (label === "high") return "pipeline-pressure-high";
    if (label === "moderate") return "pipeline-pressure-moderate";
    return "pipeline-pressure-low";
}

interface EngineMetricsCardProps {
    /** When false, polling is disabled. */
    enabled: boolean;
    /** If set, shown above the metrics blurb (e.g. standalone panel title). */
    title?: string;
}

/**
 * Live engine latency / RSS snapshot (from `get_runtime_metrics`). Reuses Pipeline Inspector styles.
 */
export function EngineMetricsCard({ enabled, title }: EngineMetricsCardProps) {
    const [runtimeMetrics, setRuntimeMetrics] = useState<RuntimeMetricsSnapshot | null>(null);
    const [runtimeMetricsError, setRuntimeMetricsError] = useState<string | null>(null);
    const [reviewStatus, setReviewStatus] = useState<MemoryReviewWorkerStatus | null>(null);

    const loadRuntimeMetrics = useCallback(async (isMounted: () => boolean) => {
        try {
            const snap = await getRuntimeMetrics();
            if (isMounted()) {
                setRuntimeMetrics(snap);
                setRuntimeMetricsError(null);
            }
        } catch (e) {
            if (isMounted()) {
                setRuntimeMetricsError(e instanceof Error ? e.message : String(e));
            }
        }
    }, []);

    const loadReviewStatus = useCallback(async (isMounted: () => boolean) => {
        try {
            const status = await getMemoryReviewStatus();
            if (isMounted()) setReviewStatus(status);
        } catch {
            // Best-effort surface — never block the primary metrics panel.
        }
    }, []);

    usePolling(loadRuntimeMetrics, 3000, enabled);
    usePolling(loadReviewStatus, 5000, enabled);

    const system = runtimeMetrics?.system;
    const modelMemory = useMemo(() => {
        return (system?.model_memory ?? []).slice().sort((a, b) => b.estimated_bytes - a.estimated_bytes);
    }, [system?.model_memory]);

    return (
        <section className="pipeline-panel-card pipeline-engine-metrics">
            {title ? <h3>{title}</h3> : <h3>Engine metrics</h3>}
            <p className="pipeline-muted">
                Activity-Monitor-grade: process + host CPU/RAM/threads, GPU, disk I/O, energy, and the
                per-model RAM breakdown. Latency aggregates capture flush, ONNX, CLIP, LLM/VLM,
                hybrid search, and graph commits. No query text stored.
            </p>
            {runtimeMetricsError && <div className="pipeline-error">{runtimeMetricsError}</div>}
            {runtimeMetrics && (
                <>
                    <div className="pipeline-engine-kv">
                        <span>RSS</span>
                        <strong>
                            {runtimeMetrics.process_rss_bytes != null
                                ? `${(runtimeMetrics.process_rss_bytes / (1024 * 1024)).toFixed(0)} MiB`
                                : "—"}
                        </strong>
                        <span>CLIP vision</span>
                        <strong>
                            {runtimeMetrics.embedding.clip_session_loaded ? "loaded" : "idle"} · last{" "}
                            {runtimeMetrics.embedding.last_clip_infer_ms} ms
                        </strong>
                        <span>Text embed (BGE)</span>
                        <strong>
                            {runtimeMetrics.embedding.backend}
                            {runtimeMetrics.embedding.degraded ? " (degraded)" : ""}
                        </strong>
                        <span>LLM / VLM</span>
                        <strong>
                            {runtimeMetrics.inference.ai_model_loaded
                                ? runtimeMetrics.inference.loaded_model_id ?? "loaded"
                                : "not loaded"}
                        </strong>
                        {reviewStatus && (
                            <>
                                <span>Memory review</span>
                                <strong
                                    data-testid="memory-review-status"
                                    title={
                                        reviewStatus.last_error_kind
                                            ? `last error: ${reviewStatus.last_error_kind}`
                                            : reviewStatus.pressure_blocked
                                            ? "blocked by system pressure / inference unavailable"
                                            : reviewStatus.worker_enabled
                                            ? "running"
                                            : "disabled"
                                    }
                                >
                                    {reviewStatus.worker_enabled
                                        ? reviewStatus.pressure_blocked
                                            ? "deferred"
                                            : "running"
                                        : "off"}
                                    {" · queue "}
                                    {reviewStatus.queue_depth}
                                </strong>
                            </>
                        )}
                    </div>

                    {system && (
                        <>
                            <h4>Continuum process</h4>
                            <div className="pipeline-engine-kv pipeline-engine-kv--metrics">
                                <span>CPU</span>
                                <strong>
                                    {system.process_cpu.cpu_percent.toFixed(1)}% ·{" "}
                                    {system.process_cpu.threads} threads
                                </strong>
                                <span>Memory (resident)</span>
                                <strong>{formatBytes(system.process_memory.rss_bytes)}</strong>
                                <span>Phys. footprint</span>
                                <strong>
                                    {formatBytes(system.process_memory.phys_footprint_bytes)} · peak{" "}
                                    {formatBytes(system.process_memory.lifetime_max_phys_footprint_bytes)}
                                </strong>
                                <span>Disk I/O (rate)</span>
                                <strong>
                                    ↓ {formatRate(system.process_io.disk_read_rate_bps)} · ↑{" "}
                                    {formatRate(system.process_io.disk_write_rate_bps)}
                                </strong>
                                <span>Energy</span>
                                <strong className={pressureClass(system.process_energy.label)}>
                                    {system.process_energy.label} · {system.process_energy.idle_wakeups} idle
                                    wakeups
                                </strong>
                            </div>

                            <h4>Host system</h4>
                            <div className="pipeline-engine-kv pipeline-engine-kv--metrics">
                                <span>CPU (all cores)</span>
                                <strong>{system.host_cpu.cpu_percent_total.toFixed(1)}%</strong>
                                <span>Memory pressure</span>
                                <strong className={pressureClass(system.host_memory.pressure_label)}>
                                    {system.host_memory.pressure_label} · {formatBytes(system.host_memory.free_bytes)}{" "}
                                    free
                                </strong>
                                <span>Memory breakdown</span>
                                <strong className="pipeline-memory-breakdown">
                                    wired {formatBytes(system.host_memory.wired_bytes)} · active{" "}
                                    {formatBytes(system.host_memory.active_bytes)} · inactive{" "}
                                    {formatBytes(system.host_memory.inactive_bytes)} · compressed{" "}
                                    {formatBytes(system.host_memory.compressed_bytes)}
                                </strong>
                            </div>

                            {system.host_cpu.cpu_percent_per_core.length > 0 && (
                                <div className="pipeline-cpu-cores" aria-label="Per-core CPU usage">
                                    {system.host_cpu.cpu_percent_per_core.map((pct, idx) => {
                                        const clamped = Math.max(0, Math.min(100, pct));
                                        return (
                                            <div
                                                className="pipeline-cpu-core"
                                                key={`core-${idx}`}
                                                title={`Core ${idx}: ${pct.toFixed(1)}%`}
                                            >
                                                <div className="pipeline-cpu-core-bar">
                                                    <div
                                                        className="pipeline-cpu-core-fill"
                                                        style={{ height: `${clamped}%` }}
                                                    />
                                                </div>
                                                <div className="pipeline-cpu-core-label">{idx}</div>
                                            </div>
                                        );
                                    })}
                                </div>
                            )}

                            <h4>GPU</h4>
                            <div className="pipeline-engine-kv pipeline-engine-kv--metrics">
                                <span>Device utilization</span>
                                <strong>
                                    {system.gpu.device_utilization_percent != null
                                        ? `${system.gpu.device_utilization_percent.toFixed(0)}%`
                                        : "—"}
                                </strong>
                                <span>Renderer</span>
                                <strong>
                                    {system.gpu.renderer_utilization_percent != null
                                        ? `${system.gpu.renderer_utilization_percent.toFixed(0)}%`
                                        : "—"}
                                </strong>
                                <span>In-use system mem</span>
                                <strong>{formatBytes(system.gpu.in_use_system_memory_bytes)}</strong>
                                <span>Recoveries</span>
                                <strong>{system.gpu.recovery_count ?? "—"}</strong>
                            </div>

                            <h4>Loaded models</h4>
                            <ul className="pipeline-model-list">
                                {modelMemory.length === 0 ? (
                                    <li className="pipeline-muted">No models tracked.</li>
                                ) : (
                                    modelMemory.map((m) => (
                                        <li key={`${m.kind}-${m.id}`}>
                                            <span className={`pipeline-model-dot pipeline-model-dot--${m.kind}`} />
                                            <code>{m.id}</code>
                                            <span className="pipeline-model-kind">{m.kind}</span>
                                            <span className="pipeline-model-state">
                                                {m.loaded ? "loaded" : "idle"}
                                            </span>
                                            <strong>{m.loaded ? formatBytes(m.estimated_bytes) : "—"}</strong>
                                        </li>
                                    ))
                                )}
                            </ul>
                        </>
                    )}

                    <h4>Latency aggregates</h4>
                    <p className="pipeline-muted" style={{ marginTop: "-6px" }}>
                        Run a few searches and wait for captures to flush to see non-zero rows.
                    </p>
                    <div className="pipeline-metrics-table-wrap">
                        <table className="pipeline-metrics-table">
                            <thead>
                                <tr>
                                    <th>Operation</th>
                                    <th>n</th>
                                    <th>ewma ms</th>
                                    <th>max ms</th>
                                    <th>avg ms</th>
                                </tr>
                            </thead>
                            <tbody>
                                {Object.keys(runtimeMetrics.aggregates)
                                    .sort()
                                    .map((key) => {
                                        const row = runtimeMetrics.aggregates[key];
                                        return (
                                            <tr key={key}>
                                                <td>
                                                    <code>{key}</code>
                                                </td>
                                                <td>{row.n}</td>
                                                <td>{row.ewma_ms.toFixed(1)}</td>
                                                <td>{row.max_ms}</td>
                                                <td>{row.avg_ms.toFixed(1)}</td>
                                            </tr>
                                        );
                                    })}
                            </tbody>
                        </table>
                    </div>
                    {Object.keys(runtimeMetrics.counters).length > 0 ? (
                        <>
                            <h4>Timeouts / events</h4>
                            <ul className="pipeline-counter-list">
                                {Object.keys(runtimeMetrics.counters)
                                    .sort()
                                    .map((k) => (
                                        <li key={k}>
                                            <code>{k}</code>: {runtimeMetrics.counters[k]}
                                        </li>
                                    ))}
                            </ul>
                        </>
                    ) : null}
                    <h4>Recent samples</h4>
                    <ul className="pipeline-recent-list">
                        {(runtimeMetrics.recent ?? []).slice(0, 20).map((r, i) => (
                            <li key={`${r.ts_ms}-${i}-${r.op}`}>
                                <code>{r.op}</code> {r.ms} ms
                                {r.meta ? ` · ${r.meta}` : ""}
                            </li>
                        ))}
                    </ul>
                </>
            )}
        </section>
    );
}

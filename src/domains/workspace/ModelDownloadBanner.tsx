import { useCallback, useEffect, useState } from "react";
import {
    ModelInfo,
    downloadModel,
    listAvailableModels,
    refreshAiModels,
} from "@/shared/ipc/onboarding";
import { useModelDownloadStatus } from "@/shared/hooks/useModelDownloadStatus";
import { formatBytes } from "@/shared/utils/format";
import "./ModelDownloadBanner.css";

export function ModelDownloadBanner() {
    const [selected, setSelected] = useState<ModelInfo | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [pendingModelId, setPendingModelId] = useState<string | null>(null);
    const [isActivatingModel, setIsActivatingModel] = useState(false);
    const downloadStatus = useModelDownloadStatus();

    const loadModels = useCallback(async () => {
        const ms = await listAvailableModels();
        setSelected((currentSelected) => {
            const preferred = ms.find((m) => m.recommended) ?? ms[0];
            if (currentSelected) {
                return ms.find((model) => model.id === currentSelected.id) ?? preferred ?? null;
            }
            return preferred ?? null;
        });
    }, []);

    useEffect(() => {
        loadModels().catch((loadError) => {
            setError(`Failed to load models: ${String(loadError)}`);
        });
    }, [loadModels]);

    useEffect(() => {
        if (!pendingModelId || downloadStatus.model_id !== pendingModelId) {
            return;
        }

        if (downloadStatus.state === "failed" && downloadStatus.error) {
            setError(downloadStatus.error);
            setPendingModelId(null);
            void loadModels();
            return;
        }

        if (downloadStatus.state !== "completed" || downloadStatus.error) {
            return;
        }

        let cancelled = false;
        setPendingModelId(null);
        setIsActivatingModel(true);

        void (async () => {
            try {
                const runtime = await refreshAiModels();
                if (!runtime.ai_model_available && !cancelled) {
                    setError(
                        `Model download finished, but FNDR still cannot see the Qwen model at ${downloadStatus.destination_path ?? "disk"}.`,
                    );
                }
            } catch (refreshError) {
                if (!cancelled) {
                    setError(`Model downloaded, but FNDR could not refresh model state: ${String(refreshError)}`);
                }
            } finally {
                if (!cancelled) {
                    setIsActivatingModel(false);
                    void loadModels();
                }
            }
        })();

        return () => {
            cancelled = true;
        };
    }, [downloadStatus.destination_path, downloadStatus.error, downloadStatus.model_id, downloadStatus.state, loadModels, pendingModelId]);

    async function handleDownload() {
        if (!selected) return;
        setError(null);

        if (selected.download_url === "already_downloaded") {
            setIsActivatingModel(true);
            try {
                const runtime = await refreshAiModels();
                if (!runtime.ai_model_available) {
                    setError("Qwen is supposed to be on disk, but FNDR could not find the local model files.");
                }
            } catch (refreshError) {
                setError(String(refreshError));
            } finally {
                setIsActivatingModel(false);
                void loadModels();
            }
            return;
        }

        setPendingModelId(selected.id);
        try {
            await downloadModel(selected.id, selected.download_url, selected.filename);
        } catch (e: unknown) {
            setError(String(e));
            setPendingModelId(null);
        }
    }

    const activeDownloadStatus =
        pendingModelId && downloadStatus.model_id === pendingModelId ? downloadStatus : null;
    const isDownloading =
        isActivatingModel ||
        (activeDownloadStatus !== null &&
            ["preparing", "downloading", "finalizing"].includes(activeDownloadStatus.state));
    const alreadyDownloaded = selected?.download_url === "already_downloaded";
    const activeModelName =
        (activeDownloadStatus && selected?.id === activeDownloadStatus.model_id ? selected.name : selected?.name)
            ?? "AI model";

    return (
        <div className="model-download-banner">
            <div className="banner-header">
                <h3>⚠️ Qwen Model Required</h3>
                <p>
                    FNDR is in OCR-only mode because the required local Qwen3-VL model is missing.
                    Search still works, but memory Q&A, summaries, and smarter indexing need the core model on disk.
                </p>
            </div>

            {error && <div className="banner-error">{error}</div>}

            {isDownloading && activeDownloadStatus?.state === "downloading" ? (
                <div className="banner-progress-area">
                    <div className="banner-progress-details">
                        <span>Downloading {activeModelName}...</span>
                        <span>
                            {formatBytes(activeDownloadStatus.bytes_downloaded)} / {formatBytes(activeDownloadStatus.total_bytes)} ({activeDownloadStatus.percent.toFixed(0)}%)
                        </span>
                    </div>
                    <div className="banner-progress-bar">
                        <div className="banner-progress-fill" style={{ width: `${activeDownloadStatus.percent}%` }} />
                    </div>
                    {activeDownloadStatus.destination_path && (
                        <div style={{ marginTop: 8, fontSize: 11, opacity: 0.75 }}>
                            {activeDownloadStatus.destination_path}
                        </div>
                    )}
                </div>
            ) : isDownloading ? (
                <div className="banner-progress-area" style={{ textAlign: "center", fontStyle: "italic", opacity: 0.8 }}>
                    <span className="ob-icon pulse" style={{ marginRight: 8 }}>⚙️</span>
                    {isActivatingModel
                        ? "Loading model into FNDR"
                        : activeDownloadStatus?.state === "finalizing"
                            ? "Finalizing model on disk"
                            : "Preparing download and connecting to HuggingFace"}
                </div>
            ) : null}

            {isDownloading && (activeDownloadStatus?.logs.length ?? 0) > 0 && (
                <div style={{
                    marginTop: 12,
                    background: "rgba(0,0,0,0.3)",
                    padding: "8px 12px",
                    borderRadius: 6,
                    fontFamily: "inherit",
                    fontSize: 10,
                    color: "rgba(255,255,255,0.6)",
                    height: 80,
                    overflowY: "auto"
                }}>
                    <div style={{ color: "rgba(255,255,255,0.9)", marginBottom: 6 }}>
                        Stage: {activeDownloadStatus?.state ?? (isActivatingModel ? "activating" : "pending")}
                    </div>
                    {activeDownloadStatus?.logs.map((line, index) => <div key={index}>{line}</div>)}
                </div>
            )}

            {!isDownloading && (
                <div className="banner-action-area">
                    <button className="banner-download-btn" onClick={handleDownload} disabled={!selected}>
                        {alreadyDownloaded
                            ? `Load ${selected?.name ?? "Qwen"}`
                            : `Download ${selected?.name ?? "Qwen"} (${selected?.size_label ?? ""})`}
                    </button>
                    <span className="banner-meta">Memory: ~{selected?.ram_gb} GB RAM</span>
                </div>
            )}
        </div>
    );
}

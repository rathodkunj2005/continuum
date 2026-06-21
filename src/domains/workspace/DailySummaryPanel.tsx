import { useState, useEffect } from "react";
import { generateDailySummaryForDate, exportDailySummaryPdf, openExportedPdf } from "@/shared/ipc/tauri";
import "./DailySummaryPanel.css";

interface DailySummaryPanelProps {
    isVisible: boolean;
    onClose: () => void;
}

export function DailySummaryPanel({ isVisible, onClose }: DailySummaryPanelProps) {
    const [dateStr, setDateStr] = useState<string>("");
    const [summary, setSummary] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [exporting, setExporting] = useState(false);
    const [showToast, setShowToast] = useState(false);
    const [exportedPdfPath, setExportedPdfPath] = useState<string | null>(null);
    const [cache, setCache] = useState<Map<string, string>>(new Map());

    // Initialize to today's date in local YYYY-MM-DD
    useEffect(() => {
        const today = new Date();
        const yyyy = today.getFullYear();
        const mm = String(today.getMonth() + 1).padStart(2, "0");
        const dd = String(today.getDate()).padStart(2, "0");
        setDateStr(`${yyyy}-${mm}-${dd}`);
    }, []);

    const handleGenerate = async () => {
        if (!dateStr) return;

        if (cache.has(dateStr)) {
            setSummary(cache.get(dateStr) ?? null);
            return;
        }

        setLoading(true);
        setError(null);
        setSummary(null);

        try {
            const rawSummary = await generateDailySummaryForDate(dateStr);
            setSummary(rawSummary);
            setCache((prevConfig) => new Map(prevConfig).set(dateStr, rawSummary));
        } catch (err) {
            setError(err instanceof Error ? err.message : "Failed to generate summary.");
        } finally {
            setLoading(false);
        }
    };

    const handleDownloadPdf = async () => {
        if (!dateStr || !summary) return;
        setExporting(true);
        setError(null);
        try {
            const path = await exportDailySummaryPdf(dateStr, summary);
            setExportedPdfPath(path);
            setShowToast(true);
            setTimeout(() => {
                setShowToast(false);
            }, 6000);
        } catch (err) {
            setError(String(err));
        } finally {
            setExporting(false);
        }
    };

    const handleOpenPdf = async () => {
        if (!exportedPdfPath) {
            return;
        }

        try {
            await openExportedPdf(exportedPdfPath);
            setShowToast(false);
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        }
    };

    if (!isVisible) {
        return null;
    }

    return (
        <div className="daily-summary-page">
            <header className="daily-summary-header">
                <div>
                    <h2>Daily Summary</h2>
                    <p>On-demand activity clustering and intelligence</p>
                </div>
                <div className="daily-summary-actions">
                    <button className="ui-action-btn daily-summary-close-btn" onClick={onClose}>X</button>
                </div>
            </header>

            <div className="daily-summary-body">
                <div className="daily-summary-controls">
                    <div className="date-picker-wrapper">
                        <label htmlFor="daily-summary-date">Select Date</label>
                        <input
                            id="daily-summary-date"
                            type="date"
                            value={dateStr}
                            onChange={(e) => setDateStr(e.target.value)}
                            max={new Date().toISOString().split("T")[0]}
                        />
                    </div>
                    <button 
                        className="ui-action-btn generate-btn" 
                        onClick={() => void handleGenerate()}
                        disabled={loading || !dateStr}
                    >
                        {loading ? "Generating..." : "Generate Summary"}
                    </button>
                    {summary && (
                        <button
                            className={`ui-action-btn generate-btn download-pdf-btn ${exporting ? "loading" : ""}`}
                            onClick={() => void handleDownloadPdf()}
                            disabled={exporting || loading}
                        >
                            {exporting ? "Exporting..." : "↓ Download PDF"}
                        </button>
                    )}
                </div>

                <div className="daily-summary-content">
                    {loading && (
                        <div className="daily-summary-state">
                            <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                            <p>Clustering the day&apos;s local memories...</p>
                        </div>
                    )}

                    {!loading && error && (
                        <div className="daily-summary-state error-state">
                            <p>{error}</p>
                        </div>
                    )}

                    {!loading && !error && summary && (
                        <div className="summary-bullets">
                            {summary.split("\n").map((line, idx) => {
                                const trim = line.trim();
                                if (!trim) return null;
                                return (
                                    <p key={idx} className="summary-bullet">
                                        {trim.startsWith("-") || trim.startsWith("•") || trim.startsWith("*") 
                                            ? trim 
                                            : `• ${trim}`}
                                    </p>
                                );
                            })}
                        </div>
                    )}

                    {!loading && !error && !summary && cache.size === 0 && (
                        <div className="daily-summary-state empty-state">
                            <span className="shining-shield">📅</span>
                            <p>Select a date and click Generate Summary to see your daily briefing.</p>
                        </div>
                    )}
                </div>
            </div>

            {showToast && (
                <div className="daily-toast" role="status" aria-live="polite">
                    <div className="daily-toast-copy">
                        <strong>PDF downloaded</strong>
                        <span>Open the exported daily summary from Continuum.</span>
                    </div>
                    <div className="daily-toast-actions">
                        <button className="daily-toast-btn primary" onClick={() => void handleOpenPdf()}>
                            Open PDF
                        </button>
                        <button className="daily-toast-btn" onClick={() => setShowToast(false)}>
                            Dismiss
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}

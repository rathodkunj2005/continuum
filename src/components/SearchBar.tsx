import { useEffect, useRef, useState } from "react";
import {
    MemoryCard,
    pauseCapture,
    resumeCapture,
    searchMemoryCards,
    summarizeSearch,
    transcribeVoiceInput,
} from "../api/tauri";
import {
    MEMORY_MENTIONS,
    SEARCH_PLACEHOLDER,
    SEARCH_SUMMARY,
    VOICE_RECORDING,
} from "../lib/config";
import { bubblePurityGate, extractAnchorTerms, scoreAnchorCoverage } from "../lib/search";
import { PLACEHOLDERS } from "./placeholders";
import "./SearchBar.css";

interface SearchBarProps {
    value: string;
    submittedValue: string;
    onChange: (value: string) => void;
    onSubmit: (value?: string) => void | Promise<void>;
    timeFilter: string | null;
    onTimeFilterChange: (filter: string | null) => void;
    appFilter: string | null;
    onAppFilterChange: (filter: string | null) => void;
    onSetMeetingPanelOpen: (open: boolean) => void;
    onSetMemoryCardsPanelOpen: (open: boolean) => void;
    appNames: string[];
    resultCount: number;
    searchResults: MemoryCard[];
    disabled?: boolean;
    disabledHint?: string;
}

const PLACEHOLDER_DISPLAY_DURATION = SEARCH_PLACEHOLDER.displayDurationMs;
const PLACEHOLDER_FADE_DURATION = SEARCH_PLACEHOLDER.fadeDurationMs;
const DEFAULT_PLACEHOLDER = "Recall a specific meeting, note, or idea...";

export function SearchBar({
    value,
    submittedValue,
    onChange,
    onSubmit,
    timeFilter,
    onTimeFilterChange,
    appFilter,
    onAppFilterChange,
    onSetMeetingPanelOpen,
    onSetMemoryCardsPanelOpen,
    appNames,
    resultCount,
    searchResults,
    disabled = false,
    disabledHint,
}: SearchBarProps) {
    const [summary, setSummary] = useState<string | null>(null);
    const [isSummarizing, setIsSummarizing] = useState(false);
    const [voiceStatus, setVoiceStatus] = useState<string | null>(null);
    const [isRecording, setIsRecording] = useState(false);
    const [isTranscribing, setIsTranscribing] = useState(false);
    const [placeholderIndex, setPlaceholderIndex] = useState(0);
    const [placeholderVisible, setPlaceholderVisible] = useState(true);

    const mediaRecorderRef = useRef<MediaRecorder | null>(null);
    const mediaStreamRef = useRef<MediaStream | null>(null);
    const audioChunksRef = useRef<Blob[]>([]);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const mimeTypeRef = useRef<string>("audio/webm");
    const recordingStartedAtRef = useRef<number>(0);
    const summaryRequestRef = useRef(0);
    const searchResultsRef = useRef(searchResults);
    const hasQuery = submittedValue.trim().length > 0;
    const hasPendingSubmit = value.trim() !== submittedValue.trim();
    const showMetaRow = hasQuery;
    const hasInput = value.length > 0;
    const activePlaceholder =
        PLACEHOLDERS[placeholderIndex % Math.max(PLACEHOLDERS.length, 1)] ?? DEFAULT_PLACEHOLDER;
    const showAnimatedPlaceholder = !hasInput;

    const atMemoryMatch = /@memory\s+(.+)/i.exec(value);
    const atMemoryQuery = atMemoryMatch?.[1]?.trim() ?? "";
    const [memoryMentionHits, setMemoryMentionHits] = useState<MemoryCard[]>([]);
    const [memoryMentionBusy, setMemoryMentionBusy] = useState(false);

    useEffect(() => {
        if (!atMemoryQuery || atMemoryQuery.length < MEMORY_MENTIONS.minQueryLength) {
            setMemoryMentionHits([]);
            return;
        }
        let cancelled = false;
        const timer = window.setTimeout(() => {
            void (async () => {
                setMemoryMentionBusy(true);
                try {
                    const hits = await searchMemoryCards(
                        atMemoryQuery,
                        undefined,
                        undefined,
                        MEMORY_MENTIONS.limit
                    );
                    if (!cancelled) {
                        setMemoryMentionHits(hits);
                    }
                } catch {
                    if (!cancelled) {
                        setMemoryMentionHits([]);
                    }
                } finally {
                    if (!cancelled) {
                        setMemoryMentionBusy(false);
                    }
                }
            })();
        }, MEMORY_MENTIONS.debounceMs);
        return () => {
            cancelled = true;
            window.clearTimeout(timer);
        };
    }, [atMemoryQuery, value]);

    useEffect(() => {
        searchResultsRef.current = searchResults;
    }, [searchResults]);

    useEffect(() => {
        if (PLACEHOLDERS.length <= 1) {
            return;
        }

        let swapTimer: number | undefined;
        const displayTimer = window.setTimeout(() => {
            setPlaceholderVisible(false);
            swapTimer = window.setTimeout(() => {
                setPlaceholderIndex((index) => (index + 1) % PLACEHOLDERS.length);
                setPlaceholderVisible(true);
            }, PLACEHOLDER_FADE_DURATION);
        }, PLACEHOLDER_DISPLAY_DURATION);

        return () => {
            window.clearTimeout(displayTimer);
            if (swapTimer !== undefined) {
                window.clearTimeout(swapTimer);
            }
        };
    }, [placeholderIndex]);

    useEffect(() => {
        const activeValue = submittedValue.trim();
        const requestId = ++summaryRequestRef.current;

        if (!activeValue || resultCount === 0) {
            setSummary(null);
            setIsSummarizing(false);
            return;
        }

        let cancelled = false;
        setIsSummarizing(true);
        setSummary(null);

        const timer = window.setTimeout(async () => {
            const latestResults = searchResultsRef.current;
            if (cancelled || requestId !== summaryRequestRef.current) {
                return;
            }

            if (latestResults.length === 0) {
                setIsSummarizing(false);
                return;
            }

            try {
                const anchorTerms = extractAnchorTerms(activeValue);
                const topicalCards = latestResults
                    .map((result) => {
                        const fallbackText = [
                            result.title,
                            result.display_summary ?? result.summary,
                            result.summary,
                            ...(result.raw_snippets ?? []),
                        ]
                            .filter(Boolean)
                            .join(" ");
                        const coverage = result.anchor_coverage_score
                            ?? scoreAnchorCoverage(fallbackText, anchorTerms);
                        return { result, coverage };
                    })
                    .filter((item) => item.coverage >= SEARCH_SUMMARY.coverageFloor)
                    .slice(0, SEARCH_SUMMARY.maxCards);

                if (topicalCards.length < 2) {
                    setSummary(null);
                    return;
                }

                const snippets = topicalCards
                    .flatMap(({ result }) => {
                        const evidence = (result.raw_snippets ?? [])
                            .map((snippet) => snippet.trim())
                            .filter(Boolean)
                            .slice(0, SEARCH_SUMMARY.snippetsPerCard);
                        if (evidence.length === 0) {
                            const fallback = (result.display_summary ?? result.summary ?? "").trim();
                            return fallback
                                ? [{ memoryId: result.id, score: result.score, appName: result.app_name, snippet: fallback }]
                                : [];
                        }
                        return evidence.map((snippet) => ({
                            memoryId: result.id,
                            score: result.score,
                            appName: result.app_name,
                            snippet,
                        }));
                    })
                    .slice(0, SEARCH_SUMMARY.maxSnippets)
                    .map(
                        (item) =>
                            `[id:${item.memoryId}][score:${item.score.toFixed(3)}][app:${item.appName}] ${item.snippet}`
                    );

                if (snippets.length < 2) {
                    setSummary(null);
                    return;
                }

                const aiSummary = await summarizeSearch(activeValue, snippets);
                if (cancelled || requestId !== summaryRequestRef.current) {
                    return;
                }
                if (!aiSummary?.trim()) {
                    setSummary(null);
                    return;
                }

                const purity = bubblePurityGate(aiSummary, anchorTerms);
                if (!purity.pass) {
                    setSummary(null);
                    return;
                }
                setSummary(aiSummary);
            } catch (err) {
                if (cancelled || requestId !== summaryRequestRef.current) {
                    return;
                }
                console.error("Summary generation failed:", err);
                setSummary(null);
            } finally {
                if (!cancelled && requestId === summaryRequestRef.current) {
                    setIsSummarizing(false);
                }
            }
        }, SEARCH_SUMMARY.delayMs);

        return () => {
            cancelled = true;
            window.clearTimeout(timer);
        };
    }, [submittedValue, resultCount]);
    
    useEffect(() => {
        if (inputRef.current && value.length > 0) {
            inputRef.current.scrollLeft = 0;
        }
    }, [value]);

    useEffect(() => {
        return () => {
            stopMediaStream(mediaStreamRef.current);
            mediaStreamRef.current = null;
        };
    }, []);

    useEffect(() => {
        const handleKeydown = (event: KeyboardEvent) => {
            const key = event.key.toLowerCase();
            if ((event.metaKey || event.ctrlKey) && key === "k") {
                event.preventDefault();
                const input = document.getElementById("fndr-search-input") as HTMLElement | null;
                input?.focus();
                return;
            }

            if (key === "escape" && !disabled) {
                onChange("");
                onSubmit("");
            }
        };

        window.addEventListener("keydown", handleKeydown);
        return () => window.removeEventListener("keydown", handleKeydown);
    }, [disabled, onChange, onSubmit]);

    async function handleVoiceTranscript(transcript: string) {
        const cleaned = transcript.trim();
        if (!cleaned) {
            setVoiceStatus("I didn't catch that.");
            return;
        }

        const normalized = cleaned.toLowerCase();
        setVoiceStatus(`Heard: ${cleaned}`);

        if (normalized === "clear" || normalized === "clear search" || normalized === "reset search") {
            onChange("");
            onSubmit("");
            setVoiceStatus("Search cleared.");
            return;
        }

        if (normalized.startsWith("search for ")) {
            const nextQuery = cleaned.slice("search for ".length).trim();
            onChange(nextQuery);
            onSubmit(nextQuery);
            setVoiceStatus(`Searching for: ${nextQuery}`);
            return;
        }

        if (normalized.startsWith("find ")) {
            const nextQuery = cleaned.slice("find ".length).trim();
            onChange(nextQuery);
            onSubmit(nextQuery);
            setVoiceStatus(`Searching for: ${nextQuery}`);
            return;
        }

        if (normalized.startsWith("look for ")) {
            const nextQuery = cleaned.slice("look for ".length).trim();
            onChange(nextQuery);
            onSubmit(nextQuery);
            setVoiceStatus(`Searching for: ${nextQuery}`);
            return;
        }

        if (normalized.includes("open meetings") || normalized.includes("open meeting recorder")) {
            onSetMeetingPanelOpen(true);
            setVoiceStatus("Opened Meetings.");
            return;
        }

        if (normalized.includes("close meetings") || normalized.includes("close meeting recorder")) {
            onSetMeetingPanelOpen(false);
            setVoiceStatus("Closed Meetings.");
            return;
        }

        if (normalized.includes("open graph") || normalized.includes("open knowledge graph")) {
            onSetMemoryCardsPanelOpen(true);
            setVoiceStatus("Opened Graph.");
            return;
        }

        if (normalized.includes("close graph") || normalized.includes("close knowledge graph")) {
            onSetMemoryCardsPanelOpen(false);
            setVoiceStatus("Closed Graph.");
            return;
        }

        if (normalized.includes("pause capture") || normalized.includes("pause recording")) {
            await pauseCapture();
            setVoiceStatus("Capture paused.");
            return;
        }

        if (normalized.includes("resume capture") || normalized.includes("start capture")) {
            await resumeCapture();
            setVoiceStatus("Capture resumed.");
            return;
        }

        onChange(cleaned);
        onSubmit(cleaned);
        setVoiceStatus(`Searching for: ${cleaned}`);
        setTimeout(() => setVoiceStatus(null), VOICE_RECORDING.statusClearMs);
    }

    async function handleVoiceToggle() {
        if (isRecording) {
            mediaRecorderRef.current?.stop();
            return;
        }

        if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
            setVoiceStatus("Voice capture is not supported in this build.");
            return;
        }

        try {
            const stream = await navigator.mediaDevices.getUserMedia({
                audio: {
                    echoCancellation: true,
                    noiseSuppression: true,
                    autoGainControl: true,
                    channelCount: VOICE_RECORDING.channelCount,
                    sampleRate: VOICE_RECORDING.sampleRate,
                },
            });
            const options = chooseRecorderOptions();
            const recorder = options ? new MediaRecorder(stream, options) : new MediaRecorder(stream);

            mediaStreamRef.current = stream;
            mediaRecorderRef.current = recorder;
            audioChunksRef.current = [];
            mimeTypeRef.current = recorder.mimeType || options?.mimeType || "audio/webm";
            recordingStartedAtRef.current = Date.now();

            recorder.ondataavailable = (event) => {
                if (event.data.size > 0) {
                    audioChunksRef.current.push(event.data);
                }
            };

            recorder.onstop = () => {
                const chunks = [...audioChunksRef.current];
                audioChunksRef.current = [];
                const durationMs = Date.now() - recordingStartedAtRef.current;
                stopMediaStream(mediaStreamRef.current);
                mediaStreamRef.current = null;
                mediaRecorderRef.current = null;
                setIsRecording(false);
                if (durationMs < VOICE_RECORDING.minDurationMs) {
                    setVoiceStatus("Hold the mic a bit longer and try again.");
                    return;
                }
                void transcribeRecordedVoice(chunks, mimeTypeRef.current);
            };

            recorder.start(VOICE_RECORDING.timesliceMs);
            setIsRecording(true);
            setVoiceStatus("Listening... tap again to stop.");
        } catch (err) {
            console.error("Voice capture failed:", err);
            setVoiceStatus("Microphone access failed.");
            stopMediaStream(mediaStreamRef.current);
            mediaStreamRef.current = null;
            mediaRecorderRef.current = null;
            setIsRecording(false);
        }
    }

    async function transcribeRecordedVoice(chunks: Blob[], mimeType: string) {
        if (chunks.length === 0) {
            setVoiceStatus("No voice input captured.");
            return;
        }

        setIsTranscribing(true);
        setVoiceStatus("Transcribing with Whisper...");

        try {
            const blob = new Blob(chunks, { type: mimeType });
            const audioBytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
            const result = await transcribeVoiceInput(audioBytes, mimeType);
            await handleVoiceTranscript(result.text);
        } catch (err) {
            console.error("Voice transcription failed:", err);
            setVoiceStatus(`Voice transcription failed: ${String(err)}`);
        } finally {
            setIsTranscribing(false);
        }
    }

    return (
        <div className="search-panel">
            {disabled && disabledHint && (
                <p className="search-disabled-hint" role="status">
                    {disabledHint}
                </p>
            )}

            <div className="search-bar" role="search">
                <div className="search-input-group">
                    <svg className="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <circle cx="11" cy="11" r="8" />
                        <path d="M21 21l-4.35-4.35" />
                    </svg>

                    <div className="search-input-wrap">
                        <textarea
                            id="fndr-search-input"
                            ref={inputRef}
                            rows={1}
                            wrap="off"
                            value={value}
                            onChange={(e) => onChange(e.target.value.replace(/\r?\n/g, " "))}
                            onKeyDown={(e) => {
                                if (e.key === "Enter") {
                                    e.preventDefault();
                                    if (!disabled) {
                                        onSubmit();
                                    }
                                }
                            }}
                            placeholder={activePlaceholder}
                            className="search-input search-input-cycling search-input-scrollable"
                            autoComplete="off"
                            disabled={disabled}
                            aria-disabled={disabled}
                            aria-label="Search memories"
                        />
                        <span
                            className="search-placeholder-overlay"
                            aria-hidden="true"
                            style={{
                                opacity: showAnimatedPlaceholder ? (placeholderVisible ? 1 : 0) : 0,
                                transform: showAnimatedPlaceholder
                                    ? placeholderVisible
                                        ? "translateY(-50%)"
                                        : "translateY(calc(-50% + 6px))"
                                    : "translateY(-50%)",
                                transition: `opacity ${PLACEHOLDER_FADE_DURATION}ms ease, transform ${PLACEHOLDER_FADE_DURATION}ms ease`,
                            }}
                        >
                            {activePlaceholder}
                        </span>
                    </div>

                    <button
                        className={`voice-btn ${isRecording ? "recording" : ""}`}
                        onClick={() => void handleVoiceToggle()}
                        aria-label={isRecording ? "Stop voice recording" : "Start voice recording"}
                        title={isRecording ? "Stop voice recording" : "Start voice recording"}
                        disabled={disabled || isTranscribing}
                    >
                        {isRecording ? "Stop" : isTranscribing ? "..." : "Mic"}
                    </button>

                    {value && (
                        <button
                            className="search-clear"
                            onClick={() => {
                                onChange("");
                                onSubmit("");
                            }}
                            aria-label="Clear search"
                            disabled={disabled}
                        >
                            ×
                        </button>
                    )}
                </div>
                {atMemoryQuery.length >= 2 && (
                    <div className="memory-mention-popover" role="listbox" aria-label="Memory matches">
                        {memoryMentionBusy ? (
                            <div className="memory-mention-loading">Searching memories…</div>
                        ) : memoryMentionHits.length === 0 ? (
                            <div className="memory-mention-empty">No hits</div>
                        ) : (
                            memoryMentionHits.map((h) => (
                                <button
                                    key={h.id}
                                    type="button"
                                    className="memory-mention-item"
                                    onClick={() => {
                                        const stamp = new Date(h.timestamp).toISOString();
                                        const block = `[memory ${h.app_name} @ ${stamp}] ${h.summary.slice(0, 200)}`;
                                        onChange(value.replace(/@memory\s+.*/i, block));
                                    }}
                                >
                                    <span className="memory-mention-title">{h.title}</span>
                                    <span className="memory-mention-snippet">{h.summary.slice(0, 120)}</span>
                                </button>
                            ))
                        )}
                    </div>
                )}
            </div>

            {showMetaRow && (
                <div className="search-meta-row">
                    <div className="search-filters">
                        <div className="select-wrapper">
                            <svg className="filter-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                                <circle cx="12" cy="12" r="8" />
                                <path d="M12 8v4l2.5 2.5" />
                            </svg>
                            <select
                                value={timeFilter || ""}
                                onChange={(e) => onTimeFilterChange(e.target.value || null)}
                                className={`filter-select ${timeFilter ? "active" : ""}`}
                                disabled={disabled}
                            >
                                <option value="">All time</option>
                                <option value="1h">Last hour</option>
                                <option value="24h">Last 24 hours</option>
                                <option value="7d">Last 7 days</option>
                            </select>
                            <svg className="select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>

                        <div className="select-wrapper">
                            <svg className="filter-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                                <path d="M4 6h7v5H4zM13 6h7v5h-7zM4 13h7v5H4zM13 13h7v5h-7z" />
                            </svg>
                            <select
                                value={appFilter || ""}
                                onChange={(e) => onAppFilterChange(e.target.value || null)}
                                className={`filter-select ${appFilter ? "active" : ""}`}
                                disabled={disabled}
                            >
                                <option value="">All apps</option>
                                {appNames.map((name) => (
                                    <option key={name} value={name}>{name}</option>
                                ))}
                            </select>
                            <svg className="select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </div>

                    <div className="result-count">
                        {hasQuery ? `${resultCount} results` : "Ready"}
                    </div>
                </div>
            )}

            {hasPendingSubmit && (
                <div className="voice-status">
                    Press Enter to search
                </div>
            )}

            {voiceStatus && (
                <div className={`voice-status ${isRecording ? "recording" : ""}`}>
                    {voiceStatus}
                </div>
            )}

            {hasQuery && resultCount > 0 && (isSummarizing || Boolean(summary)) && (
                <div className="summary-bubble">
                    {isSummarizing ? (
                        <div className="summary-loading">
                            <span className="thinking-loader thinking-loader-sm summary-loader" aria-hidden="true" />
                            <span>Synthesizing memories...</span>
                        </div>
                    ) : (
                        <p className="summary-text">
                            <span className="summary-icon">💡</span>
                            {summary}
                        </p>
                    )}
                </div>
            )}
        </div>
    );
}

function chooseRecorderOptions(): MediaRecorderOptions | undefined {
    const candidates = [
        "audio/webm;codecs=opus",
        "audio/mp4",
        "audio/ogg;codecs=opus",
        "audio/webm",
    ];

    for (const mimeType of candidates) {
        if (MediaRecorder.isTypeSupported(mimeType)) {
            return {
                mimeType,
                audioBitsPerSecond: VOICE_RECORDING.audioBitsPerSecond,
            };
        }
    }

    return undefined;
}

function stopMediaStream(stream: MediaStream | null) {
    stream?.getTracks().forEach((track) => track.stop());
}

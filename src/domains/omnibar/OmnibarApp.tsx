import { useCallback, useEffect, useRef, useState } from "react";
import {
    type ClipboardEntry,
    type ComposedAnswer,
    type MemoryCard,
    OMNIBAR_FOCUS_EVENT,
    copyClipboardEntry,
    dismissOmnibar,
    continuumAnswer,
    getClipboardHistory,
    omnibarOpenMemory,
    pasteClipboardEntry,
    searchMemoryCards,
} from "@/shared/ipc/tauri";
import { useTauriEvent } from "@/shared/hooks/useTauriEvent";

const SEARCH_DEBOUNCE_MS = 250;
const CLIP_DEBOUNCE_MS = 150;
const RESULT_LIMIT = 8;
const CLIP_LIMIT = 30;
const COPIED_FLASH_MS = 550;

type Surface = "memory" | "clipboard";

type Mode =
    | { kind: "search" }
    | { kind: "asking" }
    | { kind: "answer"; answer: ComposedAnswer };

function formatTimestamp(ms: number): string {
    const date = new Date(ms);
    const now = new Date();
    const sameDay = date.toDateString() === now.toDateString();
    if (sameDay) {
        return date.toLocaleTimeString(undefined, {
            hour: "numeric",
            minute: "2-digit",
        });
    }
    return date.toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
    });
}

export function OmnibarApp() {
    const [surface, setSurface] = useState<Surface>("memory");
    const [query, setQuery] = useState("");
    const [results, setResults] = useState<MemoryCard[]>([]);
    const [clips, setClips] = useState<ClipboardEntry[]>([]);
    const [selectedIndex, setSelectedIndex] = useState(0);
    const [searching, setSearching] = useState(false);
    const [mode, setMode] = useState<Mode>({ kind: "search" });
    const [copiedId, setCopiedId] = useState<string | null>(null);
    const inputRef = useRef<HTMLInputElement>(null);
    const listRef = useRef<HTMLDivElement>(null);
    const searchSeq = useRef(0);

    const reset = useCallback(() => {
        setSurface("memory");
        setQuery("");
        setResults([]);
        setClips([]);
        setSelectedIndex(0);
        setSearching(false);
        setMode({ kind: "search" });
        setCopiedId(null);
    }, []);

    useTauriEvent<void>(OMNIBAR_FOCUS_EVENT, () => {
        reset();
        inputRef.current?.focus();
    });

    useEffect(() => {
        inputRef.current?.focus();
    }, []);

    useEffect(() => {
        if (surface !== "memory" || mode.kind !== "search") {
            return;
        }
        const trimmed = query.trim();
        if (!trimmed) {
            setResults([]);
            setSelectedIndex(0);
            setSearching(false);
            return;
        }
        const seq = ++searchSeq.current;
        setSearching(true);
        const timer = window.setTimeout(() => {
            searchMemoryCards(trimmed, undefined, undefined, RESULT_LIMIT)
                .then((cards) => {
                    if (searchSeq.current !== seq) {
                        return;
                    }
                    setResults(cards);
                    setSelectedIndex(0);
                    setSearching(false);
                })
                .catch(() => {
                    if (searchSeq.current !== seq) {
                        return;
                    }
                    setResults([]);
                    setSearching(false);
                });
        }, SEARCH_DEBOUNCE_MS);
        return () => window.clearTimeout(timer);
    }, [query, surface, mode.kind]);

    useEffect(() => {
        if (surface !== "clipboard") {
            return;
        }
        const seq = ++searchSeq.current;
        setSearching(true);
        const timer = window.setTimeout(() => {
            getClipboardHistory(query.trim() || undefined, CLIP_LIMIT)
                .then((entries) => {
                    if (searchSeq.current !== seq) {
                        return;
                    }
                    setClips(entries);
                    setSelectedIndex(0);
                    setSearching(false);
                })
                .catch(() => {
                    if (searchSeq.current !== seq) {
                        return;
                    }
                    setClips([]);
                    setSearching(false);
                });
        }, CLIP_DEBOUNCE_MS);
        return () => window.clearTimeout(timer);
    }, [query, surface]);

    useEffect(() => {
        const selected = listRef.current?.querySelector(
            '[data-selected="true"]'
        );
        selected?.scrollIntoView({ block: "nearest" });
    }, [selectedIndex]);

    const openMemory = useCallback(
        (memoryId: string) => {
            void omnibarOpenMemory(memoryId).then(reset);
        },
        [reset]
    );

    const copyClip = useCallback(
        (clip: ClipboardEntry) => {
            setCopiedId(clip.id);
            void copyClipboardEntry(clip.text).then(() => {
                window.setTimeout(() => {
                    reset();
                    void dismissOmnibar();
                }, COPIED_FLASH_MS);
            });
        },
        [reset]
    );

    const pasteClip = useCallback(
        (clip: ClipboardEntry) => {
            void pasteClipboardEntry(clip.text).then(reset);
        },
        [reset]
    );

    const ask = useCallback(() => {
        const trimmed = query.trim();
        if (!trimmed) {
            return;
        }
        searchSeq.current += 1;
        setSearching(false);
        setMode({ kind: "asking" });
        continuumAnswer(trimmed)
            .then((answer) => setMode({ kind: "answer", answer }))
            .catch(() => setMode({ kind: "search" }));
    }, [query]);

    const dismiss = useCallback(() => {
        reset();
        void dismissOmnibar();
    }, [reset]);

    const toggleSurface = useCallback(() => {
        setSurface((s) => (s === "memory" ? "clipboard" : "memory"));
        setQuery("");
        setResults([]);
        setClips([]);
        setSelectedIndex(0);
        setMode({ kind: "search" });
        inputRef.current?.focus();
    }, []);

    const handleKeyDown = (event: React.KeyboardEvent) => {
        if (event.key === "Escape") {
            event.preventDefault();
            if (mode.kind === "answer" || mode.kind === "asking") {
                setMode({ kind: "search" });
                inputRef.current?.focus();
            } else {
                dismiss();
            }
            return;
        }
        if (event.key === "Tab") {
            event.preventDefault();
            toggleSurface();
            return;
        }
        if (mode.kind !== "search") {
            return;
        }
        const listLength =
            surface === "memory" ? results.length : clips.length;
        if (event.key === "ArrowDown") {
            event.preventDefault();
            setSelectedIndex((i) => Math.min(i + 1, listLength - 1));
        } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setSelectedIndex((i) => Math.max(i - 1, 0));
        } else if (event.key === "Enter") {
            event.preventDefault();
            if (surface === "clipboard") {
                const clip = clips[selectedIndex];
                if (!clip) {
                    return;
                }
                if (event.metaKey || event.ctrlKey) {
                    pasteClip(clip);
                } else {
                    copyClip(clip);
                }
                return;
            }
            if (event.metaKey || event.ctrlKey) {
                ask();
            } else if (results[selectedIndex]) {
                openMemory(results[selectedIndex].id);
            }
        }
    };

    return (
        <div className="omnibar" onKeyDown={handleKeyDown}>
            <div className="omnibar-input-row">
                <span className="omnibar-glyph" aria-hidden>
                    ⌕
                </span>
                <input
                    ref={inputRef}
                    className="omnibar-input"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder={
                        surface === "memory"
                            ? "Search your memory…"
                            : "Search clipboard history…"
                    }
                    spellCheck={false}
                    autoComplete="off"
                    disabled={mode.kind === "asking"}
                />
                {searching && <span className="omnibar-spinner" aria-hidden />}
                <button
                    type="button"
                    className="omnibar-surface-toggle"
                    onClick={toggleSurface}
                    tabIndex={-1}
                >
                    <span data-active={surface === "memory"}>Memory</span>
                    <span data-active={surface === "clipboard"}>Clips</span>
                </button>
            </div>

            {surface === "memory" && mode.kind === "search" && (
                <div className="omnibar-results" ref={listRef} role="listbox">
                    {results.map((card, index) => (
                        <button
                            key={card.id}
                            type="button"
                            role="option"
                            aria-selected={index === selectedIndex}
                            data-selected={index === selectedIndex}
                            className="omnibar-result"
                            onMouseEnter={() => setSelectedIndex(index)}
                            onClick={() => openMemory(card.id)}
                        >
                            <span className="omnibar-result-title">
                                {card.title || card.window_title}
                            </span>
                            <span className="omnibar-result-snippet">
                                {card.display_summary || card.summary}
                            </span>
                            <span className="omnibar-result-meta">
                                {card.app_name} ·{" "}
                                {formatTimestamp(card.timestamp)}
                            </span>
                        </button>
                    ))}
                    {!results.length && query.trim() && !searching && (
                        <div className="omnibar-empty">No matches yet</div>
                    )}
                </div>
            )}

            {surface === "clipboard" && (
                <div className="omnibar-results" ref={listRef} role="listbox">
                    {clips.map((clip, index) => (
                        <button
                            key={clip.id}
                            type="button"
                            role="option"
                            aria-selected={index === selectedIndex}
                            data-selected={index === selectedIndex}
                            className="omnibar-result"
                            onMouseEnter={() => setSelectedIndex(index)}
                            onClick={() => copyClip(clip)}
                        >
                            <span className="omnibar-result-snippet omnibar-clip-text">
                                {clip.text}
                            </span>
                            <span className="omnibar-result-meta">
                                {copiedId === clip.id
                                    ? "Copied ✓"
                                    : [
                                          clip.app_name,
                                          formatTimestamp(clip.timestamp),
                                      ]
                                          .filter(Boolean)
                                          .join(" · ")}
                            </span>
                        </button>
                    ))}
                    {!clips.length && !searching && (
                        <div className="omnibar-empty">
                            {query.trim()
                                ? "No matching clips"
                                : "Nothing copied yet"}
                        </div>
                    )}
                </div>
            )}

            {surface === "memory" && mode.kind === "asking" && (
                <div className="omnibar-answer omnibar-answer-loading">
                    Thinking through your memory…
                </div>
            )}

            {surface === "memory" && mode.kind === "answer" && (
                <div className="omnibar-answer">
                    <p className="omnibar-answer-text">{mode.answer.answer}</p>
                    {mode.answer.cards.length > 0 && (
                        <div className="omnibar-citations">
                            {mode.answer.cards.slice(0, 4).map((card) => (
                                <button
                                    key={card.id}
                                    type="button"
                                    className="omnibar-citation"
                                    onClick={() => openMemory(card.id)}
                                >
                                    <span className="omnibar-result-title">
                                        {card.title || card.window_title}
                                    </span>
                                    <span className="omnibar-result-meta">
                                        {card.app_name} ·{" "}
                                        {formatTimestamp(card.timestamp)}
                                    </span>
                                </button>
                            ))}
                        </div>
                    )}
                </div>
            )}

            <div className="omnibar-footer">
                <span>↹ {surface === "memory" ? "clips" : "memory"}</span>
                <span>↑↓ navigate</span>
                {surface === "memory" ? (
                    <>
                        <span>↵ open</span>
                        <span>⌘↵ ask</span>
                    </>
                ) : (
                    <>
                        <span>↵ copy</span>
                        <span>⌘↵ paste</span>
                    </>
                )}
                <span>esc close</span>
            </div>
        </div>
    );
}

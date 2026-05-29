/**
 * HomeHero — cinematic home screen hero with mouse parallax, hero search pill,
 * voice input, and animated scroll indicator.
 *
 * Design spec: docs/superpowers/specs/2026-05-18-hero-parallax-design.md
 *
 * Wires into the existing search flow via `onHeroSearch(query)` callback —
 * no duplicate state. Voice transcription reuses `transcribeVoiceInput` from
 * the shared IPC layer, same as SearchBar.tsx.
 */

import { useEffect, useRef, useState } from "react";
import {
    motion,
    useMotionValue,
    useSpring,
    useTransform,
} from "framer-motion";
import { transcribeVoiceInput } from "@/shared/ipc/tauri";
import { useReducedMotionSafe } from "@/shared/motion/useReducedMotionSafe";
import { VOICE_RECORDING } from "@/shared/utils/config";
import "./HomeHero.css";

// ─── Greeting helpers ─────────────────────────────────────────────────────────

function getGreeting(name: string): { salutation: string; subtitle: string } {
    const h = new Date().getHours();
    const salutation =
        h < 12
            ? `Good Morning, ${name}!`
            : h < 17
              ? `Good Afternoon, ${name}!`
              : h < 21
                ? `Good Evening, ${name}!`
                : `Good Night, ${name}!`;
    const subtitle =
        h < 12
            ? "Let's see what the morning holds."
            : h < 17
              ? "Let's pick up where you left off."
              : h < 21
                ? "Let's revisit your day."
                : "Let's dive into your memories.";
    return { salutation, subtitle };
}

function formatHeroDate(now: Date): string {
    const weekday = now
        .toLocaleDateString("en-US", { weekday: "long" })
        .toUpperCase();
    const month = now
        .toLocaleDateString("en-US", { month: "long" })
        .toUpperCase();
    const day = now.toLocaleDateString("en-US", { day: "numeric" });
    return `${weekday} • ${month} ${day}`;
}

function getTimePlaceholder(): string {
    const h = new Date().getHours();
    if (h < 12) return "What did you work on this morning?";
    if (h < 17) return "What shall we uncover this afternoon?";
    if (h < 21) return "What happened today?";
    return "What shall we uncover tonight?";
}

// ─── Voice hook ───────────────────────────────────────────────────────────────

function chooseRecorderOptions(): MediaRecorderOptions | undefined {
    const candidates = [
        "audio/webm;codecs=opus",
        "audio/mp4",
        "audio/ogg;codecs=opus",
        "audio/webm",
    ];
    for (const mimeType of candidates) {
        if (MediaRecorder.isTypeSupported(mimeType)) {
            return { mimeType, audioBitsPerSecond: VOICE_RECORDING.audioBitsPerSecond };
        }
    }
    return undefined;
}

function stopStream(s: MediaStream | null) {
    s?.getTracks().forEach((t) => t.stop());
}

/** Extracts voice recording + Whisper transcription in a self-contained hook. */
function useHeroVoice(onTranscript: (text: string) => void) {
    const [isRecording, setIsRecording] = useState(false);
    const [isTranscribing, setIsTranscribing] = useState(false);
    const [voiceStatus, setVoiceStatus] = useState<string | null>(null);

    const recorderRef = useRef<MediaRecorder | null>(null);
    const streamRef = useRef<MediaStream | null>(null);
    const chunksRef = useRef<Blob[]>([]);
    const mimeTypeRef = useRef("audio/webm");
    const startedAtRef = useRef(0);

    useEffect(
        () => () => {
            stopStream(streamRef.current);
            streamRef.current = null;
        },
        []
    );

    async function transcribeChunks(chunks: Blob[], mimeType: string) {
        if (chunks.length === 0) {
            setVoiceStatus("No input captured.");
            return;
        }
        setIsTranscribing(true);
        setVoiceStatus("Transcribing…");
        try {
            const blob = new Blob(chunks, { type: mimeType });
            const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
            const result = await transcribeVoiceInput(bytes, mimeType);
            const text = result.text.trim();
            if (text) {
                onTranscript(text);
                setVoiceStatus(null);
            } else {
                setVoiceStatus("Didn't catch that. Try again.");
            }
        } catch {
            setVoiceStatus("Transcription failed.");
        } finally {
            setIsTranscribing(false);
        }
    }

    async function toggle() {
        if (isRecording) {
            recorderRef.current?.stop();
            return;
        }

        if (
            !navigator.mediaDevices?.getUserMedia ||
            typeof MediaRecorder === "undefined"
        ) {
            setVoiceStatus("Microphone not supported.");
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
            const recorder = options
                ? new MediaRecorder(stream, options)
                : new MediaRecorder(stream);

            streamRef.current = stream;
            recorderRef.current = recorder;
            chunksRef.current = [];
            mimeTypeRef.current = recorder.mimeType || options?.mimeType || "audio/webm";
            startedAtRef.current = Date.now();

            recorder.ondataavailable = (e) => {
                if (e.data.size > 0) chunksRef.current.push(e.data);
            };
            recorder.onstop = () => {
                const chunks = [...chunksRef.current];
                chunksRef.current = [];
                const dur = Date.now() - startedAtRef.current;
                stopStream(streamRef.current);
                streamRef.current = null;
                recorderRef.current = null;
                setIsRecording(false);
                if (dur < VOICE_RECORDING.minDurationMs) {
                    setVoiceStatus("Hold the mic a bit longer.");
                    return;
                }
                void transcribeChunks(chunks, mimeTypeRef.current);
            };

            recorder.start(VOICE_RECORDING.timesliceMs);
            setIsRecording(true);
            setVoiceStatus("Listening… tap again to stop.");
        } catch {
            setVoiceStatus("Microphone access failed.");
            stopStream(streamRef.current);
            streamRef.current = null;
            recorderRef.current = null;
            setIsRecording(false);
        }
    }

    return { isRecording, isTranscribing, voiceStatus, toggle };
}

// ─── Component ────────────────────────────────────────────────────────────────

interface HomeHeroProps {
    /** Display name for greeting. Fallback: "there". */
    userName?: string | null;
    /** Current timestamp for date chip (refresh externally to update). */
    now: Date;
    /** Greeting string from the IPC layer (getFunGreeting). */
    greeting?: string;
    /** Called when the user submits a query from the hero search pill. */
    onHeroSearch: (query: string) => void;
}

export function HomeHero({
    userName,
    now,
    greeting,
    onHeroSearch,
}: HomeHeroProps) {
    const { reduced } = useReducedMotionSafe();

    // Greeting logic — prefer the IPC greeting if available.
    const name = userName?.trim() || "there";
    const localGreeting = getGreeting(name);
    const salutation = greeting
        ? (() => {
              // Extract just the "Good *, Name!" part from the IPC greeting if present.
              const excl = greeting.indexOf("!");
              return excl >= 0 ? greeting.slice(0, excl + 1).trim() : greeting.trim();
          })()
        : localGreeting.salutation;
    const subtitle = localGreeting.subtitle;
    const dateLabel = formatHeroDate(now);

    // Search state (local, hands off via onHeroSearch).
    const [draft, setDraft] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);

    // Hero visibility — pause parallax when offscreen.
    const heroRef = useRef<HTMLDivElement>(null);
    const [visible, setVisible] = useState(true);
    useEffect(() => {
        if (!heroRef.current) return;
        const io = new IntersectionObserver(
            ([entry]) => setVisible(entry.isIntersecting),
            { threshold: 0.1 }
        );
        io.observe(heroRef.current);
        return () => io.disconnect();
    }, []);

    // Mouse parallax — normalized -1..1.
    const mx = useMotionValue(0);
    const my = useMotionValue(0);
    const sx = useSpring(mx, { stiffness: 80, damping: 22, mass: 1 });
    const sy = useSpring(my, { stiffness: 80, damping: 22, mass: 1 });

    function onMouseMove(e: React.MouseEvent<HTMLDivElement>) {
        if (reduced || !visible) return;
        const rect = e.currentTarget.getBoundingClientRect();
        mx.set(((e.clientX - rect.left) / rect.width) * 2 - 1);
        my.set(((e.clientY - rect.top) / rect.height) * 2 - 1);
    }

    // Per-layer parallax transforms — each pair is a separate hook call
    // (Rules of Hooks: no hooks inside helper functions).
    const dateX = useTransform(sx, [-1, 1], reduced ? [0, 0] : [-6, 6]);
    const dateY = useTransform(sy, [-1, 1], reduced ? [0, 0] : [-6, 6]);
    const titleX = useTransform(sx, [-1, 1], reduced ? [0, 0] : [-30, 30]);
    const titleY = useTransform(sy, [-1, 1], reduced ? [0, 0] : [-30, 30]);
    const subtitleX = useTransform(sx, [-1, 1], reduced ? [0, 0] : [-16.5, 16.5]);
    const subtitleY = useTransform(sy, [-1, 1], reduced ? [0, 0] : [-16.5, 16.5]);
    const searchX = useTransform(sx, [-1, 1], reduced ? [0, 0] : [-10.5, 10.5]);
    const searchY = useTransform(sy, [-1, 1], reduced ? [0, 0] : [-10.5, 10.5]);
    const scrollX = useTransform(sx, [-1, 1], reduced ? [0, 0] : [-3.9, 3.9]);
    const scrollY = useTransform(sy, [-1, 1], reduced ? [0, 0] : [-3.9, 3.9]);

    // Voice.
    const voice = useHeroVoice((text) => {
        setDraft(text);
    });

    function handleSubmit(value?: string) {
        const q = (value ?? draft).replace(/\r?\n/g, " ").replace(/\s+/g, " ").trim();
        if (q) {
            onHeroSearch(q);
            setDraft("");
        }
    }

    return (
        <div
            ref={heroRef}
            className="home-hero"
            onMouseMove={onMouseMove}
            data-wallpaper-ignore
        >
            {/* Date chip */}
            <motion.p
                className="home-hero__date"
                style={{ x: dateX, y: dateY }}
            >
                {dateLabel}
            </motion.p>

            {/* Greeting */}
            <motion.h1
                className="home-hero__title"
                style={{ x: titleX, y: titleY }}
            >
                {salutation}
            </motion.h1>

            {/* Subtitle */}
            <motion.p
                className="home-hero__subtitle"
                style={{ x: subtitleX, y: subtitleY }}
            >
                {subtitle}
            </motion.p>

            {/* Hero search pill */}
            <motion.div
                className="home-hero__search-wrap"
                style={{ x: searchX, y: searchY }}
            >
                <div
                    className={`home-hero__search-pill${voice.isRecording ? " is-recording" : ""}`}
                    role="search"
                >
                    {/* Search icon */}
                    <svg
                        className="home-hero__search-icon"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        aria-hidden="true"
                    >
                        <circle cx="11" cy="11" r="8" />
                        <path d="M21 21l-4.35-4.35" />
                    </svg>

                    {/* Input */}
                    <input
                        ref={inputRef}
                        type="text"
                        className="home-hero__search-input"
                        placeholder={getTimePlaceholder()}
                        value={draft}
                        onChange={(e) => setDraft(e.target.value)}
                        onKeyDown={(e) => {
                            if (e.key === "Enter") {
                                e.preventDefault();
                                handleSubmit();
                            }
                        }}
                        aria-label="Search your memories"
                    />

                    {/* Voice button */}
                    <button
                        type="button"
                        className={`home-hero__voice-btn${voice.isRecording ? " is-recording" : ""}${voice.isTranscribing ? " is-transcribing" : ""}`}
                        onClick={() => void voice.toggle()}
                        aria-label={
                            voice.isRecording
                                ? "Stop voice recording"
                                : "Start voice recording"
                        }
                        title={voice.isRecording ? "Stop" : "Speak"}
                        disabled={voice.isTranscribing}
                    >
                        {voice.isRecording ? (
                            // Pulsing waveform when recording
                            <span className="home-hero__voice-wave" aria-hidden="true">
                                <span />
                                <span />
                                <span />
                            </span>
                        ) : (
                            <svg
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <rect x="5" y="8" width="2.5" height="10" rx="1.2" />
                                <rect x="10.75" y="5" width="2.5" height="14" rx="1.2" />
                                <rect x="16.5" y="9" width="2.5" height="8" rx="1.2" />
                            </svg>
                        )}
                        <span className="home-hero__voice-label">
                            {voice.isRecording ? "Stop" : "Speak"}
                        </span>
                    </button>

                    {/* Submit arrow */}
                    <button
                        type="button"
                        className="home-hero__search-submit"
                        onClick={() => handleSubmit()}
                        aria-label="Submit search"
                        disabled={!draft.trim()}
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            aria-hidden="true"
                        >
                            <path d="M5 12h14M13 6l6 6-6 6" />
                        </svg>
                    </button>
                </div>

                {/* Voice status */}
                {voice.voiceStatus && (
                    <p className="home-hero__voice-status" role="status" aria-live="polite">
                        {voice.voiceStatus}
                    </p>
                )}
            </motion.div>

            {/* Scroll indicator */}
            <motion.div
                className="home-hero__scroll-indicator"
                style={{ x: scrollX, y: scrollY }}
                aria-hidden="true"
            >
                <span className="home-hero__scroll-label">SCROLL TO EXPLORE</span>
                <div className="home-hero__scroll-line" />
                <div className="home-hero__scroll-dot" />
            </motion.div>
        </div>
    );
}

export default HomeHero;

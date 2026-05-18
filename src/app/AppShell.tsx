import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { STORAGE_KEYS } from "@/shared/utils/config";
import { WorkModeShell } from "./WorkModeShell";
import { ScrollModeShell } from "./ScrollModeShell";

/**
 * Top-level shell that toggles between two app modes:
 *   - "work":      the existing productive layout (sidebar + search + panels)
 *   - "immersive": the cinematic scroll experience
 *
 * Mode persists in localStorage under STORAGE_KEYS.appMode. Toggle with ⌘.
 * (Cmd/Ctrl-Period). Once the immersive shell is populated (Slice 2/3) a
 * visible toggle ships via ChapterRail.
 */

export type AppMode = "work" | "immersive";

const DEFAULT_MODE: AppMode = "work";

interface AppShellContextValue {
    mode: AppMode;
    setMode: (mode: AppMode) => void;
    toggleMode: () => void;
}

const AppShellContext = createContext<AppShellContextValue | null>(null);

export function useAppShell(): AppShellContextValue {
    const ctx = useContext(AppShellContext);
    if (!ctx) {
        throw new Error("useAppShell must be used inside <AppShell>");
    }
    return ctx;
}

function readStoredMode(): AppMode {
    try {
        const raw = localStorage.getItem(STORAGE_KEYS.appMode);
        if (raw === "immersive" || raw === "work") {
            return raw;
        }
    } catch {
        // localStorage unavailable (private mode, sandbox) — fall through.
    }
    return DEFAULT_MODE;
}

export function AppShell() {
    const [mode, setModeState] = useState<AppMode>(() => readStoredMode());

    const setMode = useCallback((next: AppMode) => {
        setModeState(next);
        try {
            localStorage.setItem(STORAGE_KEYS.appMode, next);
        } catch {
            // Ignore — mode still updates in-memory.
        }
    }, []);

    const toggleMode = useCallback(() => {
        setModeState((prev) => {
            const next: AppMode = prev === "work" ? "immersive" : "work";
            try {
                localStorage.setItem(STORAGE_KEYS.appMode, next);
            } catch {
                // Ignore.
            }
            return next;
        });
    }, []);

    // ⌘. / Ctrl+. toggles modes globally. Skip when typing in inputs so the
    // shortcut never steals "stop editing" from a focused field.
    useEffect(() => {
        const handler = (e: KeyboardEvent) => {
            if (!(e.metaKey || e.ctrlKey) || e.key !== ".") return;
            const target = e.target as HTMLElement | null;
            if (target) {
                const tag = target.tagName;
                if (
                    tag === "INPUT" ||
                    tag === "TEXTAREA" ||
                    tag === "SELECT" ||
                    target.isContentEditable
                ) {
                    return;
                }
            }
            e.preventDefault();
            toggleMode();
        };
        window.addEventListener("keydown", handler);
        return () => window.removeEventListener("keydown", handler);
    }, [toggleMode]);

    const value = useMemo<AppShellContextValue>(
        () => ({ mode, setMode, toggleMode }),
        [mode, setMode, toggleMode]
    );

    return (
        <AppShellContext.Provider value={value}>
            {mode === "immersive" ? <ScrollModeShell /> : <WorkModeShell />}
        </AppShellContext.Provider>
    );
}

export default AppShell;

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Subscribe to a Tauri backend event for the lifetime of the component.
 *
 * The handler is kept in a ref so callers can pass inline closures without
 * re-subscribing on every render.
 */
export function useTauriEvent<T>(event: string, handler: (payload: T) => void): void {
    const handlerRef = useRef(handler);
    handlerRef.current = handler;

    useEffect(() => {
        let active = true;
        // Registration fails outside a Tauri runtime (tests, plain browser);
        // callers fall back to their initial fetch in that case.
        const unlistenPromise = listen<T>(event, (e) => {
            if (active) {
                handlerRef.current(e.payload);
            }
        }).catch(() => null);
        return () => {
            active = false;
            void unlistenPromise.then((unlisten) => unlisten?.());
        };
    }, [event]);
}

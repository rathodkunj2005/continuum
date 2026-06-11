import { useEffect } from "react";

export function usePolling(
    callback: (isMounted: () => boolean) => void | Promise<void>,
    intervalMs: number,
    enabled = true
) {
    useEffect(() => {
        if (!enabled) {
            return;
        }

        let mounted = true;

        const isMounted = () => mounted;
        const run = async () => {
            if (!mounted || document.hidden) {
                return;
            }
            await callback(isMounted);
        };

        void run();
        const timer = window.setInterval(() => {
            void run();
        }, intervalMs);
        const onVisibilityChange = () => {
            if (!document.hidden) {
                void run();
            }
        };
        document.addEventListener("visibilitychange", onVisibilityChange);

        return () => {
            mounted = false;
            window.clearInterval(timer);
            document.removeEventListener("visibilitychange", onVisibilityChange);
        };
    }, [callback, enabled, intervalMs]);
}

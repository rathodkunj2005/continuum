import { useEffect, useRef } from "react";
import "./CursorInverter.css";

export function CursorInverter() {
    const ref = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const el = ref.current;
        if (!el) return;

        // Respect prefers-reduced-motion at effect time (more reliable than render time)
        if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

        let raf: number | null = null;
        let latestX = 0;
        let latestY = 0;

        const onMouseMove = (e: MouseEvent) => {
            latestX = e.clientX;
            latestY = e.clientY;
            if (raf !== null) return;
            raf = requestAnimationFrame(() => {
                raf = null;
                el.style.left = `${latestX}px`;
                el.style.top = `${latestY}px`;
            });
        };

        const onMouseEnter = () => el.classList.add("is-visible");
        const onMouseLeave = () => el.classList.remove("is-visible");

        window.addEventListener("mousemove", onMouseMove);
        document.documentElement.addEventListener("mouseenter", onMouseEnter);
        document.documentElement.addEventListener("mouseleave", onMouseLeave);

        return () => {
            window.removeEventListener("mousemove", onMouseMove);
            document.documentElement.removeEventListener("mouseenter", onMouseEnter);
            document.documentElement.removeEventListener("mouseleave", onMouseLeave);
            if (raf !== null) cancelAnimationFrame(raf);
        };
    }, []);

    return <div ref={ref} className="cursor-inverter" aria-hidden="true" />;
}

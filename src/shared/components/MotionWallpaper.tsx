import { useEffect, useRef } from "react";
import { VERT_SRC, lerp3, readCssAurora } from "@/shared/wallpaper/shader-common";
import { SHADER_PRESETS, SHADER_SOURCES } from "@/shared/wallpaper/shader-sources";
import { DEFAULT_WALLPAPER, type WallpaperId } from "@/shared/wallpaper/wallpaper-registry";

export interface MotionWallpaperProps {
    className?: string;
    wallpaperId?: WallpaperId;
    auroraBg?: [number, number, number];
    aurMid?: [number, number, number];
    aurAcc?: [number, number, number];
}

export function MotionWallpaper({
    className = "",
    wallpaperId = DEFAULT_WALLPAPER,
    auroraBg,
    aurMid,
    aurAcc,
}: MotionWallpaperProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const colorsRef = useRef({ bg: auroraBg, mid: aurMid, acc: aurAcc });
    const wallpaperRef = useRef(wallpaperId);

    useEffect(() => {
        colorsRef.current = { bg: auroraBg, mid: aurMid, acc: aurAcc };
    }, [auroraBg, aurMid, aurAcc]);

    useEffect(() => {
        wallpaperRef.current = wallpaperId;
    }, [wallpaperId]);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        const gl = canvas.getContext("webgl", {
            antialias: false,
            powerPreference: "high-performance",
            premultipliedAlpha: false,
        });
        if (!gl) return;

        const mkShader = (type: number, src: string) => {
            const sh = gl.createShader(type);
            if (!sh) return null;
            gl.shaderSource(sh, src);
            gl.compileShader(sh);
            if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
                console.error("MotionWallpaper:", gl.getShaderInfoLog(sh));
                gl.deleteShader(sh);
                return null;
            }
            return sh;
        };

        const compileProgram = (fragSrc: string) => {
            const vs = mkShader(gl.VERTEX_SHADER, VERT_SRC);
            const fs = mkShader(gl.FRAGMENT_SHADER, fragSrc);
            if (!vs || !fs) return null;
            const prog = gl.createProgram();
            if (!prog) return null;
            gl.attachShader(prog, vs);
            gl.attachShader(prog, fs);
            gl.linkProgram(prog);
            if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
                console.error("MotionWallpaper:", gl.getProgramInfoLog(prog));
                gl.deleteShader(vs);
                gl.deleteShader(fs);
                return null;
            }
            gl.deleteShader(vs);
            gl.deleteShader(fs);
            return prog;
        };

        const programs = new Map<WallpaperId, WebGLProgram>();
        for (const id of Object.keys(SHADER_SOURCES) as WallpaperId[]) {
            const prog = compileProgram(SHADER_SOURCES[id]);
            if (prog) programs.set(id, prog);
        }
        if (programs.size === 0) return;

        const vb = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, vb);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

        type UniformSet = ReturnType<typeof bindUniforms>;

        const bindUniforms = (prog: WebGLProgram) => {
            gl.useProgram(prog);
            const aP = gl.getAttribLocation(prog, "aP");
            gl.enableVertexAttribArray(aP);
            gl.vertexAttribPointer(aP, 2, gl.FLOAT, false, 0, 0);
            const loc = (n: string) => gl.getUniformLocation(prog, n);
            return {
                uT: loc("uT"),
                uIntens: loc("uIntens"),
                uTurb: loc("uTurb"),
                uHt: loc("uHt"),
                uHflow: loc("uHflow"),
                uBMult: loc("uBMult"),
                uBeam: loc("uBeam"),
                uStars: loc("uStars"),
                uCN: loc("uCN"),
                uR: loc("uR"),
                uM: loc("uM"),
                uBg: loc("uBg"),
                uMid: loc("uMid"),
                uAcc: loc("uAcc"),
                uCP: Array.from({ length: 8 }, (_, i) => loc(`uCP[${i}]`)),
                uCT: Array.from({ length: 8 }, (_, i) => loc(`uCT[${i}]`)),
            };
        };

        const uniformCache = new Map<WebGLProgram, UniformSet>();

        const resize = () => {
            const dpr = Math.min(window.devicePixelRatio || 1, 1.8);
            canvas.width = Math.round(window.innerWidth * dpr);
            canvas.height = Math.round(window.innerHeight * dpr);
            canvas.style.width = "100%";
            canvas.style.height = "100%";
            gl.viewport(0, 0, canvas.width, canvas.height);
        };
        resize();
        window.addEventListener("resize", resize);

        let mouse: [number, number] = [0.5, 0.5];
        let smoothMouse: [number, number] = [0.5, 0.5];
        let clicks: { p: [number, number]; t: number }[] = [];
        let baseOffset = 0;
        let smoothPalette = readCssAurora();
        let lastCssRead = -1000;
        let activeId: WallpaperId = wallpaperRef.current;

        const setPointer = (x: number, y: number) => {
            mouse = [x / window.innerWidth, 1 - y / window.innerHeight];
        };
        const pushRipple = (x: number, y: number) => {
            const preset = SHADER_PRESETS[wallpaperRef.current];
            clicks.push({
                p: [x / window.innerWidth, 1 - y / window.innerHeight],
                t: (performance.now() / 1000 - baseOffset) * preset.speed,
            });
            if (clicks.length > 8) clicks.shift();
        };
        const shouldIgnoreTarget = (el: Element | null) =>
            el?.closest?.("[data-wallpaper-ignore], [data-aurora-ignore], button, a, input, textarea, [role='button']");

        const onMove = (e: MouseEvent) => setPointer(e.clientX, e.clientY);
        const onClick = (e: MouseEvent) => {
            if (shouldIgnoreTarget(e.target as Element | null)) return;
            pushRipple(e.clientX, e.clientY);
        };
        const onTouchMove = (e: TouchEvent) => {
            const t = e.touches[0];
            if (t) setPointer(t.clientX, t.clientY);
        };
        const onTouchEnd = (e: TouchEvent) => {
            const t = e.changedTouches[0];
            if (!t) return;
            const el = document.elementFromPoint(t.clientX, t.clientY);
            if (shouldIgnoreTarget(el)) return;
            pushRipple(t.clientX, t.clientY);
        };
        window.addEventListener("mousemove", onMove);
        window.addEventListener("click", onClick);
        window.addEventListener("touchmove", onTouchMove, { passive: true });
        window.addEventListener("touchend", onTouchEnd);

        let raf = 0;
        let running = true;
        const tick = (ts: number) => {
            if (!running) return;
            raf = requestAnimationFrame(tick);

            const nextId = wallpaperRef.current;
            let wallpaperChanged = false;
            if (nextId !== activeId) {
                wallpaperChanged = true;
                activeId = nextId;
                baseOffset = ts / 1000;
                clicks = [];
            }

            const prog = programs.get(activeId) ?? programs.get(DEFAULT_WALLPAPER);
            if (!prog) return;

            let U = uniformCache.get(prog);
            if (!U) {
                U = bindUniforms(prog);
                uniformCache.set(prog, U);
            } else {
                gl.useProgram(prog);
            }

            if (ts - lastCssRead > 400) {
                lastCssRead = ts;
            }
            const css = readCssAurora();
            const target = {
                bg: colorsRef.current.bg ?? css.bg,
                mid: colorsRef.current.mid ?? css.mid,
                acc: colorsRef.current.acc ?? css.acc,
            };
            const colorDelta = Math.hypot(
                target.bg[0] - smoothPalette.bg[0],
                target.bg[1] - smoothPalette.bg[1],
                target.bg[2] - smoothPalette.bg[2]
            );
            const paletteStep = wallpaperChanged || colorDelta > 0.07 ? 0.2 : 0.065;
            smoothPalette = {
                bg: lerp3(smoothPalette.bg, target.bg, paletteStep),
                mid: lerp3(smoothPalette.mid, target.mid, paletteStep),
                acc: lerp3(smoothPalette.acc, target.acc, paletteStep),
            };
            gl.uniform3fv(U.uBg, smoothPalette.bg);
            gl.uniform3fv(U.uMid, smoothPalette.mid);
            gl.uniform3fv(U.uAcc, smoothPalette.acc);

            const preset = SHADER_PRESETS[activeId];
            const et = (performance.now() / 1000 - baseOffset) * preset.speed;
            smoothMouse[0] += (mouse[0] - smoothMouse[0]) * 0.044;
            smoothMouse[1] += (mouse[1] - smoothMouse[1]) * 0.044;
            clicks = clicks.filter((c) => et - c.t < 2.8);

            gl.uniform2f(U.uR, canvas.width, canvas.height);
            gl.uniform1f(U.uT, et);
            gl.uniform1f(U.uIntens, preset.intens);
            gl.uniform1f(U.uTurb, preset.turb);
            gl.uniform1f(U.uHt, preset.ht);
            gl.uniform1f(U.uHflow, preset.hflow);
            gl.uniform1f(U.uBMult, preset.bMult);
            gl.uniform1f(U.uBeam, preset.beam);
            gl.uniform1f(U.uStars, preset.stars);
            gl.uniform2fv(U.uM, smoothMouse);
            gl.uniform1i(U.uCN, clicks.length);
            for (let i = 0; i < 8; i++) {
                const c = clicks[i];
                gl.uniform2fv(U.uCP[i], c ? c.p : [0, 0]);
                gl.uniform1f(U.uCT[i], c ? c.t : -999);
            }
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        };

        const onVis = () => {
            if (document.hidden) {
                running = false;
                cancelAnimationFrame(raf);
            } else if (!running) {
                running = true;
                raf = requestAnimationFrame(tick);
            }
        };
        document.addEventListener("visibilitychange", onVis);
        raf = requestAnimationFrame(tick);

        return () => {
            running = false;
            cancelAnimationFrame(raf);
            window.removeEventListener("resize", resize);
            window.removeEventListener("mousemove", onMove);
            window.removeEventListener("click", onClick);
            window.removeEventListener("touchmove", onTouchMove);
            window.removeEventListener("touchend", onTouchEnd);
            document.removeEventListener("visibilitychange", onVis);
            gl.deleteBuffer(vb);
            for (const prog of programs.values()) gl.deleteProgram(prog);
            // Do not call WEBGL_lose_context — React StrictMode remounts and a lost
            // context leaves a permanently black canvas in dev.
        };
    }, []);

    return <canvas ref={canvasRef} className={className} aria-hidden="true" />;
}

export default MotionWallpaper;

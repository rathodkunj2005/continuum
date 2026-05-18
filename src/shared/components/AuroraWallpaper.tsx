import { useEffect, useRef } from "react";

/**
 * AuroraWallpaper — full-viewport WebGL aurora that morphs between
 * eight per-page presets (home / frames / timeline / graph / search /
 * smart / pinned / darkroom).
 *
 * Port of the `FNDR Page Wallpapers.html` reference design. The
 * fragment shader uses FBM-warped curtains (`hflow=0` vertical, `1`
 * horizontal bands), a beam that converges toward the cursor column,
 * a star field, and click ripples. Page params lerp smoothly (lk=.028)
 * on `page` prop change.
 *
 * Performance notes:
 *  - Single fullscreen quad; ~one tight rAF loop
 *  - DPR capped at 1.8 to bound fragment cost
 *  - Pauses (skips rAF dispatch) when document.hidden
 */

export type AuroraPageId =
    | "home"
    | "frames"
    | "timeline"
    | "graph"
    | "search"
    | "smart"
    | "pinned"
    | "darkroom";

export type AuroraTheme = "film" | "paper";

interface AuroraPagePreset {
    /** Turbulence — 0=still → 2=stormy */
    turb: number;
    /** Animation speed multiplier */
    speed: number;
    /** Aurora brightness */
    intens: number;
    /** Curtain height — 0=bottom → 1=top */
    ht: number;
    /** 0=vertical curtains (aurora), 1=horizontal bands (timeline feel) */
    hflow: number;
    /** Frequency / band-count multiplier */
    bMult: number;
    /** Cursor-column beam strength */
    beam: number;
    /** Star field density */
    stars: number;
}

const PAGES: Record<AuroraPageId, AuroraPagePreset> = {
    home: { turb: 1.0, speed: 0.8, intens: 1.0, ht: 0.62, hflow: 0.0, bMult: 1.0, beam: 0.0, stars: 0.55 },
    frames: { turb: 0.55, speed: 0.42, intens: 0.75, ht: 0.55, hflow: 0.0, bMult: 0.8, beam: 0.0, stars: 1.6 },
    timeline: { turb: 0.45, speed: 0.32, intens: 0.88, ht: 0.5, hflow: 0.92, bMult: 2.1, beam: 0.0, stars: 0.8 },
    graph: { turb: 1.6, speed: 1.3, intens: 1.1, ht: 0.6, hflow: 0.28, bMult: 1.7, beam: 0.42, stars: 0.22 },
    search: { turb: 1.9, speed: 1.5, intens: 1.2, ht: 0.68, hflow: 0.08, bMult: 0.68, beam: 1.9, stars: 0.3 },
    smart: { turb: 2.1, speed: 1.9, intens: 1.3, ht: 0.65, hflow: 0.22, bMult: 1.4, beam: 0.95, stars: 0.1 },
    pinned: { turb: 0.75, speed: 0.52, intens: 1.0, ht: 0.58, hflow: 0.1, bMult: 1.1, beam: 0.18, stars: 1.1 },
    darkroom: { turb: 0.14, speed: 0.16, intens: 0.48, ht: 0.42, hflow: 0.0, bMult: 0.55, beam: 0.0, stars: 0.65 },
};

interface AuroraPalette {
    bg: [number, number, number];
    mid: [number, number, number];
    acc: [number, number, number];
}

const PAL: Record<AuroraTheme, AuroraPalette> = {
    film: { bg: [0.102, 0.078, 0.063], mid: [0.769, 0.659, 0.471], acc: [0.831, 0.627, 0.29] },
    paper: { bg: [0.949, 0.918, 0.847], mid: [0.478, 0.416, 0.322], acc: [0.639, 0.353, 0.118] },
};

const VERT_SRC = `attribute vec2 aP;void main(){gl_Position=vec4(aP,0.,1.);}`;

const FRAG_SRC = `
precision highp float;

uniform float uT, uIntens, uTurb, uHt, uHflow, uBMult, uBeam, uStars;
uniform vec2  uR, uM;
uniform vec3  uBg, uMid, uAcc;
uniform vec2  uCP[8];
uniform float uCT[8];
uniform int   uCN;

float hash(vec2 p){p=fract(p*vec2(234.34,435.345));p+=dot(p,p+34.23);return fract(p.x*p.y);}
float noise(vec2 p){
  vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);
  return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),
             mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);
}
float fbm(vec2 p){
  float v=0.,a=.5;
  for(int i=0;i<6;i++){v+=a*noise(p);p=p*2.1+vec2(1.7,9.2);a*=.5;}
  return v;
}
float wfbm(vec2 p){
  vec2 q=vec2(fbm(p),fbm(p+vec2(5.2,1.3)));
  return fbm(p+1.5*uTurb*q+.08*uT);
}

vec2 ripples(vec2 uv){
  vec2 off=vec2(0.);
  for(int i=0;i<8;i++){
    if(i>=uCN) break;
    float age=uT-uCT[i];
    if(age<0.||age>2.8) continue;
    vec2 d=uv-uCP[i]; float r=length(d);
    float w=sin(r*20.-age*7.)*exp(-age*1.5)*exp(-r*6.)*.055;
    off+=normalize(d+.001)*w;
  }
  return off;
}

float star(vec2 uv,float sc,float sd){
  vec2 g=floor(uv*sc+sd*91.); float r=hash(g+sd);
  if(r<.963) return 0.;
  vec2 f=fract(uv*sc+sd*91.)-.5;
  return smoothstep(.22,0.,length(f))*(.40+.60*sin(uT*(2.+r*6.)+r*6.28))*(r-.963)/.037;
}

float band(vec2 uv, float yOff, float xs, float ys, float w){
  float hx  = mix(uv.x, uv.y, uHflow);
  float hy  = mix(uv.y, uv.x, uHflow);
  float bm  = uBMult;
  float wx  = hx + fbm(vec2(hx*.4*bm+uT*xs, uT*ys))*(.42*uTurb);
  float ctr = uHt + yOff + sin(wx*1.7*bm+uT*.2)*.07
            + fbm(vec2(wx*.75*bm+uT*.06, uT*.03))*(.19*uTurb);
  float stripe = wfbm(vec2(wx*2.3*bm+uT*xs*.5, uT*ys*.3))*.55+.45;
  float dist = hy - ctr;
  float fall = exp(-max(dist,0.)/w*2.2)*exp(min(dist,0.)/w*.55);
  return clamp(fall*stripe, 0., 1.);
}

float halo(vec2 uv, float yOff, float xs, float w){
  float hx = mix(uv.x,uv.y,uHflow);
  float hy = mix(uv.y,uv.x,uHflow);
  float bm = uBMult;
  float wx = hx + fbm(vec2(hx*.3*bm+uT*xs, uT*.07))*(.3*uTurb);
  float ctr= uHt+.08+yOff+fbm(vec2(wx*.5*bm+uT*.04, uT*.025))*(.14*uTurb);
  return clamp(exp(-abs(hy-ctr)/w), 0., 1.);
}

void main(){
  vec2 uv = gl_FragCoord.xy / uR;

  vec2 d2m = uM - uv; float dm = length(d2m);
  vec2 wuv = uv + d2m*.08*exp(-dm*2.5) + ripples(uv);

  float sv = star(uv,95.,.0)+star(uv,160.,1.4)*.5+star(uv,240.,3.1)*.28;
  float s  = clamp(sv*uStars*.72, 0., 1.);

  float b1 = band(wuv, .00, .20, .13, .10);
  float b3 = band(wuv,-.04, .14, .09, .13)*.52;
  float b5 = halo(wuv, .00, .11, .22)*.28;
  float body = clamp(b1+b3+b5, 0., 1.) * uIntens;

  float b2 = band(wuv, .07, .26, .11, .055)*.78;
  float b4 = band(wuv, .13, .33, .15, .038)*.48;
  float tips = clamp(b2+b4, 0., 1.) * uIntens;

  float colDist = abs(wuv.x - uM.x);
  float rowDist = abs(wuv.y - uM.y);
  float beam = exp(-colDist*6.5)*exp(-rowDist*1.2)*uBeam*.9;

  vec3 col = uBg;
  col = mix(col, uMid, smoothstep(0.,.52, body + beam*.3)*.94);
  col = mix(col, uAcc, smoothstep(.15,.82, tips + beam*.6)*.88);

  col = mix(col, mix(uMid*.65, uAcc,.5), s);

  float lum = dot(col,vec3(.299,.587,.114));
  col += uAcc*smoothstep(.52,1.,lum)*.20;

  float hy = mix(uv.y, uv.x, uHflow);
  float rim = smoothstep(.18,.0, hy)*uIntens*.32;
  col += uMid*rim*.28;

  col *= 1.-dot(uv-.5,uv-.5)*1.12;

  col = pow(max(col,0.),vec3(.4545));
  gl_FragColor = vec4(col,1.);
}`;

interface AuroraWallpaperProps {
    page: AuroraPageId;
    theme?: AuroraTheme;
    className?: string;
}

export function AuroraWallpaper({ page, theme = "film", className }: AuroraWallpaperProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    // Latest target page is read from a ref so the rAF loop can lerp toward it
    // without restarting the entire render setup on every page change.
    const targetRef = useRef<AuroraPagePreset>(PAGES[page]);
    const themeRef = useRef<AuroraTheme>(theme);

    useEffect(() => {
        targetRef.current = PAGES[page];
    }, [page]);

    useEffect(() => {
        themeRef.current = theme;
    }, [theme]);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        const gl = canvas.getContext("webgl", {
            antialias: false,
            powerPreference: "high-performance",
            premultipliedAlpha: false,
        });
        if (!gl) {
            // WebGL unavailable — leave the canvas blank; the static page-bg
            // gradient still shows beneath it.
            return;
        }

        // --- Compile + link shaders ---
        const mkShader = (type: number, src: string): WebGLShader | null => {
            const sh = gl.createShader(type);
            if (!sh) return null;
            gl.shaderSource(sh, src);
            gl.compileShader(sh);
            if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
                console.error("AuroraWallpaper shader compile failed:", gl.getShaderInfoLog(sh));
                gl.deleteShader(sh);
                return null;
            }
            return sh;
        };

        const vs = mkShader(gl.VERTEX_SHADER, VERT_SRC);
        const fs = mkShader(gl.FRAGMENT_SHADER, FRAG_SRC);
        if (!vs || !fs) return;

        const prog = gl.createProgram();
        if (!prog) return;
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
            console.error("AuroraWallpaper link failed:", gl.getProgramInfoLog(prog));
            return;
        }
        gl.useProgram(prog);

        // --- Fullscreen quad ---
        const vb = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, vb);
        gl.bufferData(
            gl.ARRAY_BUFFER,
            new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
            gl.STATIC_DRAW
        );
        const aP = gl.getAttribLocation(prog, "aP");
        gl.enableVertexAttribArray(aP);
        gl.vertexAttribPointer(aP, 2, gl.FLOAT, false, 0, 0);

        // --- Uniform locations ---
        const loc = (n: string) => gl.getUniformLocation(prog, n);
        const U = {
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
            uCP: new Array(8).fill(null).map((_, i) => loc(`uCP[${i}]`)),
            uCT: new Array(8).fill(null).map((_, i) => loc(`uCT[${i}]`)),
        };

        // --- Resize ---
        let widthCss = 0;
        let heightCss = 0;
        const resize = () => {
            const dpr = Math.min(window.devicePixelRatio || 1, 1.8);
            widthCss = window.innerWidth;
            heightCss = window.innerHeight;
            canvas.width = Math.round(widthCss * dpr);
            canvas.height = Math.round(heightCss * dpr);
            canvas.style.width = `${widthCss}px`;
            canvas.style.height = `${heightCss}px`;
            gl.viewport(0, 0, canvas.width, canvas.height);
            gl.uniform2f(U.uR, canvas.width, canvas.height);
        };
        resize();
        window.addEventListener("resize", resize);

        // --- State ---
        let cur = { ...PAGES[page] };
        let mouse: [number, number] = [0.5, 0.5];
        let smoothMouse: [number, number] = [0.5, 0.5];
        type Click = { p: [number, number]; t: number };
        let clicks: Click[] = [];
        let baseOffset = 0;

        // --- Input ---
        const onMove = (e: MouseEvent) => {
            mouse = [e.clientX / window.innerWidth, 1 - e.clientY / window.innerHeight];
        };
        const onClick = (e: MouseEvent) => {
            // Skip clicks on UI chrome — anything inside [data-aurora-ignore],
            // a button, or an interactive control. We still let the aurora
            // ripple on background clicks.
            const t = e.target as Element | null;
            if (t?.closest?.("[data-aurora-ignore], button, a, input, textarea, [role='button']")) {
                return;
            }
            const et = (performance.now() / 1000 - baseOffset) * cur.speed;
            clicks.push({
                p: [e.clientX / window.innerWidth, 1 - e.clientY / window.innerHeight],
                t: et,
            });
            if (clicks.length > 8) clicks.shift();
        };
        window.addEventListener("mousemove", onMove);
        window.addEventListener("click", onClick);

        // --- Render loop ---
        let raf = 0;
        let running = true;
        const lerp = (a: number, b: number, k: number) => a + (b - a) * k;

        const tick = (ts: number) => {
            if (!running) return;
            raf = requestAnimationFrame(tick);

            // Apply palette every frame so theme prop changes are picked up.
            const p = PAL[themeRef.current];
            gl.uniform3fv(U.uBg, p.bg);
            gl.uniform3fv(U.uMid, p.mid);
            gl.uniform3fv(U.uAcc, p.acc);

            const rawT = ts / 1000 - baseOffset;
            const t = rawT;

            const tgt = targetRef.current;
            const lk = 0.028;
            cur.turb = lerp(cur.turb, tgt.turb, lk);
            cur.speed = lerp(cur.speed, tgt.speed, lk);
            cur.intens = lerp(cur.intens, tgt.intens, lk);
            cur.ht = lerp(cur.ht, tgt.ht, lk);
            cur.hflow = lerp(cur.hflow, tgt.hflow, lk);
            cur.bMult = lerp(cur.bMult, tgt.bMult, lk);
            cur.beam = lerp(cur.beam, tgt.beam, lk);
            cur.stars = lerp(cur.stars, tgt.stars, lk);

            smoothMouse[0] += (mouse[0] - smoothMouse[0]) * 0.044;
            smoothMouse[1] += (mouse[1] - smoothMouse[1]) * 0.044;

            const et = t * cur.speed;
            clicks = clicks.filter((c) => et - c.t < 2.8);

            gl.uniform1f(U.uT, et);
            gl.uniform1f(U.uIntens, cur.intens);
            gl.uniform1f(U.uTurb, cur.turb);
            gl.uniform1f(U.uHt, cur.ht);
            gl.uniform1f(U.uHflow, cur.hflow);
            gl.uniform1f(U.uBMult, cur.bMult);
            gl.uniform1f(U.uBeam, cur.beam);
            gl.uniform1f(U.uStars, cur.stars);
            gl.uniform2fv(U.uM, smoothMouse);
            gl.uniform1i(U.uCN, clicks.length);
            for (let i = 0; i < 8; i++) {
                const c = clicks[i];
                gl.uniform2fv(U.uCP[i], c ? c.p : [0, 0]);
                gl.uniform1f(U.uCT[i], c ? c.t : -999);
            }
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        };

        // Pause/resume on tab visibility — saves battery on a desktop app.
        const onVis = () => {
            if (document.hidden) {
                running = false;
                if (raf) cancelAnimationFrame(raf);
            } else if (!running) {
                running = true;
                baseOffset = performance.now() / 1000 - (performance.now() / 1000 - baseOffset);
                raf = requestAnimationFrame(tick);
            }
        };
        document.addEventListener("visibilitychange", onVis);

        raf = requestAnimationFrame(tick);

        return () => {
            running = false;
            if (raf) cancelAnimationFrame(raf);
            window.removeEventListener("resize", resize);
            window.removeEventListener("mousemove", onMove);
            window.removeEventListener("click", onClick);
            document.removeEventListener("visibilitychange", onVis);

            gl.deleteBuffer(vb);
            gl.deleteProgram(prog);
            gl.deleteShader(vs);
            gl.deleteShader(fs);
            const lose = gl.getExtension("WEBGL_lose_context");
            lose?.loseContext();
        };
        // Only initialise once per mount — `page`/`theme` updates are
        // forwarded via refs above.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    return (
        <canvas
            ref={canvasRef}
            className={className}
            aria-hidden="true"
            style={{
                position: "fixed",
                inset: 0,
                width: "100%",
                height: "100%",
                zIndex: 0,
                pointerEvents: "none",
                display: "block",
            }}
        />
    );
}

export default AuroraWallpaper;

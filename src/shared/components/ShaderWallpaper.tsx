import { useEffect, useRef, useCallback } from "react";

export type ShaderWallpaperId =
    | "amber-halation"
    | "void-lattice"
    | "aurora-reel"
    | "neural-pulse"
    | "chromatic-leak";

interface ShaderWallpaperProps {
    shader: ShaderWallpaperId;
    className?: string;
    style?: React.CSSProperties;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/** Parse a CSS colour value (hex or rgb/rgba) into a [0..1] RGB triple. */
function parseCSSColor(val: string): [number, number, number] {
    val = val.trim();
    if (val.startsWith("#")) {
        const h = val.slice(1).padEnd(6, "0");
        return [
            parseInt(h.slice(0, 2), 16) / 255,
            parseInt(h.slice(2, 4), 16) / 255,
            parseInt(h.slice(4, 6), 16) / 255,
        ];
    }
    const m = val.match(/rgba?\s*\(([^)]+)\)/);
    if (m) {
        const [r, g, b] = m[1].split(",").map((s) => parseFloat(s.trim()));
        return [r / 255, g / 255, b / 255];
    }
    return [0.83, 0.627, 0.29]; // amber fallback
}

function readPaletteColors(el: Element): { accent: [number, number, number]; bg: [number, number, number] } {
    const cs = getComputedStyle(el);
    // --cp-accent-raw is the mode-independent vivid swatch — always bright regardless of light/dark theme.
    const accentRaw = cs.getPropertyValue("--cp-accent-raw").trim()
        || cs.getPropertyValue("--cp-accent").trim()
        || "#d4a04a";
    const accent = parseCSSColor(accentRaw);
    // Derive a guaranteed-dark bg from the accent so the shader always renders as a dark wallpaper,
    // even when the active palette is a light theme (where --cp-bg would be near-white).
    const bg: [number, number, number] = [accent[0] * 0.055, accent[1] * 0.055, accent[2] * 0.055];
    return { accent, bg };
}

// ─── GLSL shared vertex ───────────────────────────────────────────────────────
const VERT_SRC = `
attribute vec2 a_pos;
void main() { gl_Position = vec4(a_pos, 0.0, 1.0); }
`;

// Shared uniforms across all shaders:
//   u_res      – canvas size in px
//   u_mouse    – mouse position in px
//   u_time     – seconds elapsed
//   u_click    – last click in UV [0..1]
//   u_clickAge – seconds since last click
//   u_accent   – palette accent colour (vec3, 0..1)
//   u_bg       – palette background colour (vec3, 0..1)

// ─── Fragment shaders ────────────────────────────────────────────────────────
const SHADERS: Record<ShaderWallpaperId, string> = {

    // 1. Amber Halation — film grain bloom that follows mouse
    "amber-halation": `
precision mediump float;
uniform vec2  u_res;
uniform vec2  u_mouse;
uniform float u_time;
uniform vec2  u_click;
uniform float u_clickAge;
uniform vec3  u_accent;
uniform vec3  u_bg;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
    vec2 i = floor(p); vec2 f = fract(p); vec2 u = f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1,0)),u.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x),u.y);
}

void main() {
    vec2 uv = gl_FragCoord.xy / u_res;
    vec2 m  = u_mouse / u_res; m.y = 1.0 - m.y;

    float d = length(uv - vec2(0.5));
    float bg = 0.04 + 0.02 * noise(uv * 4.0 + u_time * 0.08);
    float bloom = 0.20 * exp(-7.0 * length(uv - m));

    // Click ripple
    float ripple = 0.0;
    if (u_clickAge < 1.6) {
        float wave = u_clickAge * 0.55;
        float ring = exp(-25.0 * pow(length(uv - u_click) - wave, 2.0));
        ripple = ring * (1.0 - u_clickAge / 1.6) * 0.28;
    }

    float grain = (hash(uv + fract(u_time)) - 0.5) * 0.045;
    float luma = bg + bloom + ripple + grain;

    vec3 col = mix(u_bg * 0.8, u_accent * luma * 1.3, smoothstep(0.0, 0.4, luma));
    col = mix(col, u_bg * 0.6, d * 0.7);
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}`,

    // 2. Void Lattice — dark grid distorting around the cursor
    "void-lattice": `
precision mediump float;
uniform vec2  u_res;
uniform vec2  u_mouse;
uniform float u_time;
uniform vec2  u_click;
uniform float u_clickAge;
uniform vec3  u_accent;
uniform vec3  u_bg;

void main() {
    vec2 uv = gl_FragCoord.xy / u_res;
    vec2 m  = u_mouse / u_res; m.y = 1.0 - m.y;
    vec2 ck = vec2(u_click.x, 1.0 - u_click.y);

    vec2 dir = uv - m;
    float dist = length(dir);
    vec2 warped = uv + normalize(dir + 0.001) * (0.06 / (dist * dist + 0.08)) * 0.04;

    if (u_clickAge < 1.2) {
        float r = u_clickAge * 0.7;
        float ring = exp(-30.0 * pow(length(uv - ck) - r, 2.0));
        warped += normalize(uv - ck + 0.001) * ring * 0.025 * (1.0 - u_clickAge / 1.2);
    }

    vec2 grid = fract(warped * 18.0 + u_time * 0.04);
    float line = min(min(grid.x, 1.0 - grid.x), min(grid.y, 1.0 - grid.y));
    float cell = smoothstep(0.04, 0.07, line);
    float glow = exp(-10.0 * dist);

    vec3 lineCol = mix(u_accent * 0.35, u_accent * 0.7, glow);
    vec3 col = mix(lineCol, u_bg * 0.85, cell);
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}`,

    // 3. Aurora Reel — vertical aurora waves tied to mouse X
    "aurora-reel": `
precision mediump float;
uniform vec2  u_res;
uniform vec2  u_mouse;
uniform float u_time;
uniform vec2  u_click;
uniform float u_clickAge;
uniform vec3  u_accent;
uniform vec3  u_bg;

float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){
    vec2 i=floor(p);vec2 f=fract(p);vec2 u=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1,0)),u.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x),u.y);
}
float fbm(vec2 p){float v=0.0,a=0.5;for(int i=0;i<4;i++){v+=a*noise(p);p*=2.1;a*=0.5;}return v;}

void main(){
    vec2 uv = gl_FragCoord.xy / u_res;
    vec2 m  = u_mouse / u_res; m.y = 1.0 - m.y;
    float t = u_time * 0.18;
    float shift = m.x * 0.4;
    vec2 q = vec2(fbm(uv + shift + t), fbm(uv + vec2(1.3, shift) + t * 0.7));
    float f = fbm(uv * 1.5 + q + vec2(shift, 0.0));

    float flash = 0.0;
    if (u_clickAge < 0.8) {
        flash = exp(-5.0 * length(uv - u_click)) * (1.0 - u_clickAge / 0.8) * 0.45;
    }

    float luma = f * 0.7 + 0.04 + flash;
    vec3 col = mix(u_bg * 0.9, u_accent, smoothstep(0.18, 0.7, luma));
    col = mix(col, u_accent * 1.2, smoothstep(0.55, 0.9, luma));
    col *= 0.45 + 0.55 * sin(uv.y * 3.14159);
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}`,

    // 4. Neural Pulse — node network that brightens toward the cursor
    "neural-pulse": `
precision mediump float;
uniform vec2  u_res;
uniform vec2  u_mouse;
uniform float u_time;
uniform vec2  u_click;
uniform float u_clickAge;
uniform vec3  u_accent;
uniform vec3  u_bg;

float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
vec2 hashV(vec2 p){return vec2(hash(p),hash(p+vec2(3.7,1.9)));}

void main(){
    vec2 uv = gl_FragCoord.xy / u_res; uv.y = 1.0 - uv.y;
    vec2 m  = u_mouse / u_res; m.y = 1.0 - m.y;
    vec2 ck = vec2(u_click.x, 1.0 - u_click.y);
    float t = u_time * 0.3;

    vec3 col = u_bg * 0.85;
    for (int i = 0; i < 12; i++) {
        vec2 seed = vec2(float(i) * 1.3 + 0.7, float(i) * 0.97 + 0.3);
        vec2 node = hashV(seed) * 0.85 + 0.075;
        node += sin(t + seed) * 0.035;
        float d = length(uv - node);
        float pulse = 0.5 + 0.5 * sin(t * 2.0 + float(i) * 1.4);
        col += u_accent * exp(-18.0 * d * d) * pulse * 0.55;

        // Edge toward mouse
        vec2 toM = m - node;
        float eFade = exp(-6.0 * length(toM));
        float eL = exp(-80.0 * pow(dot(normalize(toM + 0.001), normalize(uv - node + 0.001)) - 1.0, 2.0) * d * d);
        col += u_accent * eL * eFade * (1.0 - d) * 0.4;
    }

    if (u_clickAge < 1.0) {
        col += u_accent * 1.1 * exp(-20.0 * length(uv - ck)) * (1.0 - u_clickAge);
    }
    col += u_accent * 0.15 * exp(-10.0 * length(uv - m));
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}`,

    // 5. Chromatic Leak — prismatic light leaks drifting with the mouse
    "chromatic-leak": `
precision mediump float;
uniform vec2  u_res;
uniform vec2  u_mouse;
uniform float u_time;
uniform vec2  u_click;
uniform float u_clickAge;
uniform vec3  u_accent;
uniform vec3  u_bg;

float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){
    vec2 i=floor(p);vec2 f=fract(p);vec2 u=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1,0)),u.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x),u.y);
}

vec3 leak(vec2 uv, vec2 src, vec3 tint, float t){
    vec2 d = uv - src;
    float angle = atan(d.y, d.x);
    float band = smoothstep(0.0, 0.38, noise(vec2(angle * 3.0 + t, length(d) * 4.0)));
    return tint * band * exp(-5.0 * length(d)) * 0.32;
}

void main(){
    vec2 uv = gl_FragCoord.xy / u_res; uv.y = 1.0 - uv.y;
    vec2 m  = u_mouse / u_res; m.y = 1.0 - m.y;
    vec2 ck = vec2(u_click.x, 1.0 - u_click.y);
    float t = u_time * 0.12;

    // Derive two complementary tints from the accent
    vec3 warm = u_accent;
    vec3 mid  = u_accent * 0.65 + vec3(0.08, 0.04, 0.02);
    vec3 cool = vec3(u_accent.z * 0.6, u_accent.y * 0.4, u_accent.x * 0.35 + 0.18);

    vec3 col = u_bg * 0.88;
    col += leak(uv, m + vec2( 0.12*sin(t),      0.08*cos(t*0.7)), warm, t);
    col += leak(uv, m + vec2(-0.10*cos(t*1.1),  0.12*sin(t*0.9)), mid,  t + 1.0);
    col += leak(uv, m + vec2( 0.06*sin(t*0.8), -0.14*cos(t*1.3)), cool, t + 2.2);

    if (u_clickAge < 1.4) {
        float fade = 1.0 - u_clickAge / 1.4;
        float r = length(uv - ck);
        col += warm * exp(-30.0 * pow(r - u_clickAge * 0.40, 2.0)) * fade * 0.55;
        col += mid  * exp(-30.0 * pow(r - u_clickAge * 0.28, 2.0)) * fade * 0.38;
        col += cool * exp(-30.0 * pow(r - u_clickAge * 0.18, 2.0)) * fade * 0.28;
    }

    col += (hash(uv + fract(t)) - 0.5) * 0.028;
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}`,
};

// ─── Component ───────────────────────────────────────────────────────────────

export function ShaderWallpaper({ shader, className = "", style }: ShaderWallpaperProps) {
    const canvasRef  = useRef<HTMLCanvasElement>(null);
    const rafRef     = useRef<number>(0);
    const mouseRef   = useRef({ x: 0, y: 0 });
    const clickRef   = useRef({ x: 0.5, y: 0.5, age: 99 });
    const startRef   = useRef(performance.now());
    const progRef    = useRef<WebGLProgram | null>(null);
    const glRef      = useRef<WebGLRenderingContext | null>(null);

    const buildProgram = useCallback((gl: WebGLRenderingContext, fragSrc: string) => {
        const compileShader = (type: number, src: string) => {
            const s = gl.createShader(type)!;
            gl.shaderSource(s, src);
            gl.compileShader(s);
            return s;
        };
        const prog = gl.createProgram()!;
        gl.attachShader(prog, compileShader(gl.VERTEX_SHADER, VERT_SRC));
        gl.attachShader(prog, compileShader(gl.FRAGMENT_SHADER, fragSrc));
        gl.linkProgram(prog);
        return prog;
    }, []);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const gl = canvas.getContext("webgl");
        if (!gl) return;
        glRef.current = gl;

        const prog = buildProgram(gl, SHADERS[shader]);
        progRef.current = prog;
        gl.useProgram(prog);

        const buf = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 1,-1, -1,1, 1,1]), gl.STATIC_DRAW);
        const posLoc = gl.getAttribLocation(prog, "a_pos");
        gl.enableVertexAttribArray(posLoc);
        gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

        const resize = () => {
            canvas.width  = canvas.offsetWidth;
            canvas.height = canvas.offsetHeight;
            gl.viewport(0, 0, canvas.width, canvas.height);
        };
        resize();
        const ro = new ResizeObserver(resize);
        ro.observe(canvas);

        const u = (name: string) => gl.getUniformLocation(prog, name);

        const tick = () => {
            const t = (performance.now() - startRef.current) / 1000;
            clickRef.current.age += 1 / 60;

            // Sample live palette colours every frame (cheap — getComputedStyle is cached)
            const { accent, bg } = readPaletteColors(document.documentElement);

            gl.useProgram(prog);
            gl.uniform2f(u("u_res")!,      canvas.width, canvas.height);
            gl.uniform2f(u("u_mouse")!,    mouseRef.current.x, mouseRef.current.y);
            gl.uniform1f(u("u_time")!,     t);
            gl.uniform2f(u("u_click")!,    clickRef.current.x, clickRef.current.y);
            gl.uniform1f(u("u_clickAge")!, clickRef.current.age);
            gl.uniform3f(u("u_accent")!,   ...accent);
            gl.uniform3f(u("u_bg")!,       ...bg);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
            rafRef.current = requestAnimationFrame(tick);
        };
        rafRef.current = requestAnimationFrame(tick);

        return () => {
            cancelAnimationFrame(rafRef.current);
            ro.disconnect();
        };
    }, [shader, buildProgram]);

    const onMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
        const r = e.currentTarget.getBoundingClientRect();
        mouseRef.current = { x: e.clientX - r.left, y: e.clientY - r.top };
    };

    const onClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
        const r = e.currentTarget.getBoundingClientRect();
        clickRef.current = {
            x: (e.clientX - r.left) / r.width,
            y: (e.clientY - r.top)  / r.height,
            age: 0,
        };
    };

    return (
        <canvas
            ref={canvasRef}
            className={className}
            style={{ display: "block", width: "100%", height: "100%", ...style }}
            onMouseMove={onMouseMove}
            onClick={onClick}
        />
    );
}

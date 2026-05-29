import type { WallpaperId } from "./wallpaper-registry";
import { GLSL_HELPERS, GLSL_UNIFORMS } from "./shader-common";

const AURORA_FRAG = `
${GLSL_UNIFORMS}
${GLSL_HELPERS}
float wfbm(vec2 p){
  vec2 q=vec2(fbm(p),fbm(p+vec2(5.2,1.3)));
  return fbm(p+1.5*uTurb*q+.08*uT);
}
float star(vec2 uv,float sc,float sd){
  vec2 g=floor(uv*sc+sd*91.); float r=hash(g+sd);
  if(r<.963) return 0.;
  vec2 f=fract(uv*sc+sd*91.)-.5;
  return smoothstep(.22,0.,length(f))*(.40+.60*sin(uT*(2.+r*6.)+r*6.28))*(r-.963)/.037;
}
float auroraField(vec2 p){
  float f1 = wfbm(p * uBMult + vec2(uT * .05, uT * .04));
  float f2 = wfbm(p * uBMult * 1.65 + vec2(4.2, 1.8) + vec2(uT * .035, -uT * .028));
  return clamp(mix(f1, f2, .48), 0., 1.);
}
void main(){
  vec2 uv = gl_FragCoord.xy / uR;
  vec2 d2m = uM - uv;
  float dm = length(d2m);
  vec2 wuv = uv + d2m * (.06 + uBeam * .02) * exp(-dm * 3.4) + ripples(uv);
  float hx = mix(wuv.x, wuv.y, uHflow * .35);
  float hy = mix(wuv.y, wuv.x, uHflow * .35);
  vec2 fp = vec2(hx, hy - uHt * .12) * vec2(1.15, .92);
  fp += vec2(sin(uT * .11 + hy * 3.2), cos(uT * .09 + hx * 2.8)) * .018 * uTurb;
  float field = auroraField(fp) * uIntens;
  float mouseGlow = exp(-dm * 5.2) * (.22 + uBeam * .12);
  float beam = exp(-abs(wuv.x - uM.x) * 6.5) * exp(-abs(wuv.y - uM.y) * 2.0) * uBeam * .65;
  float s = clamp((star(uv,95.,.0)+star(uv,160.,1.4)*.5+star(uv,240.,3.1)*.28)*uStars*.72, 0., 1.);
  float glow = clamp(field * 0.92 + mouseGlow * 0.38 + beam * 0.22, 0., 1.);
  float pop = clamp(field * field * 0.95 + mouseGlow * 0.45 + beam * 0.28, 0., 1.);
  vec3 col = cinematicBase(uv);
  col = mix(col, uMid, glow * 0.76);
  col = mix(col, uAcc, pop * 0.58);
  col = mix(col, uAcc, s * 0.46);
  col += uAcc * mouseGlow * 0.16;
  col *= 1. - dot(uv - .5, uv - .5) * 0.62;
  gl_FragColor = vec4(pow(max(col, 0.), vec3(.4545)), 1.);
}`;

const NEBULA_FRAG = `
${GLSL_UNIFORMS}
${GLSL_HELPERS}
void main(){
  vec2 uv = gl_FragCoord.xy / uR;
  vec2 d2m = uM - uv;
  float dm = length(d2m);
  vec2 wuv = uv + d2m * .08 * exp(-dm * 2.8) + ripples(uv) * 1.4;
  vec2 p = wuv * 2.2 + vec2(sin(uT * .07), cos(uT * .05)) * .04;
  float n1 = fbm(p * 1.8 + uT * .03);
  float n2 = fbm(p * 2.6 - vec2(uT * .04, uT * .02) + n1);
  float cloud = smoothstep(.22, .92, n1 * .52 + n2 * .42);
  float core = exp(-dm * 4.2) * (.22 + uBeam * .14);
  vec3 col = cinematicBase(uv);
  col = mix(col, uMid, cloud * 0.76);
  col = mix(col, uAcc, pow(cloud, 1.25) * 0.62 + core * 0.38);
  col += uAcc * core * 0.14;
  col *= 1. - dot(uv - .5, uv - .5) * 0.65;
  gl_FragColor = vec4(pow(max(col, 0.), vec3(.4545)), 1.);
}`;

const PLASMA_FRAG = `
${GLSL_UNIFORMS}
${GLSL_HELPERS}
void main(){
  vec2 uv = gl_FragCoord.xy / uR;
  vec2 d2m = uM - uv;
  float dm = length(d2m);
  vec2 wuv = uv + ripples(uv);
  float px = wuv.x * 8. + uT * .6 + d2m.x * 4. * exp(-dm * 2.);
  float py = wuv.y * 8. - uT * .5 + d2m.y * 4. * exp(-dm * 2.);
  float v = sin(px) + sin(py) + sin(px + py + uT);
  v = v * .33 + .5;
  float pulse = exp(-dm * 4.5) * (.22 + uBeam * .12);
  vec3 col = cinematicBase(uv);
  col = mix(col, uMid, smoothstep(.14, .66, v) * .82);
  col = mix(col, uAcc, smoothstep(.48, 1., v + pulse) * .62);
  col += uAcc * pulse * .14;
  col *= 1. - dot(uv - .5, uv - .5) * .85;
  gl_FragColor = vec4(pow(max(col, 0.), vec3(.4545)), 1.);
}`;

const WARP_GRID_FRAG = `
${GLSL_UNIFORMS}
${GLSL_HELPERS}
void main(){
  vec2 uv = gl_FragCoord.xy / uR;
  vec2 d2m = uM - uv;
  float dm = length(d2m);
  vec2 wuv = uv + d2m * .05 * exp(-dm * 3.2) + ripples(uv) * .8;
  vec2 g = wuv * vec2(uR.x / uR.y, 1.) * 14.;
  g.y += uT * .15;
  float warp = exp(-dm * 2.5) * .35;
  g += normalize(d2m + .001) * warp * 2.;
  vec2 cell = abs(fract(g - .5) - .5);
  float line = 1. - smoothstep(0., .04, min(cell.x, cell.y));
  float glow = exp(-dm * 4.8) * uBeam * .45;
  vec3 col = cinematicBase(uv);
  col = mix(col, uMid, line * .64);
  col = mix(col, uAcc, line * glow + glow * .24);
  col *= 1. - dot(uv - .5, uv - .5) * .95;
  gl_FragColor = vec4(pow(max(col, 0.), vec3(.4545)), 1.);
}`;

const LIQUID_FRAG = `
${GLSL_UNIFORMS}
${GLSL_HELPERS}
void main(){
  vec2 uv = gl_FragCoord.xy / uR;
  vec2 d2m = uM - uv;
  float dm = length(d2m);
  vec2 wuv = uv + ripples(uv) * 2.;
  float blob = 0.;
  blob += .12 / (length(wuv - uM) + .14);
  for(int i=0;i<8;i++){
    if(i>=uCN) break;
    float age=uT-uCT[i];
    if(age<0.||age>2.5) continue;
    float r = length(wuv - uCP[i]);
    blob += .18 * exp(-r * 8.) * exp(-age * 1.2);
  }
  blob += fbm(wuv * 4. + uT * .08) * .12;
  float field = smoothstep(.35, 1.1, blob);
  float rim = smoothstep(.9, 1.15, blob) - smoothstep(1.1, 1.4, blob);
  vec3 col = cinematicBase(uv);
  col = mix(col, uMid, field * .78);
  col = mix(col, uAcc, field * field * .66 + rim * .44);
  col += uAcc * exp(-dm * 4.5) * .1;
  col *= 1. - dot(uv - .5, uv - .5) * .88;
  gl_FragColor = vec4(pow(max(col, 0.), vec3(.4545)), 1.);
}`;

export const SHADER_SOURCES: Record<WallpaperId, string> = {
    aurora: AURORA_FRAG,
    nebula: NEBULA_FRAG,
    plasma: PLASMA_FRAG,
    warpGrid: WARP_GRID_FRAG,
    liquid: LIQUID_FRAG,
};

export const SHADER_PRESETS: Record<
    WallpaperId,
    { turb: number; speed: number; intens: number; ht: number; hflow: number; bMult: number; beam: number; stars: number }
> = {
    aurora: { turb: 0.95, speed: 0.38, intens: 1.05, ht: 0.5, hflow: 0.0, bMult: 1.0, beam: 0.48, stars: 0.45 },
    nebula: { turb: 1.05, speed: 0.32, intens: 0.88, ht: 0.4, hflow: 0.2, bMult: 1.15, beam: 0.28, stars: 0.0 },
    plasma: { turb: 1.4, speed: 0.55, intens: 0.95, ht: 0.3, hflow: 0.0, bMult: 1.0, beam: 0.35, stars: 0.0 },
    warpGrid: { turb: 0.6, speed: 0.42, intens: 0.95, ht: 0.5, hflow: 0.0, bMult: 1.0, beam: 0.38, stars: 0.0 },
    liquid: { turb: 0.8, speed: 0.48, intens: 0.95, ht: 0.35, hflow: 0.0, bMult: 1.0, beam: 0.3, stars: 0.0 },
};

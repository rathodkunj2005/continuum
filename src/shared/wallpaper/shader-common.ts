/** Shared GLSL helpers included in every motion-background fragment shader. */

export const VERT_SRC = `attribute vec2 aP;void main(){gl_Position=vec4(aP,0.,1.);}`;

export const GLSL_HELPERS = `
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
vec3 cinematicBase(vec2 uv){
  float sweep = smoothstep(0., 1., uv.x * .58 + uv.y * .42);
  float glow = exp(-length((uv - vec2(.72, .55)) * vec2(1.05, .86)) * 2.35);
  vec3 col = mix(uBg, uMid, .16 + sweep * .12);
  return mix(col, uAcc, .035 + glow * .075);
}
`;

export const GLSL_UNIFORMS = `
precision highp float;
uniform float uT, uIntens, uTurb, uHt, uHflow, uBMult, uBeam, uStars;
uniform vec2  uR, uM;
uniform vec3  uBg, uMid, uAcc;
uniform vec2  uCP[8];
uniform float uCT[8];
uniform int   uCN;
`;

export const FALLBACK_COLORS = {
    bg: [0, 0, 0] as [number, number, number],
    mid: [0.302, 0.604, 0.302] as [number, number, number],
    acc: [0, 1, 0.255] as [number, number, number],
};

export function lerp3(
    a: [number, number, number],
    b: [number, number, number],
    k: number
): [number, number, number] {
    return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
}

export function readCssAurora(): typeof FALLBACK_COLORS {
    const s = getComputedStyle(document.documentElement);
    const n = (k: string) => parseFloat(s.getPropertyValue(k));
    const bgR = n("--cp-aurora-bg-r");
    if (isNaN(bgR)) return { ...FALLBACK_COLORS };
    return {
        bg: [bgR, n("--cp-aurora-bg-g"), n("--cp-aurora-bg-b")],
        mid: [n("--cp-aurora-mid-r"), n("--cp-aurora-mid-g"), n("--cp-aurora-mid-b")],
        acc: [n("--cp-aurora-acc-r"), n("--cp-aurora-acc-g"), n("--cp-aurora-acc-b")],
    };
}

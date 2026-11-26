import { buildCameraBasis } from "./gizmos";
import type { TerrainMeshData } from "./terrainMesh";
import {
  hexToRgbNorm,
  type CameraState,
  type ParsedObjGeometry,
  type Part3D,
  type Vec3,
  type ViewportLightingInput,
  type WorkspaceLight,
} from "./types";

type MeshGpuBuffer = {
  vao: WebGLVertexArrayObject;
  vbo: WebGLBuffer;
  ebo: WebGLBuffer | null;
  count: number;
  indexed: boolean;
};

type RenderParams = {
  width: number;
  height: number;
  cam: CameraState;
  theme: "light" | "dark";
  lighting: ViewportLightingInput;
  parts: Part3D[];
  lights: WorkspaceLight[];
  terrainMesh: TerrainMeshData | null;
  selectedPath?: string | null;
  popPath?: string;
  popProgress?: number;
  timeSec: number;
  imgCache: Map<string, HTMLImageElement>;
  meshCache: Map<string, ParsedObjGeometry | "loading" | "error">;
};

const SKY_VS = `#version 300 es
precision highp float;
out vec2 vNdc;
void main() {
  vec2 pos = vec2((gl_VertexID == 1) ? 3.0 : -1.0, (gl_VertexID == 2) ? 3.0 : -1.0);
  vNdc = pos;
  gl_Position = vec4(pos, 0.9999, 1.0);
}`;

const SKY_FS = `#version 300 es
precision highp float;
in vec2 vNdc;
uniform vec3 uSkyTop;
uniform vec3 uSkyHorizon;
uniform vec3 uGroundColor;
uniform vec3 uCamForward;
uniform vec3 uCamRight;
uniform vec3 uCamUp;
uniform float uTanHalfFov;
uniform float uAspect;
uniform vec3 uSunDir;
uniform float uSunGlare;
out vec4 fragColor;
void main() {
  vec3 dir = normalize(uCamForward + uCamRight * (vNdc.x * uAspect * uTanHalfFov) + uCamUp * (vNdc.y * uTanHalfFov));
  float y = dir.y;
  vec3 col;
  if (y >= 0.0) {
    float t = pow(clamp(y, 0.0, 1.0), 0.55);
    col = mix(uSkyHorizon, uSkyTop, t);
  } else {
    float t = pow(clamp(-y * 2.4, 0.0, 1.0), 0.7);
    col = mix(uSkyHorizon, uGroundColor, t);
  }
  float sunDot = max(0.0, dot(dir, normalize(uSunDir)));
  float halo = pow(sunDot, 12.0) * (0.22 + uSunGlare * 0.35);
  float disc = smoothstep(0.9965, 0.9992, sunDot) * 0.85;
  col += vec3(1.0, 0.93, 0.8) * (halo + disc);
  fragColor = vec4(col, 1.0);
}`;

const LIT_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec2 aUv;
layout(location = 3) in vec3 aColor;
layout(location = 4) in vec3 aMat;

uniform mat4 uModel;
uniform mat4 uViewProj;
uniform mat3 uNormalMat;

out vec3 vWorldPos;
out vec3 vNormal;
out vec2 vUv;
out vec3 vVertColor;
out vec3 vVertMat;

void main() {
  vec4 wp = uModel * vec4(aPos, 1.0);
  vWorldPos = wp.xyz;
  vNormal = normalize(uNormalMat * aNormal);
  vUv = aUv;
  vVertColor = aColor;
  vVertMat = aMat;
  gl_Position = uViewProj * wp;
}`;

const LIT_FS = `#version 300 es
precision highp float;
in vec3 vWorldPos;
in vec3 vNormal;
in vec2 vUv;
in vec3 vVertColor;
in vec3 vVertMat;

uniform vec3 uCamPos;
uniform vec3 uBaseColor;
uniform float uUseVertColor;
uniform float uRoughness;
uniform float uMetalness;
uniform float uEmissive;
uniform float uAlpha;
uniform float uSelected;
uniform float uHasTex;
uniform sampler2D uTex;
uniform float uIsWater;
uniform float uTime;

uniform vec3 uSunDir;
uniform vec3 uSunColor;
uniform vec3 uAmbientColor;
uniform vec3 uSkyHorizon;
uniform float uBrightness;
uniform vec3 uFogColor;
uniform float uFogStart;
uniform float uFogEnd;
uniform float uAtmoDensity;

uniform int uLightCount;
uniform vec3 uLightPos[8];
uniform vec3 uLightDir[8];
uniform vec3 uLightColor[8];
uniform vec4 uLightParams[8];

out vec4 fragColor;

void main() {
  vec3 N = normalize(vNormal);
  if (uIsWater > 0.5) {
    float w1 = sin(vWorldPos.x * 0.35 + uTime * 1.9) * cos(vWorldPos.z * 0.32 - uTime * 1.5);
    float w2 = sin((vWorldPos.x + vWorldPos.z) * 0.52 - uTime * 2.4);
    N = normalize(vec3((w1 + w2) * 0.09, 1.0, (w1 - w2) * 0.09));
  }

  vec3 V = normalize(uCamPos - vWorldPos);
  vec3 base = mix(uBaseColor, vVertColor, uUseVertColor);
  if (uHasTex > 0.5) {
    vec4 texel = texture(uTex, vUv);
    base = mix(base, texel.rgb, texel.a);
  }

  float rough = mix(uRoughness, vVertMat.x, uUseVertColor);
  float metal = mix(uMetalness, vVertMat.y, uUseVertColor);
  float emis = mix(uEmissive, vVertMat.z, uUseVertColor);

  vec3 L = normalize(uSunDir);
  float ndotl = max(dot(N, L), 0.0);
  vec3 H = normalize(L + V);
  float shininess = mix(96.0, 8.0, clamp(rough, 0.05, 1.0));
  float spec = pow(max(dot(N, H), 0.0), shininess) * mix(0.65, 0.08, rough);
  float fresnel = pow(1.0 - max(dot(N, V), 0.0), 3.0);

  vec3 hemi = mix(uAmbientColor * 0.65, uAmbientColor * 1.15, N.y * 0.5 + 0.5);
  vec3 diffColor = base * (1.0 - metal * 0.65);
  vec3 specTint = mix(vec3(1.0), base, metal);

  vec3 lit = diffColor * (hemi + uSunColor * ndotl * uBrightness * 0.55)
           + specTint * uSunColor * spec * (0.4 + uBrightness * 0.3)
           + uSkyHorizon * fresnel * mix(0.12, 0.55, max(metal, uIsWater));

  for (int i = 0; i < 8; i++) {
    if (i >= uLightCount) break;
    vec3 toL = uLightPos[i] - vWorldPos;
    float d = length(toL);
    float range = max(1.0, uLightParams[i].x);
    if (d < range) {
      vec3 ld = toL / max(d, 0.001);
      float att = pow(clamp(1.0 - d / range, 0.0, 1.0), 1.75);
      float kind = uLightParams[i].z;
      if (kind > 0.5) {
        vec3 spotAim = normalize(-uLightDir[i]);
        float cosA = dot(ld, spotAim);
        float cutoff = uLightParams[i].w;
        float cone = smoothstep(cutoff - 0.12, cutoff + 0.08, cosA);
        att *= cone;
      }
      float lnd = max(dot(N, ld), 0.0);
      vec3 lH = normalize(ld + V);
      float lSpec = pow(max(dot(N, lH), 0.0), shininess) * 0.45;
      float lBr = uLightParams[i].y;
      lit += (diffColor * lnd + specTint * lSpec) * uLightColor[i] * lBr * att;
    }
  }

  lit += base * emis * 0.9;
  if (uSelected > 0.5) {
    lit = mix(lit, vec3(0.24, 0.56, 1.0), fresnel * 0.65 + 0.14);
  }

  float dist = length(uCamPos - vWorldPos);
  float fogSpan = max(30.0, uFogEnd - uFogStart);
  float fogAmt = clamp((dist - uFogStart) / fogSpan, 0.0, 0.88);
  float atmoAmt = 1.0 - exp(-dist * uAtmoDensity * 0.0022);
  float totalFog = clamp(max(fogAmt, atmoAmt * 0.65), 0.0, 0.85);
  lit = mix(lit, uFogColor, totalFog);

  float outAlpha = uAlpha;
  if (uIsWater > 0.5) {
    outAlpha = clamp(uAlpha + fresnel * 0.25, 0.35, 0.88);
  }
  fragColor = vec4(lit, outAlpha);
}`;

const LINE_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec4 aColor;
uniform mat4 uViewProj;
out vec4 vColor;
void main() {
  vColor = aColor;
  gl_Position = uViewProj * vec4(aPos, 1.0);
}`;

const LINE_FS = `#version 300 es
precision highp float;
in vec4 vColor;
out vec4 fragColor;
void main() {
  fragColor = vColor;
}`;

function compileShader(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const s = gl.createShader(type);
  if (!s) throw new Error("shader alloc failed");
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(s) || "shader compile error";
    gl.deleteShader(s);
    throw new Error(log);
  }
  return s;
}

function linkProgram(gl: WebGL2RenderingContext, vsSrc: string, fsSrc: string): WebGLProgram {
  const vs = compileShader(gl, gl.VERTEX_SHADER, vsSrc);
  const fs = compileShader(gl, gl.FRAGMENT_SHADER, fsSrc);
  const prog = gl.createProgram();
  if (!prog) throw new Error("program alloc failed");
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(prog) || "program link error";
    gl.deleteProgram(prog);
    throw new Error(log);
  }
  return prog;
}

function mat4Perspective(fovRad: number, aspect: number, near: number, far: number): Float32Array {
  const f = 1 / Math.tan(fovRad * 0.5);
  const nf = 1 / (near - far);
  const m = new Float32Array(16);
  m[0] = f / aspect;
  m[5] = f;
  m[10] = (far + near) * nf;
  m[11] = -1;
  m[14] = 2 * far * near * nf;
  return m;
}

function mat4LookAtBasis(eye: Vec3, forward: Vec3, right: Vec3, up: Vec3): Float32Array {
  const m = new Float32Array(16);
  m[0] = right[0];
  m[4] = right[1];
  m[8] = right[2];
  m[12] = -(right[0] * eye[0] + right[1] * eye[1] + right[2] * eye[2]);

  m[1] = up[0];
  m[5] = up[1];
  m[9] = up[2];
  m[13] = -(up[0] * eye[0] + up[1] * eye[1] + up[2] * eye[2]);

  m[2] = -forward[0];
  m[6] = -forward[1];
  m[10] = -forward[2];
  m[14] = forward[0] * eye[0] + forward[1] * eye[1] + forward[2] * eye[2];

  m[15] = 1;
  return m;
}

function mat4Mul(a: Float32Array, b: Float32Array): Float32Array {
  const out = new Float32Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      out[c * 4 + r] =
        a[0 * 4 + r] * b[c * 4 + 0] +
        a[1 * 4 + r] * b[c * 4 + 1] +
        a[2 * 4 + r] * b[c * 4 + 2] +
        a[3 * 4 + r] * b[c * 4 + 3];
    }
  }
  return out;
}

function mat4ModelTRS(pos: Vec3, rotYDeg: number, scale: Vec3): { model: Float32Array; normalMat: Float32Array } {
  const rad = (rotYDeg * Math.PI) / 180;
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  const model = new Float32Array(16);
  // R_y(rad) * S(scale)
  model[0] = c * scale[0];
  model[1] = 0;
  model[2] = -s * scale[0];
  model[4] = 0;
  model[5] = scale[1];
  model[6] = 0;
  model[8] = s * scale[2];
  model[9] = 0;
  model[10] = c * scale[2];
  model[12] = pos[0];
  model[13] = pos[1];
  model[14] = pos[2];
  model[15] = 1;

  const isx = 1 / Math.max(0.001, scale[0]);
  const isy = 1 / Math.max(0.001, scale[1]);
  const isz = 1 / Math.max(0.001, scale[2]);
  const normalMat = new Float32Array(9);
  normalMat[0] = c * isx;
  normalMat[1] = 0;
  normalMat[2] = -s * isx;
  normalMat[3] = 0;
  normalMat[4] = isy;
  normalMat[5] = 0;
  normalMat[6] = s * isz;
  normalMat[7] = 0;
  normalMat[8] = c * isz;

  return { model, normalMat };
}

function createInterleavedMesh(
  gl: WebGL2RenderingContext,
  positions: Float32Array,
  normals: Float32Array,
  uvs: Float32Array,
  indices?: Uint16Array | Uint32Array,
  colors?: Float32Array,
  mats?: Float32Array,
): MeshGpuBuffer {
  const vCount = positions.length / 3;
  const strideFloats = 14;
  const packed = new Float32Array(vCount * strideFloats);
  for (let i = 0; i < vCount; i++) {
    const o = i * strideFloats;
    packed[o] = positions[i * 3];
    packed[o + 1] = positions[i * 3 + 1];
    packed[o + 2] = positions[i * 3 + 2];
    packed[o + 3] = normals[i * 3];
    packed[o + 4] = normals[i * 3 + 1];
    packed[o + 5] = normals[i * 3 + 2];
    packed[o + 6] = uvs[i * 2];
    packed[o + 7] = uvs[i * 2 + 1];
    if (colors) {
      packed[o + 8] = colors[i * 3];
      packed[o + 9] = colors[i * 3 + 1];
      packed[o + 10] = colors[i * 3 + 2];
    } else {
      packed[o + 8] = 1;
      packed[o + 9] = 1;
      packed[o + 10] = 1;
    }
    if (mats) {
      packed[o + 11] = mats[i * 3];
      packed[o + 12] = mats[i * 3 + 1];
      packed[o + 13] = mats[i * 3 + 2];
    } else {
      packed[o + 11] = 0.5;
      packed[o + 12] = 0;
      packed[o + 13] = 0;
    }
  }

  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);
  const vbo = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
  gl.bufferData(gl.ARRAY_BUFFER, packed, gl.STATIC_DRAW);

  const bytes = strideFloats * 4;
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 3, gl.FLOAT, false, bytes, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 3, gl.FLOAT, false, bytes, 12);
  gl.enableVertexAttribArray(2);
  gl.vertexAttribPointer(2, 2, gl.FLOAT, false, bytes, 24);
  gl.enableVertexAttribArray(3);
  gl.vertexAttribPointer(3, 3, gl.FLOAT, false, bytes, 32);
  gl.enableVertexAttribArray(4);
  gl.vertexAttribPointer(4, 3, gl.FLOAT, false, bytes, 44);

  let ebo: WebGLBuffer | null = null;
  if (indices) {
    ebo = gl.createBuffer()!;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ebo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);
  }
  gl.bindVertexArray(null);

  return {
    vao,
    vbo,
    ebo,
    count: indices ? indices.length : vCount,
    indexed: Boolean(indices),
  };
}

function buildCubeMesh(gl: WebGL2RenderingContext): MeshGpuBuffer {
  const faces: Array<{ n: Vec3; v: Vec3[] }> = [
    { n: [0, 1, 0], v: [[-0.5, 0.5, -0.5], [-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5]] },
    { n: [0, -1, 0], v: [[-0.5, -0.5, 0.5], [-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5]] },
    { n: [0, 0, 1], v: [[-0.5, 0.5, 0.5], [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5]] },
    { n: [0, 0, -1], v: [[0.5, 0.5, -0.5], [0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5]] },
    { n: [1, 0, 0], v: [[0.5, 0.5, 0.5], [0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5]] },
    { n: [-1, 0, 0], v: [[-0.5, 0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]] },
  ];
  const pos: number[] = [];
  const norm: number[] = [];
  const uv: number[] = [];
  const idx: number[] = [];
  const uvs = [[0, 0], [0, 1], [1, 1], [1, 0]];
  for (let f = 0; f < faces.length; f++) {
    const face = faces[f];
    const base = f * 4;
    for (let i = 0; i < 4; i++) {
      pos.push(face.v[i][0], face.v[i][1], face.v[i][2]);
      norm.push(face.n[0], face.n[1], face.n[2]);
      uv.push(uvs[i][0], uvs[i][1]);
    }
    idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
  return createInterleavedMesh(gl, new Float32Array(pos), new Float32Array(norm), new Float32Array(uv), new Uint16Array(idx));
}

function buildSphereMesh(gl: WebGL2RenderingContext): MeshGpuBuffer {
  const lat = 14;
  const lon = 20;
  const pos: number[] = [];
  const norm: number[] = [];
  const uv: number[] = [];
  const idx: number[] = [];
  for (let y = 0; y <= lat; y++) {
    const v = y / lat;
    const theta = v * Math.PI;
    for (let x = 0; x <= lon; x++) {
      const u = x / lon;
      const phi = u * Math.PI * 2;
      const nx = Math.sin(theta) * Math.cos(phi);
      const ny = Math.cos(theta);
      const nz = Math.sin(theta) * Math.sin(phi);
      pos.push(nx * 0.5, ny * 0.5, nz * 0.5);
      norm.push(nx, ny, nz);
      uv.push(u, v);
    }
  }
  for (let y = 0; y < lat; y++) {
    for (let x = 0; x < lon; x++) {
      const a = y * (lon + 1) + x;
      const b = a + lon + 1;
      idx.push(a, b, a + 1, b, b + 1, a + 1);
    }
  }
  return createInterleavedMesh(gl, new Float32Array(pos), new Float32Array(norm), new Float32Array(uv), new Uint16Array(idx));
}

function buildCylinderMesh(gl: WebGL2RenderingContext): MeshGpuBuffer {
  const segs = 20;
  const pos: number[] = [];
  const norm: number[] = [];
  const uv: number[] = [];
  const idx: number[] = [];
  for (let i = 0; i <= segs; i++) {
    const u = i / segs;
    const a = u * Math.PI * 2;
    const nx = Math.cos(a);
    const nz = Math.sin(a);
    pos.push(nx * 0.5, 0.5, nz * 0.5, nx * 0.5, -0.5, nz * 0.5);
    norm.push(nx, 0, nz, nx, 0, nz);
    uv.push(u, 0, u, 1);
  }
  for (let i = 0; i < segs; i++) {
    const a = i * 2;
    idx.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
  }
  const topCenter = pos.length / 3;
  pos.push(0, 0.5, 0);
  norm.push(0, 1, 0);
  uv.push(0.5, 0.5);
  const topRing = pos.length / 3;
  for (let i = 0; i <= segs; i++) {
    const a = (i / segs) * Math.PI * 2;
    pos.push(Math.cos(a) * 0.5, 0.5, Math.sin(a) * 0.5);
    norm.push(0, 1, 0);
    uv.push(0.5 + Math.cos(a) * 0.5, 0.5 + Math.sin(a) * 0.5);
  }
  for (let i = 0; i < segs; i++) idx.push(topCenter, topRing + i + 1, topRing + i);

  const botCenter = pos.length / 3;
  pos.push(0, -0.5, 0);
  norm.push(0, -1, 0);
  uv.push(0.5, 0.5);
  const botRing = pos.length / 3;
  for (let i = 0; i <= segs; i++) {
    const a = (i / segs) * Math.PI * 2;
    pos.push(Math.cos(a) * 0.5, -0.5, Math.sin(a) * 0.5);
    norm.push(0, -1, 0);
    uv.push(0.5 + Math.cos(a) * 0.5, 0.5 + Math.sin(a) * 0.5);
  }
  for (let i = 0; i < segs; i++) idx.push(botCenter, botRing + i, botRing + i + 1);

  return createInterleavedMesh(gl, new Float32Array(pos), new Float32Array(norm), new Float32Array(uv), new Uint16Array(idx));
}

function buildWaterPlaneMesh(gl: WebGL2RenderingContext): MeshGpuBuffer {
  const pos = new Float32Array([-0.5, 0, -0.5, -0.5, 0, 0.5, 0.5, 0, 0.5, 0.5, 0, -0.5]);
  const norm = new Float32Array([0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0]);
  const uv = new Float32Array([0, 0, 0, 1, 1, 1, 1, 0]);
  const idx = new Uint16Array([0, 1, 2, 0, 2, 3]);
  return createInterleavedMesh(gl, pos, norm, uv, idx);
}

export class WebGL2ViewportRenderer {
  private gl: WebGL2RenderingContext;
  private canvas: HTMLCanvasElement;
  private contextLost = false;
  private skyProg!: WebGLProgram;
  private litProg!: WebGLProgram;
  private lineProg!: WebGLProgram;
  private skyVao!: WebGLVertexArrayObject;
  private cubeMesh!: MeshGpuBuffer;
  private sphereMesh!: MeshGpuBuffer;
  private cylMesh!: MeshGpuBuffer;
  private waterMesh!: MeshGpuBuffer;
  private gridVao!: WebGLVertexArrayObject;
  private gridVbo!: WebGLBuffer;
  private gridVertexCount = 0;
  private gridTheme: "light" | "dark" | null = null;

  private terrainGpu: MeshGpuBuffer | null = null;
  private terrainRef: TerrainMeshData | null = null;
  private objBuffers = new Map<ParsedObjGeometry, MeshGpuBuffer>();
  private textures = new Map<HTMLImageElement, WebGLTexture>();
  private whiteTex!: WebGLTexture;

  private onLost?: () => void;
  private onRestored?: () => void;
  private handleLostBound: (e: Event) => void;
  private handleRestoredBound: () => void;

  private constructor(
    canvas: HTMLCanvasElement,
    gl: WebGL2RenderingContext,
    cbs?: { onContextLost?: () => void; onContextRestored?: () => void },
  ) {
    this.canvas = canvas;
    this.gl = gl;
    this.onLost = cbs?.onContextLost;
    this.onRestored = cbs?.onContextRestored;

    this.handleLostBound = (e: Event) => {
      e.preventDefault();
      this.contextLost = true;
      this.onLost?.();
    };
    this.handleRestoredBound = () => {
      this.contextLost = false;
      this.objBuffers.clear();
      this.textures.clear();
      this.terrainGpu = null;
      this.terrainRef = null;
      this.gridTheme = null;
      this.initResources();
      this.onRestored?.();
    };

    canvas.addEventListener("webglcontextlost", this.handleLostBound);
    canvas.addEventListener("webglcontextrestored", this.handleRestoredBound);
    this.initResources();
  }

  static tryCreate(
    canvas: HTMLCanvasElement,
    cbs?: { onContextLost?: () => void; onContextRestored?: () => void },
  ): WebGL2ViewportRenderer | null {
    try {
      const gl = canvas.getContext("webgl2", {
        antialias: true,
        alpha: false,
        powerPreference: "high-performance",
      });
      if (!gl) return null;
      return new WebGL2ViewportRenderer(canvas, gl, cbs);
    } catch {
      return null;
    }
  }

  isContextLost(): boolean {
    return this.contextLost || this.gl.isContextLost();
  }

  private initResources() {
    const gl = this.gl;
    this.skyProg = linkProgram(gl, SKY_VS, SKY_FS);
    this.litProg = linkProgram(gl, LIT_VS, LIT_FS);
    this.lineProg = linkProgram(gl, LINE_VS, LINE_FS);
    this.skyVao = gl.createVertexArray()!;
    this.cubeMesh = buildCubeMesh(gl);
    this.sphereMesh = buildSphereMesh(gl);
    this.cylMesh = buildCylinderMesh(gl);
    this.waterMesh = buildWaterPlaneMesh(gl);
    this.gridVao = gl.createVertexArray()!;
    this.gridVbo = gl.createBuffer()!;

    this.whiteTex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([255, 255, 255, 255]));
  }

  private ensureGridLines(theme: "light" | "dark") {
    if (this.gridTheme === theme) return;
    this.gridTheme = theme;
    const gl = this.gl;
    const isDark = theme === "dark";
    const verts: number[] = [];
    const pushLine = (a: Vec3, b: Vec3, c: [number, number, number, number]) => {
      verts.push(a[0], a[1], a[2], c[0], c[1], c[2], c[3]);
      verts.push(b[0], b[1], b[2], c[0], c[1], c[2], c[3]);
    };

    const baseHalf = 72;
    for (let g = -baseHalf; g <= baseHalf; g += 8) {
      const major = g % 24 === 0;
      const c: [number, number, number, number] = major
        ? isDark
          ? [0.58, 0.64, 0.72, 0.34]
          : [0.24, 0.23, 0.21, 0.24]
        : isDark
          ? [0.58, 0.64, 0.72, 0.14]
          : [0.24, 0.23, 0.21, 0.11];
      pushLine([g, 0.04, -baseHalf], [g, 0.04, baseHalf], c);
      pushLine([-baseHalf, 0.04, g], [baseHalf, 0.04, g], c);
    }

    pushLine([0, 0.08, 0], [18, 0.08, 0], [0.88, 0.28, 0.24, 0.85]);
    pushLine([0, 0.08, 0], [0, 0.08, 18], [0.22, 0.48, 0.91, 0.85]);

    const data = new Float32Array(verts);
    this.gridVertexCount = data.length / 7;
    gl.bindVertexArray(this.gridVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.gridVbo);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 28, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 28, 12);
    gl.bindVertexArray(null);
  }

  private getOrUploadTexture(img: HTMLImageElement): WebGLTexture | null {
    if (!img.complete || img.naturalWidth <= 0) return null;
    const existing = this.textures.get(img);
    if (existing) return existing;
    const gl = this.gl;
    const tex = gl.createTexture();
    if (!tex) return null;
    try {
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      this.textures.set(img, tex);
      return tex;
    } catch {
      gl.deleteTexture(tex);
      return null;
    }
  }

  private getOrUploadObj(geom: ParsedObjGeometry): MeshGpuBuffer {
    const cached = this.objBuffers.get(geom);
    if (cached) return cached;
    const gpu = createInterleavedMesh(this.gl, geom.positions, geom.normals, geom.uvs);
    this.objBuffers.set(geom, gpu);
    return gpu;
  }

  private ensureTerrainBuffer(terrainMesh: TerrainMeshData | null): MeshGpuBuffer | null {
    if (!terrainMesh) return null;
    if (this.terrainRef === terrainMesh && this.terrainGpu) return this.terrainGpu;
    if (this.terrainGpu) {
      this.gl.deleteVertexArray(this.terrainGpu.vao);
      this.gl.deleteBuffer(this.terrainGpu.vbo);
      if (this.terrainGpu.ebo) this.gl.deleteBuffer(this.terrainGpu.ebo);
      this.terrainGpu = null;
    }
    this.terrainRef = terrainMesh;
    if (terrainMesh.indices.length === 0) return null;
    this.terrainGpu = createInterleavedMesh(
      this.gl,
      terrainMesh.positions,
      terrainMesh.normals,
      new Float32Array((terrainMesh.positions.length / 3) * 2),
      terrainMesh.indices,
      terrainMesh.colors,
      terrainMesh.materialAttrs,
    );
    return this.terrainGpu;
  }

  render(p: RenderParams) {
    if (this.isContextLost()) return;
    const gl = this.gl;
    gl.viewport(0, 0, p.width, p.height);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);

    const isDark = p.theme === "dark";
    const basis = buildCameraBasis(p.cam, p.width, p.height);
    const fovRad = (46 * Math.PI) / 180;
    const aspect = p.width / Math.max(1, p.height);
    const proj = mat4Perspective(fovRad, aspect, 0.5, 2200);
    const view = mat4LookAtBasis(basis.eye, basis.forward, basis.right, basis.up);
    const viewProj = mat4Mul(proj, view);

    const clockTime = typeof p.lighting?.clock_time === "number" && Number.isFinite(p.lighting.clock_time) ? p.lighting.clock_time : 14;
    const isNight = clockTime < 5.5 || clockTime >= 19.5;
    const isGolden = (clockTime >= 5.5 && clockTime < 7.5) || (clockTime >= 17 && clockTime < 19.5);
    const sunAngle = ((clockTime - 6) / 12) * Math.PI;
    const sunY = isNight ? 0.35 : Math.max(0.22, Math.sin(sunAngle));
    const sunX = Math.cos(sunAngle) * 0.85;
    const sunLen = Math.hypot(sunX, sunY, 0.42) || 1;
    const sunDir: Vec3 = [sunX / sunLen, sunY / sunLen, 0.42 / sunLen];

    const skyTop: Vec3 = isNight
      ? [0.04, 0.06, 0.13]
      : isGolden
        ? isDark
          ? [0.14, 0.13, 0.23]
          : [0.76, 0.65, 0.78]
        : isDark
          ? [0.07, 0.09, 0.15]
          : [0.76, 0.86, 0.96];
    const skyHorizon: Vec3 = isNight
      ? [0.09, 0.13, 0.22]
      : isGolden
        ? isDark
          ? [0.23, 0.17, 0.23]
          : [0.95, 0.79, 0.65]
        : isDark
          ? [0.11, 0.14, 0.21]
          : [0.91, 0.94, 0.96];
    const groundColor: Vec3 = isDark ? [0.08, 0.1, 0.14] : [0.85, 0.84, 0.8];
    const sunColor: Vec3 = isNight ? [0.48, 0.58, 0.82] : isGolden ? [1.0, 0.82, 0.64] : [1.0, 0.98, 0.94];

    const ambientColor =
      typeof p.lighting?.ambient === "string" && /^#[0-9a-f]{6}$/i.test(p.lighting.ambient.trim())
        ? hexToRgbNorm(p.lighting.ambient.trim())
        : isDark
          ? ([0.42, 0.47, 0.56] as Vec3)
          : ([0.58, 0.62, 0.68] as Vec3);
    const fogColor =
      typeof p.lighting?.fog_color === "string" && /^#[0-9a-f]{6}$/i.test(p.lighting.fog_color.trim())
        ? hexToRgbNorm(p.lighting.fog_color.trim())
        : skyHorizon;
    const fogStart = typeof p.lighting?.fog_start === "number" ? p.lighting.fog_start : 60;
    const fogEnd = typeof p.lighting?.fog_end === "number" ? p.lighting.fog_end : 10000;
    const brightness = typeof p.lighting?.brightness === "number" ? Math.max(0.3, Math.min(6, p.lighting.brightness)) : 2.0;
    const atmoDensity = typeof p.lighting?.atmosphere?.density === "number" ? p.lighting.atmosphere.density : 0.28;
    const sunGlare = typeof p.lighting?.atmosphere?.glare === "number" ? p.lighting.atmosphere.glare : 0.15;

    // 1. Sky / Atmosphere Pass
    gl.depthMask(false);
    gl.disable(gl.BLEND);
    gl.useProgram(this.skyProg);
    gl.bindVertexArray(this.skyVao);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uSkyTop"), skyTop);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uSkyHorizon"), skyHorizon);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uGroundColor"), groundColor);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uCamForward"), basis.forward);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uCamRight"), basis.right);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uCamUp"), basis.up);
    gl.uniform1f(gl.getUniformLocation(this.skyProg, "uTanHalfFov"), Math.tan(fovRad * 0.5));
    gl.uniform1f(gl.getUniformLocation(this.skyProg, "uAspect"), aspect);
    gl.uniform3fv(gl.getUniformLocation(this.skyProg, "uSunDir"), sunDir);
    gl.uniform1f(gl.getUniformLocation(this.skyProg, "uSunGlare"), sunGlare);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.depthMask(true);
    gl.clear(gl.DEPTH_BUFFER_BIT);

    // Prepare Lit Program Uniforms
    gl.useProgram(this.litProg);
    gl.uniformMatrix4fv(gl.getUniformLocation(this.litProg, "uViewProj"), false, viewProj);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uCamPos"), basis.eye);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uSunDir"), sunDir);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uSunColor"), sunColor);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uAmbientColor"), ambientColor);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uSkyHorizon"), skyHorizon);
    gl.uniform1f(gl.getUniformLocation(this.litProg, "uBrightness"), brightness);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uFogColor"), fogColor);
    gl.uniform1f(gl.getUniformLocation(this.litProg, "uFogStart"), fogStart);
    gl.uniform1f(gl.getUniformLocation(this.litProg, "uFogEnd"), fogEnd);
    gl.uniform1f(gl.getUniformLocation(this.litProg, "uAtmoDensity"), atmoDensity);
    gl.uniform1f(gl.getUniformLocation(this.litProg, "uTime"), p.timeSec);

    // Pack up to 8 dynamic lights
    const activeLights = p.lights.filter((l) => l.enabled !== false).slice(0, 8);
    const lPos = new Float32Array(24);
    const lDir = new Float32Array(24);
    const lCol = new Float32Array(24);
    const lPar = new Float32Array(32);
    for (let i = 0; i < activeLights.length; i++) {
      const l = activeLights[i];
      lPos[i * 3] = l.position[0];
      lPos[i * 3 + 1] = l.position[1];
      lPos[i * 3 + 2] = l.position[2];
      const d = l.direction || [0, -1, 0];
      lDir[i * 3] = d[0];
      lDir[i * 3 + 1] = d[1];
      lDir[i * 3 + 2] = d[2];
      const rgb = hexToRgbNorm(l.color);
      lCol[i * 3] = rgb[0];
      lCol[i * 3 + 1] = rgb[1];
      lCol[i * 3 + 2] = rgb[2];
      lPar[i * 4] = Math.max(2, l.range);
      lPar[i * 4 + 1] = Math.max(0.1, l.brightness);
      lPar[i * 4 + 2] = l.kind === "PointLight" ? 0 : l.kind === "SpotLight" ? 1 : 2;
      const halfAngleRad = ((l.angle ?? 65) * 0.5 * Math.PI) / 180;
      lPar[i * 4 + 3] = Math.cos(halfAngleRad);
    }
    gl.uniform1i(gl.getUniformLocation(this.litProg, "uLightCount"), activeLights.length);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uLightPos"), lPos);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uLightDir"), lDir);
    gl.uniform3fv(gl.getUniformLocation(this.litProg, "uLightColor"), lCol);
    gl.uniform4fv(gl.getUniformLocation(this.litProg, "uLightParams"), lPar);

    const locModel = gl.getUniformLocation(this.litProg, "uModel");
    const locNormMat = gl.getUniformLocation(this.litProg, "uNormalMat");
    const locBaseColor = gl.getUniformLocation(this.litProg, "uBaseColor");
    const locUseVertColor = gl.getUniformLocation(this.litProg, "uUseVertColor");
    const locRough = gl.getUniformLocation(this.litProg, "uRoughness");
    const locMetal = gl.getUniformLocation(this.litProg, "uMetalness");
    const locEmis = gl.getUniformLocation(this.litProg, "uEmissive");
    const locAlpha = gl.getUniformLocation(this.litProg, "uAlpha");
    const locSel = gl.getUniformLocation(this.litProg, "uSelected");
    const locHasTex = gl.getUniformLocation(this.litProg, "uHasTex");
    const locIsWater = gl.getUniformLocation(this.litProg, "uIsWater");
    const locTex = gl.getUniformLocation(this.litProg, "uTex");
    gl.activeTexture(gl.TEXTURE0);
    gl.uniform1i(locTex, 0);

    const drawBuffer = (buf: MeshGpuBuffer, indexType: number) => {
      gl.bindVertexArray(buf.vao);
      if (buf.indexed) {
        gl.drawElements(gl.TRIANGLES, buf.count, indexType, 0);
      } else {
        gl.drawArrays(gl.TRIANGLES, 0, buf.count);
      }
    };

    // 2. Baseplate slab (when no terrain) or subtle origin grid
    if (!p.terrainMesh) {
      const bp = mat4ModelTRS([0, -0.5, 0], 0, [144, 1, 144]);
      gl.uniformMatrix4fv(locModel, false, bp.model);
      gl.uniformMatrix3fv(locNormMat, false, bp.normalMat);
      gl.uniform3fv(locBaseColor, isDark ? [0.14, 0.17, 0.21] : [0.87, 0.86, 0.83]);
      gl.uniform1f(locUseVertColor, 0);
      gl.uniform1f(locRough, 0.85);
      gl.uniform1f(locMetal, 0);
      gl.uniform1f(locEmis, 0);
      gl.uniform1f(locAlpha, 1);
      gl.uniform1f(locSel, 0);
      gl.uniform1f(locHasTex, 0);
      gl.uniform1f(locIsWater, 0);
      gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
      drawBuffer(this.cubeMesh, gl.UNSIGNED_SHORT);

      this.ensureGridLines(p.theme);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.useProgram(this.lineProg);
      gl.uniformMatrix4fv(gl.getUniformLocation(this.lineProg, "uViewProj"), false, viewProj);
      gl.bindVertexArray(this.gridVao);
      gl.drawArrays(gl.LINES, 0, this.gridVertexCount);
      gl.disable(gl.BLEND);
      gl.useProgram(this.litProg);
    }

    // 3. Procedural Terrain Mesh
    const tGpu = this.ensureTerrainBuffer(p.terrainMesh);
    if (tGpu) {
      const ident = mat4ModelTRS([0, 0, 0], 0, [1, 1, 1]);
      gl.uniformMatrix4fv(locModel, false, ident.model);
      gl.uniformMatrix3fv(locNormMat, false, ident.normalMat);
      gl.uniform3fv(locBaseColor, [1, 1, 1]);
      gl.uniform1f(locUseVertColor, 1);
      gl.uniform1f(locRough, 0.8);
      gl.uniform1f(locMetal, 0);
      gl.uniform1f(locEmis, 0);
      gl.uniform1f(locAlpha, 1);
      gl.uniform1f(locSel, 0);
      gl.uniform1f(locHasTex, 0);
      gl.uniform1f(locIsWater, 0);
      gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
      drawBuffer(tGpu, gl.UNSIGNED_INT);
    }

    // 4. Opaque & Transparent Scene Parts
    const opaqueParts: Part3D[] = [];
    const transparentParts: Array<{ part: Part3D; distSq: number }> = [];
    for (const part of p.parts) {
      if (part.transparency > 0.04) {
        const dx = part.pos[0] - basis.eye[0];
        const dy = part.pos[1] - basis.eye[1];
        const dz = part.pos[2] - basis.eye[2];
        transparentParts.push({ part, distSq: dx * dx + dy * dy + dz * dz });
      } else {
        opaqueParts.push(part);
      }
    }
    transparentParts.sort((a, b) => b.distSq - a.distSq);

    const drawPart = (part: Part3D) => {
      const popScale =
        p.popPath === part.path && p.popProgress !== undefined && p.popProgress < 0.99
          ? 0.25 + 0.75 * p.popProgress
          : 1;
      const scale: Vec3 = [part.size[0] * popScale, part.size[1] * popScale, part.size[2] * popScale];
      const trs = mat4ModelTRS(part.pos, part.rotYDeg, scale);
      gl.uniformMatrix4fv(locModel, false, trs.model);
      gl.uniformMatrix3fv(locNormMat, false, trs.normalMat);
      gl.uniform3fv(locBaseColor, hexToRgbNorm(part.color));
      gl.uniform1f(locUseVertColor, 0);
      gl.uniform1f(locRough, part.roughness);
      gl.uniform1f(locMetal, part.metalness);
      gl.uniform1f(locEmis, part.emissive);
      gl.uniform1f(locAlpha, 1 - part.transparency);
      gl.uniform1f(locSel, p.selectedPath === part.path ? 1 : 0);
      gl.uniform1f(locIsWater, 0);

      const decalImg = part.decalUrl ? p.imgCache.get(part.decalUrl) : undefined;
      const tex = decalImg ? this.getOrUploadTexture(decalImg) : null;
      if (tex) {
        gl.uniform1f(locHasTex, 1);
        gl.bindTexture(gl.TEXTURE_2D, tex);
      } else {
        gl.uniform1f(locHasTex, 0);
        gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
      }

      const meshEntry = part.meshUrl ? p.meshCache.get(part.meshUrl) : undefined;
      if (meshEntry && typeof meshEntry === "object") {
        const gpu = this.getOrUploadObj(meshEntry);
        drawBuffer(gpu, gl.UNSIGNED_SHORT);
      } else if (part.shape === "Ball") {
        drawBuffer(this.sphereMesh, gl.UNSIGNED_SHORT);
      } else if (part.shape === "Cylinder") {
        drawBuffer(this.cylMesh, gl.UNSIGNED_SHORT);
      } else {
        drawBuffer(this.cubeMesh, gl.UNSIGNED_SHORT);
      }
    };

    gl.disable(gl.BLEND);
    for (const part of opaqueParts) drawPart(part);

    // Light emissive indicator bulbs in 3D
    for (const light of p.lights) {
      const trs = mat4ModelTRS(light.position, 0, [1.2, 1.2, 1.2]);
      gl.uniformMatrix4fv(locModel, false, trs.model);
      gl.uniformMatrix3fv(locNormMat, false, trs.normalMat);
      gl.uniform3fv(locBaseColor, light.enabled === false ? [0.45, 0.5, 0.56] : hexToRgbNorm(light.color));
      gl.uniform1f(locUseVertColor, 0);
      gl.uniform1f(locRough, 0.2);
      gl.uniform1f(locMetal, 0);
      gl.uniform1f(locEmis, light.enabled === false ? 0.1 : 1.0);
      gl.uniform1f(locAlpha, 1);
      gl.uniform1f(locSel, p.selectedPath === light.path ? 1 : 0);
      gl.uniform1f(locHasTex, 0);
      gl.uniform1f(locIsWater, 0);
      gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
      drawBuffer(this.sphereMesh, gl.UNSIGNED_SHORT);
    }

    // 5. Transparent Pass (Water Planes + Transparent Parts)
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.depthMask(false);

    if (p.terrainMesh && p.terrainMesh.waterQuads.length > 0) {
      for (const wq of p.terrainMesh.waterQuads) {
        const cx = (wq.minX + wq.maxX) * 0.5;
        const cz = (wq.minZ + wq.maxZ) * 0.5;
        const sx = Math.max(1, wq.maxX - wq.minX);
        const sz = Math.max(1, wq.maxZ - wq.minZ);
        const trs = mat4ModelTRS([cx, wq.y, cz], 0, [sx, 1, sz]);
        gl.uniformMatrix4fv(locModel, false, trs.model);
        gl.uniformMatrix3fv(locNormMat, false, trs.normalMat);
        gl.uniform3fv(locBaseColor, [0.14, 0.52, 0.78]);
        gl.uniform1f(locUseVertColor, 0);
        gl.uniform1f(locRough, 0.12);
        gl.uniform1f(locMetal, 0.1);
        gl.uniform1f(locEmis, 0);
        gl.uniform1f(locAlpha, 0.58);
        gl.uniform1f(locSel, 0);
        gl.uniform1f(locHasTex, 0);
        gl.uniform1f(locIsWater, 1);
        gl.bindTexture(gl.TEXTURE_2D, this.whiteTex);
        drawBuffer(this.waterMesh, gl.UNSIGNED_SHORT);
      }
    }

    for (const item of transparentParts) drawPart(item.part);

    gl.depthMask(true);
    gl.disable(gl.BLEND);
    gl.bindVertexArray(null);
  }

  dispose() {
    this.canvas.removeEventListener("webglcontextlost", this.handleLostBound);
    this.canvas.removeEventListener("webglcontextrestored", this.handleRestoredBound);
    const gl = this.gl;
    if (this.contextLost || gl.isContextLost()) return;
    const delMesh = (m: MeshGpuBuffer | null) => {
      if (!m) return;
      gl.deleteVertexArray(m.vao);
      gl.deleteBuffer(m.vbo);
      if (m.ebo) gl.deleteBuffer(m.ebo);
    };
    delMesh(this.cubeMesh);
    delMesh(this.sphereMesh);
    delMesh(this.cylMesh);
    delMesh(this.waterMesh);
    delMesh(this.terrainGpu);
    for (const m of this.objBuffers.values()) delMesh(m);
    for (const t of this.textures.values()) gl.deleteTexture(t);
    gl.deleteTexture(this.whiteTex);
    gl.deleteVertexArray(this.skyVao);
    gl.deleteVertexArray(this.gridVao);
    gl.deleteBuffer(this.gridVbo);
    gl.deleteProgram(this.skyProg);
    gl.deleteProgram(this.litProg);
    gl.deleteProgram(this.lineProg);
  }
}

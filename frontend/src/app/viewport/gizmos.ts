import { terrainColorHexFromRgb, type TerrainMeshData } from "./terrainMesh";
import {
  hexToRgb,
  shadeHex,
  type AgentAnimState,
  type CameraState,
  type GizmoHandleHit,
  type ParsedObjGeometry,
  type Part3D,
  type StudioTool,
  type Vec2,
  type Vec3,
  type ViewCubeHit,
  type ViewportLightingInput,
  type WorkspaceLight,
} from "./types";

export const VIEWPORT_CANVAS_W = 920;
export const VIEWPORT_CANVAS_H = 580;

export function buildCameraBasis(cam: CameraState, width = VIEWPORT_CANVAS_W, height = VIEWPORT_CANVAS_H) {
  const cp = Math.cos(cam.pitch);
  const sp = Math.sin(cam.pitch);
  const cy = Math.cos(cam.yaw);
  const sy = Math.sin(cam.yaw);
  const ex = cam.tx + cam.dist * cp * sy;
  const ey = cam.ty + cam.dist * sp;
  const ez = cam.tz + cam.dist * cp * cy;

  let fx = cam.tx - ex;
  let fy = cam.ty - ey;
  let fz = cam.tz - ez;
  const fl = Math.hypot(fx, fy, fz) || 1;
  fx /= fl;
  fy /= fl;
  fz /= fl;

  let rx = -fz;
  const ry = 0;
  let rz = fx;
  const rl = Math.hypot(rx, rz) || 1;
  rx /= rl;
  rz /= rl;

  const ux = ry * fz - rz * fy;
  const uy = rz * fx - rx * fz;
  const uz = rx * fy - ry * fx;

  const focal = (height * 0.5) / Math.tan((46 * Math.PI) / 360);

  const project = (x: number, y: number, z: number): { sx: number; sy: number; depth: number; visible: boolean } => {
    const dx = x - ex;
    const dy = y - ey;
    const dz = z - ez;
    const cz = dx * fx + dy * fy + dz * fz;
    if (cz <= 0.6) return { sx: 0, sy: 0, depth: cz, visible: false };
    const cx = dx * rx + dy * ry + dz * rz;
    const cyCam = dx * ux + dy * uy + dz * uz;
    const scale = focal / cz;
    return {
      sx: width * 0.5 + cx * scale,
      sy: height * 0.5 - cyCam * scale,
      depth: cz,
      visible: true,
    };
  };

  const unprojectRay = (mx: number, my: number): { origin: Vec3; dir: Vec3 } => {
    const ndcX = (mx - width * 0.5) / focal;
    const ndcY = (height * 0.5 - my) / focal;
    let dx = fx + rx * ndcX + ux * ndcY;
    let dy = fy + ry * ndcX + uy * ndcY;
    let dz = fz + rz * ndcX + uz * ndcY;
    const dl = Math.hypot(dx, dy, dz) || 1;
    dx /= dl;
    dy /= dl;
    dz /= dl;
    return { origin: [ex, ey, ez], dir: [dx, dy, dz] };
  };

  return {
    eye: [ex, ey, ez] as Vec3,
    forward: [fx, fy, fz] as Vec3,
    right: [rx, ry, rz] as Vec3,
    up: [ux, uy, uz] as Vec3,
    focal,
    project,
    unprojectRay,
  };
}

function intersectRaySphere(origin: Vec3, dir: Vec3, center: Vec3, radius: number): number | null {
  const ocx = origin[0] - center[0];
  const ocy = origin[1] - center[1];
  const ocz = origin[2] - center[2];
  const b = ocx * dir[0] + ocy * dir[1] + ocz * dir[2];
  const c = ocx * ocx + ocy * ocy + ocz * ocz - radius * radius;
  const disc = b * b - c;
  if (disc < 0) return null;
  const t = -b - Math.sqrt(disc);
  return t > 0.2 ? t : null;
}

function intersectRayOBB(origin: Vec3, dir: Vec3, part: Part3D): number | null {
  const radY = (part.rotYDeg * Math.PI) / 180;
  const cosY = Math.cos(radY);
  const sinY = Math.sin(radY);
  const relO: Vec3 = [origin[0] - part.pos[0], origin[1] - part.pos[1], origin[2] - part.pos[2]];

  const localO: Vec3 = [
    relO[0] * cosY - relO[2] * sinY,
    relO[1],
    relO[0] * sinY + relO[2] * cosY,
  ];
  const localD: Vec3 = [
    dir[0] * cosY - dir[2] * sinY,
    dir[1],
    dir[0] * sinY + dir[2] * cosY,
  ];

  const half: Vec3 = [part.size[0] * 0.5, part.size[1] * 0.5, part.size[2] * 0.5];
  let tMin = -Infinity;
  let tMax = Infinity;

  for (let i = 0; i < 3; i++) {
    const d = localD[i];
    const o = localO[i];
    const h = half[i];
    if (Math.abs(d) < 1e-6) {
      if (o < -h || o > h) return null;
    } else {
      let t1 = (-h - o) / d;
      let t2 = (h - o) / d;
      if (t1 > t2) {
        const tmp = t1;
        t1 = t2;
        t2 = tmp;
      }
      tMin = Math.max(tMin, t1);
      tMax = Math.min(tMax, t2);
      if (tMin > tMax) return null;
    }
  }
  return tMin > 0.2 ? tMin : tMax > 0.2 ? tMax : null;
}

export function rayPickScene(
  mx: number,
  my: number,
  cam: CameraState,
  width: number,
  height: number,
  parts: Part3D[],
  lights: WorkspaceLight[],
): string | null {
  const { origin, dir } = buildCameraBasis(cam, width, height).unprojectRay(mx, my);
  let bestPath: string | null = null;
  let bestT = Infinity;

  for (const l of lights) {
    const t = intersectRaySphere(origin, dir, l.position, 1.8);
    if (t !== null && t < bestT) {
      bestT = t;
      bestPath = l.path;
    }
  }

  for (const part of parts) {
    let t: number | null = null;
    if (part.shape === "Ball") {
      t = intersectRaySphere(origin, dir, part.pos, Math.max(part.size[0], part.size[1], part.size[2]) * 0.5);
    } else {
      t = intersectRayOBB(origin, dir, part);
    }
    if (t !== null && t < bestT) {
      bestT = t;
      bestPath = part.path;
    }
  }

  return bestPath;
}

function drawTexturedTriangle(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  u0: number,
  v0: number,
  u1: number,
  v1: number,
  u2: number,
  v2: number,
) {
  const denom = u0 * (v1 - v2) - v0 * (u1 - u2) + (u1 * v2 - u2 * v1);
  if (Math.abs(denom) < 1e-4) return;
  const a = (x0 * (v1 - v2) - v0 * (x1 - x2) + (x1 * v2 - x2 * v1)) / denom;
  const b = (y0 * (v1 - v2) - v0 * (y1 - y2) + (y1 * v2 - y2 * v1)) / denom;
  const c = (u0 * (x1 - x2) - x0 * (u1 - u2) + (u1 * x2 - u2 * x1)) / denom;
  const d = (u0 * (y1 - y2) - y0 * (u1 - u2) + (u1 * y2 - u2 * y1)) / denom;
  const e = (u0 * (v1 * x2 - v2 * x1) - v0 * (u1 * x2 - u2 * x1) + x0 * (u1 * v2 - u2 * v1)) / denom;
  const f = (u0 * (v1 * y2 - v2 * y1) - v0 * (u1 * y2 - u2 * y1) + y0 * (u1 * v2 - u2 * v1)) / denom;

  ctx.save();
  ctx.beginPath();
  ctx.moveTo(x0, y0);
  ctx.lineTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.closePath();
  ctx.clip();
  ctx.transform(a, b, c, d, e, f);
  ctx.drawImage(img, 0, 0);
  ctx.restore();
}

export function drawCanvas2DFallbackScene(
  ctx: CanvasRenderingContext2D,
  opts: {
    width: number;
    height: number;
    cam: CameraState;
    theme: "light" | "dark";
    lighting: ViewportLightingInput;
    parts: Part3D[];
    lights: WorkspaceLight[];
    terrainMesh: TerrainMeshData | null;
    selectedPath?: string | null;
    anim: AgentAnimState;
    imgCache: Map<string, HTMLImageElement>;
    meshCache: Map<string, ParsedObjGeometry | "loading" | "error">;
  },
) {
  const { width, height, cam, theme, lighting, parts, lights, terrainMesh, selectedPath, anim, imgCache, meshCache } = opts;
  const isDark = theme === "dark";
  const { project, right } = buildCameraBasis(cam, width, height);

  const clockTime = typeof lighting?.clock_time === "number" && Number.isFinite(lighting.clock_time) ? lighting.clock_time : null;
  const lightBrightness = typeof lighting?.brightness === "number" && Number.isFinite(lighting.brightness) ? lighting.brightness : 2.0;
  const brightnessFactor = Math.max(0.65, Math.min(1.25, 0.72 + lightBrightness * 0.14));
  const ambientHex = typeof lighting?.ambient === "string" && /^#[0-9a-f]{6}$/i.test(lighting.ambient.trim()) ? lighting.ambient.trim() : null;
  const fogEnd = typeof lighting?.fog_end === "number" && Number.isFinite(lighting.fog_end) ? lighting.fog_end : 10000;
  const fogColor = typeof lighting?.fog_color === "string" && /^#[0-9a-f]{6}$/i.test(lighting.fog_color.trim()) ? lighting.fog_color.trim() : ambientHex;

  // 1. Sky / Atmosphere gradient
  const sky = ctx.createLinearGradient(0, 0, 0, height);
  if (clockTime !== null && (clockTime < 5.5 || clockTime >= 19.5)) {
    sky.addColorStop(0, "#0B1020");
    sky.addColorStop(0.58, "#162038");
    sky.addColorStop(1, "#141A29");
  } else if (clockTime !== null && ((clockTime >= 5.5 && clockTime < 7.5) || (clockTime >= 17 && clockTime < 19.5))) {
    sky.addColorStop(0, isDark ? "#23223A" : "#C9A8C8");
    sky.addColorStop(0.55, isDark ? "#3A2C3B" : "#F1C9A5");
    sky.addColorStop(1, isDark ? "#1C1B24" : "#ECE4D8");
  } else if (isDark) {
    sky.addColorStop(0, "#121722");
    sky.addColorStop(0.58, "#1A2233");
    sky.addColorStop(1, "#161B24");
  } else {
    sky.addColorStop(0, "#CFE3F7");
    sky.addColorStop(0.55, "#E8F0F7");
    sky.addColorStop(1, "#ECEAE2");
  }
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, width, height);
  if (ambientHex) {
    const [ar, ag, ab] = hexToRgb(ambientHex);
    ctx.fillStyle = `rgba(${ar},${ag},${ab},0.10)`;
    ctx.fillRect(0, 0, width, height);
  }

  // 2. Baseplate & Grid (shown when no procedural terrain, or subtle edge grid)
  if (!terrainMesh) {
    const baseHalf = 72;
    const bCorners = [
      project(-baseHalf, 0, -baseHalf),
      project(baseHalf, 0, -baseHalf),
      project(baseHalf, 0, baseHalf),
      project(-baseHalf, 0, baseHalf),
    ];
    if (bCorners.every((c) => c.visible)) {
      ctx.beginPath();
      ctx.moveTo(bCorners[0].sx, bCorners[0].sy);
      for (let i = 1; i < 4; i++) ctx.lineTo(bCorners[i].sx, bCorners[i].sy);
      ctx.closePath();
      ctx.fillStyle = isDark ? "#232A36" : "#DFDDD4";
      ctx.fill();
      ctx.strokeStyle = isDark ? "rgba(148,163,184,0.22)" : "rgba(60,58,52,0.18)";
      ctx.lineWidth = 1;
      ctx.stroke();
    }

    for (let g = -baseHalf; g <= baseHalf; g += 8) {
      const major = g % 24 === 0;
      ctx.strokeStyle = major
        ? isDark
          ? "rgba(148,163,184,0.24)"
          : "rgba(55,52,46,0.18)"
        : isDark
          ? "rgba(148,163,184,0.10)"
          : "rgba(55,52,46,0.08)";
      ctx.lineWidth = major ? 1.1 : 0.7;

      const a1 = project(g, 0.02, -baseHalf);
      const a2 = project(g, 0.02, baseHalf);
      if (a1.visible && a2.visible) {
        ctx.beginPath();
        ctx.moveTo(a1.sx, a1.sy);
        ctx.lineTo(a2.sx, a2.sy);
        ctx.stroke();
      }
      const b1 = project(-baseHalf, 0.02, g);
      const b2 = project(baseHalf, 0.02, g);
      if (b1.visible && b2.visible) {
        ctx.beginPath();
        ctx.moveTo(b1.sx, b1.sy);
        ctx.lineTo(b2.sx, b2.sy);
        ctx.stroke();
      }
    }
  }

  // World Origin X / Z axis rails
  const orig = project(0, 0.06, 0);
  const xEnd = project(18, 0.06, 0);
  const zEnd = project(0, 0.06, 18);
  if (orig.visible && xEnd.visible) {
    ctx.strokeStyle = "rgba(220,76,62,0.55)";
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(orig.sx, orig.sy);
    ctx.lineTo(xEnd.sx, xEnd.sy);
    ctx.stroke();
  }
  if (orig.visible && zEnd.visible) {
    ctx.strokeStyle = "rgba(56,122,232,0.55)";
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(orig.sx, orig.sy);
    ctx.lineTo(zEnd.sx, zEnd.sy);
    ctx.stroke();
  }

  const sunDir: Vec3 = [0.46, 0.81, 0.36];

  // 2b. Procedural Terrain Triangles + Water Plane in Canvas2D Fallback
  if (terrainMesh) {
    type ProjTerrainTri = { pts: [Vec2, Vec2, Vec2]; depth: number; fill: string };
    const tTris: ProjTerrainTri[] = [];
    for (const tri of terrainMesh.tris2D) {
      const p0 = project(tri.pts[0][0], tri.pts[0][1], tri.pts[0][2]);
      const p1 = project(tri.pts[1][0], tri.pts[1][1], tri.pts[1][2]);
      const p2 = project(tri.pts[2][0], tri.pts[2][1], tri.pts[2][2]);
      if (!p0.visible || !p1.visible || !p2.visible) continue;
      const ndotl = Math.max(0, tri.normal[0] * sunDir[0] + tri.normal[1] * sunDir[1] + tri.normal[2] * sunDir[2]);
      const shade = (0.52 + ndotl * 0.5) * brightnessFactor;
      tTris.push({
        pts: [[p0.sx, p0.sy], [p1.sx, p1.sy], [p2.sx, p2.sy]],
        depth: (p0.depth + p1.depth + p2.depth) / 3,
        fill: terrainColorHexFromRgb(tri.rgb, shade, tri.alpha),
      });
    }
    tTris.sort((a, b) => b.depth - a.depth);
    for (const tt of tTris) {
      ctx.beginPath();
      ctx.moveTo(tt.pts[0][0], tt.pts[0][1]);
      ctx.lineTo(tt.pts[1][0], tt.pts[1][1]);
      ctx.lineTo(tt.pts[2][0], tt.pts[2][1]);
      ctx.closePath();
      ctx.fillStyle = tt.fill;
      ctx.fill();
      ctx.strokeStyle = tt.fill;
      ctx.lineWidth = 0.6;
      ctx.stroke();
    }

    for (const wq of terrainMesh.waterQuads) {
      const wc = [
        project(wq.minX, wq.y, wq.minZ),
        project(wq.maxX, wq.y, wq.minZ),
        project(wq.maxX, wq.y, wq.maxZ),
        project(wq.minX, wq.y, wq.maxZ),
      ];
      if (wc.every((c) => c.visible)) {
        ctx.beginPath();
        ctx.moveTo(wc[0].sx, wc[0].sy);
        for (let i = 1; i < 4; i++) ctx.lineTo(wc[i].sx, wc[i].sy);
        ctx.closePath();
        ctx.fillStyle = "rgba(41, 138, 199, 0.44)";
        ctx.fill();
        ctx.strokeStyle = "rgba(120, 200, 245, 0.35)";
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }
  }

  // 2c. Dynamic Light Ground Glows in Canvas2D Fallback
  for (const light of lights) {
    if (light.enabled === false) continue;
    const lp = project(light.position[0], Math.max(0.1, light.position[1] - 1.5), light.position[2]);
    if (!lp.visible) continue;
    const rad = Math.max(18, Math.min(110, (light.range * 14) / lp.depth));
    const [lr, lg, lb] = hexToRgb(light.color);
    const alpha = Math.min(0.45, Math.max(0.08, light.brightness * 0.12));
    const grad = ctx.createRadialGradient(lp.sx, lp.sy, 2, lp.sx, lp.sy, rad);
    grad.addColorStop(0, `rgba(${lr},${lg},${lb},${alpha.toFixed(2)})`);
    grad.addColorStop(1, `rgba(${lr},${lg},${lb},0)`);
    ctx.fillStyle = grad;
    ctx.beginPath();
    ctx.arc(lp.sx, lp.sy, rad, 0, Math.PI * 2);
    ctx.fill();
  }

  // 3. Collect all 3D Part faces & sort back-to-front
  type DrawFace = {
    pts: Vec2[];
    depth: number;
    fill: string;
    stroke: string;
    lineWidth: number;
    isTopSpawn?: boolean;
    spawnCorners?: Vec2[];
    decalImg?: HTMLImageElement;
    decalCorners?: Vec2[];
    sphere?: { cx: number; cy: number; r: number; baseHex: string; alpha: number };
  };

  const faces: DrawFace[] = [];
  const meshWires: Array<{ segs: Array<[Vec2, Vec2]>; stroke: string; depth: number }> = [];

  const FACE_DEFS: Array<{ id: string; idx: [number, number, number, number]; n: Vec3 }> = [
    { id: "top", idx: [4, 5, 6, 7], n: [0, 1, 0] },
    { id: "bottom", idx: [0, 3, 2, 1], n: [0, -1, 0] },
    { id: "front", idx: [3, 2, 6, 7], n: [0, 0, 1] },
    { id: "back", idx: [1, 0, 4, 5], n: [0, 0, -1] },
    { id: "right", idx: [2, 1, 5, 6], n: [1, 0, 0] },
    { id: "left", idx: [0, 3, 7, 4], n: [-1, 0, 0] },
  ];

  for (const part of parts) {
    const popScale = anim.popPath === part.path && anim.popProgress < 0.99 ? 0.25 + 0.75 * anim.popProgress : 1;
    const hx = (part.size[0] * popScale) / 2;
    const hy = (part.size[1] * popScale) / 2;
    const hz = (part.size[2] * popScale) / 2;
    const [px, py, pz] = part.pos;
    const radY = (part.rotYDeg * Math.PI) / 180;
    const cosY = Math.cos(radY);
    const sinY = Math.sin(radY);

    const localCorners: Vec3[] = [
      [-hx, -hy, -hz],
      [hx, -hy, -hz],
      [hx, -hy, hz],
      [-hx, -hy, hz],
      [-hx, hy, -hz],
      [hx, hy, -hz],
      [hx, hy, hz],
      [-hx, hy, hz],
    ];

    const worldCorners: Vec3[] = localCorners.map(([lx, ly, lz]) => [
      px + lx * cosY + lz * sinY,
      py + ly,
      pz - lx * sinY + lz * cosY,
    ]);

    const projCorners = worldCorners.map(([wx, wy, wz]) => project(wx, wy, wz));
    if (projCorners.some((c) => !c.visible)) continue;

    // Ground contact shadow on Y = 0.05
    if (!terrainMesh) {
      const shPts = [
        project(worldCorners[0][0], 0.05, worldCorners[0][2]),
        project(worldCorners[1][0], 0.05, worldCorners[1][2]),
        project(worldCorners[2][0], 0.05, worldCorners[2][2]),
        project(worldCorners[3][0], 0.05, worldCorners[3][2]),
      ];
      if (shPts.every((s) => s.visible)) {
        faces.push({
          pts: shPts.map((s) => [s.sx, s.sy]),
          depth: (shPts[0].depth + shPts[1].depth + shPts[2].depth + shPts[3].depth) / 4 + 14,
          fill: isDark ? "rgba(0,0,0,0.34)" : "rgba(25,28,36,0.14)",
          stroke: "transparent",
          lineWidth: 0,
        });
      }
    }

    const isSelected = selectedPath === part.path;
    const isNeon = part.material === "Neon";
    const meshEntry = part.meshUrl ? meshCache.get(part.meshUrl) : undefined;
    const parsedMesh = meshEntry && typeof meshEntry === "object" ? meshEntry : null;
    const alpha = parsedMesh ? Math.min(1 - part.transparency, 0.28) : 1 - part.transparency;

    if (parsedMesh) {
      const projVerts = parsedMesh.normVerts.map(([vx, vy, vz]) => {
        const lx = vx * part.size[0] * popScale;
        const ly = vy * part.size[1] * popScale;
        const lz = vz * part.size[2] * popScale;
        return project(px + lx * cosY + lz * sinY, py + ly, pz - lx * sinY + lz * cosY);
      });
      const segs: Array<[Vec2, Vec2]> = [];
      let depthSum = 0;
      let depthN = 0;
      for (const [ia, ib] of parsedMesh.edges) {
        const va = projVerts[ia];
        const vb = projVerts[ib];
        if (va?.visible && vb?.visible) {
          segs.push([[va.sx, va.sy], [vb.sx, vb.sy]]);
          depthSum += (va.depth + vb.depth) * 0.5;
          depthN++;
        }
      }
      if (segs.length > 0) {
        meshWires.push({
          segs,
          stroke: isSelected ? "#2F80FF" : shadeHex(part.color, Math.min(1.3, 0.9 * brightnessFactor), 0.88),
          depth: depthN > 0 ? depthSum / depthN : 999,
        });
      }
    }

    const decalImg = part.decalUrl ? imgCache.get(part.decalUrl) : undefined;
    const decalReady = Boolean(decalImg && decalImg.complete && decalImg.naturalWidth > 0);

    if (part.shape === "Ball") {
      const ctr = project(px, py, pz);
      const edge = project(
        px + right[0] * Math.max(hx, hy, hz),
        py + right[1] * Math.max(hx, hy, hz),
        pz + right[2] * Math.max(hx, hy, hz),
      );
      if (ctr.visible && edge.visible) {
        const sr = Math.max(6, Math.hypot(edge.sx - ctr.sx, edge.sy - ctr.sy));
        const ring: Vec2[] = [];
        for (let k = 0; k < 10; k++) {
          const ang = (k / 10) * Math.PI * 2;
          ring.push([ctr.sx + Math.cos(ang) * sr, ctr.sy + Math.sin(ang) * sr]);
        }
        faces.push({
          pts: ring,
          depth: ctr.depth,
          fill: shadeHex(part.color, 1, alpha),
          stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.22)" : "rgba(20,24,32,0.22)",
          lineWidth: isSelected ? 2 : 1,
          sphere: { cx: ctr.sx, cy: ctr.sy, r: sr, baseHex: part.color, alpha },
        });
      }
      continue;
    }

    if (part.shape === "Cylinder") {
      const segs = 10;
      const bot: Array<ReturnType<typeof project>> = [];
      const top: Array<ReturnType<typeof project>> = [];
      for (let k = 0; k < segs; k++) {
        const a = (k / segs) * Math.PI * 2 + radY;
        const ox = Math.cos(a) * hx;
        const oz = Math.sin(a) * hz;
        bot.push(project(px + ox, py - hy, pz + oz));
        top.push(project(px + ox, py + hy, pz + oz));
      }
      if (bot.every((c) => c.visible) && top.every((c) => c.visible)) {
        const topPts: Vec2[] = top.map((c) => [c.sx, c.sy]);
        const topDepth = top.reduce((acc, c) => acc + c.depth, 0) / segs;
        faces.push({
          pts: topPts,
          depth: topDepth,
          fill: shadeHex(part.color, isNeon ? 1.15 : 0.98, alpha),
          stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.18)" : "rgba(20,24,32,0.20)",
          lineWidth: isSelected ? 1.8 : 0.85,
        });
        for (let k = 0; k < segs; k++) {
          const nk = (k + 1) % segs;
          const midA = ((k + 0.5) / segs) * Math.PI * 2 + radY;
          const nx = Math.cos(midA);
          const nz = Math.sin(midA);
          const ndotl = Math.max(0, nx * sunDir[0] + nz * sunDir[2]);
          const shade = isNeon ? 0.94 + ndotl * 0.22 : 0.55 + ndotl * 0.45;
          const pts: Vec2[] = [
            [bot[k].sx, bot[k].sy],
            [bot[nk].sx, bot[nk].sy],
            [top[nk].sx, top[nk].sy],
            [top[k].sx, top[k].sy],
          ];
          const dAvg = (bot[k].depth + bot[nk].depth + top[nk].depth + top[k].depth) / 4;
          faces.push({
            pts,
            depth: dAvg,
            fill: shadeHex(part.color, shade, alpha),
            stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.12)" : "rgba(20,24,32,0.14)",
            lineWidth: isSelected ? 1.4 : 0.7,
          });
        }
        continue;
      }
    }

    for (const fd of FACE_DEFS) {
      const [i0, i1, i2, i3] = fd.idx;
      const p0 = projCorners[i0];
      const p1 = projCorners[i1];
      const p2 = projCorners[i2];
      const p3 = projCorners[i3];

      const cross = (p1.sx - p0.sx) * (p2.sy - p0.sy) - (p1.sy - p0.sy) * (p2.sx - p0.sx);
      if (cross > 0 && part.transparency < 0.05) continue;

      const nx = fd.n[0] * cosY + fd.n[2] * sinY;
      const ny = fd.n[1];
      const nz = -fd.n[0] * sinY + fd.n[2] * cosY;
      const ndotl = Math.max(0, nx * sunDir[0] + ny * sunDir[1] + nz * sunDir[2]);
      const shade = isNeon ? 0.96 + ndotl * 0.22 : (0.58 + ndotl * 0.46) * brightnessFactor;

      const pts: Vec2[] = [
        [p0.sx, p0.sy],
        [p1.sx, p1.sy],
        [p2.sx, p2.sy],
        [p3.sx, p3.sy],
      ];
      const depth = (p0.depth + p1.depth + p2.depth + p3.depth) / 4;
      const showDecal = decalReady && (fd.id === "front" || fd.id === "back");
      const decalCorners: Vec2[] | undefined = showDecal
        ? fd.id === "front"
          ? [
              [projCorners[7].sx, projCorners[7].sy],
              [projCorners[6].sx, projCorners[6].sy],
              [projCorners[2].sx, projCorners[2].sy],
              [projCorners[3].sx, projCorners[3].sy],
            ]
          : [
              [projCorners[5].sx, projCorners[5].sy],
              [projCorners[4].sx, projCorners[4].sy],
              [projCorners[0].sx, projCorners[0].sy],
              [projCorners[1].sx, projCorners[1].sy],
            ]
        : undefined;

      faces.push({
        pts,
        depth,
        fill: shadeHex(part.color, shade, alpha),
        stroke: isSelected
          ? "#2F80FF"
          : isNeon
            ? shadeHex(part.color, 1.35, 0.92)
            : isDark
              ? "rgba(255,255,255,0.16)"
              : "rgba(20,24,32,0.20)",
        lineWidth: isSelected ? 1.8 : isNeon ? 1.4 : 0.85,
        isTopSpawn: fd.id === "top" && (part.className === "SpawnLocation" || /spawn/i.test(part.name)),
        spawnCorners: fd.id === "top" ? pts : undefined,
        decalImg: showDecal ? decalImg : undefined,
        decalCorners,
      });
    }
  }

  faces.sort((a, b) => b.depth - a.depth);

  for (const f of faces) {
    if (f.sphere) {
      const { cx, cy, r, baseHex, alpha } = f.sphere;
      const grad = ctx.createRadialGradient(cx - r * 0.32, cy - r * 0.34, Math.max(1, r * 0.12), cx, cy, r);
      grad.addColorStop(0, shadeHex(baseHex, 1.38, alpha));
      grad.addColorStop(0.55, shadeHex(baseHex, 0.96, alpha));
      grad.addColorStop(1, shadeHex(baseHex, 0.52, alpha));
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
      if (f.stroke !== "transparent") {
        ctx.strokeStyle = f.stroke;
        ctx.lineWidth = f.lineWidth;
        ctx.stroke();
      }
      continue;
    }

    ctx.beginPath();
    ctx.moveTo(f.pts[0][0], f.pts[0][1]);
    for (let i = 1; i < f.pts.length; i++) ctx.lineTo(f.pts[i][0], f.pts[i][1]);
    ctx.closePath();
    ctx.fillStyle = f.fill;
    ctx.fill();

    if (f.decalImg && f.decalCorners) {
      const [tl, tr, br, bl] = f.decalCorners;
      const iw = f.decalImg.naturalWidth;
      const ih = f.decalImg.naturalHeight;
      if (iw > 0 && ih > 0) {
        try {
          drawTexturedTriangle(ctx, f.decalImg, tl[0], tl[1], tr[0], tr[1], br[0], br[1], 0, 0, iw, 0, iw, ih);
          drawTexturedTriangle(ctx, f.decalImg, tl[0], tl[1], br[0], br[1], bl[0], bl[1], 0, 0, iw, ih, 0, ih);
        } catch {
          /* ignore broken decal texture frame */
        }
      }
    }

    if (f.isTopSpawn && f.spawnCorners) {
      const [c0, c1, c2, c3] = f.spawnCorners;
      const cx = (c0[0] + c1[0] + c2[0] + c3[0]) / 4;
      const cy = (c0[1] + c1[1] + c2[1] + c3[1]) / 4;
      const rx = Math.hypot(c1[0] - c0[0], c1[1] - c0[1]) * 0.24;
      const ry = Math.hypot(c3[0] - c0[0], c3[1] - c0[1]) * 0.24;
      ctx.save();
      ctx.strokeStyle = "rgba(255,255,255,0.9)";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.ellipse(cx, cy, Math.max(4, rx), Math.max(2.5, ry * 0.7), 0, 0, Math.PI * 2);
      ctx.stroke();
      ctx.fillStyle = "rgba(255,255,255,0.9)";
      ctx.beginPath();
      ctx.arc(cx, cy, Math.max(2, Math.min(4, rx * 0.3)), 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    if (f.stroke !== "transparent") {
      ctx.strokeStyle = f.stroke;
      ctx.lineWidth = f.lineWidth;
      ctx.stroke();
    }

    if (fogEnd < 600 && fogColor && f.depth > 12) {
      const fogAmt = Math.min(0.55, Math.max(0, (f.depth - 12) / Math.max(24, fogEnd)));
      if (fogAmt > 0.02) {
        const [fr, fg, fb] = hexToRgb(fogColor);
        ctx.fillStyle = `rgba(${fr},${fg},${fb},${fogAmt.toFixed(2)})`;
        ctx.fill();
      }
    }
  }

  if (meshWires.length > 0) {
    meshWires.sort((a, b) => b.depth - a.depth);
    ctx.save();
    ctx.lineWidth = 1.1;
    for (const mw of meshWires) {
      ctx.strokeStyle = mw.stroke;
      ctx.beginPath();
      for (const [a, b] of mw.segs) {
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
      }
      ctx.stroke();
    }
    ctx.restore();
  }
}

export function drawViewportHud(
  ctx: CanvasRenderingContext2D,
  opts: {
    clear?: boolean;
    width: number;
    height: number;
    cam: CameraState;
    theme: "light" | "dark";
    parts: Part3D[];
    lights: WorkspaceLight[];
    selectedPath?: string | null;
    selectedPart: Part3D | null;
    tool: StudioTool;
    building?: boolean;
    replaying: boolean;
    reduce: boolean;
    hasTerrain: boolean;
    anim: AgentAnimState;
  },
): { gizmoHits: GizmoHandleHit[]; viewCubeHits: ViewCubeHit[] } {
  const { clear, width, height, cam, theme, parts, lights, selectedPath, selectedPart, tool, building, replaying, reduce, hasTerrain, anim } = opts;
  if (clear) ctx.clearRect(0, 0, width, height);
  const isDark = theme === "dark";
  const { project, right, up } = buildCameraBasis(cam, width, height);

  // 1. Light markers (PointLight / SpotLight / SurfaceLight)
  for (const l of lights) {
    const lp = project(l.position[0], l.position[1], l.position[2]);
    if (!lp.visible) continue;
    const isSel = selectedPath === l.path;
    const [lr, lg, lb] = hexToRgb(l.color);
    const fillCol = l.enabled === false ? "rgba(148,163,184,0.55)" : `rgb(${lr},${lg},${lb})`;

    if (l.kind !== "PointLight" && l.direction) {
      const dir = l.direction;
      const dl = Math.hypot(dir[0], dir[1], dir[2]) || 1;
      const endP = project(
        l.position[0] + (dir[0] / dl) * 5,
        l.position[1] + (dir[1] / dl) * 5,
        l.position[2] + (dir[2] / dl) * 5,
      );
      if (endP.visible) {
        ctx.strokeStyle = isSel ? "#2F80FF" : `rgba(${lr},${lg},${lb},0.85)`;
        ctx.lineWidth = isSel ? 2 : 1.4;
        ctx.beginPath();
        ctx.moveTo(lp.sx, lp.sy);
        ctx.lineTo(endP.sx, endP.sy);
        ctx.stroke();
      }
    }

    ctx.beginPath();
    ctx.arc(lp.sx, lp.sy, isSel ? 7 : 5.5, 0, Math.PI * 2);
    ctx.fillStyle = fillCol;
    ctx.fill();
    ctx.lineWidth = isSel ? 2.2 : 1.2;
    ctx.strokeStyle = isSel ? "#2F80FF" : isDark ? "#FFFFFF" : "#1E293B";
    ctx.stroke();

    if (isSel) {
      const ringEdge = project(l.position[0] + l.range, l.position[1], l.position[2]);
      if (ringEdge.visible) {
        const rr = Math.max(12, Math.min(180, Math.hypot(ringEdge.sx - lp.sx, ringEdge.sy - lp.sy)));
        ctx.save();
        ctx.setLineDash([4, 4]);
        ctx.strokeStyle = `rgba(${lr},${lg},${lb},0.55)`;
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.arc(lp.sx, lp.sy, rr, 0, Math.PI * 2);
        ctx.stroke();
        ctx.restore();
      }
    }
  }

  // 2. Floating Part Name Labels + ParticleEmitter/Sound chips
  ctx.font = '500 10.5px "Geist", system-ui, sans-serif';
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (const part of parts) {
    const topC = project(part.pos[0], part.pos[1] + part.size[1] * 0.5 + 1.2, part.pos[2]);
    if (!topC.visible) continue;
    const isSel = part.path === selectedPath;
    if (!isSel && /billboard|showcase/i.test(part.name)) continue;
    if (
      !isSel &&
      parts.length > 10 &&
      !/spawn|pedestal|tower|shrine|alchemist|lava/i.test(part.name) &&
      !part.hasParticleEmitter &&
      !part.hasSound
    ) {
      continue;
    }
    const suffix = part.hasParticleEmitter ? " · FX" : part.hasSound ? " · SFX" : "";
    const lbl = (part.cardTitle || part.name) + suffix;
    const tw = ctx.measureText(lbl).width + 10;
    const bx = topC.sx - tw / 2;
    const by = topC.sy - 14;
    ctx.fillStyle = isSel
      ? "#2563EB"
      : isDark
        ? "rgba(20,24,34,0.82)"
        : "rgba(255,255,255,0.86)";
    ctx.strokeStyle = isSel ? "#60A5FA" : isDark ? "rgba(255,255,255,0.18)" : "rgba(30,34,42,0.16)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(bx, by, tw, 17, 5);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = isSel ? "#FFFFFF" : isDark ? "#EAECEF" : "#1D2026";
    ctx.fillText(lbl, topC.sx, by + 8.5);
  }

  // 3. Selected Part Wireframe Highlight + Transform Gizmos
  const gizmoHits: GizmoHandleHit[] = [];
  if (selectedPart) {
    const [px, py, pz] = selectedPart.pos;
    const hx = selectedPart.size[0] * 0.52;
    const hy = selectedPart.size[1] * 0.52;
    const hz = selectedPart.size[2] * 0.52;
    const radY = (selectedPart.rotYDeg * Math.PI) / 180;
    const cosY = Math.cos(radY);
    const sinY = Math.sin(radY);
    const boxPts = [
      [-hx, -hy, -hz],
      [hx, -hy, -hz],
      [hx, -hy, hz],
      [-hx, -hy, hz],
      [-hx, hy, -hz],
      [hx, hy, -hz],
      [hx, hy, hz],
      [-hx, hy, hz],
    ].map(([lx, ly, lz]) => project(px + lx * cosY + lz * sinY, py + ly, pz - lx * sinY + lz * cosY));

    if (boxPts.every((p) => p.visible)) {
      const boxEdges: Array<[number, number]> = [
        [0, 1], [1, 2], [2, 3], [3, 0],
        [4, 5], [5, 6], [6, 7], [7, 4],
        [0, 4], [1, 5], [2, 6], [3, 7],
      ];
      ctx.strokeStyle = "rgba(47,128,255,0.88)";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      for (const [ia, ib] of boxEdges) {
        ctx.moveTo(boxPts[ia].sx, boxPts[ia].sy);
        ctx.lineTo(boxPts[ib].sx, boxPts[ib].sy);
      }
      ctx.stroke();
    }

    const origin = project(px, py, pz);
    if (origin.visible) {
      const handleLen = Math.max(7, Math.min(15, Math.max(...selectedPart.size) * 0.9));
      if (tool === "move" || tool === "scale") {
        const axes: Array<{ axis: "x" | "y" | "z"; end: Vec3; color: string }> = [
          { axis: "x", end: [px + handleLen, py, pz], color: "#E5484D" },
          { axis: "y", end: [px, py + handleLen, pz], color: "#30A46C" },
          { axis: "z", end: [px, py, pz + handleLen], color: "#3B82F6" },
        ];
        for (const ax of axes) {
          const ep = project(ax.end[0], ax.end[1], ax.end[2]);
          if (!ep.visible) continue;
          ctx.strokeStyle = ax.color;
          ctx.lineWidth = 2.6;
          ctx.beginPath();
          ctx.moveTo(origin.sx, origin.sy);
          ctx.lineTo(ep.sx, ep.sy);
          ctx.stroke();

          const dx = ep.sx - origin.sx;
          const dy = ep.sy - origin.sy;
          const len = Math.hypot(dx, dy) || 1;
          const ux2 = dx / len;
          const uy2 = dy / len;

          ctx.fillStyle = ax.color;
          if (tool === "move") {
            ctx.beginPath();
            ctx.moveTo(ep.sx + ux2 * 6, ep.sy + uy2 * 6);
            ctx.lineTo(ep.sx - ux2 * 5 - uy2 * 4.5, ep.sy - uy2 * 5 + ux2 * 4.5);
            ctx.lineTo(ep.sx - ux2 * 5 + uy2 * 4.5, ep.sy - uy2 * 5 - ux2 * 4.5);
            ctx.closePath();
            ctx.fill();
          } else {
            ctx.fillRect(ep.sx - 4.5, ep.sy - 4.5, 9, 9);
          }
          gizmoHits.push({ axis: ax.axis, x: ep.sx, y: ep.sy, dir2d: [ux2, uy2] });
        }
      } else if (tool === "rotate") {
        const ringR = Math.max(6, Math.max(selectedPart.size[0], selectedPart.size[2]) * 0.72);
        ctx.strokeStyle = "#30A46C";
        ctx.lineWidth = 2.2;
        ctx.beginPath();
        let started = false;
        for (let a = 0; a <= 36; a++) {
          const ang = (a / 36) * Math.PI * 2;
          const pt = project(px + Math.cos(ang) * ringR, py, pz + Math.sin(ang) * ringR);
          if (!pt.visible) continue;
          if (!started) {
            ctx.moveTo(pt.sx, pt.sy);
            started = true;
          } else {
            ctx.lineTo(pt.sx, pt.sy);
          }
        }
        ctx.stroke();
        const handlePt = project(px + ringR, py, pz);
        if (handlePt.visible) {
          ctx.fillStyle = "#30A46C";
          ctx.beginPath();
          ctx.arc(handlePt.sx, handlePt.sy, 6, 0, Math.PI * 2);
          ctx.fill();
          gizmoHits.push({ axis: "y", x: handlePt.sx, y: handlePt.sy, dir2d: [1, 0] });
        }
      }
    }
  }

  // 4. Live Agent Builder Cursor + Laser Beam + Floating Callout
  const now = performance.now();
  const showAgent = building || replaying || anim.beam > 0.04 || now < anim.calloutUntil;
  if (showAgent) {
    const hoverY = anim.y + (reduce ? 0 : Math.sin(anim.time * 4.2) * 0.65);
    const drone = project(anim.x, hoverY, anim.z);
    const groundTarget = project(anim.tx, Math.max(0.5, anim.ty - 8.5), anim.tz);

    if (drone.visible && groundTarget.visible) {
      const beamAlpha = Math.max(0.18, anim.beam * 0.72);
      const beamGrad = ctx.createLinearGradient(drone.sx, drone.sy, groundTarget.sx, groundTarget.sy);
      beamGrad.addColorStop(0, `rgba(217, 107, 67, ${beamAlpha.toFixed(2)})`);
      beamGrad.addColorStop(1, `rgba(56, 139, 253, ${(beamAlpha * 0.35).toFixed(2)})`);
      ctx.fillStyle = beamGrad;
      ctx.beginPath();
      ctx.moveTo(drone.sx - 2, drone.sy + 5);
      ctx.lineTo(drone.sx + 2, drone.sy + 5);
      ctx.lineTo(groundTarget.sx + 16, groundTarget.sy);
      ctx.lineTo(groundTarget.sx - 16, groundTarget.sy);
      ctx.closePath();
      ctx.fill();

      ctx.strokeStyle = `rgba(217, 107, 67, ${beamAlpha.toFixed(2)})`;
      ctx.lineWidth = 1.8;
      ctx.beginPath();
      ctx.ellipse(groundTarget.sx, groundTarget.sy, 16 + (1 - anim.beam) * 14, 7 + (1 - anim.beam) * 6, 0, 0, Math.PI * 2);
      ctx.stroke();

      ctx.fillStyle = "#D96B43";
      ctx.strokeStyle = "#FFFFFF";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(drone.sx, drone.sy - 8);
      ctx.lineTo(drone.sx + 7, drone.sy);
      ctx.lineTo(drone.sx, drone.sy + 8);
      ctx.lineTo(drone.sx - 7, drone.sy);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();

      const calloutText = anim.callout || (building ? "Agent building in Studio…" : "");
      if (calloutText) {
        ctx.font = '600 11px "Geist", system-ui, sans-serif';
        const tw = Math.min(340, ctx.measureText(calloutText).width + 18);
        const cx = Math.max(tw / 2 + 8, Math.min(width - tw / 2 - 8, drone.sx));
        const cy = Math.max(24, drone.sy - 26);
        ctx.fillStyle = isDark ? "#1B2230" : "#181C24";
        ctx.strokeStyle = "#D96B43";
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.roundRect(cx - tw / 2, cy - 11, tw, 22, 6);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = "#FFFFFF";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(calloutText.slice(0, 44), cx, cy);
      }
    }
  }

  // 5. Interactive 3D Axis ViewCube (X / Y / Z)
  const vcX = width - 44;
  const vcY = 44;
  const vcR = 24;
  ctx.fillStyle = isDark ? "rgba(22,27,36,0.84)" : "rgba(255,255,255,0.86)";
  ctx.strokeStyle = isDark ? "rgba(255,255,255,0.16)" : "rgba(30,34,42,0.16)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(vcX, vcY, vcR + 6, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();

  const triad: Array<{ axis: "x" | "y" | "z"; lbl: string; v: Vec3; color: string }> = [
    { axis: "x", lbl: "X", v: [1, 0, 0], color: "#E5484D" },
    { axis: "y", lbl: "Y", v: [0, 1, 0], color: "#30A46C" },
    { axis: "z", lbl: "Z", v: [0, 0, 1], color: "#3B82F6" },
  ];
  const viewCubeHits: ViewCubeHit[] = [];
  ctx.font = '600 9.5px "Geist Mono", monospace';
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (const t of triad) {
    const sx = t.v[0] * right[0] + t.v[1] * right[1] + t.v[2] * right[2];
    const sy = -(t.v[0] * up[0] + t.v[1] * up[1] + t.v[2] * up[2]);
    const ex = vcX + sx * vcR;
    const ey = vcY + sy * vcR;
    ctx.strokeStyle = t.color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(vcX, vcY);
    ctx.lineTo(ex, ey);
    ctx.stroke();
    ctx.fillStyle = t.color;
    ctx.beginPath();
    ctx.arc(ex, ey, 6.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "#FFFFFF";
    ctx.fillText(t.lbl, ex, ey + 0.5);
    viewCubeHits.push({ axis: t.axis, x: ex, y: ey });
  }

  // 6. Empty baseplate helper hint
  if (parts.length === 0 && !building && !hasTerrain) {
    ctx.font = '500 12px "Geist", system-ui, sans-serif';
    ctx.fillStyle = isDark ? "rgba(232,236,244,0.75)" : "rgba(30,34,42,0.72)";
    ctx.textAlign = "center";
    ctx.fillText("Roblox Studio 3D Baseplate ready: run a blueprint or ask OpenRO to watch parts build live.", width / 2, height - 28);
  }

  return { gizmoHits, viewCubeHits };
}

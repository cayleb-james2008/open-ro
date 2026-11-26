import type { TerrainGeneratorConfig, TerrainRegion, Vec3, WorkspaceTerrain } from "./types";

export type TerrainMaterialSpec = {
  rgb: Vec3;
  hex: string;
  roughness: number;
  metalness: number;
  emissive: number;
};

export const TERRAIN_MATERIALS: Record<string, TerrainMaterialSpec> = {
  Grass: { rgb: [0.29, 0.57, 0.28], hex: "#4A9147", roughness: 0.88, metalness: 0.0, emissive: 0 },
  LeafyGrass: { rgb: [0.23, 0.5, 0.24], hex: "#3B803D", roughness: 0.9, metalness: 0.0, emissive: 0 },
  Ground: { rgb: [0.43, 0.32, 0.23], hex: "#6E523B", roughness: 0.92, metalness: 0.0, emissive: 0 },
  Mud: { rgb: [0.35, 0.25, 0.18], hex: "#59402E", roughness: 0.86, metalness: 0.0, emissive: 0 },
  Sand: { rgb: [0.84, 0.76, 0.54], hex: "#D6C28A", roughness: 0.88, metalness: 0.0, emissive: 0 },
  Rock: { rgb: [0.46, 0.48, 0.51], hex: "#757A82", roughness: 0.8, metalness: 0.02, emissive: 0 },
  Slate: { rgb: [0.39, 0.42, 0.46], hex: "#636B75", roughness: 0.78, metalness: 0.02, emissive: 0 },
  Basalt: { rgb: [0.22, 0.24, 0.27], hex: "#383D45", roughness: 0.82, metalness: 0.04, emissive: 0 },
  Snow: { rgb: [0.9, 0.94, 0.98], hex: "#E6F0FA", roughness: 0.38, metalness: 0.0, emissive: 0 },
  Ice: { rgb: [0.68, 0.86, 0.96], hex: "#ADDBF5", roughness: 0.18, metalness: 0.05, emissive: 0 },
  Glacier: { rgb: [0.76, 0.9, 0.97], hex: "#C2E6F7", roughness: 0.25, metalness: 0.04, emissive: 0 },
  Water: { rgb: [0.16, 0.54, 0.78], hex: "#298AC7", roughness: 0.12, metalness: 0.08, emissive: 0 },
  CrackedLava: { rgb: [0.92, 0.32, 0.1], hex: "#EB521A", roughness: 0.42, metalness: 0.0, emissive: 0.75 },
  Lava: { rgb: [0.94, 0.34, 0.11], hex: "#F0571C", roughness: 0.35, metalness: 0.0, emissive: 0.85 },
  WoodPlanks: { rgb: [0.56, 0.38, 0.22], hex: "#8F6138", roughness: 0.74, metalness: 0.0, emissive: 0 },
  Brick: { rgb: [0.65, 0.32, 0.26], hex: "#A65242", roughness: 0.82, metalness: 0.0, emissive: 0 },
};

export function getTerrainMaterial(name: string): TerrainMaterialSpec {
  if (TERRAIN_MATERIALS[name]) return TERRAIN_MATERIALS[name];
  const key = Object.keys(TERRAIN_MATERIALS).find((k) => k.toLowerCase() === name.toLowerCase());
  return key ? TERRAIN_MATERIALS[key] : TERRAIN_MATERIALS.Grass;
}

export type TerrainWaterQuad = {
  minX: number;
  maxX: number;
  minZ: number;
  maxZ: number;
  y: number;
};

export type TerrainTri2D = {
  pts: [Vec3, Vec3, Vec3];
  normal: Vec3;
  rgb: Vec3;
  alpha: number;
};

export type TerrainMeshData = {
  positions: Float32Array;
  normals: Float32Array;
  colors: Float32Array;
  materialAttrs: Float32Array;
  indices: Uint32Array;
  waterQuads: TerrainWaterQuad[];
  tris2D: TerrainTri2D[];
  bounds: { minX: number; maxX: number; minZ: number; maxZ: number; minY: number; maxY: number };
};

function hash2D(ix: number, iz: number, seed: number): number {
  let h = Math.imul(ix, 374761393) ^ Math.imul(iz, 668265263) ^ Math.imul(seed | 0, 1274126177);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h = h ^ (h >>> 16);
  return (h >>> 0) / 4294967295;
}

function valueNoise2D(x: number, z: number, seed: number): number {
  const ix = Math.floor(x);
  const iz = Math.floor(z);
  const fx = x - ix;
  const fz = z - iz;
  const ux = fx * fx * (3 - 2 * fx);
  const uz = fz * fz * (3 - 2 * fz);
  const a = hash2D(ix, iz, seed);
  const b = hash2D(ix + 1, iz, seed);
  const c = hash2D(ix, iz + 1, seed);
  const d = hash2D(ix + 1, iz + 1, seed);
  return a + (b - a) * ux + (c - a) * uz + (a - b - c + d) * ux * uz;
}

function fbm2D(x: number, z: number, seed: number, octaves = 3): number {
  let sum = 0;
  let amp = 0.55;
  let freq = 1;
  let norm = 0;
  for (let i = 0; i < octaves; i++) {
    sum += valueNoise2D(x * freq, z * freq, seed + i * 101) * amp;
    norm += amp;
    amp *= 0.5;
    freq *= 2.05;
  }
  return norm > 0 ? sum / norm : 0.5;
}

export function sampleGeneratorHeight(
  x: number,
  z: number,
  minB: Vec3,
  maxB: Vec3,
  gen: TerrainGeneratorConfig,
): number {
  const baseY = minB[1] <= 0 && maxB[1] >= 0 ? 0 : minB[1];
  const amp = Math.max(0, Math.min(256, Number.isFinite(gen.amplitude) ? gen.amplitude : 18));
  const seed = Number.isFinite(gen.seed) ? gen.seed : 42;
  const cx = (minB[0] + maxB[0]) * 0.5;
  const cz = (minB[2] + maxB[2]) * 0.5;
  const halfX = Math.max(8, (maxB[0] - minB[0]) * 0.5);
  const halfZ = Math.max(8, (maxB[2] - minB[2]) * 0.5);

  let y = baseY;
  if (gen.kind === "flat") {
    y = baseY;
  } else if (gen.kind === "hills") {
    const n = fbm2D(x * 0.025, z * 0.025, seed, 3);
    const ridge = Math.sin(x * 0.03 + seed * 0.1) * Math.cos(z * 0.028) * 0.18;
    y = baseY + (n - 0.35 + ridge) * amp;
  } else if (gen.kind === "island") {
    const nx = (x - cx) / halfX;
    const nz = (z - cz) / halfZ;
    const radial = Math.hypot(nx, nz);
    const falloff = Math.max(0, 1 - radial * radial * 1.15);
    const n = fbm2D(x * 0.03, z * 0.03, seed, 3);
    const floorY = Math.min(baseY - 5, (gen.water_level ?? -2) - 4);
    y = floorY + falloff * (amp + 5) * (0.65 + n * 0.55);
  } else if (gen.kind === "canyon") {
    const meander = Math.sin(x * 0.032 + seed * 0.17) * halfZ * 0.28;
    const distFromChannel = Math.abs(z - cz - meander);
    const channelHalf = Math.max(10, halfZ * 0.22);
    const t = Math.max(0, Math.min(1, (distFromChannel - channelHalf * 0.45) / channelHalf));
    const shelf = t * t * (3 - 2 * t);
    const n = fbm2D(x * 0.035, z * 0.035, seed, 3);
    const floorY = Math.min(baseY - 4, (gen.water_level ?? -3) - 2);
    y = floorY + shelf * (amp + 4) * (0.78 + n * 0.36);
  }

  return Math.max(minB[1], Math.min(maxB[1], y));
}

function rgbToHex(rgb: Vec3): string {
  const toByte = (v: number) => Math.max(0, Math.min(255, Math.round(v * 255))).toString(16).padStart(2, "0");
  return `#${toByte(rgb[0])}${toByte(rgb[1])}${toByte(rgb[2])}`;
}

function appendBoxMesh(
  boxMin: Vec3,
  boxMax: Vec3,
  mat: TerrainMaterialSpec,
  posOut: number[],
  normOut: number[],
  colOut: number[],
  matOut: number[],
  idxOut: number[],
  tris2D: TerrainTri2D[],
) {
  const [x0, y0, z0] = boxMin;
  const [x1, y1, z1] = boxMax;
  const corners: Vec3[] = [
    [x0, y0, z0],
    [x1, y0, z0],
    [x1, y0, z1],
    [x0, y0, z1],
    [x0, y1, z0],
    [x1, y1, z0],
    [x1, y1, z1],
    [x0, y1, z1],
  ];
  const faces: Array<{ idx: [number, number, number, number]; n: Vec3 }> = [
    { idx: [4, 7, 6, 5], n: [0, 1, 0] },
    { idx: [0, 1, 2, 3], n: [0, -1, 0] },
    { idx: [3, 2, 6, 7], n: [0, 0, 1] },
    { idx: [1, 0, 4, 5], n: [0, 0, -1] },
    { idx: [2, 1, 5, 6], n: [1, 0, 0] },
    { idx: [0, 3, 7, 4], n: [-1, 0, 0] },
  ];
  const alpha = mat === TERRAIN_MATERIALS.Water ? 0.65 : 1;
  for (const f of faces) {
    const base = posOut.length / 3;
    for (const ci of f.idx) {
      const c = corners[ci];
      posOut.push(c[0], c[1], c[2]);
      normOut.push(f.n[0], f.n[1], f.n[2]);
      colOut.push(mat.rgb[0], mat.rgb[1], mat.rgb[2]);
      matOut.push(mat.roughness, mat.metalness, mat.emissive);
    }
    idxOut.push(base, base + 1, base + 2, base, base + 2, base + 3);
    if (f.n[1] >= 0) {
      tris2D.push({
        pts: [corners[f.idx[0]], corners[f.idx[1]], corners[f.idx[2]]],
        normal: f.n,
        rgb: mat.rgb,
        alpha,
      });
      tris2D.push({
        pts: [corners[f.idx[0]], corners[f.idx[2]], corners[f.idx[3]]],
        normal: f.n,
        rgb: mat.rgb,
        alpha,
      });
    }
  }
}

function hasValidBounds(minB: Vec3 | undefined, maxB: Vec3 | undefined): boolean {
  if (!Array.isArray(minB) || !Array.isArray(maxB) || minB.length < 3 || maxB.length < 3) return false;
  for (let i = 0; i < 3; i++) {
    if (!Number.isFinite(minB[i]) || !Number.isFinite(maxB[i]) || minB[i] >= maxB[i]) return false;
  }
  return true;
}

export function buildTerrainMeshData(terrain: WorkspaceTerrain | null | undefined): TerrainMeshData | null {
  if (!terrain || !terrain.enabled || !Array.isArray(terrain.regions) || terrain.regions.length === 0) {
    return null;
  }

  const validRegions = terrain.regions.filter((r) => r && hasValidBounds(r.bounds_min, r.bounds_max));
  if (validRegions.length === 0) return null;

  const generatorRegions = validRegions.filter((r) => Boolean(r.generator));
  const paintRegions = validRegions.filter((r) => !r.generator);

  const posOut: number[] = [];
  const normOut: number[] = [];
  const colOut: number[] = [];
  const matOut: number[] = [];
  const idxOut: number[] = [];
  const waterQuads: TerrainWaterQuad[] = [];
  const tris2D: TerrainTri2D[] = [];

  let bMinX = Infinity, bMaxX = -Infinity;
  let bMinZ = Infinity, bMaxZ = -Infinity;
  let bMinY = Infinity, bMaxY = -Infinity;

  const trackBounds = (x: number, y: number, z: number) => {
    if (x < bMinX) bMinX = x; if (x > bMaxX) bMaxX = x;
    if (y < bMinY) bMinY = y; if (y > bMaxY) bMaxY = y;
    if (z < bMinZ) bMinZ = z; if (z > bMaxZ) bMaxZ = z;
  };

  const findPaintOverride = (x: number, y: number, z: number): TerrainRegion | null => {
    for (let i = paintRegions.length - 1; i >= 0; i--) {
      const pr = paintRegions[i];
      if (
        x >= pr.bounds_min[0] &&
        x <= pr.bounds_max[0] &&
        z >= pr.bounds_min[2] &&
        z <= pr.bounds_max[2] &&
        y >= pr.bounds_min[1] - 6 &&
        y <= pr.bounds_max[1] + 6
      ) {
        return pr;
      }
    }
    return null;
  };

  const cellBase = Math.max(2, Number.isFinite(terrain.cell_size) ? terrain.cell_size : 4);

  for (const reg of generatorRegions) {
    const gen = reg.generator!;
    const minB = reg.bounds_min;
    const maxB = reg.bounds_max;
    const spanX = Math.max(4, maxB[0] - minB[0]);
    const spanZ = Math.max(4, maxB[2] - minB[2]);
    const nx = Math.max(4, Math.min(40, Math.round(spanX / cellBase)));
    const nz = Math.max(4, Math.min(40, Math.round(spanZ / cellBase)));
    const stepX = spanX / nx;
    const stepZ = spanZ / nz;

    const heights = new Float32Array((nx + 1) * (nz + 1));
    for (let iz = 0; iz <= nz; iz++) {
      const z = minB[2] + iz * stepZ;
      for (let ix = 0; ix <= nx; ix++) {
        const x = minB[0] + ix * stepX;
        const h = sampleGeneratorHeight(x, z, minB, maxB, gen);
        heights[iz * (nx + 1) + ix] = h;
        trackBounds(x, h, z);
      }
    }

    const baseMat = getTerrainMaterial(reg.material || "Grass");
    const rockMat = TERRAIN_MATERIALS.Rock;
    const sandMat = TERRAIN_MATERIALS.Sand;
    const snowMat = TERRAIN_MATERIALS.Snow;
    const waterLevel = Number.isFinite(gen.water_level) ? gen.water_level : -2;
    const hasWater = gen.kind === "island" || gen.kind === "canyon" || (Number.isFinite(gen.water_level) && gen.water_level > minB[1]);
    if (hasWater) {
      waterQuads.push({ minX: minB[0], maxX: maxB[0], minZ: minB[2], maxZ: maxB[2], y: waterLevel });
    }

    const vertStart = posOut.length / 3;
    for (let iz = 0; iz <= nz; iz++) {
      const z = minB[2] + iz * stepZ;
      for (let ix = 0; ix <= nx; ix++) {
        const x = minB[0] + ix * stepX;
        const h = heights[iz * (nx + 1) + ix];
        const hL = heights[iz * (nx + 1) + Math.max(0, ix - 1)];
        const hR = heights[iz * (nx + 1) + Math.min(nx, ix + 1)];
        const hD = heights[Math.max(0, iz - 1) * (nx + 1) + ix];
        const hU = heights[Math.min(nz, iz + 1) * (nx + 1) + ix];

        let nmx = (hL - hR) / (2 * stepX);
        let nmy = 1;
        let nmz = (hD - hU) / (2 * stepZ);
        const nl = Math.hypot(nmx, nmy, nmz) || 1;
        nmx /= nl;
        nmy /= nl;
        nmz /= nl;

        let chosen = baseMat;
        const paint = findPaintOverride(x, h, z);
        if (paint) {
          chosen = getTerrainMaterial(paint.material);
        } else {
          const slope = 1 - nmy;
          if (hasWater && h <= waterLevel + 1.4) {
            chosen = sandMat;
          } else if (slope > 0.22) {
            chosen = rockMat;
          } else if (gen.amplitude >= 16 && h > gen.amplitude * 0.72) {
            chosen = snowMat;
          }
        }

        const tintVar = 0.94 + hash2D(ix, iz, (gen.seed || 1) + 7) * 0.12;
        posOut.push(x, h, z);
        normOut.push(nmx, nmy, nmz);
        colOut.push(
          Math.min(1, chosen.rgb[0] * tintVar),
          Math.min(1, chosen.rgb[1] * tintVar),
          Math.min(1, chosen.rgb[2] * tintVar),
        );
        matOut.push(chosen.roughness, chosen.metalness, chosen.emissive);
      }
    }

    for (let iz = 0; iz < nz; iz++) {
      for (let ix = 0; ix < nx; ix++) {
        const a = vertStart + iz * (nx + 1) + ix;
        const b = a + 1;
        const c = a + (nx + 1);
        const d = c + 1;
        idxOut.push(a, c, b, b, c, d);
      }
    }

    // Coarse 2D fallback triangles (stride 2 or 3 so 2D software rasterizer stays fast at 60fps)
    const stride = Math.max(2, Math.floor(Math.max(nx, nz) / 14));
    for (let iz = 0; iz < nz; iz += stride) {
      const iz2 = Math.min(nz, iz + stride);
      const z0 = minB[2] + iz * stepZ;
      const z1 = minB[2] + iz2 * stepZ;
      for (let ix = 0; ix < nx; ix += stride) {
        const ix2 = Math.min(nx, ix + stride);
        const x0 = minB[0] + ix * stepX;
        const x1 = minB[0] + ix2 * stepX;
        const h00 = heights[iz * (nx + 1) + ix];
        const h10 = heights[iz * (nx + 1) + ix2];
        const h01 = heights[iz2 * (nx + 1) + ix];
        const h11 = heights[iz2 * (nx + 1) + ix2];
        const avgH = (h00 + h10 + h01 + h11) * 0.25;
        const paint = findPaintOverride((x0 + x1) * 0.5, avgH, (z0 + z1) * 0.5);
        const mat = paint
          ? getTerrainMaterial(paint.material)
          : hasWater && avgH <= waterLevel + 1.2
            ? sandMat
            : baseMat;

        const pushTri2D = (p0: Vec3, p1: Vec3, p2: Vec3) => {
          const ax = p1[0] - p0[0], ay = p1[1] - p0[1], az = p1[2] - p0[2];
          const bx = p2[0] - p0[0], by = p2[1] - p0[1], bz = p2[2] - p0[2];
          let fnx = ay * bz - az * by;
          let fny = az * bx - ax * bz;
          let fnz = ax * by - ay * bx;
          if (fny < 0) { fnx = -fnx; fny = -fny; fnz = -fnz; }
          const l = Math.hypot(fnx, fny, fnz) || 1;
          tris2D.push({ pts: [p0, p1, p2], normal: [fnx / l, fny / l, fnz / l], rgb: mat.rgb, alpha: 1 });
        };

        pushTri2D([x0, h00, z0], [x0, h01, z1], [x1, h10, z0]);
        pushTri2D([x1, h10, z0], [x0, h01, z1], [x1, h11, z1]);
      }
    }
  }

  // Paint boxes that have no overlapping generator (or standalone Water / Lava / Rock paint volumes)
  for (const pr of paintRegions) {
    const overlapsGen = generatorRegions.some(
      (gr) =>
        pr.bounds_min[0] <= gr.bounds_max[0] &&
        pr.bounds_max[0] >= gr.bounds_min[0] &&
        pr.bounds_min[2] <= gr.bounds_max[2] &&
        pr.bounds_max[2] >= gr.bounds_min[2],
    );
    if (overlapsGen && pr.material.toLowerCase() !== "water") continue;

    if (pr.material.toLowerCase() === "water") {
      waterQuads.push({
        minX: pr.bounds_min[0],
        maxX: pr.bounds_max[0],
        minZ: pr.bounds_min[2],
        maxZ: pr.bounds_max[2],
        y: pr.bounds_max[1],
      });
      trackBounds(pr.bounds_min[0], pr.bounds_max[1], pr.bounds_min[2]);
      trackBounds(pr.bounds_max[0], pr.bounds_max[1], pr.bounds_max[2]);
    } else {
      const mat = getTerrainMaterial(pr.material || "Grass");
      appendBoxMesh(pr.bounds_min, pr.bounds_max, mat, posOut, normOut, colOut, matOut, idxOut, tris2D);
      trackBounds(pr.bounds_min[0], pr.bounds_min[1], pr.bounds_min[2]);
      trackBounds(pr.bounds_max[0], pr.bounds_max[1], pr.bounds_max[2]);
    }
  }

  if (posOut.length === 0 && waterQuads.length === 0) return null;

  return {
    positions: new Float32Array(posOut),
    normals: new Float32Array(normOut),
    colors: new Float32Array(colOut),
    materialAttrs: new Float32Array(matOut),
    indices: new Uint32Array(idxOut),
    waterQuads,
    tris2D,
    bounds: {
      minX: Number.isFinite(bMinX) ? bMinX : -64,
      maxX: Number.isFinite(bMaxX) ? bMaxX : 64,
      minZ: Number.isFinite(bMinZ) ? bMinZ : -64,
      maxZ: Number.isFinite(bMaxZ) ? bMaxZ : 64,
      minY: Number.isFinite(bMinY) ? bMinY : 0,
      maxY: Number.isFinite(bMaxY) ? bMaxY : 16,
    },
  };
}

export function terrainColorHexFromRgb(rgb: Vec3, shade: number, alpha = 1): string {
  const scaled: Vec3 = [
    Math.min(1, Math.max(0, rgb[0] * shade)),
    Math.min(1, Math.max(0, rgb[1] * shade)),
    Math.min(1, Math.max(0, rgb[2] * shade)),
  ];
  if (alpha < 0.99) {
    const r = Math.round(scaled[0] * 255);
    const g = Math.round(scaled[1] * 255);
    const b = Math.round(scaled[2] * 255);
    return `rgba(${r},${g},${b},${alpha.toFixed(2)})`;
  }
  return rgbToHex(scaled);
}

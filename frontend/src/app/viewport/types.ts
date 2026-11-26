import type { Place, ProjectAssetRecord } from "../data";
import type { WorkspaceLighting, WorkspaceMeshEntry } from "../ship/api";

export type Vec2 = [number, number];
export type Vec3 = [number, number, number];
export type StudioTool = "select" | "move" | "scale" | "rotate";
export type RendererMode = "webgl2" | "canvas2d-fallback";

export type TerrainGeneratorKind = "flat" | "hills" | "island" | "canyon";

export type TerrainGeneratorConfig = {
  kind: TerrainGeneratorKind;
  seed: number;
  amplitude: number;
  water_level: number;
};

export type TerrainRegion = {
  id: string;
  bounds_min: Vec3;
  bounds_max: Vec3;
  material: string;
  generator?: TerrainGeneratorConfig | null;
};

export type WorkspaceTerrain = {
  enabled: boolean;
  cell_size: number;
  regions: TerrainRegion[];
};

export type WorkspaceLightKind = "PointLight" | "SpotLight" | "SurfaceLight";

export type WorkspaceLight = {
  id: string;
  path: string;
  kind: WorkspaceLightKind;
  parent_path: string;
  position: Vec3;
  direction?: Vec3;
  color: string;
  brightness: number;
  range: number;
  angle?: number;
  shadows?: boolean;
  enabled?: boolean;
};

export type CameraState = {
  tx: number;
  ty: number;
  tz: number;
  yaw: number;
  pitch: number;
  dist: number;
};

export type Part3D = {
  path: string;
  name: string;
  className: string;
  shape: "Block" | "Ball" | "Cylinder";
  pos: Vec3;
  size: Vec3;
  rotYDeg: number;
  rotDeg: Vec3;
  color: string;
  material: string;
  roughness: number;
  metalness: number;
  emissive: number;
  transparency: number;
  decalUrl?: string;
  meshUrl?: string;
  cardTitle?: string;
  hasParticleEmitter?: boolean;
  hasSound?: boolean;
  rawProps: Record<string, unknown>;
};

export type ParsedObjGeometry = {
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  vertexCount: number;
  normVerts: Vec3[];
  edges: Array<[number, number]>;
};

export type GizmoHandleHit = {
  axis: "x" | "y" | "z";
  x: number;
  y: number;
  dir2d: Vec2;
};

export type ViewCubeHit = {
  axis: "x" | "y" | "z";
  x: number;
  y: number;
};

export type AgentAnimState = {
  x: number;
  y: number;
  z: number;
  tx: number;
  ty: number;
  tz: number;
  beam: number;
  popPath: string;
  popProgress: number;
  callout: string;
  calloutUntil: number;
  time: number;
};

export type ViewportLightingInput = WorkspaceLighting | null | undefined;
export type ViewportMeshInput = WorkspaceMeshEntry;

export function parseVec3(v: unknown, fallback: Vec3): Vec3 {
  if (Array.isArray(v) && v.length >= 3 && v.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [v[0], v[1], v[2]];
  }
  return fallback;
}

export function hexToRgb(hex: string): [number, number, number] {
  const c = hex.replace("#", "").trim();
  if (c.length !== 6) return [139, 147, 158];
  const n = parseInt(c, 16);
  if (!Number.isFinite(n)) return [139, 147, 158];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function hexToRgbNorm(hex: string): Vec3 {
  const [r, g, b] = hexToRgb(hex);
  return [r / 255, g / 255, b / 255];
}

export function shadeHex(hex: string, factor: number, alpha = 1): string {
  const [r, g, b] = hexToRgb(hex);
  const clamp = (x: number) => Math.max(0, Math.min(255, Math.round(x)));
  const rr = clamp(r * factor);
  const gg = clamp(g * factor);
  const bb = clamp(b * factor);
  return alpha < 0.99 ? `rgba(${rr},${gg},${bb},${alpha.toFixed(2)})` : `rgb(${rr},${gg},${bb})`;
}

export function parseColor(name: string, className: string, props: Record<string, unknown>): string {
  if (typeof props.Color === "string" && /^#[0-9a-fA-F]{6}$/.test(props.Color.trim())) {
    return props.Color.trim();
  }
  const low = name.toLowerCase();
  if (className === "SpawnLocation" || low.includes("spawn")) return "#2D68DF";
  if (low.includes("lava") || low.includes("kill") || low.includes("shrine") || low.includes("boss")) return "#DF522D";
  if (low.includes("roll") || low.includes("pedestal") || low.includes("coin") || low.includes("gold")) return "#D49B26";
  if (low.includes("tower") || low.includes("portal") || low.includes("gate")) return "#7B4ED4";
  if (low.includes("alchemist") || low.includes("jump") || low.includes("checkpoint")) return "#24946E";
  if (low.includes("ice") || low.includes("frost")) return "#65B7E8";
  if (low.includes("moving") || low.includes("platform")) return "#3F78B5";
  if (low.includes("billboard") || low.includes("gallery")) return "#475569";
  return "#8B939E";
}

function materialPbrHints(material: string): { roughness: number; metalness: number; emissive: number } {
  switch (material) {
    case "Neon":
      return { roughness: 0.25, metalness: 0.0, emissive: 0.85 };
    case "Metal":
    case "DiamondPlate":
    case "Foil":
      return { roughness: 0.28, metalness: 0.85, emissive: 0 };
    case "CorrodedMetal":
      return { roughness: 0.62, metalness: 0.68, emissive: 0 };
    case "Glass":
    case "Ice":
      return { roughness: 0.14, metalness: 0.05, emissive: 0 };
    case "SmoothPlastic":
    case "Plastic":
      return { roughness: 0.42, metalness: 0.04, emissive: 0 };
    case "Wood":
    case "WoodPlanks":
      return { roughness: 0.78, metalness: 0.0, emissive: 0 };
    case "Grass":
    case "Sand":
    case "Fabric":
      return { roughness: 0.9, metalness: 0.0, emissive: 0 };
    case "Slate":
    case "Concrete":
    case "Granite":
    case "Brick":
      return { roughness: 0.82, metalness: 0.02, emissive: 0 };
    default:
      return { roughness: 0.5, metalness: 0.05, emissive: 0 };
  }
}

const NON_PART_CLASSES = new Set([
  "Decal",
  "SurfaceGui",
  "Script",
  "LocalScript",
  "ModuleScript",
  "Folder",
  "PointLight",
  "SpotLight",
  "SurfaceLight",
  "ParticleEmitter",
  "Sound",
  "SurfaceAppearance",
  "Attachment",
  "BillboardGui",
]);

function resolveImageUrl(rawTex: string | undefined, cardTitle: string | undefined, name: string, assets: ProjectAssetRecord[]): string | undefined {
  if (!rawTex && !cardTitle && !/billboard|decalboard|showcase/i.test(name)) return undefined;
  const matched = assets.find(
    (a) =>
      (cardTitle && a.title.toLowerCase() === cardTitle.toLowerCase()) ||
      (rawTex && a.file && rawTex.toLowerCase().includes(a.file.toLowerCase())) ||
      (rawTex && rawTex.toLowerCase().includes(a.id.toLowerCase())),
  );
  if (matched) return matched.image_url;
  if (
    rawTex &&
    (rawTex.startsWith("data:image/") ||
      rawTex.startsWith("/api/assets/") ||
      rawTex.startsWith("/api/projects/"))
  ) {
    return rawTex;
  }
  return undefined;
}

export function extractParts(
  place: Place,
  assets: ProjectAssetRecord[],
  meshes: WorkspaceMeshEntry[] = [],
): Part3D[] {
  const out: Part3D[] = [];
  if (!place || !place.instances || typeof place.instances !== "object") return out;
  const entries = Object.entries(place.instances);
  let fallbackIdx = 0;

  const surfaceAppearances = new Map<string, Record<string, unknown>>();
  const emitterParents = new Set<string>();
  const soundParents = new Set<string>();

  for (const [path, inst] of entries) {
    if (!path.startsWith("Workspace.") || !inst || typeof inst !== "object") continue;
    const cls = typeof inst.class_name === "string" ? inst.class_name : "";
    const lastDot = path.lastIndexOf(".");
    const parentPath = lastDot > 0 ? path.slice(0, lastDot) : "Workspace";
    if (cls === "SurfaceAppearance") {
      surfaceAppearances.set(parentPath, (inst.properties || {}) as Record<string, unknown>);
    } else if (cls === "ParticleEmitter") {
      emitterParents.add(parentPath);
    } else if (cls === "Sound") {
      soundParents.add(parentPath);
    }
  }

  const clampNum = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

  for (const [path, inst] of entries) {
    if (!path.startsWith("Workspace.") || !inst || typeof inst !== "object") continue;
    const cls = typeof inst.class_name === "string" && inst.class_name ? inst.class_name : "Part";
    if (NON_PART_CLASSES.has(cls)) continue;
    const name = path.split(".").slice(1).join(".") || path;
    const props = (inst.properties && typeof inst.properties === "object" ? inst.properties : {}) as Record<string, unknown>;
    const rawSize = parseVec3(props.Size, cls === "SpawnLocation" ? [8, 1, 8] : [8, 2, 8]);
    const size: Vec3 = [
      clampNum(rawSize[0], 0.5, 512),
      clampNum(rawSize[1], 0.5, 512),
      clampNum(rawSize[2], 0.5, 512),
    ];
    const hasPos = Array.isArray(props.Position) && props.Position.length >= 3;
    const rawPos: Vec3 = hasPos
      ? parseVec3(props.Position, [0, size[1] / 2, 0])
      : [(fallbackIdx % 4 - 1.5) * 14, Math.max(1, size[1] / 2), Math.floor(fallbackIdx / 4) * 14];
    const pos: Vec3 = [
      clampNum(rawPos[0], -1024, 1024),
      clampNum(rawPos[1], -256, 1024),
      clampNum(rawPos[2], -1024, 1024),
    ];
    if (!hasPos) fallbackIdx++;

    const rotArr = parseVec3(props.Rotation, [0, 0, 0]);
    const rotYDeg = rotArr[1];
    const transparency =
      typeof props.Transparency === "number" && Number.isFinite(props.Transparency)
        ? Math.max(0, Math.min(0.9, props.Transparency))
        : 0;
    const material = typeof props.Material === "string" ? props.Material : "SmoothPlastic";
    const color = parseColor(name, cls, props);

    const saProps = surfaceAppearances.get(path);
    const pbrBase = materialPbrHints(material);
    const roughness =
      typeof saProps?.Roughness === "number"
        ? clampNum(saProps.Roughness, 0.05, 1)
        : pbrBase.roughness;
    const metalness =
      typeof saProps?.Metalness === "number"
        ? clampNum(saProps.Metalness, 0, 1)
        : pbrBase.metalness;

    const cardTitle =
      typeof props.CardTitle === "string"
        ? props.CardTitle
        : typeof props.DisplayTitle === "string"
          ? props.DisplayTitle
          : undefined;
    const rawTex =
      typeof saProps?.ColorMap === "string"
        ? saProps.ColorMap
        : typeof props.DecalTexture === "string"
          ? props.DecalTexture
          : typeof props.TextureID === "string"
            ? props.TextureID
            : typeof props.Texture === "string"
              ? props.Texture
              : undefined;
    let decalUrl = resolveImageUrl(rawTex, cardTitle, name, assets);

    let meshUrl: string | undefined;
    const rawMeshRef =
      typeof props.MeshId === "string"
        ? props.MeshId
        : typeof props.mesh_id === "string"
          ? props.mesh_id
          : typeof props.ObjPath === "string"
            ? props.ObjPath
            : "";
    if (rawMeshRef || cls === "MeshPart") {
      const refLow = rawMeshRef.toLowerCase();
      const nameLow = name.toLowerCase();
      const matchedMesh = meshes.find((m) => {
        const mName = m.name.toLowerCase().replace(/\.obj$/i, "");
        const mPath = (m.obj_path || "").toLowerCase();
        if (refLow) {
          return (
            m.obj_url.toLowerCase() === refLow ||
            (mPath && refLow.includes(mPath)) ||
            refLow.includes(mName)
          );
        }
        return cls === "MeshPart" && (nameLow.includes(mName) || mName.includes(nameLow));
      });
      if (matchedMesh) {
        meshUrl = matchedMesh.obj_url;
        if (!decalUrl && matchedMesh.texture_url) decalUrl = matchedMesh.texture_url;
      } else if (rawMeshRef.startsWith("/api/projects/")) {
        meshUrl = rawMeshRef;
      }
    }

    const rawShape = typeof props.Shape === "string" ? props.Shape : typeof props.shape === "string" ? props.shape : "";
    const shape: Part3D["shape"] =
      /ball|sphere/i.test(rawShape) || /sphere|ball/i.test(name)
        ? "Ball"
        : /cylinder/i.test(rawShape) || /cylinder|pillar/i.test(name)
          ? "Cylinder"
          : "Block";

    out.push({
      path,
      name,
      className: cls,
      shape,
      pos,
      size,
      rotYDeg,
      rotDeg: rotArr,
      color,
      material,
      roughness,
      metalness,
      emissive: pbrBase.emissive,
      transparency,
      decalUrl,
      meshUrl,
      cardTitle,
      hasParticleEmitter: emitterParents.has(path),
      hasSound: soundParents.has(path),
      rawProps: props,
    });
  }

  return out;
}

function directionFromFace(face: unknown): Vec3 {
  const f = typeof face === "string" ? face.toLowerCase() : "";
  if (f === "front") return [0, 0, 1];
  if (f === "back") return [0, 0, -1];
  if (f === "left") return [-1, 0, 0];
  if (f === "right") return [1, 0, 0];
  if (f === "top") return [0, 1, 0];
  return [0, -1, 0];
}

export function extractLightsFromPlace(
  place: Place,
  serverLights?: WorkspaceLight[] | null,
): WorkspaceLight[] {
  const byPath = new Map<string, WorkspaceLight>();
  if (Array.isArray(serverLights)) {
    for (const l of serverLights) {
      if (l && l.path) byPath.set(l.path, l);
    }
  }
  const instances = place?.instances || {};
  for (const [path, inst] of Object.entries(instances)) {
    if (!inst || typeof inst !== "object") continue;
    const cls = inst.class_name;
    if (cls !== "PointLight" && cls !== "SpotLight" && cls !== "SurfaceLight") continue;
    const props = (inst.properties && typeof inst.properties === "object" ? inst.properties : {}) as Record<string, unknown>;
    const lastDot = path.lastIndexOf(".");
    const parentPath = lastDot > 0 ? path.slice(0, lastDot) : "Workspace";
    const parentInst = instances[parentPath];
    const parentProps = (parentInst?.properties || {}) as Record<string, unknown>;
    const parentPos = parseVec3(parentProps.Position, [0, 4, 0]);
    const parentSize = parseVec3(parentProps.Size, [4, 4, 4]);
    const hasOwnPos = Array.isArray(props.Position) && props.Position.length >= 3;
    const position: Vec3 = hasOwnPos
      ? parseVec3(props.Position, [0, 6, 0])
      : [parentPos[0], parentPos[1] + parentSize[1] * 0.5 + 1.5, parentPos[2]];

    const direction = Array.isArray(props.Direction) && props.Direction.length >= 3
      ? parseVec3(props.Direction, [0, -1, 0])
      : directionFromFace(props.Face);

    const color =
      typeof props.Color === "string" && /^#[0-9a-fA-F]{6}$/.test(props.Color.trim())
        ? props.Color.trim()
        : "#FFE4B5";
    const brightness = typeof props.Brightness === "number" && Number.isFinite(props.Brightness) ? props.Brightness : 2;
    const range = typeof props.Range === "number" && Number.isFinite(props.Range) ? props.Range : 28;
    const angle = typeof props.Angle === "number" && Number.isFinite(props.Angle) ? props.Angle : 65;
    const shadows = props.Shadows !== false;
    const enabled = props.Enabled !== false;

    byPath.set(path, {
      id: path,
      path,
      kind: cls,
      parent_path: parentPath,
      position,
      direction,
      color,
      brightness,
      range,
      angle,
      shadows,
      enabled,
    });
  }
  return Array.from(byPath.values());
}

export function parseObjGeometry(text: string): ParsedObjGeometry | null {
  if (!text || text.length > 2_000_000) return null;
  const rawVerts: Vec3[] = [];
  const rawNormals: Vec3[] = [];
  const rawUvs: Vec2[] = [];
  const edgeSet = new Set<string>();
  const rawEdges: Array<[number, number]> = [];
  const posList: number[] = [];
  const normList: number[] = [];
  const uvList: number[] = [];

  const resolveIdx = (raw: number, len: number) => (raw > 0 ? raw - 1 : len + raw);

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    if (line.startsWith("v ")) {
      const p = line.slice(2).trim().split(/\s+/);
      if (p.length >= 3) {
        const x = Number(p[0]);
        const y = Number(p[1]);
        const z = Number(p[2]);
        if (Number.isFinite(x) && Number.isFinite(y) && Number.isFinite(z)) rawVerts.push([x, y, z]);
      }
    } else if (line.startsWith("vn ")) {
      const p = line.slice(3).trim().split(/\s+/);
      if (p.length >= 3) {
        const x = Number(p[0]);
        const y = Number(p[1]);
        const z = Number(p[2]);
        if (Number.isFinite(x) && Number.isFinite(y) && Number.isFinite(z)) rawNormals.push([x, y, z]);
      }
    } else if (line.startsWith("vt ")) {
      const p = line.slice(3).trim().split(/\s+/);
      if (p.length >= 2) {
        const u = Number(p[0]);
        const v = Number(p[1]);
        if (Number.isFinite(u) && Number.isFinite(v)) rawUvs.push([u, v]);
      }
    } else if (line.startsWith("f ")) {
      const tokens = line.slice(2).trim().split(/\s+/);
      const faceVerts: Array<{ vi: number; ti: number; ni: number }> = [];
      for (const tok of tokens) {
        const parts = tok.split("/");
        const viRaw = parseInt(parts[0] || "", 10);
        if (!Number.isFinite(viRaw) || viRaw === 0) continue;
        const vi = resolveIdx(viRaw, rawVerts.length);
        if (vi < 0 || vi >= rawVerts.length) continue;
        const tiRaw = parts[1] ? parseInt(parts[1], 10) : 0;
        const niRaw = parts[2] ? parseInt(parts[2], 10) : 0;
        const ti = tiRaw !== 0 && Number.isFinite(tiRaw) ? resolveIdx(tiRaw, rawUvs.length) : -1;
        const ni = niRaw !== 0 && Number.isFinite(niRaw) ? resolveIdx(niRaw, rawNormals.length) : -1;
        faceVerts.push({ vi, ti, ni });
      }
      if (faceVerts.length < 3) continue;

      for (let i = 0; i < faceVerts.length; i++) {
        const a = faceVerts[i].vi;
        const b = faceVerts[(i + 1) % faceVerts.length].vi;
        if (a !== b) {
          const lo = Math.min(a, b);
          const hi = Math.max(a, b);
          const key = `${lo}:${hi}`;
          if (!edgeSet.has(key)) {
            edgeSet.add(key);
            rawEdges.push([lo, hi]);
          }
        }
      }

      for (let i = 1; i + 1 < faceVerts.length; i++) {
        const tri = [faceVerts[0], faceVerts[i], faceVerts[i + 1]];
        const v0 = rawVerts[tri[0].vi];
        const v1 = rawVerts[tri[1].vi];
        const v2 = rawVerts[tri[2].vi];
        const ax = v1[0] - v0[0];
        const ay = v1[1] - v0[1];
        const az = v1[2] - v0[2];
        const bx = v2[0] - v0[0];
        const by = v2[1] - v0[1];
        const bz = v2[2] - v0[2];
        let fnx = ay * bz - az * by;
        let fny = az * bx - ax * bz;
        let fnz = ax * by - ay * bx;
        const fl = Math.hypot(fnx, fny, fnz) || 1;
        fnx /= fl;
        fny /= fl;
        fnz /= fl;

        for (const corner of tri) {
          const vp = rawVerts[corner.vi];
          posList.push(vp[0], vp[1], vp[2]);
          if (corner.ni >= 0 && corner.ni < rawNormals.length) {
            const np = rawNormals[corner.ni];
            normList.push(np[0], np[1], np[2]);
          } else {
            normList.push(fnx, fny, fnz);
          }
          if (corner.ti >= 0 && corner.ti < rawUvs.length) {
            const uv = rawUvs[corner.ti];
            uvList.push(uv[0], 1 - uv[1]);
          } else {
            uvList.push((vp[0] + 0.5) % 1, (vp[1] + 0.5) % 1);
          }
        }
      }
    }
  }

  if (rawVerts.length === 0 || posList.length < 9 || posList.length % 9 !== 0) return null;
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const [x, y, z] of rawVerts) {
    if (x < minX) minX = x; if (x > maxX) maxX = x;
    if (y < minY) minY = y; if (y > maxY) maxY = y;
    if (z < minZ) minZ = z; if (z > maxZ) maxZ = z;
  }
  if (!Number.isFinite(minX) || !Number.isFinite(maxX)) return null;
  const cx = (minX + maxX) * 0.5;
  const cy = (minY + maxY) * 0.5;
  const cz = (minZ + maxZ) * 0.5;
  const sx = Math.max(1e-4, maxX - minX);
  const sy = Math.max(1e-4, maxY - minY);
  const sz = Math.max(1e-4, maxZ - minZ);

  const positions = new Float32Array(posList.length);
  for (let i = 0; i < posList.length; i += 3) {
    positions[i] = (posList[i] - cx) / sx;
    positions[i + 1] = (posList[i + 1] - cy) / sy;
    positions[i + 2] = (posList[i + 2] - cz) / sz;
  }

  const normVerts: Vec3[] = rawVerts.map(([x, y, z]) => [(x - cx) / sx, (y - cy) / sy, (z - cz) / sz]);
  const MAX_EDGES = 360;
  let edges = rawEdges;
  if (rawEdges.length > MAX_EDGES) {
    const step = rawEdges.length / MAX_EDGES;
    edges = [];
    for (let i = 0; i < MAX_EDGES; i++) edges.push(rawEdges[Math.floor(i * step)]);
  }

  return {
    positions,
    normals: new Float32Array(normList),
    uvs: new Float32Array(uvList),
    vertexCount: positions.length / 3,
    normVerts,
    edges,
  };
}

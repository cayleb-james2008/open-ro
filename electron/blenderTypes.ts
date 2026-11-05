/**
 * Blender 3D Asset Pipeline & Headless Task Runner Types
 * Supports 5 standard 3D pipeline tasks, binary discovery, manifests,
 * metrics, and IPC data contracts for Open-Ro.
 */

// ==========================================
// 1. Binary Discovery & Capabilities
// ==========================================

export type BlenderInstallSource =
  | 'env'
  | 'path'
  | 'windows-program-files'
  | 'windows-registry'
  | 'linux-system'
  | 'linux-snap'
  | 'linux-flatpak'
  | 'macos-app'
  | 'custom';

export interface BlenderBinaryInfo {
  executablePath: string;
  version: string;
  major: number;
  minor: number;
  patch: number;
  source: BlenderInstallSource;
  isFlatpak?: boolean;
}

export interface BlenderStatusView {
  installed: boolean;
  binaryPath: string | null;
  version: string | null;
  majorVersion: number | null;
  supported: boolean;
  error?: string;
  installGuideUrl: string;
}

export interface BlenderDiscoveryOptions {
  customPath?: string;
  forceRefresh?: boolean;
  probeTimeoutMs?: number;
}

// ==========================================
// 2. Core 3D Pipeline Task Types & Parameters
// ==========================================

export type Blender3DTaskType =
  | 'inspect_mesh'
  | 'convert_format'
  | 'decimate_mesh'
  | 'generate_collision'
  | 'export_roblox_package';

export type BlenderTaskType = Blender3DTaskType | 'custom';

export interface BoundingBoxCoordinates {
  min: [number, number, number];
  max: [number, number, number];
  dimensions: [number, number, number];
  center: [number, number, number];
}

export interface MeshMetrics {
  vertexCount: number;
  edgeCount: number;
  faceCount: number;
  triangleCount: number;
  boundingBox: {
    blender: BoundingBoxCoordinates;
    roblox: BoundingBoxCoordinates;
  };
  uvLayers: string[];
  uvLayerCount: number;
  materials: Array<{
    name: string;
    textures: Array<{ name: string; filepath: string }>;
  }>;
  materialCount: number;
  hasArmature: boolean;
  hasAnimations: boolean;
  hasVertexColors: boolean;
}

export interface RobloxComplianceCheck {
  isUnder10k: boolean;
  isUnder20k: boolean;
  singleMaterial: boolean;
  hasUV: boolean;
  warnings: string[];
}

// --- Task 1: inspect_mesh ---
export interface InspectMeshParams {
  filePath: string;
  calculateTriangles?: boolean;
  detailedMaterialAudit?: boolean;
}

export interface InspectMeshResult extends MeshMetrics {
  filePath: string;
  format: string;
  fileSizeBytes: number;
  objectCount: number;
  meshCount: number;
  robloxCompliance: RobloxComplianceCheck;
  success: boolean;
  error?: string;
}

// --- Task 2: convert_format ---
export type PivotNormalizationMode = 'bottom_center' | 'geometric_center' | 'preserve';

export interface ConvertFormatParams {
  inputPath: string;
  outputPath: string;
  targetFormat: 'fbx' | 'obj' | 'gltf' | 'glb';
  coordinateSystem?: 'roblox_y_up' | 'preserve';
  applyTransforms?: boolean;
  normalizePivot?: PivotNormalizationMode;
  scaleMultiplier?: number;
  triangulateMesh?: boolean;
}

export interface ConvertFormatResult {
  outputPath: string;
  format: string;
  fileSizeBytes: number;
  appliedTransforms: {
    scaleMultiplier: number;
    pivotMode: PivotNormalizationMode;
    appliedTransforms: boolean;
    robloxAxes: { axis_forward: string; axis_up: string } | string;
  };
  success: boolean;
  error?: string;
}

// --- Task 3: decimate_mesh ---
export interface DecimateMeshParams {
  inputPath: string;
  outputPath: string;
  targetTriangles?: number;
  ratio?: number;
  preserveUVs?: boolean;
  preserveNormals?: boolean;
  symmetry?: boolean;
}

export interface DecimateMeshResult {
  outputPath: string;
  originalTriangles: number;
  originalVertices: number;
  decimatedTriangles: number;
  decimatedVertices: number;
  reductionRatio: number;
  targetMet: boolean;
  success: boolean;
  error?: string;
}

// --- Task 4: generate_collision ---
export type CollisionGeometryType = 'convex_hull' | 'bounding_box' | 'simplified_mesh';

export interface GenerateCollisionParams {
  inputPath: string;
  outputPath: string;
  collisionType: CollisionGeometryType;
  maxCollisionTriangles?: number;
}

export interface GenerateCollisionResult {
  outputPath: string;
  collisionType: CollisionGeometryType;
  triangleCount: number;
  vertexCount: number;
  recommendedRobloxFidelity: 'Hull' | 'Box' | 'PreciseConvexDecomposition';
  success: boolean;
  error?: string;
}

// --- Task 5: export_roblox_package ---
export interface RobloxAssetAttribution {
  author: string;
  license: string;
  sourceUrl?: string;
  notes?: string;
}

export interface RobloxAssetManifest {
  schemaVersion: '1.0';
  name: string;
  generator: string;
  createdAt: string;
  files: {
    visualMesh: string;
    collisionMesh?: string | null;
    textures: Record<string, string>;
  };
  geometry: {
    triangleCount: number;
    vertexCount: number;
    boundingBox: {
      dimensions: [number, number, number];
      unit: string;
    };
  };
  robloxSettings: {
    recommendedScale: number;
    collisionFidelity: string;
    useSurfaceAppearance: boolean;
  };
  attribution: RobloxAssetAttribution;
}

export interface ExportRobloxPackageParams {
  inputMeshPath: string;
  outputDir: string;
  assetName: string;
  format?: 'fbx' | 'obj' | 'glb';
  decimateToBudget?: number;
  generateCollision?: boolean;
  collisionType?: CollisionGeometryType;
  textureDirectory?: string;
  textureFiles?: Record<string, string>;
  attribution?: Partial<RobloxAssetAttribution>;
}

export interface ExportRobloxPackageResult {
  manifestPath: string;
  packageDir: string;
  assetName: string;
  manifest: RobloxAssetManifest;
  exportedFiles: string[];
  success: boolean;
  error?: string;
}

// ==========================================
// 3. Execution, Telemetry & IPC Contracts
// ==========================================

export interface BlenderFileDetail {
  path: string;
  bytes: number;
  relPath?: string;
  sha256?: string;
}

export interface BlenderTelemetry {
  objects_count?: number;
  meshes_count?: number;
  materials_count?: number;
  blender_version?: number[];
  [key: string]: unknown;
}

/** Legacy & standard generic task contract (matches tests/e2e/harness.ts) */
export interface BlenderTask {
  scriptPath?: string;
  inlineScript?: string;
  args?: string[];
  outputDir: string;
  timeoutMs?: number;
  description?: string;
  targetFiles?: string[];
  binaryPath?: string;
}

export interface BlenderTaskResult {
  success: boolean;
  generatedFiles: string[];
  logs: string;
  fileDetails?: BlenderFileDetail[];
  error?: string;
  durationMs?: number;
  exitCode?: number | null;
  telemetry?: BlenderTelemetry;
}

export interface BlenderRunTaskInput {
  taskType: BlenderTaskType;
  inputFile?: string;
  outputDir?: string;
  targetFormat?: 'obj' | 'fbx' | 'gltf' | 'glb';
  targetTriangles?: number;
  decimateRatio?: number;
  preserveUv?: boolean;
  preserveNormals?: boolean;
  coordinateNormalization?: boolean;
  collisionType?: 'box' | 'hull' | 'exact';
  texturePaths?: string[];
  scriptPath?: string;
  customArgs?: string[];
  timeoutMs?: number;
  stageOnly?: boolean;
  assetName?: string;
  author?: string;
  license?: string;
}

export interface BlenderTaskResultView {
  ok: boolean;
  taskId: string;
  taskType: string;
  summary: string;
  generatedFiles: Array<{ path: string; bytes: number; relPath?: string }>;
  metrics?: {
    trianglesBefore?: number;
    trianglesAfter?: number;
    verticesBefore?: number;
    verticesAfter?: number;
    boundsX?: number;
    boundsY?: number;
    boundsZ?: number;
    uvMapCount?: number;
    materialCount?: number;
  };
  logs: string;
  durationMs: number;
  error?: string;
}

export interface BlenderTaskHistoryEntry {
  taskId: string;
  taskType: string;
  createdAt: string;
  status: 'COMPLETED' | 'FAILED' | 'CANCELLED';
  durationMs: number;
  summary: string;
  generatedFiles: string[];
}

export type BlenderTaskEvent =
  | { type: 'task-started'; taskId: string; taskType: string }
  | { type: 'task-progress'; taskId: string; percent?: number; stage: string; logLine?: string }
  | { type: 'task-completed'; taskId: string; success: boolean; generatedFiles: string[]; durationMs: number }
  | { type: 'task-cancelled'; taskId: string }
  | { type: 'task-error'; taskId: string; error: string };

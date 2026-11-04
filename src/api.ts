import type { ProviderKind, ProviderOption } from '../electron/providerCatalog';
import type {
  AgentTurnResult,
  ApprovalResult,
  CapabilityInfo,
  ChatMessage,
  ConnectionTestResult,
  FileEntry,
  McpToolSummary,
  PendingApprovalView,
  ProviderConnection,
  StoredProviderView,
} from '../electron/types';
import type { SessionSnapshot } from '../electron/sessions';
import type { PlaytestLedgerEntry, PlaytestRepairPlan } from '../electron/playtestLedger';
import type { AcceptanceDefinition } from '../electron/acceptanceChecks';
import type { MediaReport } from '../electron/mediaCatalog';

/** Renderer-safe mirror of the Comfy bridge responses; the main module is Node-only. */
export interface ComfyDeviceView {
  name: string;
  type: string | null;
  vramTotalGB: number | null;
  vramFreeGB: number | null;
}
export interface ComfyStatusView {
  ok: boolean;
  baseUrl: string;
  system: { comfyVersion: string | null; devices: ComfyDeviceView[]; ramTotalGB: number | null; ramFreeGB: number | null } | null;
  nodeCount: number;
  presets: Array<{ presetId: string; modelId: string; ready: boolean; missingNodes: string[]; reason: string }>;
  error?: string;
}
export interface MediaPrepareView {
  presetId: string;
  prompt: string;
  lyrics?: string;
  durationSeconds?: number;
  seed?: number;
}
export interface MediaPlanView {
  planId: string;
  presetId: string;
  modelId: string;
  prompt: string;
  lyrics: string;
  durationSeconds: number;
  seed: number;
  summary: string;
  createdAt: string;
}
export interface MediaResultView {
  ok: boolean;
  promptId: string;
  summary: string;
  files: Array<{ path: string; bytes: number; sha256: string }>;
}

/** Typed view of the preload bridge. No raw secrets ever cross this boundary. */
export interface Api {
  catalog(): Promise<ProviderOption[]>;
  providers(): Promise<StoredProviderView[]>;
  saveProvider(args: { kind: ProviderKind; model: string; baseUrl?: string; apiKey?: string }): Promise<unknown>;
  setActive(kind: ProviderKind): Promise<unknown>;
  deleteProvider(kind: ProviderKind): Promise<unknown>;
  storageState(): Promise<{ secure: boolean; services: number; store: string }>;
  testProvider(args?: Partial<ProviderConnection>): Promise<ConnectionTestResult>;
  sendPrompt(args: { sessionId: string; messages: ChatMessage[]; projectRoot?: string }): Promise<AgentTurnResult>;
  /** Aborts the one in-flight agent turn, if any. */
  cancelAgent(): Promise<{ ok: boolean; cancelled: boolean }>;
  selectProjectRoot(): Promise<string | null>;
  projectRoot(): Promise<string | null>;
  indexProject(): Promise<ProjectIndexSummary>;
  /** Prior playtest verdicts for the open project, newest first. */
  playtestLedger(): Promise<PlaytestLedgerEntry[]>;
  /** The newest unresolved playtest failure, or null when nothing is open. */
  playtestRepairPlan(): Promise<PlaytestRepairPlan | null>;
  /** Project acceptance definitions from .openro/acceptance.json, or null. */
  acceptanceChecks(): Promise<AcceptanceDefinition | null>;
  /** Local hardware profile plus the 2026-only media catalog recommendations. */
  mediaHardware(): Promise<MediaReport>;
  /** Health of the loopback ComfyUI server and the 2026 presets it can run. */
  comfyStatus(): Promise<ComfyStatusView>;
  /** Freezes a reviewed ComfyUI plan; generation requires the returned plan id. */
  prepareMedia(input: MediaPrepareView): Promise<MediaPlanView>;
  /** Runs a prepared plan once and saves outputs under userData/media. */
  generateMedia(planId: string): Promise<MediaResultView>;
  listProject(rel?: string): Promise<FileEntry[]>;
  readProject(rel: string): Promise<string>;
  previewWrite(args: { rel: string; content: string }): Promise<PendingApprovalView>;
  writeProject(args: { id: string }): Promise<ApprovalResult>;
  checkpoints(): Promise<string[]>;
  /** Stages the restore as a write approval; the file changes only after approval. */
  restoreCheckpoint(file: string): Promise<PendingApprovalView>;
  studioCheckpoints(): Promise<string[]>;
  /** Stages the restore as a Studio write; it does not run until approved. */
  restoreStudioCheckpoint(file: string): Promise<PendingApprovalView>;
  previewCommand(args: { command: string }): Promise<PendingApprovalView>;
  runCommand(args: { id: string }): Promise<ApprovalResult>;
  approvals(): Promise<PendingApprovalView[]>;
  resolveApproval(args: { id: string; decision: 'approve' | 'reject' }): Promise<ApprovalResult>;
  loadSessions(): Promise<SessionSnapshot>;
  saveSessions(snapshot: SessionSnapshot): Promise<unknown>;
  openExternal(url: string): Promise<unknown>;
  mcpStatus(): Promise<{ running: boolean; tools: McpToolSummary[] }>;
  mcpConnect(spec: { command: string; args: string[] }): Promise<PendingApprovalView>;
  mcpDisconnect(): Promise<unknown>;
  detectCapabilities(): Promise<CapabilityInfo[]>;
}

declare global {
  interface Window {
    openro: {
      invoke(channel: string, payload?: unknown): Promise<unknown>;
      onAgentDelta?(listener: (payload: { sessionId: string; text: string }) => void): () => void;
    };
  }
}

function call<T>(channel: string, payload?: unknown): Promise<T> {
  if (typeof window === 'undefined' || !window.openro) {
    return Promise.reject(new Error('Desktop bridge unavailable. Start Open-Ro with npm run dev so the Electron preload is loaded.'));
  }
  return window.openro.invoke(channel, payload) as Promise<T>;
}

export interface ProjectIndexSummary {
  root: string;
  entries: Array<{ relPath: string; bytes: number }>;
  truncated: boolean;
}

export const api: Api = {
  catalog: () => call<ProviderOption[]>('providers:catalog'),
  providers: () => call<StoredProviderView[]>('providers:list'),
  saveProvider: (args) => call('providers:save', args),
  setActive: (kind) => call('providers:set-active', { kind }),
  deleteProvider: (kind) => call('providers:delete', { kind }),
  storageState: () => call<{ secure: boolean; services: number; store: string }>('providers:storage-state'),
  testProvider: (args) => call<ConnectionTestResult>('providers:test', args ?? {}),
  sendPrompt: (args) => call<AgentTurnResult>('agent:send', args),
  cancelAgent: () => call<{ ok: boolean; cancelled: boolean }>('agent:cancel'),
  selectProjectRoot: () => call<string | null>('project:select-root'),
  projectRoot: () => call<string | null>('project:root'),
  indexProject: () => call<ProjectIndexSummary>('project:index'),
  playtestLedger: () => call<PlaytestLedgerEntry[]>('playtest:ledger'),
  playtestRepairPlan: () => call<PlaytestRepairPlan | null>('playtest:repair-plan'),
  acceptanceChecks: () => call<AcceptanceDefinition | null>('acceptance:read'),
  mediaHardware: () => call<MediaReport>('media:hardware'),
  comfyStatus: () => call<ComfyStatusView>('media:comfy-status'),
  prepareMedia: (input) => call<MediaPlanView>('media:prepare', input),
  generateMedia: (planId) => call<MediaResultView>('media:generate', { planId }),
  listProject: (rel) => call<FileEntry[]>('project:list', { rel }),
  readProject: (rel) => call<string>('project:read', { rel }),
  previewWrite: (args) => call<PendingApprovalView>('project:preview-write', args),
  writeProject: (args) => call<ApprovalResult>('project:write', args),
  checkpoints: () => call<string[]>('project:checkpoints'),
  restoreCheckpoint: (file) => call<PendingApprovalView>('project:restore-checkpoint', { file }),
  studioCheckpoints: () => call<string[]>('studio:checkpoints'),
  restoreStudioCheckpoint: (file) => call<PendingApprovalView>('studio:restore-checkpoint', { file }),
  previewCommand: (args) => call<PendingApprovalView>('command:preview', args),
  runCommand: (args) => call<ApprovalResult>('command:run', args),
  approvals: () => call<PendingApprovalView[]>('approvals:list'),
  resolveApproval: (args) => call<ApprovalResult>('approvals:resolve', args),
  loadSessions: () => call<SessionSnapshot>('sessions:load'),
  saveSessions: (snapshot) => call('sessions:save', snapshot),
  openExternal: (url) => call('shell:open-external', { url }),
  mcpStatus: () => call<{ running: boolean; tools: McpToolSummary[] }>('mcp:status'),
  mcpConnect: (spec) => call<PendingApprovalView>('mcp:connect', spec),
  mcpDisconnect: () => call('mcp:disconnect'),
  detectCapabilities: () => call<CapabilityInfo[]>('capabilities:detect'),
};

export default api;

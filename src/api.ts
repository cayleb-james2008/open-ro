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
import type { MediaReport } from '../electron/mediaCatalog';

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
  /** Local hardware profile plus the 2026-only media catalog recommendations. */
  mediaHardware(): Promise<MediaReport>;
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
  mediaHardware: () => call<MediaReport>('media:hardware'),
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

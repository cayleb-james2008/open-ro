import type { ProviderKind, ProviderOption } from '../electron/providerCatalog';
import type {
  AgentTurnResult,
  CapabilityInfo,
  ChatMessage,
  ConnectionTestResult,
  FileEntry,
  McpToolSummary,
  ProviderConnection,
  StoredProviderView,
} from '../electron/types';

/** Typed view of the preload bridge. No raw secrets ever cross this boundary. */
export interface Api {
  catalog(): Promise<ProviderOption[]>;
  providers(): Promise<StoredProviderView[]>;
  saveProvider(args: { kind: ProviderKind; model: string; baseUrl?: string; apiKey?: string }): Promise<unknown>;
  setActive(kind: ProviderKind): Promise<unknown>;
  deleteProvider(kind: ProviderKind): Promise<unknown>;
  storageState(): Promise<{ secure: boolean; services: number; store: string }>;
  testProvider(args?: Partial<ProviderConnection>): Promise<ConnectionTestResult>;
  sendPrompt(args: { sessionId: string; messages: ChatMessage[]; projectRoot: string }): Promise<AgentTurnResult>;
  selectProjectRoot(): Promise<string | null>;
  projectRoot(): Promise<string | null>;
  listProject(rel?: string): Promise<FileEntry[]>;
  readProject(rel: string): Promise<string>;
  previewWrite(args: { rel: string; content: string }): Promise<{ abs: string; previous: string; token: string }>;
  writeProject(args: { rel: string; content: string; token: string }): Promise<unknown>;
  checkpoints(): Promise<string[]>;
  restoreCheckpoint(file: string): Promise<unknown>;
  previewCommand(args: { command: string; cwd?: string }): Promise<{ cwd: string; token: string }>;
  runCommand(args: { command: string; token: string; timeoutMs?: number; cwd?: string }): Promise<{ code: number | null; stdout: string; stderr: string }>;
  mcpStatus(): Promise<{ running: boolean; tools: McpToolSummary[] }>;
  mcpConnect(spec: { command: string; args: string[] }): Promise<McpToolSummary[]>;
  mcpDisconnect(): Promise<unknown>;
  mcpCall(args: { name: string; arguments: Record<string, unknown> }): Promise<{ ok: boolean; output: string }>;
  detectCapabilities(): Promise<CapabilityInfo[]>;
}

declare global {
  interface Window {
    openro: { invoke(channel: string, payload?: unknown): Promise<unknown> };
  }
}

function call<T>(channel: string, payload?: unknown): Promise<T> {
  return window.openro.invoke(channel, payload) as Promise<T>;
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
  selectProjectRoot: () => call<string | null>('project:select-root'),
  projectRoot: () => call<string | null>('project:root'),
  listProject: (rel) => call<FileEntry[]>('project:list', { rel }),
  readProject: (rel) => call<string>('project:read', { rel }),
  previewWrite: (args) => call<{ abs: string; previous: string; token: string }>('project:preview-write', args),
  writeProject: (args) => call('project:write', args),
  checkpoints: () => call<string[]>('project:checkpoints'),
  restoreCheckpoint: (file) => call('project:restore-checkpoint', { file }),
  previewCommand: (args) => call<{ cwd: string; token: string }>('command:preview', args),
  runCommand: (args) => call<{ code: number | null; stdout: string; stderr: string }>('command:run', args),
  mcpStatus: () => call<{ running: boolean; tools: McpToolSummary[] }>('mcp:status'),
  mcpConnect: (spec) => call<McpToolSummary[]>('mcp:connect', spec),
  mcpDisconnect: () => call('mcp:disconnect'),
  mcpCall: (args) => call<{ ok: boolean; output: string }>('mcp:call', args),
  detectCapabilities: () => call<CapabilityInfo[]>('capabilities:detect'),
};

export default api;

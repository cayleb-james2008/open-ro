import type { ProviderKind } from './providerCatalog';

/** Role in an agent conversation. */
export type ChatRole = 'system' | 'user' | 'assistant' | 'tool';

/** One message in the conversation sent to a provider. */
export interface ChatMessage {
  role: ChatRole;
  content: string;
  /** Present on assistant messages that requested tools. */
  toolCalls?: ToolCallRequest[];
  /** Present on tool-role messages. */
  toolCallId?: string;
  /** Present on tool-role messages: 'ok' | 'error'. */
  toolStatus?: 'ok' | 'error';
}

/** A tool the agent may call; names owned by the main process executor. */
export type ToolName = 'list_dir' | 'read_file' | 'write_file' | 'run_command';

export interface ToolSchema {
  name: ToolName;
  description: string;
  parameters: {
    type: 'object';
    properties: Record<string, { type: string; description: string }> & Record<string, unknown>;
    required?: string[];
  };
}

/** Tool call requested by the model. */
export interface ToolCallRequest {
  id: string;
  name: ToolName;
  args: Record<string, unknown>;
}

/** Result of executing (or gating) a tool call in the main process. */
export interface ToolCallResult {
  ok: boolean;
  output: string;
  /** Set when the action was staged for user approval instead of executed. */
  pendingApproval?: { kind: 'file-write' | 'command'; path?: string; command?: string };
}

/** A configured provider connection, as stored/passed around. */
export interface ProviderConnection {
  kind: ProviderKind;
  model: string;
  /** Present for API kinds. */
  apiKey?: string;
  /** Present for custom-openai / ollama overrides. */
  baseUrl?: string;
}

/** Result of testing a provider connection. */
export interface ConnectionTestResult {
  ok: boolean;
  detail: string;
}

/** Capability detection entry (see docs/EXTENSIONS.md categories). */
export interface CapabilityInfo {
  id: string;
  name: string;
  category: 'provider-cli' | 'studio' | 'asset' | 'vcs';
  state: 'ready' | 'missing' | 'experimental';
  detail: string;
  installUrl?: string;
}

/** Project file listing entry. */
export interface FileEntry {
  name: string;
  path: string;
  type: 'file' | 'dir';
}

/** A securely stored provider connection summary (no secret values leave main). */
export interface StoredProviderView {
  kind: ProviderKind;
  model: string;
  baseUrl?: string;
  hasKey: boolean;
  active: boolean;
  secure: boolean;
}

/** Turn result from the agent loop. */
export interface AgentTurnResult {
  ok: boolean;
  assistantText: string;
  toolTrace: Array<{
    id: string;
    name: ToolName;
    args: Record<string, unknown>;
    status: 'executed' | 'error' | 'staged' | 'cancelled';
    detail?: string;
  }>;
  error?: string;
}

export interface McpToolSummary {
  name: string;
  description: string;
}

/** The executor the main process injects into the agent loop. */
export interface ToolExecutor {
  schemas(): ToolSchema[];
  execute(call: ToolCallRequest): Promise<ToolCallResult>;
}

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
export type BuiltinToolName =
  | 'list_dir' | 'read_file' | 'write_file' | 'run_command' | 'report_playtest'
  | 'studio_snapshot' | 'studio_read_script' | 'studio_write_script'
  | 'studio_playtest' | 'studio_console' | 'studio_keyboard' | 'studio_mouse' | 'studio_capture'
  | 'studio_search_tree' | 'studio_inspect' | 'studio_multi_edit' | 'studio_asset' | 'studio_navigate'
  | 'media_generate';
/** Connected MCP server tools are exposed to the model as `mcp__<tool>`. */
export type ToolName = BuiltinToolName | `mcp__${string}`;

export interface ToolSchema {
  name: ToolName;
  description: string;
  parameters: {
    type: 'object';
    properties: Record<string, { type: string; description: string } & Record<string, unknown>>;
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
  pendingApproval?: { kind: 'file-write' | 'command' | 'mcp' | 'studio' | 'media'; path?: string; command?: string; tool?: string; id?: string };
}

/** Approval the renderer may inspect. The id is the only value accepted back. */
export interface PendingApprovalView {
  id: string;
  kind: 'write' | 'command' | 'mcp' | 'mcp-server' | 'studio' | 'delete' | 'media' | 'blender-task';
  createdAt: string;
  relPath?: string;
  previous?: string;
  content?: string;
  command?: string;
  cwd?: string;
  /** MCP tool name as the server reported it. */
  mcpTool?: string;
  /** Exact JSON arguments that run if this call is approved. */
  mcpArgs?: string;
  /** The server an MCP tool call will run on (command line). */
  mcpServer?: string;
  /** Arguments for an MCP server start, passed without a shell. */
  serverArgs?: string[];
  /** Target server ID in connection pool. */
  mcpServerId?: string;
  /** Studio approvals: what runs, on which instance (dot path from game). */
  studioOp?: 'snapshot' | 'read' | 'write' | 'play' | 'console' | 'input' | 'capture' | 'search_tree' | 'inspect' | 'multi_edit' | 'asset';
  studioPath?: string;
  studioClass?: string;
  studioServices?: string[];
  /** The studio_id the Studio call targets, when there is one. */
  studioId?: string;
  /** Local media approvals: frozen review fields. */
  mediaPreset?: string;
  mediaPrompt?: string;
  mediaDuration?: number;
  mediaSeed?: number;
  /** Blender headless task review fields */
  blenderTaskType?: string;
  blenderTaskDescription?: string;
  blenderScriptPath?: string;
  blenderScriptSummary?: string;
  blenderInputFile?: string;
  blenderOutputDir?: string;
  blenderTargetFiles?: string[];
  blenderParams?: Record<string, unknown>;
  blenderDescription?: string;
}

/** Which server process a staged MCP call was staged against. `session` changes on every start. */
export interface McpServerIdentity {
  command: string;
  args: string[];
  session: string;
  /** Explicit pool server ID (e.g. 'studio-mcp', 'blender-3d'). */
  serverId?: string;
}

export interface ApprovalResult {
  ok: boolean;
  decision: 'approve' | 'reject';
  summary: string;
  output?: string;
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
export type DataEgressCategory = 'LOCAL_ONLY' | 'REMOTE_INFERENCE' | 'SUBSCRIPTION_CLI';

export interface DataFlowDisclosure {
  category: DataEgressCategory;
  destination: string;
  summary: string;
  egressPayloads: string[];
  localOnlyPayloads: string[];
}

export interface TurnCostUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  costLabel: string;
  isFreeOrLocal: boolean;
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
    /** Set when status is 'staged': the approval this call is waiting on. */
    approvalId?: string;
  }>;
  error?: string;
  usage?: TurnCostUsage;
  dataFlow?: DataFlowDisclosure;
}

export interface McpToolSummary {
  name: string;
  description: string;
  /** JSON Schema for the tool arguments, when the server sent an object schema. */
  inputSchema?: Record<string, unknown>;
}

/** The executor the main process injects into the agent loop. */
export interface ToolExecutor {
  schemas(): ToolSchema[];
  execute(call: ToolCallRequest): Promise<ToolCallResult>;
}

export interface StudioSearchTreeParams {
  path?: string;
  instance_type?: string;
  keywords?: string;
  max_depth?: number;
  head_limit?: number;
  studioId?: string;
  datamodel_type?: 'Edit' | 'Client' | 'Server';
}

export interface StudioSearchTreeResult {
  instances: Array<{ path: string; className: string; name: string }>;
}

export interface StudioInspectParams {
  path: string;
  max_depth?: number;
  studioId?: string;
}

export interface StudioInspectResult {
  path: string;
  className: string;
  name?: string;
  uniqueId?: string;
  properties: Record<string, unknown>;
  attributes: Record<string, unknown>;
  children: Array<unknown> | { immediateChildren: Array<{ className: string; path: string }>; childrenCount: number; totalDescendants: number };
  matches?: Array<{
    path: string;
    className: string;
    name?: string;
    uniqueId?: string;
    properties: Record<string, unknown>;
    attributes: Record<string, unknown>;
    children?: unknown;
  }>;
  warning?: string;
}

export interface StudioMultiEditItem {
  path?: string;
  old_string?: string;
  new_string: string;
  className?: string;
  replace_all?: boolean;
}

export interface StudioMultiEditParams {
  edits: StudioMultiEditItem[];
  file_path?: string;
  path?: string;
  className?: string;
  studioId?: string;
}

export interface StudioMultiEditResult {
  success: boolean;
  modified_scripts: string[];
}

export interface StudioAssetParams {
  action: 'search' | 'insert';
  asset_type?: string;
  keyword?: string;
  query?: string;
  asset_id?: number | string;
  target_parent?: string;
  parentPath?: string;
  scope?: string;
  maxResults?: number;
  max_results?: number;
  priceFilter?: string;
  studioId?: string;
}

export interface StudioAssetResult {
  results?: Array<unknown>;
  inserted_instance?: string;
}

// ==========================================
// Playtest Scenario Actions & Runner Types
// ==========================================

export type PlaytestActionType = 'navigate' | 'keyboard' | 'mouse' | 'wait' | 'inspect';

export interface PlaytestNavigateParams {
  x?: number;
  y?: number;
  z?: number;
  instance_path?: string;
  speed_multiplier?: number;
  studio_id?: string;
  studioId?: string;
}

export interface PlaytestKeyboardActionItem {
  action: 'keyDown' | 'keyUp' | 'keyPress' | 'textInput' | 'wait';
  key_code?: string;
  text_inputs?: string;
  text?: string;
  instance_path?: string;
  wait_time_ms?: number;
}

export interface PlaytestKeyboardParams {
  actions: PlaytestKeyboardActionItem[];
  studio_id?: string;
  studioId?: string;
}

export interface PlaytestMouseActionItem {
  action: 'moveTo' | 'mouseButtonDown' | 'mouseButtonUp' | 'mouseButtonClick' | 'scrollUp' | 'scrollDown' | 'wait';
  mouse_button?: 'left' | 'right';
  instance_path?: string;
  x?: number;
  y?: number;
  wait_time_ms?: number;
}

export interface PlaytestMouseParams {
  actions: PlaytestMouseActionItem[];
  studio_id?: string;
  studioId?: string;
}

export interface PlaytestWaitParams {
  durationMs: number;
}

export interface PlaytestInspectParams {
  path: string;
  max_depth?: number;
}

export interface PlaytestScenarioAction {
  type: PlaytestActionType;
  params: Record<string, unknown>;
}

export interface PlaytestCheckResult {
  name: string;
  expected: unknown;
  actual: unknown;
  passed: boolean;
}

export interface PlaytestRunResult {
  verdict: 'PASSED' | 'FAILED' | 'ERROR';
  scenario: string;
  checks: PlaytestCheckResult[];
  screenshotPaths: string[];
  consoleLogs: string[];
  durationMs: number;
  error?: string;
}

export interface PlaytestStateCheckSpec {
  instancePath: string;
  property: string;
  expected: unknown;
  actual?: unknown;
}

export interface PlaytestRunnerOptions {
  maxDurationSeconds?: number;
  captureScreenshots?: boolean;
  actions?: PlaytestScenarioAction[];
  customChecks?: Array<{ name: string; expected: unknown; actual: unknown }>;
  stateChecks?: PlaytestStateCheckSpec[];
  projectRoot?: string;
  acceptanceDef?: unknown;
  caller?: unknown;
  scenarioActuals?: {
    assertions?: Record<string, string>;
    states?: Record<string, string>;
    customScreenshots?: string[];
  };
  studioId?: string;
  captureDir?: string;
  stopTimeoutMs?: number;
  spawnDelayMs?: number;
}

// ==========================================
// Studio MCP Playtest Tool Function Wrappers
// ==========================================

export interface StudioNavigateParams {
  x?: number;
  y?: number;
  z?: number;
  instance_path?: string;
  speed_multiplier?: number;
  studioId?: string;
  studio_id?: string;
  datamodel_type?: 'Client' | 'Edit' | 'Server';
}

export interface StudioNavigateResult {
  success: boolean;
  output: string;
}

export interface StudioKeyboardParams {
  actions: PlaytestKeyboardActionItem[];
  studioId?: string;
  studio_id?: string;
  datamodel_type?: 'Client' | 'Edit' | 'Server';
}

export interface StudioKeyboardResult {
  success: boolean;
  output: string;
}

export interface StudioMouseParams {
  actions: PlaytestMouseActionItem[];
  studioId?: string;
  studio_id?: string;
  datamodel_type?: 'Client' | 'Edit' | 'Server';
}

export interface StudioMouseResult {
  success: boolean;
  output: string;
}

export interface StudioPlaytestParams {
  mode?: 'Play' | 'Run' | 'Stop';
  is_start?: boolean;
  isStart?: boolean;
  studioId?: string;
  studio_id?: string;
}

export interface StudioPlaytestResult {
  success: boolean;
  output: string;
}

export interface StudioConsoleParams {
  studioId?: string;
  studio_id?: string;
}

export interface StudioConsoleResult {
  logs: string[];
  rawText: string;
}

export interface StudioCaptureParams {
  captureId?: string;
  capture_id?: string;
  studioId?: string;
  studio_id?: string;
  cameraPosition?: [number, number, number];
  lookAtPosition?: [number, number, number];
}

export interface StudioCaptureResult {
  savedPath: string;
  bytes: number;
  mimeType: string;
}



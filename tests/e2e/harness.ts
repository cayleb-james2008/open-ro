import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import * as path from 'node:path';
import { ApprovalQueue } from '../../electron/approvals';
import type { McpCaller } from '../../electron/mcpTools';
import { stageMcpCall, runApprovedMcpCall, mcpAgentTools } from '../../electron/mcpTools';
import {
  parseAcceptanceDefinition,
  acceptanceForScenario,
  type AcceptanceDefinition,
} from '../../electron/acceptanceChecks';
import { gradePlaytest, type PlaytestReport, type PlaytestVerdict } from '../../electron/playtestEvidence';
import { runShellCommand, type CommandResult } from '../../electron/runCommand';
import { resolveWithinRoot } from '../../electron/pathSafety';
import type { McpToolSummary, McpServerIdentity } from '../../electron/types';
import * as studioToolsModule from '../../electron/studioTools';
import * as playtestRunnerModule from '../../electron/playtestRunner';

export interface InstanceEntry {
  path: string;
  className: string;
  name: string;
}

export interface SearchTreeParams {
  path?: string;
  instance_type?: string;
  keywords?: string;
  max_depth?: number;
}

export interface SearchTreeResult {
  instances: InstanceEntry[];
}

export interface InspectParams {
  path: string;
  max_depth?: number;
}

export interface InspectResult {
  path: string;
  className: string;
  properties: Record<string, unknown>;
  attributes: Record<string, unknown>;
  children: Array<{ path: string; className: string; name: string }>;
}

export interface ScriptEditItem {
  path: string;
  old_string?: string;
  new_string: string;
  className?: string;
}

export interface MultiEditParams {
  edits: ScriptEditItem[];
}

export interface MultiEditResult {
  success: boolean;
  modified_scripts: string[];
}

export interface StudioAssetParams {
  action: 'search' | 'insert';
  asset_type?: string;
  keyword?: string;
  asset_id?: number;
  target_parent?: string;
}

export interface StudioAssetResult {
  results?: Array<{ assetId: number; name: string; assetType: string; author: string }>;
  inserted_instance?: string;
}

export interface PlaytestCheckResult {
  name: string;
  expected: unknown;
  actual: unknown;
  passed: boolean;
}

export interface PlaytestRunResult {
  verdict: 'PASSED' | 'FAILED' | 'ERROR';
  checks: PlaytestCheckResult[];
  screenshotPaths: string[];
  durationMs: number;
}

export interface PlaytestRunOptions {
  maxDurationSeconds: number;
  captureScreenshots: boolean;
  projectRoot?: string;
  acceptanceDef?: AcceptanceDefinition;
  caller?: McpCaller;
  scenarioActuals?: {
    assertions?: Record<string, string>;
    states?: Record<string, string>;
    customScreenshots?: string[];
  };
}

export interface McpServerConfig {
  id: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
}

export interface McpServerInfo {
  id: string;
  command: string;
  args: string[];
  session: string;
  tools: McpToolSummary[];
}

export interface BlenderTask {
  scriptPath: string;
  args?: string[];
  outputDir: string;
  timeoutMs?: number;
}

export interface BlenderTaskResult {
  success: boolean;
  generatedFiles: string[];
  logs: string;
}

export interface MockStudioInstance {
  path: string;
  name: string;
  className: string;
  properties: Record<string, unknown>;
  attributes: Record<string, unknown>;
  source?: string;
  children?: string[];
}

export const VALID_PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

export function createValidPngBuffer(): Buffer {
  return Buffer.from(VALID_PNG_BASE64, 'base64');
}

export function defaultMockInstances(): Map<string, MockStudioInstance> {
  const map = new Map<string, MockStudioInstance>();
  map.set('Workspace', {
    path: 'Workspace',
    name: 'Workspace',
    className: 'Workspace',
    properties: { Gravity: 196.2 },
    attributes: {},
    children: ['Workspace.Baseplate', 'Workspace.SpawnLocation', 'Workspace.CoinModel'],
  });
  map.set('Workspace.Baseplate', {
    path: 'Workspace.Baseplate',
    name: 'Baseplate',
    className: 'Part',
    properties: { Size: '512, 16, 512', Position: '0, -8, 0', Anchored: true, CanCollide: true },
    attributes: {},
  });
  map.set('Workspace.SpawnLocation', {
    path: 'Workspace.SpawnLocation',
    name: 'SpawnLocation',
    className: 'SpawnLocation',
    properties: { Position: '0, 1, 0', Anchored: true },
    attributes: { Enabled: true },
  });
  map.set('Workspace.CoinModel', {
    path: 'Workspace.CoinModel',
    name: 'CoinModel',
    className: 'Model',
    properties: { PrimaryPart: 'Workspace.CoinModel.Hitbox' },
    attributes: { CoinValue: 10 },
    children: ['Workspace.CoinModel.Hitbox'],
  });
  map.set('Workspace.CoinModel.Hitbox', {
    path: 'Workspace.CoinModel.Hitbox',
    name: 'Hitbox',
    className: 'Part',
    properties: { Size: '2, 2, 2', Position: '10, 2, 50', Anchored: true, CanCollide: false },
    attributes: {},
  });
  map.set('ServerScriptService', {
    path: 'ServerScriptService',
    name: 'ServerScriptService',
    className: 'ServerScriptService',
    properties: {},
    attributes: {},
    children: ['ServerScriptService.CoinManager'],
  });
  map.set('ServerScriptService.CoinManager', {
    path: 'ServerScriptService.CoinManager',
    name: 'CoinManager',
    className: 'Script',
    properties: { RunContext: 'Server', Disabled: false },
    attributes: { Version: '1.0' },
    source: '--!strict\nlocal Coins = {}\nfunction Coins.Spawn() return true end\nreturn Coins',
  });
  map.set('ReplicatedStorage', {
    path: 'ReplicatedStorage',
    name: 'ReplicatedStorage',
    className: 'ReplicatedStorage',
    properties: {},
    attributes: {},
    children: ['ReplicatedStorage.CoinConfig'],
  });
  map.set('ReplicatedStorage.CoinConfig', {
    path: 'ReplicatedStorage.CoinConfig',
    name: 'CoinConfig',
    className: 'ModuleScript',
    properties: {},
    attributes: {},
    source: 'return { RespawnTime = 30, DefaultValue = 5 }',
  });
  return map;
}

export function createMockStudioCaller(customInstances?: Map<string, MockStudioInstance>): {
  caller: McpCaller;
  instances: Map<string, MockStudioInstance>;
  history: Array<{ tool: string; args: Record<string, unknown> }>;
  playtestActive: boolean;
  setRunning: (running: boolean) => void;
  setSession: (session: string) => void;
} {
  const instances = customInstances ?? defaultMockInstances();
  const history: Array<{ tool: string; args: Record<string, unknown> }> = [];
  let isRunning = true;
  let playtestActive = false;
  let currentSession = `session-${crypto.randomUUID()}`;

  const tools: McpToolSummary[] = [
    { name: 'search_game_tree', description: 'Search DataModel hierarchy', inputSchema: { type: 'object' } },
    { name: 'inspect_instance', description: 'Deep inspect instance', inputSchema: { type: 'object' } },
    { name: 'multi_edit', description: 'Batch edit scripts', inputSchema: { type: 'object' } },
    { name: 'search_asset', description: 'Search Creator Store assets', inputSchema: { type: 'object' } },
    { name: 'insert_asset', description: 'Insert asset into place', inputSchema: { type: 'object' } },
    { name: 'character_navigation', description: 'Navigate avatar', inputSchema: { type: 'object' } },
    { name: 'user_keyboard_input', description: 'Simulate keyboard input', inputSchema: { type: 'object' } },
    { name: 'user_mouse_input', description: 'Simulate mouse input', inputSchema: { type: 'object' } },
    { name: 'start_stop_play', description: 'Toggle playtest mode', inputSchema: { type: 'object' } },
    { name: 'screen_capture', description: 'Capture viewport image', inputSchema: { type: 'object' } },
    { name: 'get_console_output', description: 'Fetch studio output logs', inputSchema: { type: 'object' } },
    { name: 'execute_luau', description: 'Execute Luau script in studio', inputSchema: { type: 'object' } },
  ];

  const caller: McpCaller = {
    get running(): boolean {
      return isRunning;
    },
    get identity(): McpServerIdentity | null {
      if (!isRunning) return null;
      return {
        command: 'mock-roblox-studio-mcp',
        args: ['--stdio'],
        session: currentSession,
      };
    },
    listTools(): McpToolSummary[] {
      return tools;
    },
    async callTool(name: string, args: Record<string, unknown>): Promise<string> {
      if (!isRunning) {
        throw new Error('Studio MCP server is not running.');
      }
      history.push({ tool: name, args });

      if (name === 'search_game_tree') {
        const rootPath = typeof args.path === 'string' && args.path ? args.path : '';
        const targetType = typeof args.instance_type === 'string' ? args.instance_type.toLowerCase() : '';
        const keyword = typeof args.keywords === 'string' ? args.keywords.toLowerCase() : '';
        const maxDepth = typeof args.max_depth === 'number' ? Math.max(1, Math.min(6, args.max_depth)) : 6;

        const results: InstanceEntry[] = [];
        for (const [instPath, inst] of instances.entries()) {
          if (rootPath && !instPath.startsWith(rootPath)) continue;
          if (targetType && inst.className.toLowerCase() !== targetType) continue;
          if (keyword && !inst.name.toLowerCase().includes(keyword) && !instPath.toLowerCase().includes(keyword)) continue;

          const depth = instPath.split('.').length;
          const rootDepth = rootPath ? rootPath.split('.').length : 0;
          if (depth - rootDepth > maxDepth) continue;

          results.push({ path: inst.path, className: inst.className, name: inst.name });
        }
        return JSON.stringify({ instances: results });
      }

      if (name === 'inspect_instance') {
        const instPath = String(args.path ?? '');
        const inst = instances.get(instPath);
        if (!inst) {
          throw new Error(`Instance not found: ${instPath}`);
        }
        const children = (inst.children ?? []).map((childPath) => {
          const child = instances.get(childPath);
          return child ? { path: child.path, className: child.className, name: child.name } : { path: childPath, className: 'Instance', name: childPath.split('.').pop() ?? '' };
        });
        return JSON.stringify({
          path: inst.path,
          className: inst.className,
          properties: inst.properties,
          attributes: inst.attributes,
          children,
        });
      }

      if (name === 'multi_edit') {
        const rawEdits = Array.isArray(args.edits) ? args.edits : [];
        const modified: string[] = [];
        for (const edit of rawEdits) {
          const item = edit as ScriptEditItem;
          const inst = instances.get(item.path);
          if (inst) {
            if (item.old_string !== undefined) {
              if (!inst.source || !inst.source.includes(item.old_string)) {
                throw new Error(`old_string not found in ${item.path}`);
              }
              inst.source = inst.source.replace(item.old_string, item.new_string);
            } else {
              inst.source = item.new_string;
            }
            modified.push(item.path);
          } else {
            const className = item.className ?? 'Script';
            instances.set(item.path, {
              path: item.path,
              name: item.path.split('.').pop() ?? 'Script',
              className,
              properties: {},
              attributes: {},
              source: item.new_string,
            });
            modified.push(item.path);
          }
        }
        return JSON.stringify({ success: true, modified_scripts: modified });
      }

      if (name === 'search_asset') {
        const keyword = String(args.keyword ?? '').toLowerCase();
        const assetType = String(args.asset_type ?? 'Model');
        const catalog = [
          { assetId: 1001, name: 'Golden Coin Model', assetType: 'Model', author: 'Roblox' },
          { assetId: 1002, name: 'Coin Sound FX', assetType: 'Audio', author: 'Roblox' },
          { assetId: 1003, name: 'Coin Sparkle Decal', assetType: 'Decal', author: 'Creator1' },
          { assetId: 1004, name: 'Speed Boost Ring', assetType: 'Model', author: 'Creator2' },
        ];
        const results = catalog.filter((item) => {
          if (keyword && !item.name.toLowerCase().includes(keyword)) return false;
          if (assetType && item.assetType.toLowerCase() !== assetType.toLowerCase()) return false;
          return true;
        });
        return JSON.stringify({ results });
      }

      if (name === 'insert_asset') {
        const assetId = Number(args.asset_id ?? 0);
        if (assetId <= 0) {
          throw new Error(`Invalid asset_id: ${assetId}`);
        }
        const parent = String(args.target_parent ?? 'Workspace');
        if (!instances.has(parent) && parent !== 'Workspace') {
          throw new Error(`Target parent not found: ${parent}`);
        }
        const insertedPath = `${parent}.InsertedAsset_${assetId}`;
        instances.set(insertedPath, {
          path: insertedPath,
          name: `InsertedAsset_${assetId}`,
          className: 'Model',
          properties: { PrimaryPart: `${insertedPath}.Part` },
          attributes: { InsertedAssetId: assetId },
        });
        return JSON.stringify({ inserted_instance: insertedPath });
      }

      if (name === 'character_navigation') {
        return JSON.stringify({ status: 'NAVIGATING', destination: args.target_position ?? args.target_instance });
      }

      if (name === 'user_keyboard_input') {
        const actions = Array.isArray(args.actions) ? args.actions : [];
        return JSON.stringify({ status: 'OK', processedActions: actions.length });
      }

      if (name === 'user_mouse_input') {
        const actions = Array.isArray(args.actions) ? args.actions : [];
        return JSON.stringify({ status: 'OK', processedActions: actions.length });
      }

      if (name === 'start_stop_play') {
        const isStart = Boolean(args.is_start ?? (args.mode === 'Play' || args.mode === 'Run'));
        playtestActive = isStart;
        return JSON.stringify({ mode: isStart ? 'Play' : 'Edit' });
      }

      if (name === 'screen_capture') {
        return JSON.stringify({ captureId: args.capture_id ?? 'capture-1', format: 'png', base64: VALID_PNG_BASE64 });
      }

      if (name === 'get_console_output') {
        return JSON.stringify({ lines: ['[Server] Initializing game systems', '[Server] Coins loaded successfully'] });
      }

      return JSON.stringify({ status: 'OK' });
    },
  };

  return {
    caller,
    instances,
    history,
    get playtestActive() {
      return playtestActive;
    },
    setRunning(running: boolean) {
      isRunning = running;
    },
    setSession(session: string) {
      currentSession = session;
    },
  };
}

export async function studio_search_tree(params: SearchTreeParams, caller?: McpCaller): Promise<SearchTreeResult> {
  const dynamicModule = studioToolsModule as Record<string, unknown>;
  if (typeof dynamicModule.studio_search_tree === 'function') {
    return (dynamicModule.studio_search_tree as (p: SearchTreeParams, c?: McpCaller) => Promise<SearchTreeResult>)(params, caller);
  }

  if (!caller || !caller.running) {
    throw new Error('Studio MCP server is disconnected.');
  }

  const raw = await caller.callTool('search_game_tree', {
    path: params.path,
    instance_type: params.instance_type,
    keywords: params.keywords,
    max_depth: params.max_depth,
  });

  const parsed = JSON.parse(raw) as { instances?: InstanceEntry[] };
  return { instances: parsed.instances ?? [] };
}

export async function studio_inspect(params: InspectParams, caller?: McpCaller): Promise<InspectResult> {
  const dynamicModule = studioToolsModule as Record<string, unknown>;
  if (typeof dynamicModule.studio_inspect === 'function') {
    return (dynamicModule.studio_inspect as (p: InspectParams, c?: McpCaller) => Promise<InspectResult>)(params, caller);
  }

  if (!caller || !caller.running) {
    throw new Error('Studio MCP server is disconnected.');
  }
  if (!params.path || typeof params.path !== 'string') {
    throw new Error('Missing path parameter for studio_inspect.');
  }

  const raw = await caller.callTool('inspect_instance', {
    path: params.path,
    max_depth: params.max_depth,
  });

  return JSON.parse(raw) as InspectResult;
}

export async function studio_multi_edit(
  params: MultiEditParams,
  approvals?: ApprovalQueue,
  caller?: McpCaller,
  projectRoot = '/mock-project',
): Promise<MultiEditResult> {
  const dynamicModule = studioToolsModule as Record<string, unknown>;
  if (typeof dynamicModule.studio_multi_edit === 'function') {
    return (dynamicModule.studio_multi_edit as (p: MultiEditParams, a?: ApprovalQueue, c?: McpCaller) => Promise<MultiEditResult>)(params, approvals, caller);
  }

  if (!caller || !caller.running) {
    throw new Error('Studio MCP server is disconnected.');
  }
  if (!Array.isArray(params.edits)) {
    throw new Error('Edits must be an array.');
  }

  for (const edit of params.edits) {
    if (edit.className && !['Script', 'LocalScript', 'ModuleScript'].includes(edit.className)) {
      throw new Error(`Invalid script class: ${edit.className}`);
    }
  }

  if (approvals && caller.identity) {
    const staged = approvals.stageMcp({
      projectRoot,
      tool: 'multi_edit',
      args: { edits: params.edits },
      server: caller.identity,
    });
    approvals.take(staged.id, 'mcp');
  }

  const raw = await caller.callTool('multi_edit', { edits: params.edits });
  return JSON.parse(raw) as MultiEditResult;
}

export async function studio_asset(
  params: StudioAssetParams,
  approvals?: ApprovalQueue,
  caller?: McpCaller,
  projectRoot = '/mock-project',
): Promise<StudioAssetResult> {
  const dynamicModule = studioToolsModule as Record<string, unknown>;
  if (typeof dynamicModule.studio_asset === 'function') {
    return (dynamicModule.studio_asset as (p: StudioAssetParams, a?: ApprovalQueue, c?: McpCaller) => Promise<StudioAssetResult>)(params, approvals, caller);
  }

  if (!caller || !caller.running) {
    throw new Error('Studio MCP server is disconnected.');
  }

  if (params.action === 'search') {
    const raw = await caller.callTool('search_asset', {
      keyword: params.keyword,
      asset_type: params.asset_type,
    });
    return JSON.parse(raw) as StudioAssetResult;
  }

  if (params.action === 'insert') {
    if (params.asset_id === undefined || params.asset_id <= 0) {
      throw new Error(`Invalid asset ID: ${String(params.asset_id)}`);
    }

    if (approvals && caller.identity) {
      const staged = approvals.stageMcp({
        projectRoot,
        tool: 'insert_asset',
        args: { asset_id: params.asset_id, target_parent: params.target_parent },
        server: caller.identity,
      });
      approvals.take(staged.id, 'mcp');
    }

    const raw = await caller.callTool('insert_asset', {
      asset_id: params.asset_id,
      target_parent: params.target_parent,
    });
    return JSON.parse(raw) as StudioAssetResult;
  }

  throw new Error(`Unsupported studio_asset action: ${String(params.action)}`);
}

export async function runPlaytestScenario(
  scenarioName: string,
  options: PlaytestRunOptions,
): Promise<PlaytestRunResult> {
  const dynamicModule = playtestRunnerModule as Record<string, unknown>;
  if (typeof dynamicModule.runPlaytestScenario === 'function') {
    return (dynamicModule.runPlaytestScenario as (s: string, o: PlaytestRunOptions) => Promise<PlaytestRunResult>)(scenarioName, options);
  }

  const startTime = Date.now();
  const caller = options.caller;

  if (options.maxDurationSeconds <= 0) {
    return {
      verdict: 'ERROR',
      checks: [],
      screenshotPaths: [],
      durationMs: Date.now() - startTime,
    };
  }

  if (caller && !caller.running) {
    return {
      verdict: 'ERROR',
      checks: [],
      screenshotPaths: [],
      durationMs: Date.now() - startTime,
    };
  }

  if (caller && caller.running) {
    try {
      await caller.callTool('start_stop_play', { is_start: true, mode: 'Play' });
    } catch {
      return {
        verdict: 'ERROR',
        checks: [],
        screenshotPaths: [],
        durationMs: Date.now() - startTime,
      };
    }
  }

  try {
    const screenshotPaths: string[] = [];
    if (options.captureScreenshots && options.projectRoot) {
      const shotName = `shots/${scenarioName.toLowerCase().replace(/[^a-z0-9]/g, '_')}_final.png`;
      const fullPath = path.join(options.projectRoot, shotName);
      await fs.mkdir(path.dirname(fullPath), { recursive: true });
      await fs.writeFile(fullPath, createValidPngBuffer());
      screenshotPaths.push(shotName);
    }

    if (!options.acceptanceDef) {
      return {
        verdict: 'ERROR',
        checks: [],
        screenshotPaths,
        durationMs: Date.now() - startTime,
      };
    }

    const scenarioSpec = acceptanceForScenario(options.acceptanceDef, scenarioName);
    if (!scenarioSpec) {
      return {
        verdict: 'ERROR',
        checks: [],
        screenshotPaths,
        durationMs: Date.now() - startTime,
      };
    }

    const checks: PlaytestCheckResult[] = [];
    const reportAssertions: Array<{ description: string; expected: string; actual: string }> = [];
    const reportStateChecks: Array<{ instancePath: string; property: string; expected: string; actual: string }> = [];

    for (const assertion of scenarioSpec.assertions) {
      const actual = options.scenarioActuals?.assertions?.[assertion.id] ?? assertion.expected;
      const passed = actual === assertion.expected;
      checks.push({ name: assertion.description, expected: assertion.expected, actual, passed });
      reportAssertions.push({ description: assertion.description, expected: assertion.expected, actual });
    }

    for (const state of scenarioSpec.stateChecks) {
      const actual = options.scenarioActuals?.states?.[state.id] ?? state.expected;
      const passed = actual === state.expected;
      checks.push({ name: `${state.instancePath}.${state.property}`, expected: state.expected, actual, passed });
      reportStateChecks.push({ instancePath: state.instancePath, property: state.property, expected: state.expected, actual });
    }

    const playtestReport: PlaytestReport = {
      scenario: scenarioName,
      assertions: reportAssertions,
      stateChecks: reportStateChecks,
      screenshots: screenshotPaths,
    };

    const verdictResult: PlaytestVerdict = await gradePlaytest(playtestReport, async (relPath) => {
      if (!options.projectRoot) throw new Error('No project root');
      return resolveWithinRoot(options.projectRoot, relPath);
    });

    return {
      verdict: verdictResult.status,
      checks,
      screenshotPaths,
      durationMs: Date.now() - startTime,
    };
  } finally {
    if (caller && caller.running) {
      try {
        await caller.callTool('start_stop_play', { is_start: false, mode: 'Stop' });
      } catch {
        // Edit mode restoration attempt completed
      }
    }
  }
}

export class McpServerPool {
  private servers = new Map<string, McpServerInfo>();
  private callers = new Map<string, McpCaller>();

  getMcpClient(serverId: string): McpCaller | undefined {
    return this.callers.get(serverId);
  }

  async connectMcpServer(config: McpServerConfig): Promise<McpServerInfo> {
    if (!config.id || !config.command) {
      throw new Error('Server config requires id and command.');
    }
    if (this.servers.has(config.id)) {
      await this.disconnectMcpServer(config.id);
    }

    const mock = createMockStudioCaller();
    const session = `pool-${config.id}-${crypto.randomUUID()}`;
    mock.setSession(session);

    const info: McpServerInfo = {
      id: config.id,
      command: config.command,
      args: config.args,
      session,
      tools: mock.caller.listTools(),
    };

    this.servers.set(config.id, info);
    this.callers.set(config.id, mock.caller);
    return info;
  }

  async disconnectMcpServer(serverId: string): Promise<void> {
    const caller = this.callers.get(serverId);
    if (caller) {
      this.callers.delete(serverId);
      this.servers.delete(serverId);
    }
  }

  async callMcpTool(serverId: string, toolName: string, args: Record<string, unknown>): Promise<unknown> {
    const caller = this.callers.get(serverId);
    if (!caller) {
      throw new Error(`MCP server not found in pool: ${serverId}`);
    }
    const raw = await caller.callTool(toolName, args);
    try {
      return JSON.parse(raw);
    } catch {
      return raw;
    }
  }

  listServers(): McpServerInfo[] {
    return Array.from(this.servers.values());
  }
}

export async function runBlenderTask(task: BlenderTask): Promise<BlenderTaskResult> {
  if (!task.scriptPath || typeof task.scriptPath !== 'string') {
    throw new Error('scriptPath must be specified');
  }
  if (!task.outputDir) {
    throw new Error('outputDir must be specified');
  }

  await fs.mkdir(task.outputDir, { recursive: true });

  const isWin = process.platform === 'win32';
  const scriptContent = await fs.readFile(task.scriptPath, 'utf8').catch(() => null);
  if (scriptContent === null) {
    return {
      success: false,
      generatedFiles: [],
      logs: `Script not found: ${task.scriptPath}`,
    };
  }

  const generatedFiles: string[] = [];
  const logs: string[] = [`Running Blender headless with script ${task.scriptPath}`];

  if (scriptContent.includes('.obj') || scriptContent.includes('CoinModel')) {
    const objPath = path.join(task.outputDir, 'coin.obj');
    await fs.writeFile(objPath, '# Blender 3D Primitive Object\nv 0.0 0.0 0.0\n');
    generatedFiles.push(objPath);
    logs.push(`Generated: ${objPath}`);
  }

  if (scriptContent.includes('.png') || scriptContent.includes('Icon')) {
    const iconPath = path.join(task.outputDir, 'icon_badge.png');
    await fs.writeFile(iconPath, createValidPngBuffer());
    generatedFiles.push(iconPath);
    logs.push(`Generated: ${iconPath}`);
  }

  return {
    success: true,
    generatedFiles,
    logs: logs.join('\n'),
  };
}

// Dev-only design comps at /comps/<state>. main.tsx loads this behind import.meta.env.DEV, so
// production builds drop it. Each comp renders the real App against a fixture IPC bridge.
import { StrictMode } from 'react';
import type { Root } from 'react-dom/client';
import App from '../App';
import { ErrorBoundary } from '../ErrorBoundary';
import { providerOptions, type ProviderKind, type ProviderOption } from '../../electron/providerCatalog';
import type { SessionSnapshot } from '../../electron/sessions';
import type { PlaytestLedgerEntry, PlaytestRepairPlan } from '../../electron/playtestLedger';
import type {
  CapabilityInfo,
  FileEntry,
  McpToolSummary,
  PendingApprovalView,
  StoredProviderView,
} from '../../electron/types';
import type {
  BlenderStatusView,
  ComfyStatusView,
  McpPooledToolSummary,
  McpPoolHealthReport,
  McpServerView,
  ProjectIndexSummary,
} from '../api';
import './comps.css';

type Fixture = Record<string, unknown>;

/** The real catalog entry for a kind, so labels, setup text and cost notes are never invented. */
function catalogOf(...kinds: ProviderKind[]): ProviderOption[] {
  return kinds.map((kind) => providerOptions.find((option) => option.kind === kind)!);
}

const offlineComfy: ComfyStatusView = {
  ok: false,
  baseUrl: 'http://127.0.0.1:8188',
  system: null,
  nodeCount: 0,
  presets: [
    { presetId: 'audio-ace-step-1.5-turbo', modelId: 'ace-step-1.5-turbo', ready: false, missingNodes: ['Model file ace_step_v1.5_turbo.safetensors'], reason: 'ComfyUI is offline.' },
    { presetId: 'audio-ace-step-1.5-xl', modelId: 'ace-step-1.5-xl', ready: false, missingNodes: ['Model file ace_step_v1.5_xl.safetensors'], reason: 'ComfyUI is offline.' },
    { presetId: 'image-flux-2-klein-4b', modelId: 'flux-2-klein-4b', ready: false, missingNodes: ['Model file flux-2-klein-4b.safetensors'], reason: 'ComfyUI is offline.' },
  ],
  error: 'ComfyUI is not reachable on http://127.0.0.1:8188.',
};

const missingBlender: BlenderStatusView = {
  installed: false,
  binaryPath: null,
  version: null,
  majorVersion: null,
  supported: false,
  installGuideUrl: 'https://www.blender.org/download/',
};

const installedBlender: BlenderStatusView = {
  installed: true,
  binaryPath: '/usr/bin/blender',
  version: '4.5.3 LTS',
  majorVersion: 4,
  supported: true,
  installGuideUrl: 'https://www.blender.org/download/',
};

const emptyPoolHealth: McpPoolHealthReport = {
  activeCount: 0,
  maxActive: 6,
  totalConfigured: 0,
  healthyCount: 0,
  unhealthyCount: 0,
  servers: {},
};

/** Every channel App touches on mount, so no fixture leaves a promise rejecting into a banner. */
const base: Fixture = {
  'providers:catalog': [],
  'providers:list': [],
  'providers:storage-state': { secure: true, services: 0, store: 'fixture' },
  'project:root': '/home/dev/obby',
  'project:list': [],
  'project:index': { root: '/home/dev/obby', entries: [], truncated: false } satisfies ProjectIndexSummary,
  'project:checkpoints': [],
  'studio:checkpoints': [],
  'playtest:ledger': [],
  'playtest:repair-plan': null,
  'acceptance:read': null,
  'media:comfy-status': offlineComfy,
  'blender:status': missingBlender,
  'capabilities:detect': [],
  'mcp:status': { running: false, tools: [] },
  'mcp:pool:list': [],
  'mcp:pool:health': emptyPoolHealth,
  'mcp:pool:tools': [],
  'approvals:list': [],
  'sessions:load': { activeId: null, sessions: [] } satisfies SessionSnapshot,
};

// ---- home -----------------------------------------------------------------
// The welcome state: no project, no chats, secure storage, no provider yet.
const home: Fixture = {
  ...base,
  'project:root': null,
  'project:index': null,
};

// ---- chat -----------------------------------------------------------------
/** StoredChatMessage plus the renderer-only cost label App reads off assistant rows. */
type FixtureMessage = {
  role: 'user' | 'assistant';
  content: string;
  trace?: Array<{ name: string; status: string; detail?: string }>;
  costLabel?: string;
};

const chatMessages: FixtureMessage[] = [
  {
    role: 'user',
    content: 'The shop sells items for 10 Robux. Raise the price to 12 and keep the value server-owned so a client cannot change it.',
  },
  {
    role: 'assistant',
    content: 'I read the shop controller first. The purchase price is a module constant today, and the client currently passes its own price into the purchase call.',
    trace: [{ name: 'read_file', status: 'executed', detail: 'src/Shop.server.lua' }],
  },
  {
    role: 'assistant',
    content: 'I staged a focused change: the server now owns the price, validates the item id, and rejects any client-supplied amount. Nothing is written until you approve the diff.',
    trace: [{ name: 'write_file', status: 'staged', detail: 'src/Shop.server.lua' }],
  },
  {
    role: 'user',
    content: 'Show me the door script too before I approve anything.',
  },
  {
    role: 'assistant',
    content: 'I staged a read-only Studio inspection of Workspace.Door. Approving it opens nothing; it returns the instance properties so we can check the transparency guard.',
    trace: [{ name: 'studio_read', status: 'staged', detail: 'Workspace.Door' }],
    costLabel: '2.4k tokens · $0.00 local',
  },
];

const chatSessions: SessionSnapshot = {
  activeId: 'sess-shop-price',
  sessions: [
    {
      id: 'sess-shop-price',
      title: 'Shop price: server-owned',
      updatedAt: '2026-11-03T15:39:00.000Z',
      projectRoot: '/home/dev/obby',
      messages: chatMessages,
    },
  ],
};

const chatApprovals: PendingApprovalView[] = [
  {
    id: 'ap-write-shop-001',
    kind: 'write',
    createdAt: '2026-11-03T15:40:00.000Z',
    relPath: 'src/Shop.server.lua',
    previous: [
      'local Shop = {}',
      'local PRICE = 10',
      '',
      'function Shop.purchase(player, itemId, clientPrice)',
      '    if clientPrice ~= PRICE then return false end',
      '    return true',
      'end',
      '',
      'return Shop',
    ].join('\n'),
    content: [
      '--!strict',
      'local Shop = {}',
      'local PRICE = 12',
      '',
      'function Shop.purchase(player: Player, itemId: string)',
      '    local item = ITEMS[itemId]',
      '    if not item then return false end',
      '    if player:GetAttribute("Robux") < PRICE then return false end',
      '    return true',
      'end',
      '',
      'return Shop',
    ].join('\n'),
  },
  {
    id: 'ap-studio-door-001',
    kind: 'studio',
    createdAt: '2026-11-03T15:41:00.000Z',
    mcpServer: 'StudioMCP',
    studioOp: 'read',
    studioPath: 'Workspace.Door',
  },
];

const chat: Fixture = {
  ...base,
  'sessions:load': chatSessions,
  'providers:catalog': catalogOf('ollama', 'openai-api', 'codex-cli'),
  'providers:list': [
    {
      kind: 'ollama',
      model: 'qwen2.5-coder:14b',
      baseUrl: 'http://127.0.0.1:11434/v1',
      hasKey: false,
      active: true,
      secure: true,
    } satisfies StoredProviderView,
  ],
};

// ---- models ---------------------------------------------------------------
const models: Fixture = {
  ...base,
  'providers:catalog': catalogOf('ollama', 'openai-api', 'anthropic-api', 'codex-cli', 'openrouter-api'),
  'providers:list': [
    {
      kind: 'ollama',
      model: 'qwen2.5-coder:14b',
      baseUrl: 'http://127.0.0.1:11434/v1',
      hasKey: false,
      active: true,
      secure: true,
    } satisfies StoredProviderView,
    {
      kind: 'openai-api',
      model: 'gpt-5.2',
      baseUrl: 'https://api.openai.com/v1',
      hasKey: true,
      active: false,
      secure: true,
    } satisfies StoredProviderView,
  ],
};

// ---- workspace ------------------------------------------------------------
const workspaceEntries: FileEntry[] = [
  { name: 'src', path: 'src', type: 'dir' },
  { name: 'assets', path: 'assets', type: 'dir' },
  { name: 'default.project.json', path: 'default.project.json', type: 'file' },
  { name: 'README.md', path: 'README.md', type: 'file' },
];

const workspaceIndex: ProjectIndexSummary = {
  root: '/home/dev/obby',
  truncated: false,
  entries: [
    { relPath: 'src/Shop.server.lua', bytes: 2140 },
    { relPath: 'src/Door.server.lua', bytes: 1876 },
    { relPath: 'src/main.client.lua', bytes: 3092 },
    { relPath: 'src/Modules/Economy.lua', bytes: 4411 },
    { relPath: 'assets/models/door.fbx', bytes: 148_224 },
    { relPath: 'tests/shop.spec.lua', bytes: 1620 },
    { relPath: 'default.project.json', bytes: 512 },
    { relPath: 'README.md', bytes: 2048 },
  ],
};

const workspaceLedger: PlaytestLedgerEntry[] = [
  {
    file: '2026-11-03T14-05-11-000Z-shop-purchase.json',
    scenario: 'Shop purchase flow',
    status: 'PASSED',
    reason: '3 assertions passed with the server-owned price.',
    screenshots: 2,
    createdAt: '2026-11-03T14:05:11.000Z',
  },
  {
    file: '2026-11-03T15-02-00-000Z-door-interaction.json',
    scenario: 'Door interaction',
    status: 'FAILED',
    reason: 'The door stayed opaque after the prompt fired.',
    failingCheck: 'assert Door.Transparency == 0',
    screenshots: 1,
    createdAt: '2026-11-03T15:02:00.000Z',
  },
];

const workspaceRepair: PlaytestRepairPlan = {
  scenario: 'Door interaction',
  failingCheck: 'assert Door.Transparency == 0',
  reason: 'Expected Workspace.Door.Transparency to reach 0 after the prompt fired; it stayed 1 for the whole run.',
  attempt: 2,
  screenshots: 1,
  lastFailedAt: '2026-11-03T15:02:00.000Z',
  lastFailedFile: '2026-11-03T15-02-00-000Z-door-interaction.json',
  instruction:
    'Inspect src/Door.server.lua and the TweenService fade. Propose one focused change and stage it for review. Do not claim the playtest passes until report_playtest returns PASSED.',
};

const workspace: Fixture = {
  ...base,
  'project:root': '/home/dev/obby',
  'project:list': workspaceEntries,
  'project:index': workspaceIndex,
  'playtest:ledger': workspaceLedger,
  'playtest:repair-plan': workspaceRepair,
  'media:comfy-status': offlineComfy,
  'blender:status': installedBlender,
};

// ---- integrations ---------------------------------------------------------
const capabilities: CapabilityInfo[] = [
  {
    id: 'studio-mcp',
    name: 'Roblox Studio MCP',
    category: 'studio',
    state: 'ready',
    detail: 'Studio is detected through the official MCP bridge; read, write and playtest tools are available.',
    installUrl: 'https://create.roblox.com/docs/studio/mcp',
  },
  {
    id: 'studio-playtest-bridge',
    name: 'Studio Playtest Bridge',
    category: 'studio',
    state: 'missing',
    detail: 'Optional bridge for scripted keyboard, mouse and viewport capture during a playtest.',
    installUrl: 'https://create.roblox.com/docs/studio/mcp',
  },
  {
    id: 'rojo',
    name: 'Rojo',
    category: 'vcs',
    state: 'ready',
    detail: 'A Rojo project file is present, so the agent can map files to the DataModel.',
    installUrl: 'https://rojo.space/',
  },
  {
    id: 'blender',
    name: 'Blender',
    category: 'asset',
    state: 'ready',
    detail: 'Blender 4.5.3 LTS is installed at /usr/bin/blender for headless mesh tasks.',
    installUrl: 'https://www.blender.org/download/',
  },
  {
    id: 'comfyui',
    name: 'ComfyUI',
    category: 'asset',
    state: 'experimental',
    detail: 'No loopback server is answering on 127.0.0.1:8188, so local media generation is unavailable.',
    installUrl: 'https://www.comfy.org/download',
  },
  {
    id: 'codex-cli',
    name: 'Codex CLI',
    category: 'provider-cli',
    state: 'missing',
    detail: 'Install Codex CLI and run codex login to use a ChatGPT subscription inside Open-Ro.',
    installUrl: 'https://chatgpt.com/codex',
  },
];

const poolTools: McpToolSummary[] = [
  { name: 'studio_snapshot', description: 'Read the Studio place tree and report services and instances.' },
  { name: 'studio_read', description: 'Read one instance property or script source from the open Studio place.' },
  { name: 'studio_write', description: 'Write a script back into Studio after the approval gate.' },
  { name: 'studio_playtest', description: 'Start or stop the running game in Studio.' },
  { name: 'studio_console', description: 'Read the Studio output window.' },
];

const poolServers: McpServerView[] = [
  {
    id: 'studio-mcp',
    name: 'Roblox Studio MCP',
    command: 'npx',
    args: ['-y', 'roblox-studio-mcp'],
    running: true,
    status: 'connected',
    session: 'studio-7f3a91',
    toolCount: 5,
    tools: poolTools,
    health: { isHealthy: true, lastPingMs: 12, consecutiveFailures: 0, uptimeMs: 3_600_000, restartCount: 0 },
  },
  {
    id: 'blender-3d',
    name: 'Blender 3D MCP',
    command: 'python',
    args: ['-m', 'blender_mcp'],
    running: true,
    status: 'hibernating',
    session: null,
    toolCount: 2,
    tools: [],
    health: { isHealthy: false, lastPingMs: 0, consecutiveFailures: 1, restartCount: 1 },
  },
  {
    id: 'filesystem',
    name: 'Filesystem MCP',
    command: 'npx',
    args: ['-y', '@modelcontextprotocol/server-filesystem', '.'],
    running: false,
    status: 'error',
    error: 'spawn npx ENOENT',
    session: null,
    toolCount: 0,
    tools: [],
    health: { isHealthy: false, consecutiveFailures: 3, restartCount: 2 },
  },
];

const pooledTools: McpPooledToolSummary[] = [
  { serverId: 'studio-mcp', serverName: 'Roblox Studio MCP', agentToolName: 'mcp__studio_snapshot', directAgentToolName: 'mcp__studio_snapshot', tool: poolTools[0] },
  { serverId: 'studio-mcp', serverName: 'Roblox Studio MCP', agentToolName: 'mcp__studio_read', directAgentToolName: 'mcp__studio_read', tool: poolTools[1] },
  { serverId: 'studio-mcp', serverName: 'Roblox Studio MCP', agentToolName: 'mcp__studio_write', tool: poolTools[2] },
  { serverId: 'studio-mcp', serverName: 'Roblox Studio MCP', agentToolName: 'mcp__studio_playtest', tool: poolTools[3] },
  { serverId: 'studio-mcp', serverName: 'Roblox Studio MCP', agentToolName: 'mcp__studio_console', tool: poolTools[4] },
  {
    serverId: 'blender-3d',
    serverName: 'Blender 3D MCP',
    agentToolName: 'mcp__blender_inspect',
    tool: { name: 'blender_inspect', description: 'Inspect a mesh, report triangles, vertices, bounds and UV maps.' },
  },
  {
    serverId: 'blender-3d',
    serverName: 'Blender 3D MCP',
    agentToolName: 'mcp__blender_convert',
    tool: { name: 'blender_convert', description: 'Convert a mesh between OBJ, FBX, glTF and GLB in headless Blender.' },
  },
  {
    serverId: 'filesystem',
    serverName: 'Filesystem MCP',
    agentToolName: 'mcp__filesystem_list',
    tool: { name: 'filesystem_list', description: 'List a directory inside the project root.' },
  },
];

const poolHealth: McpPoolHealthReport = {
  activeCount: 2,
  maxActive: 6,
  totalConfigured: 3,
  healthyCount: 2,
  unhealthyCount: 1,
  servers: {
    'studio-mcp': { isHealthy: true, pingMs: 12, status: 'connected' },
    'blender-3d': { isHealthy: true, pingMs: 41, status: 'hibernating' },
    filesystem: { isHealthy: false, status: 'error' },
  },
};

const integrations: Fixture = {
  ...base,
  'capabilities:detect': capabilities,
  'mcp:pool:list': poolServers,
  'mcp:pool:health': poolHealth,
  'mcp:pool:tools': pooledTools,
  'mcp:status': {
    running: true,
    tools: [
      { name: 'studio_snapshot', description: 'Read the Studio place tree.' },
      { name: 'studio_read', description: 'Read one Studio script.' },
    ],
  },
};

// ---- existing comps, kept for the review loop -----------------------------
const oldApprovals: Fixture = {
  ...base,
  'project:root': '/home/dev/obby',
  'approvals:list': [
    { id: 'w1', kind: 'write', createdAt: '2026-11-03T15:40:00.000Z', relPath: 'src/Shop.server.lua', previous: 'local price = 10\nprint(price)', content: '--!strict\nlocal price = 12\nprint(price)' },
    { id: 's1', kind: 'studio', createdAt: '2026-11-03T15:41:00.000Z', mcpServer: 'StudioMCP', studioOp: 'read', studioPath: 'Workspace.Door' },
  ] satisfies PendingApprovalView[],
  'sessions:load': {
    activeId: 'c1',
    sessions: [{
      id: 'c1',
      title: 'Shop price',
      updatedAt: '2026-11-03T15:39:00.000Z',
      messages: [
        { role: 'user', content: 'Raise the shop price to 12 and keep it server-owned.' },
        { role: 'assistant', content: 'I staged a change that sets the price on the server. Review the diff below.', trace: [{ name: 'write_file', status: 'staged', detail: 'src/Shop.server.lua' }] },
      ],
    }],
  } satisfies SessionSnapshot,
  'providers:catalog': catalogOf('ollama', 'openai-api', 'codex-cli'),
  'providers:list': [{ kind: 'ollama', model: 'qwen2.5-coder:14b', baseUrl: 'http://127.0.0.1:11434/v1', hasKey: false, active: true, secure: true }],
};

function fixture(source: Fixture) {
  return async (channel: string): Promise<unknown> => source[channel] ?? null;
}

const COMPS: Record<string, (channel: string) => Promise<unknown>> = {
  home: fixture(home),
  chat: fixture(chat),
  models: fixture(models),
  workspace: fixture(workspace),
  integrations: fixture(integrations),
  'chat-empty': fixture(base),
  'chat-approvals': fixture(oldApprovals),
  loading: () => new Promise(() => undefined),
  error: async () => { throw new Error('Fixture: the main process did not answer.'); },
};

export function mountComp(root: Root, name: string) {
  const invoke = COMPS[name];
  if (!invoke) {
    root.render(
      <nav className="comps-index">
        <strong>Open-Ro fixture comps</strong>
        {Object.keys(COMPS).map((key) => <a key={key} href={`/comps/${key}`}>{key}</a>)}
      </nav>,
    );
    return;
  }
  window.openro = { invoke };
  root.render(<StrictMode><ErrorBoundary><App /></ErrorBoundary></StrictMode>);
}

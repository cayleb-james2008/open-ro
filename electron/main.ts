import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron';
import { execFile } from 'node:child_process';
import * as crypto from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { promisify } from 'node:util';
import { registerApprovalChannels } from './approvalChannels';
import { ApprovalQueue, type ApprovalKind, type PendingApproval } from './approvals';
import { writeTextAtomic } from './atomicWrite';
import { detectCapabilities } from './capabilities';
import { parseCheckpointRecord, stageCheckpointRestore } from './checkpoints';
import { McpStdioClient } from './mcp';
import { isMcpAgentTool, mcpToolSchemas, runApprovedMcpCall, runApprovedMcpServer, serverLabel, stageMcpCall, stageMcpConnect } from './mcpTools';
import { reportMediaHardware, type MediaHardwareProfile, type MediaReport } from './mediaCatalog';
import {
  buildComfyPlan,
  getComfyHealth,
  normalizeComfyBaseUrl,
  prepareComfyGeneration,
  publicPlan,
  runComfyPlan,
  type ComfyGenerationInput,
  type ComfyGenerationPlan,
  type ComfyGenerationResult,
  type ComfyHealth,
  type ComfyPlanSummary,
} from './comfyClient';
import { gradePlaytest, parsePlaytestReport, resolvePlaytestScreenshot } from './playtestEvidence';
import { deriveRepairPlan, summarizePlaytestLedger, type PlaytestLedgerEntry, type PlaytestLedgerRecord, type PlaytestRepairPlan } from './playtestLedger';
import { runApprovedStudio, stageStudioRestore, stageStudioTool, StudioReads, type StudioCheckpoint } from './studioTools';
import { isIgnoredName, isSecretName, resolveWithinRoot, sameProjectRoot } from './pathSafety';
import { indexProject, buildProjectContext, type ProjectIndex } from './projectIndex';
import { getProviderOption, isProviderKind, providerOptions, type ProviderKind } from './providerCatalog';
import {
  activateProvider,
  listProviderViews,
  parseProviderState,
  removeProvider,
  rememberProjectRoot,
  upsertProvider,
  validateProviderSave,
  type ProviderAppState,
} from './providerState';
import { runShellCommand } from './runCommand';
import { sanitizeSessions } from './sessions';
import { deleteCredential, listCredentialServices, loadCredential, saveCredential, secureStorageAvailable, storePath } from './secureStore';
import { runAgentTurn, testConnection, toolSchemas } from './agent';
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
  ToolExecutor,
  ToolSchema,
} from './types';

/**
 * Electron main process. Privileged work stays here: credential access, the
 * project root the user picked, and the approval queue. The renderer cannot
 * substitute a different path, file body, or command after something is staged.
 */

let mainWindow: BrowserWindow | null = null;
let projectRoot = '';
const mcp = new McpStdioClient();
const approvals = new ApprovalQueue();
const mediaPlans = new Map<string, { plan: ComfyGenerationPlan; expiresAt: number }>();
const MEDIA_PLAN_TTL_MS = 10 * 60_000;

function comfyBaseUrl(): string {
  return normalizeComfyBaseUrl(process.env.OPENRO_COMFY_URL?.trim() || 'http://127.0.0.1:8188');
}

function pruneMediaPlans(): void {
  const now = Date.now();
  for (const [id, item] of mediaPlans) {
    if (item.expiresAt <= now) {
      mediaPlans.delete(id);
    }
  }
}
/** What approved Studio reads saw; a Studio write needs one for its diff. */
const studioReads = new StudioReads();

/** The one in-flight agent turn, if any. Cancelling aborts its fetch or CLI process. */
let activeTurn: AbortController | null = null;

const statePath = (): string => path.join(app.getPath('userData'), 'state.json');
const sessionsPath = (): string => path.join(app.getPath('userData'), 'sessions.json');
const checkpointRoot = (): string => path.join(app.getPath('userData'), 'checkpoints');

function toApprovalView(item: PendingApproval): PendingApprovalView {
  return {
    id: item.id,
    kind: item.kind,
    createdAt: item.createdAt,
    relPath: item.relPath,
    previous: item.previous,
    content: item.content,
    command: item.command,
    cwd: item.cwd,
    mcpTool: item.mcpTool,
    mcpArgs: item.mcpArgs,
    mcpServer: item.mcpServer ? serverLabel(item.mcpServer) : undefined,
    serverArgs: item.serverArgs,
    studioOp: item.studioOp,
    studioPath: item.studioPath,
    studioClass: item.studioClass,
    studioServices: item.studioServices,
    studioId: item.studioId,
    mediaPreset: item.mediaPreset,
    mediaPrompt: item.mediaPrompt,
    mediaDuration: item.mediaDuration,
    mediaSeed: item.mediaSeed,
  };
}

async function readAppState(): Promise<ProviderAppState> {
  try {
    return parseProviderState(JSON.parse(await fs.readFile(statePath(), 'utf8')) as unknown);
  } catch {
    return { providers: [] };
  }
}

async function writeAppState(state: ProviderAppState): Promise<void> {
  await writeTextAtomic(statePath(), JSON.stringify(state, null, 2));
}

async function loadSavedProjectRoot(): Promise<void> {
  const state = await readAppState();
  projectRoot = state.projectRoot ?? '';
}

async function saveProjectRoot(root: string): Promise<void> {
  const state = await readAppState();
  await writeAppState(rememberProjectRoot(state, root));
}

async function resolveActiveConnection(provided?: Partial<ProviderConnection>): Promise<ProviderConnection | null> {
  const state = await readAppState();
  const kind = provided?.kind ?? state.activeKind;
  if (!kind || !isProviderKind(kind)) {
    return null;
  }
  const record = state.providers.find((item) => item.kind === kind);
  const model = provided?.model?.trim() || record?.model || '';
  const baseUrl = provided?.baseUrl?.trim() || record?.baseUrl;
  const typedKey = provided?.apiKey?.trim();
  const apiKey = typedKey || loadCredential(`provider:${kind}`);
  return { kind, model, baseUrl, apiKey };
}

async function listFiles(root: string, rel: string): Promise<FileEntry[]> {
  const target = await resolveWithinRoot(root, rel || '.');
  const entries = await fs.readdir(target, { withFileTypes: true });
  return entries
    .filter((entry) => !isIgnoredName(entry.name) && !isSecretName(entry.name))
    .map((entry): FileEntry => ({
      name: entry.name,
      path: path.posix.join((rel || '.').replace(/\\/g, '/'), entry.name),
      type: entry.isDirectory() ? 'dir' : 'file',
    }))
    .sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === 'dir' ? -1 : 1));
}

async function readTextIfExists(target: string): Promise<string | null> {
  try {
    return await fs.readFile(target, 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return null;
    }
    throw error;
  }
}

async function writeCheckpoint(root: string, relPath: string, previous: string | null): Promise<void> {
  const dir = checkpointRoot();
  await fs.mkdir(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const entry = { ts: stamp, projectRoot: root, relPath, previous };
  const name = `${stamp}-${crypto.createHash('sha1').update(`${root}:${relPath}`).digest('hex').slice(0, 8)}.json`;
  await fs.writeFile(path.join(dir, name), JSON.stringify(entry), 'utf8');
}

async function writePlaytestRecord(root: string, report: unknown, verdict: { status: string }): Promise<void> {
  const dir = path.join(app.getPath('userData'), 'playtests');
  await fs.mkdir(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  await fs.writeFile(path.join(dir, `${stamp}-${verdict.status.toLowerCase()}.json`), JSON.stringify({ projectRoot: root, report, verdict }, null, 2), 'utf8');
}

async function readPlaytestLedger(root: string): Promise<PlaytestLedgerEntry[]> {
  const dir = path.join(app.getPath('userData'), 'playtests');
  let names: string[];
  try {
    names = await fs.readdir(dir);
  } catch {
    return [];
  }
  const records: PlaytestLedgerRecord[] = [];
  for (const file of names.filter((name) => name.endsWith('.json')).sort().reverse().slice(0, 100)) {
    const full = path.join(dir, file);
    const stat = await fs.stat(full).catch(() => null);
    if (!stat || stat.size > 1_000_000) {
      continue;
    }
    const raw = await fs.readFile(full, 'utf8').catch(() => null);
    if (raw !== null) {
      records.push({ file, modifiedMs: stat.mtimeMs, raw });
    }
  }
  return summarizePlaytestLedger(records, root);
}

const execFileAsync = promisify(execFile);
const GIB = 1024 ** 3;

async function detectNvidiaGpu(): Promise<{ name: string | null; vramsGB: number[]; count: number }> {
  if (process.platform !== 'win32' && process.platform !== 'linux') {
    return { name: null, vramsGB: [], count: 0 };
  }
  try {
    const { stdout } = await execFileAsync('nvidia-smi', ['--query-gpu=name,memory.total', '--format=csv,noheader,nounits'], {
      timeout: 3000,
      windowsHide: true,
    });
    const rows = stdout
      .trim()
      .split(/\r?\n/)
      .map((line) => line.split(',').map((part) => part.trim()))
      .filter((parts) => parts.length >= 2 && Number.isFinite(Number(parts[1])));
    if (rows.length === 0) {
      return { name: null, vramsGB: [], count: 0 };
    }
    const vramsGB = rows.map((parts) => Math.round((Number(parts[1]) / 1024) * 10) / 10);
    return {
      name: rows.map((parts) => parts[0]).join(' + '),
      vramsGB,
      count: rows.length,
    };
  } catch {
    return { name: null, vramsGB: [], count: 0 };
  }
}

async function detectMediaReport(): Promise<MediaReport> {
  const cpus = os.cpus();
  const gpu = await detectNvidiaGpu();
  let freeDiskGB: number | null = null;
  try {
    const stats = await fs.statfs(app.getPath('userData'));
    freeDiskGB = Math.round(((stats.bavail * stats.bsize) / GIB) * 10) / 10;
  } catch {
    freeDiskGB = null;
  }
  const profile: MediaHardwareProfile = {
    totalRamGB: Math.round((os.totalmem() / GIB) * 10) / 10,
    freeRamGB: Math.round((os.freemem() / GIB) * 10) / 10,
    cpuModel: cpus[0]?.model ?? 'unknown',
    cores: cpus.length,
    platform: process.platform,
    arch: process.arch,
    gpuName: gpu.name,
    vramGB: gpu.vramsGB.length > 0 ? Math.max(...gpu.vramsGB) : null,
    gpuVramGB: gpu.vramsGB,
    gpuCount: gpu.count,
    vramSource: gpu.count > 0 ? 'nvidia-smi' : 'unknown',
    freeDiskGB,
  };
  return reportMediaHardware(profile);
}

async function saveStudioCapture(data: Buffer, mimeType: string, captureId: string): Promise<string> {
  const dir = path.join(app.getPath('userData'), 'studio-captures');
  await fs.mkdir(dir, { recursive: true });
  const safe = captureId.replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 80) || 'capture';
  const extension = mimeType === 'image/png' ? '.png' : mimeType === 'image/jpeg' ? '.jpg' : '.bin';
  const file = path.join(dir, `${safe}-${Date.now()}${extension}`);
  await fs.writeFile(file, data, { mode: 0o600 });
  return file;
}

/** Studio snapshots and checkpoints are kept as JSON under userData/<folder>. */
async function writeStudioRecord(folder: 'studio-snapshots' | 'studio-checkpoints', record: object): Promise<void> {  const dir = path.join(app.getPath('userData'), folder);
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, `${new Date().toISOString().replace(/[:.]/g, '-')}.json`), JSON.stringify(record, null, 2), 'utf8');
}

const MEDIA_TOOL_SCHEMA: ToolSchema = {
  name: 'media_generate',
  description:
    'Generate local audio from a reviewed ComfyUI plan. The exact preset, prompt, duration and seed are staged for approval; the app never downloads models and only talks to a loopback ComfyUI server.',
  parameters: {
    type: 'object',
    properties: {
      presetId: {
        type: 'string',
        enum: ['audio-ace-step-1.5-turbo', 'audio-ace-step-1.5-xl'],
        description: 'The 2026 ACE-Step preset. Turbo is the CPU/low-VRAM option; XL needs a larger GPU.',
      },
      prompt: { type: 'string', description: 'Music style, instruments, mood and production tags.' },
      lyrics: { type: 'string', description: 'Lyrics with section tags, or [Instrumental]. Optional.' },
      durationSeconds: { type: 'number', description: 'Output length from 5 to 60 seconds. Defaults to 10.' },
      seed: { type: 'integer', description: 'Optional integer seed from 0 to 2147483647.' },
    },
    required: ['presetId', 'prompt'],
  },
};

function buildExecutor(root: string): ToolExecutor {
  return {
    schemas: () => [...toolSchemas(), MEDIA_TOOL_SCHEMA, ...mcpToolSchemas(mcp.running ? mcp.listTools() : [])],
    async execute(call) {
      if (!root) {
        return { ok: false, output: 'No project folder selected.' };
      }
      const { name, args } = call;
      try {
        switch (name) {
          case 'media_generate': {
            // Stage only. Health is checked by the approve-time execution so the card can be reviewed offline.
            const plan = buildComfyPlan({
              presetId: typeof args.presetId === 'string' ? args.presetId : '',
              prompt: typeof args.prompt === 'string' ? args.prompt : '',
              lyrics: typeof args.lyrics === 'string' ? args.lyrics : undefined,
              durationSeconds: typeof args.durationSeconds === 'number' ? args.durationSeconds : undefined,
              seed: typeof args.seed === 'number' ? args.seed : undefined,
            });
            const pending = approvals.stageMedia({ projectRoot: root, plan });
            return {
              ok: true,
              output: `Staged ${plan.modelId} for approval. Nothing runs until you approve it.`,
              pendingApproval: { kind: 'media', tool: 'media_generate', id: pending.id },
            };
          }
          case 'list_dir': {
            const rel = typeof args.path === 'string' ? args.path : '.';
            const entries = await listFiles(root, rel);
            return { ok: true, output: entries.map((entry) => `${entry.type === 'dir' ? '[d]' : '[f]'} ${entry.path}`).join('\n') || '(empty directory)' };
          }
          case 'read_file': {
            const text = await fs.readFile(await resolveWithinRoot(root, String(args.path ?? '')), 'utf8');
            return { ok: true, output: text.slice(0, 40_000) };
          }
          case 'write_file': {
            const rel = String(args.path ?? '');
            const content = String(args.content ?? '');
            const target = await resolveWithinRoot(root, rel);
            const previous = (await readTextIfExists(target)) ?? '';
            const pending = approvals.stageWrite({ projectRoot: root, relPath: rel, content, previous });
            return {
              ok: true,
              output: `Staged ${rel} for approval. Nothing was written.`,
              pendingApproval: { kind: 'file-write', path: rel, id: pending.id },
            };
          }
          case 'run_command': {
            const command = String(args.command ?? '');
            const cwd = await resolveWithinRoot(root, '.');
            const pending = approvals.stageCommand({ projectRoot: root, command, cwd });
            return {
              ok: true,
              output: 'Staged the command for approval. It has not run.',
              pendingApproval: { kind: 'command', command: pending.command, id: pending.id },
            };
          }
          case 'report_playtest': {
            const report = parsePlaytestReport(args);
            const verdict = await gradePlaytest(report, (reported) =>
              resolvePlaytestScreenshot(root, reported, path.join(app.getPath('userData'), 'studio-captures')),
            );
            await writePlaytestRecord(root, report, verdict);
            const failing = verdict.failingCheck ? ` Failing check: ${verdict.failingCheck}` : '';
            const plan = verdict.status === 'FAILED' ? deriveRepairPlan(await readPlaytestLedger(root).catch(() => [])) : null;
            const repair = plan ? ` Repair loop: attempt ${plan.attempt} for "${plan.scenario}". Fix the failing check, then call report_playtest again; do not claim success until PASSED.` : '';
            return { ok: verdict.status === 'PASSED', output: `Playtest ${verdict.status}. ${verdict.reason}${failing}${repair}` };
          }
          case 'studio_snapshot':
          case 'studio_read_script':
          case 'studio_write_script':
          case 'studio_playtest':
          case 'studio_console':
          case 'studio_keyboard':
          case 'studio_mouse':
          case 'studio_capture':
            return stageStudioTool(approvals, mcp, studioReads, { projectRoot: root, name, args });
          default:
            if (isMcpAgentTool(name)) {
              return stageMcpCall(approvals, mcp, { projectRoot: root, agentName: name, args });
            }
            return { ok: false, output: `Unknown tool: ${String(name)}` };
        }
      } catch (error) {
        return { ok: false, output: error instanceof Error ? error.message : 'Tool failed.' };
      }
    },
  };
}

function requireMessages(messages: ChatMessage[]): ChatMessage[] {
  if (!Array.isArray(messages)) {
    throw new Error('Messages must be a list.');
  }
  return messages
    .filter((message) => message && typeof message.content === 'string' && (message.role === 'user' || message.role === 'assistant' || message.role === 'system'))
    .slice(-40)
    .map((message) => ({ role: message.role, content: message.content.slice(0, 20_000) }));
}

async function resolveApproval(id: string, decision: 'approve' | 'reject', kind?: ApprovalKind): Promise<ApprovalResult> {
  const item = approvals.take(id, kind);
  // MCP server starts are not tied to a project. Everything else must match the current project.
  if (item.kind !== 'mcp-server' && (!projectRoot || !sameProjectRoot(item.projectRoot, projectRoot))) {
    throw new Error('Project folder changed. The staged action was discarded.');
  }
  if (decision === 'reject') {
    return {
      ok: true,
      decision,
      summary:
        item.kind === 'write'
          ? `Rejected the write to ${item.relPath ?? 'the file'}. Nothing was changed.`
          : item.kind === 'delete'
            ? `Rejected deleting ${item.relPath ?? 'the file'}. Nothing was changed.`
            : item.kind === 'media'
            ? 'Rejected the local media generation. Nothing ran.'
            : item.kind === 'mcp'
            ? `Rejected MCP tool ${item.mcpTool ?? ''}. It did not run.`
            : item.kind === 'mcp-server'
              ? 'Rejected starting the MCP server. It did not run.'
            : item.kind === 'studio'
              ? `Rejected the Studio ${item.studioOp ?? 'call'} of game.${item.studioPath ?? ''}. Nothing ran in Studio.`
            : 'Rejected the command. It did not run.',
    };
  }
  if (item.kind === 'delete') {
    const relPath = item.relPath ?? '';
    try {
      const target = await resolveWithinRoot(projectRoot, relPath);
      const previous = await readTextIfExists(target);
      if (previous === null) {
        return { ok: true, decision, summary: `${relPath} is already absent. Nothing was changed.` };
      }
      await writeCheckpoint(projectRoot, relPath, previous);
      await fs.rm(target, { force: true });
      return { ok: true, decision, summary: `Deleted ${relPath}. A checkpoint of the deleted contents was saved.` };
    } catch (error) {
      approvals.stageDelete({ projectRoot, relPath, previous: item.previous ?? '' });
      throw error;
    }
  }
  if (item.kind === 'write') {
    const relPath = item.relPath ?? '';
    const content = item.content ?? '';
    try {
      const target = await resolveWithinRoot(projectRoot, relPath);
      const previous = await readTextIfExists(target);
      await writeCheckpoint(projectRoot, relPath, previous);
      await fs.mkdir(path.dirname(target), { recursive: true });
      await fs.writeFile(target, content, { encoding: 'utf8', mode: 0o644 });
      return { ok: true, decision, summary: `Wrote ${relPath}. A checkpoint of the previous contents was saved.` };
    } catch (error) {
      approvals.stageWrite({ projectRoot, relPath, content, previous: item.previous ?? '' });
      throw error;
    }
  }
  if (item.kind === 'media') {
    if (!item.mediaPlan) {
      return { ok: false, decision, summary: 'The frozen media plan is missing. Nothing ran.' };
    }
    try {
      const result = await runComfyPlan(comfyBaseUrl(), item.mediaPlan, path.join(app.getPath('userData'), 'media'));
      return {
        ok: true,
        decision,
        summary: `Generated ${result.files.length} local media file(s) with ${result.promptId}.`,
        output: result.files.map((file) => `${file.path} (${file.bytes} bytes, sha256 ${file.sha256})`).join('\n'),
      };
    } catch (error) {
      approvals.stageMedia({ projectRoot: item.projectRoot, plan: item.mediaPlan });
      throw error;
    }
  }
  if (item.kind === 'mcp') {
    return runApprovedMcpCall(item, mcp);
  }
  if (item.kind === 'studio') {
    const { snapshot, checkpoint, ...result } = await runApprovedStudio(item, mcp, studioReads, saveStudioCapture);
    if (snapshot) {
      await writeStudioRecord('studio-snapshots', { projectRoot, server: item.mcpServer?.command, snapshot });
    }
    if (checkpoint) {
      await writeStudioRecord('studio-checkpoints', { projectRoot, checkpoint });
    }
    return result;
  }
  if (item.kind === 'mcp-server') {
    return runApprovedMcpServer(item, mcp);
  }
  const cwd = await resolveWithinRoot(projectRoot, '.');
  const result = await runShellCommand({ command: item.command ?? '', cwd, timeoutMs: 60_000 });
  const output = [result.stdout, result.stderr].filter((part) => part.trim().length > 0).join('\n');
  if (result.timedOut) {
    return { ok: false, decision, summary: 'Command timed out and was stopped.', output };
  }
  return {
    ok: result.code === 0,
    decision,
    summary: `Command exited with code ${String(result.code)}.`,
    output,
  };
}

function registerIpc(): void {
  ipcMain.handle('providers:catalog', () => providerOptions);
  ipcMain.handle('providers:list', async (): Promise<StoredProviderView[]> => {
    const state = await readAppState();
    return listProviderViews(state, { credentialServices: listCredentialServices(), secure: secureStorageAvailable() });
  });
  ipcMain.handle('providers:save', async (_event, args: { kind: ProviderKind; model: string; baseUrl?: string; apiKey?: string }) => {
    if (!args || !isProviderKind(args.kind)) {
      throw new Error('Unknown provider.');
    }
    const option = getProviderOption(args.kind);
    const validated = validateProviderSave(option, {
      model: args.model ?? '',
      baseUrl: args.baseUrl,
      apiKey: args.apiKey,
      existingKey: Boolean(loadCredential(`provider:${args.kind}`)),
    });
    if (validated.apiKey) {
      saveCredential(`provider:${args.kind}`, validated.apiKey);
    }
    const state = await readAppState();
    await writeAppState(upsertProvider(state, { kind: args.kind, model: validated.model, baseUrl: validated.baseUrl }));
    return { ok: true, secure: secureStorageAvailable() };
  });
  ipcMain.handle('providers:set-active', async (_event, args: { kind: ProviderKind }) => {
    if (!args || !isProviderKind(args.kind)) {
      throw new Error('Unknown provider.');
    }
    const state = await readAppState();
    await writeAppState(activateProvider(state, args.kind));
    return { ok: true };
  });
  ipcMain.handle('providers:delete', async (_event, args: { kind: ProviderKind }) => {
    if (!args || !isProviderKind(args.kind)) {
      throw new Error('Unknown provider.');
    }
    deleteCredential(`provider:${args.kind}`);
    const state = await readAppState();
    await writeAppState(removeProvider(state, args.kind));
    return { ok: true };
  });
  ipcMain.handle('providers:storage-state', () => ({ secure: secureStorageAvailable(), services: listCredentialServices().length, store: storePath() }));
  ipcMain.handle('providers:test', async (_event, args: Partial<ProviderConnection>): Promise<ConnectionTestResult> => {
    const connection = await resolveActiveConnection(args);
    if (!connection) {
      return { ok: false, detail: 'No provider connection configured yet.' };
    }
    return testConnection(connection, projectRoot);
  });

  ipcMain.handle('agent:send', async (event, args: { messages: ChatMessage[]; projectRoot?: string; sessionId?: string }): Promise<AgentTurnResult> => {
    const connection = await resolveActiveConnection();
    if (!connection) {
      return { ok: false, assistantText: '', toolTrace: [], error: 'Connect a model first (Models).' };
    }
    if (!connection.model && getProviderOption(connection.kind).connection !== 'cli') {
      return { ok: false, assistantText: '', toolTrace: [], error: 'Enter a model ID for the active provider.' };
    }
    if (activeTurn) {
      return { ok: false, assistantText: '', toolTrace: [], error: 'A turn is already running. Wait for it to finish or cancel it.' };
    }
    const controller = new AbortController();
    activeTurn = controller;
    try {
      const requestedRoot = typeof args?.projectRoot === 'string' ? args.projectRoot.trim() : '';
      const currentRoot = projectRoot ? path.resolve(projectRoot) : '';
      let turnRoot = currentRoot;
      if (requestedRoot) {
        const next = path.resolve(requestedRoot);
        const info = await fs.stat(next).catch(() => null);
        if (!info?.isDirectory()) {
          return { ok: false, assistantText: '', toolTrace: [], error: "This chat's project folder is no longer available. Open the folder again before sending." };
        }
        if (!currentRoot || !sameProjectRoot(currentRoot, next)) {
          approvals.clear();
          studioReads.clear();
          projectRoot = next;
          await saveProjectRoot(next);
          turnRoot = next;
        }
        // Same folder under a different spelling: keep the canonical active root so
        // approvals staged during this turn bind to the same string resolveApproval checks.
      }
      if (controller.signal.aborted) {
        return { ok: false, assistantText: '', toolTrace: [], error: 'Cancelled.' };
      }
      const index = await indexProject(turnRoot).catch(() => null);
      const projectContext = index ? buildProjectContext(index) : '';
      const sessionId = typeof args?.sessionId === 'string' ? args.sessionId : '';
      const sender = event.sender;
      const onDelta = sessionId
        ? (text: string): void => {
            if (!sender.isDestroyed()) {
              sender.send('agent:delta', { sessionId, text });
            }
          }
        : undefined;
      return await runAgentTurn({
        connection,
        messages: requireMessages(args?.messages ?? []),
        projectRoot: turnRoot,
        executor: buildExecutor(turnRoot),
        signal: controller.signal,
        projectContext: projectContext || undefined,
        onDelta,
      });
    } catch (error) {
      if (controller.signal.aborted) {
        return { ok: false, assistantText: '', toolTrace: [], error: 'Cancelled.' };
      }
      throw error;
    } finally {
      if (activeTurn === controller) {
        activeTurn = null;
      }
    }
  });
  ipcMain.handle('agent:cancel', () => {
    if (!activeTurn) {
      return { ok: true, cancelled: false };
    }
    activeTurn.abort();
    return { ok: true, cancelled: true };
  });

  ipcMain.handle('project:select-root', async () => {
    if (!mainWindow) {
      return null;
    }
    const result = await dialog.showOpenDialog(mainWindow, { properties: ['openDirectory', 'createDirectory'], title: 'Open-Ro — choose a project folder' });
    if (result.canceled || result.filePaths.length === 0) {
      return projectRoot || null;
    }
    const next = result.filePaths[0];
    if (projectRoot && sameProjectRoot(projectRoot, next)) {
      // Same folder under another spelling: keep the canonical root so pending
      // approvals (staged with that casing) still resolve.
      return projectRoot;
    }
    if (projectRoot) {
      approvals.clear();
      studioReads.clear();
    }
    projectRoot = next;
    await saveProjectRoot(next);
    return projectRoot;
  });
  ipcMain.handle('project:root', () => projectRoot || null);
  ipcMain.handle('project:list', async (_event, args: { rel?: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    return listFiles(projectRoot, args?.rel ?? '.');
  });
  ipcMain.handle('project:read', async (_event, args: { rel: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    return fs.readFile(await resolveWithinRoot(projectRoot, args.rel), 'utf8');
  });
  ipcMain.handle('project:preview-write', async (_event, args: { rel: string; content: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    const target = await resolveWithinRoot(projectRoot, args.rel);
    const previous = (await readTextIfExists(target)) ?? '';
    return toApprovalView(approvals.stageWrite({ projectRoot, relPath: args.rel, content: String(args.content ?? ''), previous }));
  });
  ipcMain.handle('project:checkpoints', async () => {
    try {
      return (await fs.readdir(checkpointRoot())).filter((name) => name.endsWith('.json')).sort().reverse().slice(0, 20);
    } catch {
      return [];
    }
  });
  ipcMain.handle('project:restore-checkpoint', async (_event, args: { file: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    if (!args || !/^[\w.-]+\.json$/.test(args.file)) {
      throw new Error('Invalid checkpoint file.');
    }
    const record = parseCheckpointRecord(await fs.readFile(path.join(checkpointRoot(), args.file), 'utf8'), projectRoot);
    const target = await resolveWithinRoot(projectRoot, record.relPath);
    const current = (await readTextIfExists(target)) ?? '';
    if (record.previous === null) {
      return toApprovalView(approvals.stageDelete({ projectRoot, relPath: record.relPath, previous: current }));
    }
    return toApprovalView(stageCheckpointRestore(approvals, { projectRoot, record, current }));
  });

  ipcMain.handle('project:index', async (): Promise<ProjectIndex> => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    const info = await fs.stat(projectRoot).catch(() => null);
    if (!info?.isDirectory()) {
      throw new Error('The project folder is no longer available.');
    }
    return indexProject(projectRoot);
  });

  ipcMain.handle('playtest:ledger', async (): Promise<PlaytestLedgerEntry[]> => {
    if (!projectRoot) {
      return [];
    }
    return readPlaytestLedger(projectRoot);
  });

  ipcMain.handle('playtest:repair-plan', async (): Promise<PlaytestRepairPlan | null> => {
    if (!projectRoot) {
      return null;
    }
    return deriveRepairPlan(await readPlaytestLedger(projectRoot));
  });

  ipcMain.handle('media:hardware', async (): Promise<MediaReport> => detectMediaReport());

  ipcMain.handle('media:comfy-status', async (): Promise<ComfyHealth> => getComfyHealth(comfyBaseUrl()));

  ipcMain.handle('media:prepare', async (_event, input: ComfyGenerationInput): Promise<ComfyPlanSummary> => {
    if (!input || typeof input.presetId !== 'string' || typeof input.prompt !== 'string') {
      throw new Error('A ComfyUI preset and prompt are required.');
    }
    pruneMediaPlans();
    if (mediaPlans.size >= 30) {
      throw new Error('Too many prepared media plans. Generate or wait for one to expire.');
    }
    const plan = await prepareComfyGeneration(comfyBaseUrl(), input);
    mediaPlans.set(plan.planId, { plan, expiresAt: Date.now() + MEDIA_PLAN_TTL_MS });
    return publicPlan(plan);
  });

  ipcMain.handle('media:generate', async (_event, args: { planId?: unknown }): Promise<ComfyGenerationResult> => {
    const planId = typeof args?.planId === 'string' ? args.planId : '';
    const stored = planId ? mediaPlans.get(planId) : undefined;
    if (!stored || stored.expiresAt <= Date.now()) {
      if (planId) {
        mediaPlans.delete(planId);
      }
      throw new Error('That media plan is missing or expired. Prepare it again.');
    }
    mediaPlans.delete(planId);
    return runComfyPlan(comfyBaseUrl(), stored.plan, path.join(app.getPath('userData'), 'media'));
  });

  ipcMain.handle('command:preview', async (_event, args: { command: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    const cwd = await resolveWithinRoot(projectRoot, '.');
    return toApprovalView(approvals.stageCommand({ projectRoot, command: args.command, cwd }));
  });

  ipcMain.handle('approvals:list', () => approvals.list().map(toApprovalView));
  registerApprovalChannels((channel, listener) => ipcMain.handle(channel, listener), resolveApproval);

  ipcMain.handle('mcp:status', (): { running: boolean; tools: McpToolSummary[] } => ({ running: mcp.running, tools: mcp.listTools() }));
  // Starting a server runs a local program, so it is staged like any other command.
  ipcMain.handle('mcp:connect', (_event, spec: unknown): PendingApprovalView => toApprovalView(stageMcpConnect(approvals, mcp, spec)));
  ipcMain.handle('mcp:disconnect', () => {
    studioReads.clear();
    return mcp.stop();
  });
  const studioCheckpoints = (): string => path.join(app.getPath('userData'), 'studio-checkpoints');
  ipcMain.handle('studio:checkpoints', async () => {
    try {
      return (await fs.readdir(studioCheckpoints())).filter((name) => name.endsWith('.json')).sort().reverse().slice(0, 20);
    } catch {
      return [];
    }
  });
  // Restoring a Studio script is another Studio write, so it is staged for approval like the original.
  ipcMain.handle('studio:restore-checkpoint', async (_event, args: { file: string }): Promise<PendingApprovalView> => {
    if (!projectRoot || !args || !/^[\w.-]+\.json$/.test(args.file)) {
      throw new Error('Invalid checkpoint file.');
    }
    const record = JSON.parse(await fs.readFile(path.join(studioCheckpoints(), args.file), 'utf8')) as { projectRoot: string; checkpoint: StudioCheckpoint };
    if (path.resolve(record.projectRoot) !== path.resolve(projectRoot)) {
      throw new Error('Checkpoint belongs to a different project.');
    }
    return toApprovalView(stageStudioRestore(approvals, mcp, record.checkpoint, { projectRoot }));
  });
  ipcMain.handle('capabilities:detect', (): Promise<CapabilityInfo[]> => detectCapabilities());

  ipcMain.handle('sessions:load', async () => {
    try {
      return sanitizeSessions(JSON.parse(await fs.readFile(sessionsPath(), 'utf8')) as unknown);
    } catch {
      return { activeId: null, sessions: [] };
    }
  });
  ipcMain.handle('sessions:save', async (_event, payload: unknown) => {
    const snapshot = sanitizeSessions(payload);
    await writeTextAtomic(sessionsPath(), JSON.stringify(snapshot));
    return { ok: true, count: snapshot.sessions.length };
  });

  ipcMain.handle('shell:open-external', async (_event, args: { url?: string }) => {
    let parsed: URL;
    try {
      parsed = new URL(String(args?.url ?? ''));
    } catch {
      throw new Error('Invalid link.');
    }
    if (parsed.protocol !== 'https:') {
      throw new Error('Only https links can be opened.');
    }
    await shell.openExternal(parsed.toString());
    return { ok: true };
  });
}

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1240,
    height: 820,
    minWidth: 960,
    minHeight: 640,
    autoHideMenuBar: true,
    title: 'Open-Ro Studio',
    backgroundColor: '#0c0e12',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
  });
  const allowNavigation = (url: string): boolean => (app.isPackaged ? url.startsWith('file:') : url.startsWith('http://127.0.0.1:5173'));
  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (!allowNavigation(url)) {
      event.preventDefault();
      if (url.startsWith('https://')) {
        void shell.openExternal(url);
      }
    }
  });
  if (!app.isPackaged) {
    void mainWindow.loadURL('http://127.0.0.1:5173');
  } else {
    void mainWindow.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('https://')) {
      void shell.openExternal(url);
    }
    return { action: 'deny' };
  });
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

app.whenReady().then(async () => {
  registerIpc();
  await loadSavedProjectRoot();
  createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  void mcp.stop();
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

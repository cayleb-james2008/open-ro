import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron';
import * as crypto from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { registerApprovalChannels } from './approvalChannels';
import { ApprovalQueue, type ApprovalKind, type PendingApproval } from './approvals';
import { detectCapabilities } from './capabilities';
import { McpStdioClient } from './mcp';
import { isMcpAgentTool, mcpToolSchemas, runApprovedMcpCall, runApprovedMcpServer, serverLabel, stageMcpCall, stageMcpConnect } from './mcpTools';
import { gradePlaytest, parsePlaytestReport } from './playtestEvidence';
import { runApprovedStudio, stageStudioRestore, stageStudioTool, StudioReads, type StudioCheckpoint } from './studioTools';
import { isIgnoredName, isSecretName, resolveWithinRoot } from './pathSafety';
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
/** What approved Studio reads saw; a Studio write needs one for its diff. */
const studioReads = new StudioReads();

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
  await fs.mkdir(app.getPath('userData'), { recursive: true });
  await fs.writeFile(statePath(), JSON.stringify(state, null, 2), { encoding: 'utf8', mode: 0o600 });
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

/** Studio snapshots and checkpoints are kept as JSON under userData/<folder>. */
async function writeStudioRecord(folder: 'studio-snapshots' | 'studio-checkpoints', record: object): Promise<void> {
  const dir = path.join(app.getPath('userData'), folder);
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, `${new Date().toISOString().replace(/[:.]/g, '-')}.json`), JSON.stringify(record, null, 2), 'utf8');
}

function buildExecutor(root: string): ToolExecutor {
  return {
    schemas: () => [...toolSchemas(), ...mcpToolSchemas(mcp.running ? mcp.listTools() : [])],
    async execute(call) {
      if (!root) {
        return { ok: false, output: 'No project folder selected.' };
      }
      const { name, args } = call;
      try {
        switch (name) {
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
            const verdict = await gradePlaytest(report, (reported) => resolveWithinRoot(root, reported));
            await writePlaytestRecord(root, report, verdict);
            const failing = verdict.failingCheck ? ` Failing check: ${verdict.failingCheck}` : '';
            return { ok: verdict.status === 'PASSED', output: `Playtest ${verdict.status}. ${verdict.reason}${failing}` };
          }
          case 'studio_snapshot':
          case 'studio_read_script':
          case 'studio_write_script':
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
  if (item.kind !== 'mcp-server' && (!projectRoot || path.resolve(item.projectRoot) !== path.resolve(projectRoot))) {
    throw new Error('Project folder changed. The staged action was discarded.');
  }
  if (decision === 'reject') {
    return {
      ok: true,
      decision,
      summary:
        item.kind === 'write'
          ? `Rejected the write to ${item.relPath ?? 'the file'}. Nothing was changed.`
          : item.kind === 'mcp'
            ? `Rejected MCP tool ${item.mcpTool ?? ''}. It did not run.`
            : item.kind === 'mcp-server'
              ? 'Rejected starting the MCP server. It did not run.'
            : item.kind === 'studio'
              ? `Rejected the Studio ${item.studioOp ?? 'call'} of game.${item.studioPath ?? ''}. Nothing ran in Studio.`
            : 'Rejected the command. It did not run.',
    };
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
  if (item.kind === 'mcp') {
    return runApprovedMcpCall(item, mcp);
  }
  if (item.kind === 'studio') {
    const { snapshot, checkpoint, ...result } = await runApprovedStudio(item, mcp, studioReads);
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

  ipcMain.handle('agent:send', async (_event, args: { messages: ChatMessage[] }): Promise<AgentTurnResult> => {
    const connection = await resolveActiveConnection();
    if (!connection) {
      return { ok: false, assistantText: '', toolTrace: [], error: 'Connect a model first (Models).' };
    }
    if (!connection.model && getProviderOption(connection.kind).connection !== 'cli') {
      return { ok: false, assistantText: '', toolTrace: [], error: 'Enter a model ID for the active provider.' };
    }
    return runAgentTurn({
      connection,
      messages: requireMessages(args?.messages ?? []),
      projectRoot,
      executor: buildExecutor(projectRoot),
    });
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
    if (projectRoot && path.resolve(next) !== path.resolve(projectRoot)) {
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
    const record = JSON.parse(await fs.readFile(path.join(checkpointRoot(), args.file), 'utf8')) as { projectRoot: string; relPath: string; previous: string | null };
    if (path.resolve(record.projectRoot) !== path.resolve(projectRoot)) {
      throw new Error('Checkpoint belongs to a different project.');
    }
    const target = await resolveWithinRoot(projectRoot, record.relPath);
    if (record.previous === null) {
      await fs.rm(target, { force: true });
    } else {
      await fs.mkdir(path.dirname(target), { recursive: true });
      await fs.writeFile(target, record.previous, 'utf8');
    }
    return { ok: true };
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
    await fs.mkdir(app.getPath('userData'), { recursive: true });
    await fs.writeFile(sessionsPath(), JSON.stringify(snapshot), { encoding: 'utf8', mode: 0o600 });
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

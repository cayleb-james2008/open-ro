import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron';
import { spawn } from 'node:child_process';
import * as crypto from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { detectCapabilities } from './capabilities';
import { McpStdioClient, type McpServerSpec } from './mcp';
import {
  deleteCredential,
  loadCredential,
  saveCredential,
  secureStorageAvailable,
  listCredentialServices,
  storePath,
} from './secureStore';
import { getProviderOption, providerOptions, type ProviderKind } from './providerCatalog';
import { runAgentTurn, testConnection, toolSchemas } from './agent';
import type {
  AgentTurnResult,
  CapabilityInfo,
  ChatMessage,
  ConnectionTestResult,
  FileEntry,
  McpToolSummary,
  ProviderConnection,
  StoredProviderView,
  ToolExecutor,
} from './types';

/**
 * Electron main process: owns privileged operations behind a validated typed
 * IPC boundary (docs/ARCHITECTURE.md). The renderer never sees raw provider
 * credentials, Node objects, or unrestricted filesystem access.
 */

let mainWindow: BrowserWindow | null = null;
const mcp = new McpStdioClient();

// --- Approval tokens ---------------------------------------------------------
// Writes and shell commands are staged: the preview step returns a short-lived
// token bound to the exact (path, content) / (cwd, command); only a matching
// token executes. This keeps the approval gate inside the main process.
const APPROVAL_TTL_MS = 10 * 60_000;
const approvals = new Map<string, { action: 'write' | 'command'; hash: string; payload: string; expires: number }>();

function approvalToken(action: 'write' | 'command', payload: string): string {
  const hash = crypto.createHash('sha256').update(`${action}:${payload}`).digest('hex');
  const token = crypto.randomUUID();
  approvals.set(token, { action, hash, payload, expires: Date.now() + APPROVAL_TTL_MS });
  return token;
}

function consumeApproval(action: 'write' | 'command', payload: string, token: string): boolean {
  const entry = approvals.get(token);
  if (!entry || entry.action !== action || entry.hash !== crypto.createHash('sha256').update(`${action}:${payload}`).digest('hex')) {
    return false;
  }
  approvals.delete(token);
  return true;
}

setInterval(() => {
  const now = Date.now();
  for (const [token, entry] of approvals) {
    if (entry.expires < now) {
      approvals.delete(token);
    }
  }
}, 60_000).unref?.();

// --- Active provider connection ----------------------------------------------
interface AppState {
  active?: { kind: ProviderKind; model: string; baseUrl?: string };
}
const statePath = (): string => path.join(app.getPath('userData'), 'state.json');

async function readAppState(): Promise<AppState> {
  try {
    return JSON.parse(await fs.readFile(statePath(), 'utf8')) as AppState;
  } catch {
    return {};
  }
}

async function writeAppState(state: AppState): Promise<void> {
  await fs.mkdir(app.getPath('userData'), { recursive: true });
  await fs.writeFile(statePath(), JSON.stringify(state, null, 2), 'utf8');
}

async function resolveActiveConnection(provided?: Partial<ProviderConnection>): Promise<ProviderConnection | null> {
  const state = await readAppState();
  const kind = (provided?.kind ?? state.active?.kind) as ProviderKind | undefined;
  if (!kind) {
    return null;
  }
  const model = provided?.model ?? state.active?.model ?? '';
  const baseUrl = provided?.baseUrl ?? state.active?.baseUrl;
  const apiKey = provided?.apiKey ?? loadCredential(`provider:${kind}`);
  return { kind, model, baseUrl, apiKey };
}

// --- Project tools ------------------------------------------------------------
const IGNORED = new Set(['.git', 'node_modules', 'dist', 'dist-electron', 'release', 'coverage', '.openro']);

function resolveWithin(root: string, rel: string): string {
  const target = path.resolve(root, rel.replace(/^[/\\]+/, ''));
  const normalizedRoot = path.resolve(root);
  if (target !== normalizedRoot && !target.startsWith(normalizedRoot + path.sep)) {
    throw new Error('Path escapes the selected project root.');
  }
  return target;
}

async function listFiles(root: string, rel: string): Promise<FileEntry[]> {
  const target = resolveWithin(root, rel || '.');
  const entries = await fs.readdir(target, { withFileTypes: true });
  return entries
    .filter((entry) => !IGNORED.has(entry.name) && !entry.name.startsWith('.'))
    .map((entry): FileEntry => ({ name: entry.name, path: path.join(rel || '.', entry.name), type: entry.isDirectory() ? 'dir' : 'file' }))
    .sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === 'dir' ? -1 : 1));
}  // --- Agent executor -----------------------------------------------------------
function buildExecutor(root: string): ToolExecutor {
  return {
    schemas: () => toolSchemas(),
    async execute(call) {
      const { name, args } = call;
      switch (name) {
        case 'list_dir': {
          const rel = typeof args.path === 'string' ? args.path : '.';
          try {
            const entries = await listFiles(root, rel);
            return { ok: true, output: entries.map((entry) => `${entry.type === 'dir' ? '[d]' : '[f]'} ${entry.path}`).join('\n') || '(empty directory)' };
          } catch (error) {
            return { ok: false, output: error instanceof Error ? error.message : 'list_dir failed.' };
          }
        }
        case 'read_file': {
          try {
            const text = await fs.readFile(resolveWithin(root, String(args.path ?? '')), 'utf8');
            return { ok: true, output: text.slice(0, 40_000) };
          } catch (error) {
            return { ok: false, output: error instanceof Error ? error.message : 'read_file failed.' };
          }
        }
        case 'write_file': {
          // Staged: agent proposes; the user approves in the UI, then main writes.
          const rel = String(args.path ?? '');
          const content = String(args.content ?? '');
          await stageCheckpoint(root, rel);
          return {
            ok: true,
            output: 'Staged for user approval. Do not assume it was written until the user approves.',
            pendingApproval: { kind: 'file-write', path: rel },
          };
        }
        case 'run_command': {
          const command = String(args.command ?? '');
          return {
            ok: true,
            output: 'Staged for user approval. Do not assume it ran until the user approves.',
            pendingApproval: { kind: 'command', command },
          };
        }
        default:
          return { ok: false, output: `Unknown tool: ${String(name)}` };
      }
    },
  };
}

// --- Checkpoints --------------------------------------------------------------
const checkpointRoot = () => path.join(app.getPath('userData'), 'checkpoints');

async function stageCheckpoint(projectRoot: string, relPath: string): Promise<void> {
  const target = resolveWithin(projectRoot, relPath);
  let previous: string | null = null;
  try {
    previous = await fs.readFile(target, 'utf8');
  } catch {
    previous = null;
  }
  const dir = path.join(checkpointRoot());
  await fs.mkdir(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const entry = { ts: stamp, projectRoot, relPath, previous };
  await fs.writeFile(path.join(dir, `${stamp}-${crypto.createHash('sha1').update(`${projectRoot}:${relPath}`).digest('hex').slice(0, 8)}.json`), JSON.stringify(entry), 'utf8');
}

// --- IPC registration -----------------------------------------------------------
function registerIpc(): void {
  // Providers
  ipcMain.handle('providers:catalog', () => providerOptions);
  ipcMain.handle('providers:list', async (): Promise<StoredProviderView[]> => {
    const state = await readAppState();
    const services = listCredentialServices().filter((service) => service.startsWith('provider:'));
    const views: StoredProviderView[] = [];
    for (const service of services) {
      const kind = service.slice('provider:'.length) as ProviderKind;
      views.push({
        kind,
        model: state.active?.kind === kind ? state.active.model : '',
        baseUrl: state.active?.kind === kind ? state.active.baseUrl : undefined,
        hasKey: true,
        active: state.active?.kind === kind,
        secure: secureStorageAvailable(),
      });
    }
    if (state.active && !services.some((service) => service === `provider:${state.active!.kind}`) && !getProviderOption(state.active.kind).keyless) {
      // active without persisted key: keep the row visible for diagnostics
      views.push({ kind: state.active.kind, model: state.active.model, baseUrl: state.active.baseUrl, hasKey: false, active: true, secure: secureStorageAvailable() });
    }
    return views;
  });
  ipcMain.handle('providers:save', async (_event, args: { kind: ProviderKind; model: string; baseUrl?: string; apiKey?: string }) => {
    const option = getProviderOption(args.kind);
    if (args.apiKey && args.apiKey.length > 0) {
      saveCredential(`provider:${args.kind}`, args.apiKey);
    }
    if (option.needsBaseUrl && !args.baseUrl) {
      throw new Error('This provider requires a base URL.');
    }
    const state = await readAppState();
    state.active = { kind: args.kind, model: args.model, baseUrl: args.baseUrl };
    await writeAppState(state);
    return { ok: true, service: `provider:${args.kind}`, secure: secureStorageAvailable() };
  });
  ipcMain.handle('providers:set-active', async (_event, args: { kind: ProviderKind }) => {
    const kind = args.kind;
    if (kind) {
      const state = await readAppState();
      const existing = state.active?.kind === kind ? state.active : undefined;
      const model = existing?.model ?? '';
      state.active = { kind, model, baseUrl: existing?.baseUrl };
      await writeAppState(state);
    } else {
      await writeAppState({});
    }
    return { ok: true };
  });
  ipcMain.handle('providers:delete', (_event, args: { kind: ProviderKind }) => {
    deleteCredential(`provider:${args.kind}`);
    return { ok: true };
  });
  ipcMain.handle('providers:storage-state', () => ({ secure: secureStorageAvailable(), services: listCredentialServices().length, store: storePath() }));
  ipcMain.handle('providers:test', async (_event, args: Partial<ProviderConnection>): Promise<ConnectionTestResult> => {
    const connection = await resolveActiveConnection(args);
    if (!connection) {
      return { ok: false, detail: 'No provider connection configured yet.' };
    }
    return testConnection(connection, getProjectRoot());
  });

  // Agent
  ipcMain.handle('agent:send', (_event, args: { sessionId: string; messages: ChatMessage[]; projectRoot: string }): Promise<AgentTurnResult> => {
    const connectionPromise = resolveActiveConnection();
    return (async () => {
      const connection = await connectionPromise;
      if (!connection) {
        return { ok: false, assistantText: '', toolTrace: [], error: 'Connect a model first (Models screen).' };
      }
      if (!connection.model && getProviderOption(connection.kind).connection !== 'cli') {
        return { ok: false, assistantText: '', toolTrace: [], error: 'Select a model for the active provider.' };
      }
      return runAgentTurn({ connection, messages: args.messages, projectRoot: args.projectRoot, executor: buildExecutor(args.projectRoot) });
    })();
  });

  // Project tools
  let projectRoot = '';
  const getProjectRoot = () => projectRoot;
  ipcMain.handle('project:select-root', async () => {
    if (!mainWindow) {
      return null;
    }
    const result = await dialog.showOpenDialog(mainWindow, { properties: ['openDirectory', 'createDirectory'], title: 'Open-Ro — choose a project folder' });
    projectRoot = result.canceled || result.filePaths.length === 0 ? projectRoot : result.filePaths[0];
    return projectRoot || null;
  });
  ipcMain.handle('project:root', () => projectRoot || null);
  ipcMain.handle('project:list', async (_event, args: { rel?: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    return listFiles(projectRoot, args.rel ?? '.');
  });
  ipcMain.handle('project:read', async (_event, args: { rel: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    return fs.readFile(resolveWithin(projectRoot, args.rel), 'utf8');
  });
  ipcMain.handle('project:preview-write', async (_event, args: { rel: string; content: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    const target = resolveWithin(projectRoot, args.rel);
    const payload = JSON.stringify({ abs: target, content: args.content });
    let previous = '';
    try {
      previous = await fs.readFile(target, 'utf8');
    } catch {
      previous = '';
    }
    return {
      abs: target,
      previous,
      token: approvalToken('write', payload),
    };
  });
  ipcMain.handle('project:write', async (_event, args: { rel: string; content: string; token: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    const target = resolveWithin(projectRoot, args.rel);
    await stageCheckpoint(projectRoot, args.rel);
    if (!consumeApproval('write', JSON.stringify({ abs: target, content: args.content }), args.token)) {
      throw new Error('Write rejected: missing or stale approval. Preview the diff again to approve.');
    }
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.writeFile(target, args.content, { encoding: 'utf8', mode: 0o644 });
    return { ok: true, path: target };
  });
  ipcMain.handle('project:checkpoints', async () => {
    try {
      return (await fs.readdir(checkpointRoot())).sort().reverse().slice(0, 20);
    } catch {
      return [];
    }
  });
  ipcMain.handle('project:restore-checkpoint', async (_event, args: { file: string }) => {
    if (!projectRoot) {
      throw new Error('No project folder selected.');
    }
    if (!/^[\w.-]+\.json$/.test(args.file)) {
      throw new Error('Invalid checkpoint file.');
    }
    const record = JSON.parse(await fs.readFile(path.join(checkpointRoot(), args.file), 'utf8')) as { projectRoot: string; relPath: string; previous: string | null };
    if (path.resolve(record.projectRoot) !== path.resolve(projectRoot)) {
      throw new Error('Checkpoint belongs to a different project.');
    }
    const target = resolveWithin(projectRoot, record.relPath);
    if (record.previous === null) {
      await fs.rm(target, { force: true });
    } else {
      await fs.writeFile(target, record.previous, 'utf8');
    }
    return { ok: true };
  });

  // Approval-gated shell execution
  ipcMain.handle('command:preview', (_event, args: { command: string; cwd?: string }) => {
    const cwd = args.cwd || projectRoot || os.homedir();
    const payload = JSON.stringify({ cwd, command: args.command });
    return { cwd, token: approvalToken('command', payload), executable: args.command.split(/\s+/)[0] };
  });
  ipcMain.handle('command:run', (_event, args: { command: string; token: string; timeoutMs?: number; cwd?: string }) => {
    const cwd = args.cwd || projectRoot || os.homedir();
    if (!consumeApproval('command', JSON.stringify({ cwd, command: args.command }), args.token)) {
      throw new Error('Command rejected: missing or stale approval.');
    }
    return new Promise<{ code: number | null; stdout: string; stderr: string; timedOut: boolean }>((resolve, reject) => {
      const timeoutMs = args.timeoutMs ?? 60_000;
      const isWin = process.platform === 'win32';
      const child = spawn(isWin ? 'cmd.exe' : '/bin/sh', isWin ? ['/c', args.command] : ['-lc', args.command], { cwd, env: { PATH: process.env.PATH ?? '', HOME: process.env.HOME ?? '', APPDATA: process.env.APPDATA ?? '' } });
      let stdout = '';
      let stderr = '';
      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        child.kill();
      }, timeoutMs);
      child.stdout.setEncoding('utf8');
      child.stdout.on('data', (chunk) => {
        stdout += chunk;
      });
      child.stderr.setEncoding('utf8');
      child.stderr.on('data', (chunk) => {
        stderr += chunk;
      });
      child.on('error', (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.on('exit', (code) => {
        clearTimeout(timer);
        if (timedOut) {
          reject(new Error(`Command timed out after ${timeoutMs} ms and was killed: ${stdout.slice(0, 2000)}${stderr.slice(0, 2000)}`));
        } else {
          resolve({ code, stdout: stdout.slice(0, 20_000), stderr: stderr.slice(0, 20_000), timedOut });
        }
      });
    });
  });

  // MCP (single active server; limitation kept per docs/BUILD-STATUS.md)
  ipcMain.handle('mcp:status', (): { running: boolean; tools: McpToolSummary[] } => ({ running: mcp.running, tools: mcp.listTools() }));
  ipcMain.handle('mcp:connect', async (_event, spec: McpServerSpec): Promise<McpToolSummary[]> => {
    await mcp.start({ command: `${spec.command}`, args: spec.args });
    const tools = mcp.listTools();
    return tools;
  });
  ipcMain.handle('mcp:disconnect', () => mcp.stop());
  ipcMain.handle('mcp:call', async (_event, args: { name: string; arguments: Record<string, unknown> }) => {
    if (!mcp.running) {
      throw new Error('No MCP server is connected.');
    }
    const text = await mcp.callTool(`${args.name}`, args.arguments ?? {});
    return { ok: true, output: text.slice(0, 20_000) };
  });

  // Capabilities
  ipcMain.handle('capabilities:detect', (): Promise<CapabilityInfo[]> => detectCapabilities());
}

// --- Window lifecycle ----------------------------------------------------------
function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1240,
    height: 800,
    minWidth: 960,
    minHeight: 640,
    autoHideMenuBar: true,
    title: 'Open-Ro Studio',
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
  });
  if (!app.isPackaged) {
    void mainWindow.loadURL('http://127.0.0.1:5173');
    mainWindow.webContents.openDevTools({ mode: 'detach' });
  } else {
    void mainWindow.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https:\/\//.test(url)) {
      void shell.openExternal(url);
    }
    return { action: 'deny' };
  });
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

app.whenReady().then(() => {
  registerIpc();
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

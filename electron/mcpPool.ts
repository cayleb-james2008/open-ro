import * as crypto from 'node:crypto';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { pushMcpChunk } from './mcpFraming';
import { scrubbedEnv } from './toolEnv';
import type { McpServerIdentity, McpToolSummary, ToolSchema } from './types';
import { extractServerIdFromIdentity, type McpCaller, type McpServerStarter } from './mcpTools';

/**
 * Multi-Server MCP Architecture & Connection Pooling (Milestone 3).
 *
 * Manages concurrent MCP child processes with connection pooling (max 6 active),
 * LRU hibernation, active ping heartbeats, crash recovery with exponential backoff
 * and circuit breaker, process tree kill zombie prevention, tool namespacing,
 * and session security binding.
 */

export interface McpServerConfig {
  id: string;
  name?: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
  autoRestart?: boolean;
  maxRestarts?: number;
  requestTimeoutMs?: number;
  disabled?: boolean;
}

export interface McpServerInfo {
  id: string;
  command: string;
  args: string[];
  session: string;
  tools: McpToolSummary[];
}

export type McpServerStatus =
  | 'connected'
  | 'connecting'
  | 'disconnected'
  | 'hibernating'
  | 'error'
  | 'reconnecting'
  | 'crashed'
  | 'failed';

export interface McpServerView {
  id: string;
  name: string;
  command: string;
  args: string[];
  running: boolean;
  status: McpServerStatus;
  error?: string;
  session: string | null;
  toolCount: number;
  tools: McpToolSummary[];
  health: {
    isHealthy: boolean;
    lastPingMs?: number;
    consecutiveFailures: number;
    uptimeMs?: number;
    restartCount: number;
  };
}

export interface McpPooledToolSummary {
  serverId: string;
  serverName: string;
  agentToolName: string;
  directAgentToolName?: string;
  tool: McpToolSummary;
}

export interface McpPoolHealthReport {
  activeCount: number;
  maxActive: number;
  totalConfigured: number;
  healthyCount: number;
  unhealthyCount: number;
  servers: Record<string, { isHealthy: boolean; pingMs?: number; status: string }>;
}

export type McpServerPoolEvent =
  | { type: 'server-started'; serverId: string; toolCount: number }
  | { type: 'server-stopped'; serverId: string; reason?: string }
  | { type: 'server-crashed'; serverId: string; error: string; willRestart: boolean }
  | { type: 'server-restarted'; serverId: string; session: string }
  | { type: 'health-updated'; serverId: string; isHealthy: boolean; pingMs: number };

type JsonRpcMessage = {
  jsonrpc: '2.0';
  id?: number | string;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
  error?: { code: number; message: string };
};

/**
 * Cross-platform process tree termination:
 * On Windows, executes taskkill.exe /pid <pid> /t /f.
 * On POSIX, executes SIGKILL on the detached process group (-pid).
 */
export async function killProcessTree(child: ChildProcessWithoutNullStreams): Promise<void> {
  if (!child.pid || child.exitCode !== null) {
    return;
  }
  const pid = child.pid;
  if (process.platform === 'win32') {
    await new Promise<void>((resolve) => {
      const killer = spawn('taskkill.exe', ['/pid', String(pid), '/t', '/f'], {
        windowsHide: true,
        windowsVerbatimArguments: false,
      });
      const finish = () => {
        try {
          child.kill('SIGKILL');
        } catch {
          // Process may have already exited
        }
        resolve();
      };
      killer.on('exit', finish);
      killer.on('error', finish);
    });
  } else {
    try {
      process.kill(-pid, 'SIGKILL');
    } catch {
      try {
        child.kill('SIGKILL');
      } catch {
        // Process may have already exited
      }
    }
  }
}

export interface McpPoolClientOptions {
  heartbeatIntervalMs?: number;
  heartbeatTimeoutMs?: number;
  maxRestarts?: number;
  restartWindowMs?: number;
  onEvent?: (event: McpServerPoolEvent) => void;
  /** Hook called prior to auto-waking a hibernating server to enforce pool capacity */
  onBeforeWake?: (client: McpPoolClient) => Promise<void>;
}

/**
 * Managed client process inside the connection pool.
 */
export class McpPoolClient implements McpCaller {
  private child: ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: unknown) => void; reject: (err: Error) => void; timer: NodeJS.Timeout }>();
  private buffer = '';
  private tools: McpToolSummary[] = [];
  private serverIdentity: McpServerIdentity | null = null;
  private currentStatus: McpServerStatus = 'disconnected';
  private currentError?: string;
  private currentSession: string | null = null;
  private heartbeatTimer: NodeJS.Timeout | null = null;
  private restartWindowMs: number;
  private maxRestarts: number;
  private restartTimestamps: number[] = [];
  private restartTimer: NodeJS.Timeout | null = null;
  private startTime = 0;

  public lastUsed = 0;
  public health = {
    isHealthy: true,
    lastPingMs: undefined as number | undefined,
    consecutiveFailures: 0,
    uptimeMs: 0,
    restartCount: 0,
  };

  constructor(
    public readonly config: McpServerConfig,
    private readonly options: McpPoolClientOptions = {},
  ) {
    this.restartWindowMs = options.restartWindowMs ?? 60_000;
    this.maxRestarts = config.maxRestarts ?? options.maxRestarts ?? 3;
  }

  get running(): boolean {
    return this.child !== null && this.child.exitCode === null && this.currentStatus === 'connected';
  }

  get status(): McpServerStatus {
    return this.currentStatus;
  }

  get error(): string | undefined {
    return this.currentError;
  }

  get identity(): McpServerIdentity | null {
    if (this.running) {
      return this.serverIdentity;
    }
    if (this.currentStatus === 'hibernating' && this.currentSession) {
      return {
        command: this.config.command,
        args: [...this.config.args],
        session: this.currentSession,
        serverId: this.config.id,
      };
    }
    return null;
  }

  get session(): string | null {
    return this.currentSession;
  }

  get serverId(): string {
    return this.config.id;
  }

  listTools(): McpToolSummary[] {
    return [...this.tools];
  }

  async start(): Promise<void> {
    if (this.running) {
      return;
    }
    const isWaking = this.currentStatus === 'hibernating';
    this.stopHeartbeat();
    this.clearRestartTimer();
    this.currentStatus = 'connecting';
    this.currentError = undefined;
    if (!isWaking) {
      this.forgetServer();
    }

    const child = spawn(this.config.command, this.config.args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...scrubbedEnv(), ...(this.config.env ?? {}) },
      shell: false,
      windowsHide: true,
      detached: process.platform !== 'win32',
    });
    this.child = child;
    this.buffer = '';

    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => {
      if (this.child === child) {
        this.onData(chunk);
      }
    });

    child.stderr.setEncoding('utf8');
    child.stderr.on('data', () => {
      // Server debug logs are ignored
    });

    child.on('error', (err) => {
      if (this.child === child) {
        this.onChildGone(err);
      }
    });

    child.on('exit', () => {
      if (this.child === child) {
        this.onChildGone(new Error('MCP server exited unexpectedly.'));
      }
    });

    child.stdin.on('error', (err) => {
      if (this.child === child) {
        this.rejectAll(err);
      }
    });

    try {
      const timeoutMs = this.config.requestTimeoutMs ?? 15_000;
      await this.request('initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'Open-Ro Studio', version: '0.1.0' },
      }, timeoutMs);
      this.notify('notifications/initialized');
      await this.refreshTools(timeoutMs);

      const session = (isWaking && this.currentSession)
        ? this.currentSession
        : `pool-${this.config.id}-${crypto.randomUUID()}`;
      this.currentSession = session;
      this.serverIdentity = {
        command: this.config.command,
        args: [...this.config.args],
        session,
        serverId: this.config.id,
      };
      this.currentStatus = 'connected';
      this.lastUsed = Date.now();
      this.startTime = Date.now();
      this.health.isHealthy = true;
      this.health.consecutiveFailures = 0;

      this.startHeartbeat();
      this.options.onEvent?.({
        type: 'server-started',
        serverId: this.config.id,
        toolCount: this.tools.length,
      });
    } catch (err) {
      await this.stop();
      this.currentStatus = 'error';
      this.currentError = err instanceof Error ? err.message : String(err);
      throw err;
    }
  }

  async stop(): Promise<void> {
    this.stopHeartbeat();
    this.clearRestartTimer();
    const prevStatus = this.currentStatus;
    this.currentStatus = 'disconnected';
    this.forgetServer();
    const child = this.child;
    this.child = null;
    this.rejectAll(new Error('MCP server stopped.'));
    if (child) {
      await killProcessTree(child);
    }
    if (prevStatus === 'connected') {
      this.options.onEvent?.({
        type: 'server-stopped',
        serverId: this.config.id,
      });
    }
  }

  async hibernate(): Promise<void> {
    this.stopHeartbeat();
    this.clearRestartTimer();
    this.currentStatus = 'hibernating';
    // Retain this.tools and this.currentSession across hibernation
    const child = this.child;
    this.child = null;
    this.rejectAll(new Error('MCP server hibernated.'));
    if (child) {
      await killProcessTree(child);
    }
    this.options.onEvent?.({
      type: 'server-stopped',
      serverId: this.config.id,
      reason: 'LRU hibernation',
    });
  }

  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }

  async ping(): Promise<number> {
    const start = Date.now();
    const timeoutMs = this.options.heartbeatTimeoutMs ?? 5_000;
    await this.request('ping', {}, timeoutMs);
    const duration = Date.now() - start;
    this.lastUsed = Date.now();
    return duration;
  }

  async refreshTools(timeoutMs = 15_000): Promise<McpToolSummary[]> {
    const result = (await this.request('tools/list', {}, timeoutMs)) as {
      tools?: Array<{ name: string; description?: string; inputSchema?: unknown }>;
    };
    this.tools = (result?.tools ?? []).map((tool) => {
      const schema = tool.inputSchema;
      const objectSchema = schema && typeof schema === 'object' && !Array.isArray(schema) && (schema as { type?: unknown }).type === 'object';
      return {
        name: String(tool.name),
        description: String(tool.description ?? ''),
        ...(objectSchema ? { inputSchema: schema as Record<string, unknown> } : {}),
      };
    });
    return this.listTools();
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<string> {
    const result = await this.callToolResult(name, args);
    if (result.isError) {
      throw new Error(result.text || `MCP tool ${name} reported an error.`);
    }
    return result.text;
  }

  async callToolResult(
    name: string,
    args: Record<string, unknown>,
  ): Promise<{ text: string; images: Array<{ mimeType: string; data: string }>; isError: boolean }> {
    if (this.currentStatus === 'hibernating') {
      if (this.options.onBeforeWake) {
        await this.options.onBeforeWake(this);
      }
      await this.start();
    }
    this.lastUsed = Date.now();
    const timeoutMs = this.config.requestTimeoutMs ?? 15_000;
    const result = (await this.request('tools/call', { name, arguments: args ?? {} }, timeoutMs)) as {
      content?: Array<{ type: string; text?: string; data?: string; mimeType?: string }>;
      isError?: boolean;
    };
    const content = result?.content ?? [];
    const text = content
      .filter((entry) => entry.type === 'text' && typeof entry.text === 'string')
      .map((entry) => entry.text as string)
      .join('\n');
    const images = content
      .filter((entry) => entry.type === 'image' && typeof entry.data === 'string')
      .map((entry) => ({
        mimeType: typeof entry.mimeType === 'string' && entry.mimeType ? entry.mimeType : 'image/png',
        data: entry.data as string,
      }));
    return { text, images, isError: result?.isError === true };
  }

  getView(): McpServerView {
    const uptimeMs = this.running && this.startTime > 0 ? Date.now() - this.startTime : 0;
    return {
      id: this.config.id,
      name: this.config.name ?? this.config.id,
      command: this.config.command,
      args: [...this.config.args],
      running: this.running,
      status: this.currentStatus,
      error: this.currentError,
      session: this.currentSession,
      toolCount: this.tools.length,
      tools: this.listTools(),
      health: {
        ...this.health,
        uptimeMs,
      },
    };
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    const interval = this.options.heartbeatIntervalMs ?? 30_000;
    const timer = setInterval(() => {
      if (!this.running) {
        return;
      }
      this.ping()
        .then((pingMs) => {
          this.health.isHealthy = true;
          this.health.lastPingMs = Date.now();
          this.health.consecutiveFailures = 0;
          this.options.onEvent?.({
            type: 'health-updated',
            serverId: this.config.id,
            isHealthy: true,
            pingMs,
          });
        })
        .catch(() => {
          this.health.consecutiveFailures++;
          if (this.health.consecutiveFailures >= 2) {
            this.health.isHealthy = false;
            this.options.onEvent?.({
              type: 'health-updated',
              serverId: this.config.id,
              isHealthy: false,
              pingMs: -1,
            });
            void this.handleDeadlock();
          }
        });
    }, interval);
    timer.unref();
    this.heartbeatTimer = timer;
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private clearRestartTimer(): void {
    if (this.restartTimer) {
      clearTimeout(this.restartTimer);
      this.restartTimer = null;
    }
  }

  private async handleDeadlock(): Promise<void> {
    this.stopHeartbeat();
    const child = this.child;
    this.child = null;
    this.forgetServer();
    this.rejectAll(new Error('MCP server hung or deadlocked.'));
    if (child) {
      await killProcessTree(child);
    }
    this.onChildGone(new Error('MCP server terminated due to deadlock.'));
  }

  private onChildGone(err: Error): void {
    if (this.currentStatus === 'disconnected' || this.currentStatus === 'hibernating') {
      return;
    }
    this.stopHeartbeat();
    this.forgetServer();
    this.rejectAll(err);

    this.currentStatus = 'crashed';
    this.currentError = err.message;

    const now = Date.now();
    this.restartTimestamps = this.restartTimestamps.filter((t) => now - t < this.restartWindowMs);

    const autoRestart = this.config.autoRestart ?? true;
    const willRestart = autoRestart && this.restartTimestamps.length < this.maxRestarts;

    this.options.onEvent?.({
      type: 'server-crashed',
      serverId: this.config.id,
      error: err.message,
      willRestart,
    });

    if (!willRestart) {
      this.currentStatus = 'failed';
      this.currentError = 'Circuit breaker tripped: exceeded restart limit within 60s.';
      this.options.onEvent?.({
        type: 'server-stopped',
        serverId: this.config.id,
        reason: 'Circuit breaker tripped',
      });
      return;
    }

    const delays = [500, 1000, 2000];
    const delay = delays[this.restartTimestamps.length] ?? 2000;
    this.restartTimestamps.push(now);
    this.health.restartCount++;
    this.currentStatus = 'reconnecting';

    this.clearRestartTimer();
    const timer = setTimeout(async () => {
      try {
        await this.start();
        if (this.currentSession) {
          this.options.onEvent?.({
            type: 'server-restarted',
            serverId: this.config.id,
            session: this.currentSession,
          });
        }
      } catch (startErr) {
        this.currentStatus = 'failed';
        this.currentError = startErr instanceof Error ? startErr.message : String(startErr);
      }
    }, delay);
    timer.unref();
    this.restartTimer = timer;
  }

  private onData(chunk: string): void {
    const parsed = pushMcpChunk(this.buffer, chunk);
    this.buffer = parsed.buffer;
    for (const message of parsed.messages) {
      if (message && typeof message === 'object') {
        this.onMessage(message as JsonRpcMessage);
      }
    }
  }

  private onMessage(message: JsonRpcMessage): void {
    if (typeof message.method === 'string') {
      if (message.id !== undefined) {
        this.answerServerRequest(message.id, message.method);
      }
      return;
    }
    if (typeof message.id === 'number') {
      const pending = this.pending.get(message.id);
      if (pending) {
        clearTimeout(pending.timer);
        this.pending.delete(message.id);
        if (message.error) {
          pending.reject(new Error(message.error.message));
        } else {
          pending.resolve(message.result);
        }
      }
    }
  }

  private answerServerRequest(id: number | string, method: string): void {
    if (method === 'ping') {
      this.send({ jsonrpc: '2.0', id, result: {} });
      return;
    }
    this.send({ jsonrpc: '2.0', id, error: { code: -32601, message: `Method not supported by Open-Ro: ${method}` } });
  }

  private notify(method: string): void {
    this.send({ jsonrpc: '2.0', method, params: {} });
  }

  private request(method: string, params: Record<string, unknown>, timeoutMs: number): Promise<unknown> {
    const child = this.child;
    if (!child || child.exitCode !== null) {
      return Promise.reject(new Error('MCP server is not running.'));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new Error(`MCP request ${method} timed out.`));
        }
      }, timeoutMs);
      timer.unref();
      this.pending.set(id, { resolve, reject, timer });
      this.send({ jsonrpc: '2.0', id, method, params });
    });
  }

  private send(message: unknown): void {
    const line = JSON.stringify(message).replace(/[\u2028\u2029]/g, (char) => `\\u${char.charCodeAt(0).toString(16)}`);
    this.child?.stdin.write(`${line}\n`);
  }

  private forgetServer(): void {
    this.serverIdentity = null;
    this.tools = [];
  }

  private rejectAll(err: Error): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }
}

export interface McpServerPoolOptions {
  maxActiveServers?: number;
  heartbeatIntervalMs?: number;
  heartbeatTimeoutMs?: number;
  maxRestarts?: number;
  restartWindowMs?: number;
  onEvent?: (event: McpServerPoolEvent) => void;
  clientFactory?: (config: McpServerConfig, options: McpPoolClientOptions) => McpPoolClient;
}

/**
 * McpServerPool manages concurrent MCP servers, pooling capacity (max 6 active),
 * LRU hibernation, routing, and tool schemas.
 */
export class McpServerPool extends EventEmitter implements McpCaller, McpServerStarter {
  private configs = new Map<string, McpServerConfig>();
  private clients = new Map<string, McpPoolClient>();
  private readonly maxActiveServers: number;
  private capacityLock: Promise<void> = Promise.resolve();

  private isDiscoverable(client: McpPoolClient): boolean {
    return client.running || client.status === 'hibernating';
  }

  private async withCapacityLock<T>(fn: () => Promise<T>): Promise<T> {
    const prev = this.capacityLock;
    let release: () => void;
    this.capacityLock = new Promise<void>((resolve) => {
      release = resolve;
    });
    await prev;
    try {
      return await fn();
    } finally {
      release!();
    }
  }

  constructor(private readonly options: McpServerPoolOptions = {}) {
    super();
    this.maxActiveServers = options.maxActiveServers ?? 6;
  }

  get running(): boolean {
    return this.getStudioClient()?.running ?? this.hasAnyRunningServer();
  }

  get identity(): McpServerIdentity | null {
    return this.getStudioClient()?.identity ?? this.getDefaultClient()?.identity ?? null;
  }

  getMcpClient(serverId: string): McpCaller | undefined {
    const client = this.clients.get(serverId);
    if (!client || !client.running) {
      return undefined;
    }
    return client;
  }

  async connectMcpServer(config: McpServerConfig): Promise<McpServerInfo> {
    if (!config || !config.id || typeof config.id !== 'string' || !config.id.trim()) {
      throw new Error('Server config requires id.');
    }
    if (!config.command || typeof config.command !== 'string' || !config.command.trim()) {
      throw new Error('Server config requires id and command.');
    }

    const cleanConfig: McpServerConfig = {
      ...config,
      id: config.id.trim(),
      command: config.command.trim(),
      args: [...(config.args ?? [])],
    };

    // Connection reuse check: identical spec running -> reuse running instance
    const existing = this.clients.get(cleanConfig.id);
    if (existing && existing.running && this.specsMatch(existing.config, cleanConfig)) {
      existing.lastUsed = Date.now();
      return this.getServerInfo(cleanConfig.id);
    }

    // Spec changed or restarting: cleanly disconnect old process
    if (existing) {
      await this.disconnectMcpServer(cleanConfig.id);
    }

    // Pool capacity management: evict least recently used active server to hibernate
    await this.withCapacityLock(async () => {
      await this.enforceCapacity(cleanConfig.id);
    });

    // Create client instance
    const clientOptions: McpPoolClientOptions = {
      heartbeatIntervalMs: this.options.heartbeatIntervalMs,
      heartbeatTimeoutMs: this.options.heartbeatTimeoutMs,
      maxRestarts: cleanConfig.maxRestarts ?? this.options.maxRestarts,
      restartWindowMs: this.options.restartWindowMs,
      onEvent: (event) => {
        this.emit('event', event);
        this.options.onEvent?.(event);
      },
      onBeforeWake: async (client) => {
        await this.withCapacityLock(async () => {
          await this.enforceCapacity(client.config.id);
        });
      },
    };

    const client = this.options.clientFactory
      ? this.options.clientFactory(cleanConfig, clientOptions)
      : new McpPoolClient(cleanConfig, clientOptions);

    this.configs.set(cleanConfig.id, cleanConfig);
    this.clients.set(cleanConfig.id, client);

    await client.start();
    return this.getServerInfo(cleanConfig.id);
  }

  async disconnectMcpServer(serverId: string): Promise<void> {
    const client = this.clients.get(serverId);
    if (client) {
      this.clients.delete(serverId);
      await client.stop();
    }
  }

  async disconnectAll(): Promise<void> {
    const all = Array.from(this.clients.values());
    this.clients.clear();
    await Promise.all(all.map((c) => c.stop()));
  }

  async wakeServer(serverId: string): Promise<McpPoolClient> {
    const client = this.clients.get(serverId);
    if (!client) {
      throw new Error(`MCP server not found in pool: ${serverId}`);
    }
    if (client.status === 'hibernating') {
      await this.withCapacityLock(async () => {
        if (client.status === 'hibernating') {
          await this.enforceCapacity(serverId);
          await client.start();
        }
      });
    }
    return client;
  }

  async callMcpTool(serverId: string, toolName: string, args: Record<string, unknown>): Promise<unknown> {
    const client = await this.wakeServer(serverId);
    const raw = await client.callTool(toolName, args);
    try {
      return JSON.parse(raw);
    } catch {
      return raw;
    }
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<string> {
    const studioClient = this.getStudioClient();
    if (studioClient && studioClient.running && studioClient.listTools().some((t) => t.name === name)) {
      return studioClient.callTool(name, args);
    }
    const resolved = this.resolveAgentTool(name);
    if (resolved) {
      return resolved.caller.callTool(resolved.tool.name, args);
    }
    for (const client of this.clients.values()) {
      if (client.running && client.listTools().some((t) => t.name === name)) {
        return client.callTool(name, args);
      }
    }
    throw new Error(`Tool ${name} not found across connected pool servers.`);
  }

  listServers(): McpServerInfo[] {
    const infos: McpServerInfo[] = [];
    for (const [id, client] of this.clients.entries()) {
      if (client.running && client.session) {
        infos.push({
          id,
          command: client.config.command,
          args: client.config.args,
          session: client.session,
          tools: client.listTools(),
        });
      }
    }
    return infos;
  }

  listTools(): McpToolSummary[] {
    return this.listAllUniqueTools();
  }

  listAllUniqueTools(): McpToolSummary[] {
    const seen = new Set<string>();
    const out: McpToolSummary[] = [];
    for (const client of this.clients.values()) {
      if (!this.isDiscoverable(client)) continue;
      for (const tool of client.listTools()) {
        if (!seen.has(tool.name)) {
          seen.add(tool.name);
          out.push(tool);
        }
      }
    }
    return out;
  }

  listAllAgentTools(): McpPooledToolSummary[] {
    const toolCounts = new Map<string, number>();
    for (const client of this.clients.values()) {
      if (!this.isDiscoverable(client)) continue;
      for (const tool of client.listTools()) {
        toolCounts.set(tool.name, (toolCounts.get(tool.name) ?? 0) + 1);
      }
    }

    const summaries: McpPooledToolSummary[] = [];
    for (const [serverId, client] of this.clients.entries()) {
      if (!this.isDiscoverable(client)) continue;
      const serverName = client.config.name || serverId;
      const cleanServer = serverId.replace(/[^A-Za-z0-9_-]/g, '_');
      for (const tool of client.listTools()) {
        const cleanTool = tool.name.replace(/[^A-Za-z0-9_-]/g, '_');
        const agentToolName = `mcp__${cleanServer}__${cleanTool}`.slice(0, 64);
        const isUnique = toolCounts.get(tool.name) === 1;
        const directAgentToolName = isUnique ? `mcp__${cleanTool}`.slice(0, 64) : undefined;
        summaries.push({
          serverId,
          serverName,
          agentToolName,
          directAgentToolName,
          tool,
        });
      }
    }
    return summaries;
  }

  resolveAgentTool(agentName: string): { tool: McpToolSummary; serverIdentity: McpServerIdentity; caller: McpCaller } | null {
    const all = this.listAllAgentTools();
    const exactMatch = all.find((item) => item.agentToolName === agentName);
    if (exactMatch) {
      const client = this.clients.get(exactMatch.serverId);
      if (client && this.isDiscoverable(client) && client.identity) {
        return { tool: exactMatch.tool, serverIdentity: client.identity, caller: client };
      }
    }

    const directMatches = all.filter((item) => item.directAgentToolName === agentName);
    if (directMatches.length === 1) {
      const match = directMatches[0];
      const client = this.clients.get(match.serverId);
      if (client && this.isDiscoverable(client) && client.identity) {
        return { tool: match.tool, serverIdentity: client.identity, caller: client };
      }
    } else if (directMatches.length > 1) {
      const servers = directMatches.map((m) => m.serverId).join(', ');
      throw new Error(`Agent tool "${agentName}" is ambiguous across servers: ${servers}. Use namespaced form.`);
    }

    return null;
  }

  getAgentToolSchemas(): ToolSchema[] {
    const tools = this.listAllAgentTools();
    const schemas: ToolSchema[] = [];
    for (const item of tools) {
      const names = [item.agentToolName];
      if (item.directAgentToolName) {
        names.push(item.directAgentToolName);
      }
      for (const name of names) {
        schemas.push({
          name: name as ToolSchema['name'],
          description: `Call MCP tool "${item.tool.name}" on server "${item.serverName}". User must approve exact arguments. ${item.tool.description}`.trim().slice(0, 1000),
          parameters: this.providerSafeSchema(item.tool.inputSchema),
        });
      }
    }
    return schemas;
  }

  hasAnyRunningServer(): boolean {
    for (const client of this.clients.values()) {
      if (client.running) return true;
    }
    return false;
  }

  getDefaultClient(): McpPoolClient | undefined {
    return this.clients.get('studio') ?? this.clients.get('default') ?? Array.from(this.clients.values()).find((c) => c.running);
  }

  getStudioClient(): McpCaller | undefined {
    return this.clients.get('studio') ?? this.getDefaultClient();
  }

  getClientForApproval(serverIdentity?: McpServerIdentity): McpPoolClient | undefined {
    if (!serverIdentity) {
      return this.getDefaultClient();
    }
    const targetId = extractServerIdFromIdentity(serverIdentity);
    if (targetId) {
      const client = this.clients.get(targetId);
      if (client) {
        return client;
      }
    }
    for (const client of this.clients.values()) {
      if (client.session === serverIdentity.session) {
        return client;
      }
    }
    return this.getDefaultClient();
  }

  listServerViews(): McpServerView[] {
    const views: McpServerView[] = [];
    const allIds = new Set([...this.configs.keys(), ...this.clients.keys()]);
    for (const id of allIds) {
      const client = this.clients.get(id);
      if (client) {
        views.push(client.getView());
      } else {
        const config = this.configs.get(id)!;
        views.push({
          id: config.id,
          name: config.name ?? config.id,
          command: config.command,
          args: config.args,
          running: false,
          status: 'disconnected',
          session: null,
          toolCount: 0,
          tools: [],
          health: {
            isHealthy: true,
            consecutiveFailures: 0,
            restartCount: 0,
          },
        });
      }
    }
    return views;
  }

  getServerView(serverId: string): McpServerView | null {
    const client = this.clients.get(serverId);
    if (client) {
      return client.getView();
    }
    const config = this.configs.get(serverId);
    if (config) {
      return {
        id: config.id,
        name: config.name ?? config.id,
        command: config.command,
        args: config.args,
        running: false,
        status: 'disconnected',
        session: null,
        toolCount: 0,
        tools: [],
        health: {
          isHealthy: true,
          consecutiveFailures: 0,
          restartCount: 0,
        },
      };
    }
    return null;
  }

  getHealthReport(): McpPoolHealthReport {
    const views = this.listServerViews();
    const active = views.filter((v) => v.running);
    const healthy = views.filter((v) => v.health.isHealthy);
    const serversRecord: McpPoolHealthReport['servers'] = {};
    for (const v of views) {
      serversRecord[v.id] = {
        isHealthy: v.health.isHealthy,
        pingMs: v.health.lastPingMs,
        status: v.status,
      };
    }
    return {
      activeCount: active.length,
      maxActive: this.maxActiveServers,
      totalConfigured: views.length,
      healthyCount: healthy.length,
      unhealthyCount: views.length - healthy.length,
      servers: serversRecord,
    };
  }

  async saveServerConfig(config: McpServerConfig): Promise<void> {
    this.configs.set(config.id, { ...config });
  }

  async deleteServerConfig(serverId: string): Promise<void> {
    await this.disconnectMcpServer(serverId);
    this.configs.delete(serverId);
  }

  // McpServerStarter implementation for legacy default client support
  async start(spec: { command: string; args: string[] }): Promise<void> {
    await this.connectMcpServer({
      id: 'default',
      command: spec.command,
      args: spec.args,
    });
  }

  private getServerInfo(serverId: string): McpServerInfo {
    const client = this.clients.get(serverId);
    if (!client || !client.session) {
      throw new Error(`MCP server not found in pool: ${serverId}`);
    }
    return {
      id: serverId,
      command: client.config.command,
      args: client.config.args,
      session: client.session,
      tools: client.listTools(),
    };
  }

  private async enforceCapacity(excludeServerId?: string): Promise<void> {
    while (true) {
      const active = Array.from(this.clients.values()).filter(
        (c) => (c.status === 'connected' || c.status === 'connecting') && c.config.id !== excludeServerId,
      );
      if (active.length < this.maxActiveServers) {
        break;
      }
      const lru = active.sort((a, b) => a.lastUsed - b.lastUsed)[0];
      if (!lru) {
        break;
      }
      await lru.hibernate();
    }
  }

  private specsMatch(a: McpServerConfig, b: McpServerConfig): boolean {
    if (a.command !== b.command || a.args.length !== b.args.length) {
      return false;
    }
    for (let i = 0; i < a.args.length; i++) {
      if (a.args[i] !== b.args[i]) return false;
    }
    const envA = a.env ?? {};
    const envB = b.env ?? {};
    const keysA = Object.keys(envA);
    const keysB = Object.keys(envB);
    if (keysA.length !== keysB.length) return false;
    for (const key of keysA) {
      if (envA[key] !== envB[key]) return false;
    }
    return true;
  }

  private providerSafeSchema(schema: Record<string, unknown> | undefined): ToolSchema['parameters'] {
    if (!schema) {
      return { type: 'object', properties: {} };
    }
    const { $schema: _schema, $id: _id, ...rest } = schema;
    return { properties: {}, ...rest, type: 'object' } as ToolSchema['parameters'];
  }
}

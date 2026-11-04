import * as crypto from 'node:crypto';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { pushMcpChunk } from './mcpFraming';
import { scrubbedEnv } from './toolEnv';
import type { McpServerIdentity, McpToolSummary } from './types';

/**
 * Minimal MCP stdio client: initialize handshake, tool discovery and tool
 * calls. Supports one active server at a time (0.1.0 limitation kept visible
 * in the UI and docs/BUILD-STATUS.md).
 */

export interface McpServerSpec {
  command: string;
  args: string[];
}

type JsonRpcMessage = {
  jsonrpc: '2.0';
  id?: number | string;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
  error?: { code: number; message: string };
};

export class McpStdioClient {
  private child: ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: unknown) => void; reject: (err: Error) => void; timer: NodeJS.Timeout }>();
  private buffer = '';
  private tools: McpToolSummary[] = [];
  private serverIdentity: McpServerIdentity | null = null;

  constructor(private readonly requestTimeoutMs = 15_000) {}

  /** Command, arguments and a per-start session id of the running server, or null. */
  get identity(): McpServerIdentity | null {
    return this.running ? this.serverIdentity : null;
  }

  get running(): boolean {
    return this.child !== null && this.child.exitCode === null;
  }

  async start(spec: McpServerSpec): Promise<void> {
    if (this.running) {
      throw new Error('An MCP server is already active. Stop it before connecting another.');
    }
    // Nothing from an earlier process (for example one that crashed without stop()) may describe this one.
    this.forgetServer();
    const child = spawn(spec.command, spec.args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: scrubbedEnv(),
      shell: false,
      windowsHide: true,
    });
    this.child = child;
    this.buffer = '';
    // Events from an older process must not reach requests sent to a newer one.
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => this.child === child && this.onData(chunk));
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', () => {
      /* server logs are not treated as protocol failures */
    });
    child.on('error', (error) => this.child === child && this.onChildGone(error));
    child.on('exit', () => this.child === child && this.onChildGone(new Error('MCP server exited unexpectedly.')));
    child.stdin.on('error', (error) => this.child === child && this.rejectAll(error));
    try {
      await this.request('initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'Open-Ro Studio', version: '0.1.0' },
      });
      this.notify('notifications/initialized');
      await this.refreshTools();
      this.serverIdentity = { command: spec.command, args: [...spec.args], session: crypto.randomUUID() };
    } catch (error) {
      // A failed handshake must not leave a live process that blocks the next connect.
      await this.stop();
      throw error;
    }
  }

  async stop(): Promise<void> {
    this.serverIdentity = null;
    const child = this.child;
    if (!child) {
      return;
    }
    this.child = null;
    this.tools = [];
    this.buffer = '';
    this.rejectAll(new Error('MCP server stopped.'));
    child.kill();
  }

  listTools(): McpToolSummary[] {
    return [...this.tools];
  }

  async refreshTools(): Promise<McpToolSummary[]> {
    const result = (await this.request('tools/list', {})) as { tools?: Array<{ name: string; description?: string; inputSchema?: unknown }> };
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

  /** Full tool result: text plus any base64 image blocks (for example screen_capture). */
  async callToolResult(name: string, args: Record<string, unknown>): Promise<{ text: string; images: Array<{ mimeType: string; data: string }>; isError: boolean }> {
    const result = (await this.request('tools/call', { name, arguments: args ?? {} })) as {
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

  /**
   * MCP stdio transport: one JSON-RPC message per line, no embedded newlines.
   * U+2028 and U+2029 are escaped because line readers such as Node's readline
   * treat them as line breaks, and JSON.stringify leaves them raw.
   */
  private send(message: unknown): void {
    const line = JSON.stringify(message).replace(/[\u2028\u2029]/g, (char) => `\\u${char.charCodeAt(0).toString(16)}`);
    this.child?.stdin.write(`${line}\n`);
  }

  /** Server-to-client requests: ping gets an empty result; this client offers no other capabilities. */
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

  private request(method: string, params: Record<string, unknown>): Promise<unknown> {
    const child = this.child;
    if (!child || !this.running) {
      return Promise.reject(new Error('MCP server is not running.'));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new Error(`MCP request ${method} timed out.`));
        }
      }, this.requestTimeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.send({ jsonrpc: '2.0', id, method, params });
    });
  }

  /** The identity is set only after a completed handshake, so it is null while a new server starts. */
  private forgetServer(): void {
    this.serverIdentity = null;
    this.tools = [];
  }

  private onChildGone(err: Error): void {
    this.forgetServer();
    this.rejectAll(err);
  }

  private rejectAll(err: Error): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }
}

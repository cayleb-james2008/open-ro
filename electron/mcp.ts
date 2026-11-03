import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { pushMcpChunk } from './mcpFraming';
import { scrubbedEnv } from './toolEnv';
import type { McpToolSummary } from './types';

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
  id?: number;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
  error?: { code: number; message: string };
};

export class McpStdioClient {
  private child: ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: unknown) => void; reject: (err: Error) => void }>();
  private buffer = '';
  private tools: McpToolSummary[] = [];

  get running(): boolean {
    return this.child !== null && this.child.exitCode === null;
  }

  async start(spec: McpServerSpec): Promise<void> {
    if (this.running) {
      throw new Error('An MCP server is already active. Stop it before connecting another.');
    }
    this.child = spawn(spec.command, spec.args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: scrubbedEnv(),
      shell: false,
      windowsHide: true,
    });
    this.child.stdout.setEncoding('utf8');
    this.child.stdout.on('data', (chunk: string) => this.onData(chunk));
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', () => {
      /* server logs are not treated as protocol failures */
    });
    this.child.on('error', (error) => this.rejectAll(error));
    this.child.on('exit', () => this.rejectAll(new Error('MCP server exited unexpectedly.')));
    await this.request('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'Open-Ro Studio', version: '0.1.0' },
    });
    this.notify('notifications/initialized');
    await this.refreshTools();
  }

  async stop(): Promise<void> {
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
    const result = (await this.request('tools/list', {})) as { tools?: Array<{ name: string; description?: string }> };
    this.tools = (result?.tools ?? []).map((tool) => ({
      name: String(tool.name),
      description: String(tool.description ?? ''),
    }));
    return this.listTools();
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<string> {
    const result = (await this.request('tools/call', { name, arguments: args ?? {} })) as {
      content?: Array<{ type: string; text?: string }>;
      isError?: boolean;
    };
    const text = (result?.content ?? [])
      .filter((entry) => entry.type === 'text' && typeof entry.text === 'string')
      .map((entry) => entry.text)
      .join('\n');
    if (result?.isError) {
      throw new Error(text || `MCP tool ${name} reported an error.`);
    }
    return text;
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
    if (typeof message.id === 'number') {
      const pending = this.pending.get(message.id);
      if (pending) {
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
      this.pending.set(id, { resolve, reject });
      this.send({ jsonrpc: '2.0', id, method, params });
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`MCP request ${method} timed out.`));
        }
      }, 15_000);
    });
  }

  private rejectAll(err: Error): void {
    for (const pending of this.pending.values()) {
      pending.reject(err);
    }
    this.pending.clear();
  }
}

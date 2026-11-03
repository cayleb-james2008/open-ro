import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
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
      env: { ...process.env },
    });
    this.child.stdout.setEncoding('utf8');
    this.child.stdout.on('data', (chunk: string) => this.onData(chunk));
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', () => {
      /* server logs are surfaced via tools, not crashed on */
    });
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
    this.buffer += chunk;
    let newline = this.buffer.indexOf('\n');
    while (newline >= 0) {
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (line.length > 0) {
        this.onLine(line);
      }
      newline = this.buffer.indexOf('\n');
    }
  }

  private onLine(line: string): void {
    let message: JsonRpcMessage;
    try {
      message = JSON.parse(line) as JsonRpcMessage;
    } catch {
      return;
    }
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

  private notify(method: string): void {
    this.child?.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method, params: {} })}\n`,
    );
  }

  private request(method: string, params: Record<string, unknown>): Promise<unknown> {
    const child = this.child;
    if (!child || !this.running) {
      return Promise.reject(new Error('MCP server is not running.'));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
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

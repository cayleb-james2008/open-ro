import * as crypto from 'node:crypto';
import type { McpServerIdentity } from './types';

export type ApprovalKind = 'write' | 'command' | 'mcp' | 'mcp-server' | 'studio';
export type StudioOp = 'snapshot' | 'read' | 'write';
export type ApprovalDecision = 'approve' | 'reject';

export interface PendingApproval {
  id: string;
  kind: ApprovalKind;
  projectRoot: string;
  createdAt: string;
  expiresAt: number;
  order: number;
  relPath?: string;
  content?: string;
  previous?: string;
  command?: string;
  cwd?: string;
  mcpTool?: string;
  mcpArgs?: string;
  serverArgs?: string[];
  /** The server an MCP call was staged against; the call runs only on that same server session. */
  mcpServer?: McpServerIdentity;
  studioOp?: StudioOp;
  /** Dot path from game, for example ServerScriptService.Shop. */
  studioPath?: string;
  studioClass?: string;
  studioServices?: string[];
}

const MAX_PENDING = 30;
const MAX_TEXT = 1_000_000;
const MAX_MCP_ARGS = 100_000;

/**
 * In-memory approval queue. The staged payload stays in the main process;
 * the renderer only receives an id and later approves or rejects that id.
 */
export class ApprovalQueue {
  private readonly items = new Map<string, PendingApproval>();
  private nextOrder = 0;

  constructor(
    private readonly now: () => number = Date.now,
    private readonly ttlMs = 30 * 60_000,
  ) {}

  stageWrite(input: { projectRoot: string; relPath: string; content: string; previous: string }): PendingApproval {
    this.assertRoom();
    if (input.content.length > MAX_TEXT) {
      throw new Error('Refusing to stage a write larger than 1,000,000 characters.');
    }
    const item: PendingApproval = {
      id: crypto.randomUUID(),
      kind: 'write',
      projectRoot: input.projectRoot,
      createdAt: new Date(this.now()).toISOString(),
      expiresAt: this.now() + this.ttlMs,
      order: this.nextOrder++,
      relPath: input.relPath,
      content: input.content,
      previous: input.previous,
    };
    this.items.set(item.id, item);
    return item;
  }

  stageCommand(input: { projectRoot: string; command: string; cwd: string }): PendingApproval {
    this.assertRoom();
    const command = input.command.trim();
    if (!command) {
      throw new Error('Command is empty.');
    }
    if (command.length > 4_000) {
      throw new Error('Command is too long.');
    }
    const item: PendingApproval = {
      id: crypto.randomUUID(),
      kind: 'command',
      projectRoot: input.projectRoot,
      createdAt: new Date(this.now()).toISOString(),
      expiresAt: this.now() + this.ttlMs,
      order: this.nextOrder++,
      command,
      cwd: input.cwd,
    };
    this.items.set(item.id, item);
    return item;
  }

  /** Stage starting an MCP server. The exact command and arguments are what will run. */
  stageMcpServer(input: { projectRoot: string; command: string; args: string[] }): PendingApproval {
    this.assertRoom();
    const item: PendingApproval = {
      id: crypto.randomUUID(),
      kind: 'mcp-server',
      projectRoot: input.projectRoot,
      createdAt: new Date(this.now()).toISOString(),
      expiresAt: this.now() + this.ttlMs,
      order: this.nextOrder++,
      command: input.command,
      serverArgs: [...input.args],
    };
    this.items.set(item.id, item);
    return item;
  }

  /** Stage an MCP tool call. Arguments are frozen as JSON so they cannot change after review. */
  stageMcp(input: { projectRoot: string; tool: string; args: Record<string, unknown>; server?: McpServerIdentity }): PendingApproval {
    this.assertRoom();
    if (!input.tool.trim()) {
      throw new Error('MCP tool name is empty.');
    }
    const mcpArgs = JSON.stringify(input.args ?? {});
    if (mcpArgs.length > MAX_MCP_ARGS) {
      throw new Error('Refusing to stage MCP arguments larger than 100,000 characters.');
    }
    const item: PendingApproval = {
      id: crypto.randomUUID(),
      kind: 'mcp',
      projectRoot: input.projectRoot,
      createdAt: new Date(this.now()).toISOString(),
      expiresAt: this.now() + this.ttlMs,
      order: this.nextOrder++,
      mcpTool: input.tool,
      mcpArgs,
      mcpServer: input.server ? { ...input.server, args: [...input.server.args] } : undefined,
    };
    this.items.set(item.id, item);
    return item;
  }

  /**
   * Stage a Roblox Studio edit or read. The Luau tool call is frozen like an
   * MCP call; a write also keeps the previous and new source for the diff.
   */
  stageStudio(input: {
    projectRoot: string;
    op: StudioOp;
    path: string;
    tool: string;
    args: Record<string, unknown>;
    className?: string;
    services?: string[];
    previous?: string;
    content?: string;
  }): PendingApproval {
    const item = this.stageMcp({ projectRoot: input.projectRoot, tool: input.tool, args: input.args });
    Object.assign(item, {
      kind: 'studio',
      studioOp: input.op,
      studioPath: input.path,
      studioClass: input.className,
      studioServices: input.services ? [...input.services] : undefined,
      previous: input.previous,
      content: input.content,
    });
    return item;
  }

  list(): PendingApproval[] {
    this.prune();
    return [...this.items.values()].sort((a, b) => a.order - b.order);
  }

  /** Remove and return a still-valid approval. A second call throws. */
  take(id: string): PendingApproval {
    this.prune();
    const item = this.items.get(id);
    if (!item) {
      throw new Error('That approval is no longer pending.');
    }
    this.items.delete(id);
    return item;
  }

  clear(): void {
    this.items.clear();
  }

  private prune(): void {
    const now = this.now();
    for (const [id, item] of this.items) {
      if (item.expiresAt <= now) {
        this.items.delete(id);
      }
    }
  }

  private assertRoom(): void {
    this.prune();
    if (this.items.size >= MAX_PENDING) {
      throw new Error('Too many pending approvals. Resolve them before staging more.');
    }
  }
}

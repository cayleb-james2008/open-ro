import * as crypto from 'node:crypto';

export type ApprovalKind = 'write' | 'command';
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
}

const MAX_PENDING = 30;
const MAX_TEXT = 1_000_000;

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

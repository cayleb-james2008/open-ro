import { describe, expect, it } from 'vitest';
import { ApprovalQueue, type ApprovalKind } from '../electron/approvals';
import { registerApprovalChannels } from '../electron/approvalChannels';

const server = { command: 'node', args: ['studio.js'], session: 's1' };

function stageAll(queue: ApprovalQueue): Record<ApprovalKind, string> {
  return {
    write: queue.stageWrite({ projectRoot: '/p', relPath: 'a.lua', content: 'next', previous: 'prev' }).id,
    command: queue.stageCommand({ projectRoot: '/p', command: 'rojo build', cwd: '/p' }).id,
    mcp: queue.stageMcp({ projectRoot: '/p', tool: 'echo', args: { message: 'hi' }, server }).id,
    'mcp-server': queue.stageMcpServer({ projectRoot: '/p', command: 'node', args: ['server.js'] }).id,
    studio: queue.stageStudio({ projectRoot: '/p', op: 'read', path: 'Workspace.Door', tool: 'execute_luau', args: { code: 'x' }, server }).id,
  };
}

describe('approvals taken by a run channel', () => {
  it.each([['write'], ['command']] as const)('the %s channel refuses every other kind and leaves it pending', (channel) => {
    const queue = new ApprovalQueue();
    const ids = stageAll(queue);
    for (const [kind, id] of Object.entries(ids)) {
      if (kind === channel) {
        continue;
      }
      expect(() => queue.take(id, channel)).toThrow(`That approval is a ${kind} approval, not a ${channel} one. It is still pending.`);
    }
    expect(queue.list().map((item) => item.kind).sort()).toEqual(['command', 'mcp', 'mcp-server', 'studio', 'write']);
    expect(queue.take(ids[channel], channel).kind).toBe(channel);
    expect(queue.list()).toHaveLength(4);
    expect(() => queue.take(ids[channel], channel)).toThrow('That approval is no longer pending.');
  });

  it('the general resolve path still takes any kind', () => {
    const queue = new ApprovalQueue();
    const ids = stageAll(queue);
    expect(queue.take(ids.studio).kind).toBe('studio');
    expect(queue.take(ids.mcp).kind).toBe('mcp');
  });
});

describe('approval IPC channels as main.ts registers them', () => {
  function wire(queue: ApprovalQueue) {
    const handlers = new Map<string, (event: unknown, args: unknown) => unknown>();
    const calls: Array<[string, string, ApprovalKind | undefined]> = [];
    registerApprovalChannels(
      (channel, listener) => handlers.set(channel, listener),
      (id, decision, kind) => {
        calls.push([id, decision, kind]);
        return queue.take(id, kind);
      },
    );
    const invoke = (channel: string, args: unknown) => handlers.get(channel)!(undefined, args);
    return { handlers, calls, invoke };
  }

  it('project:write approves only writes and command:run only commands', () => {
    const queue = new ApprovalQueue();
    const ids = stageAll(queue);
    const { handlers, calls, invoke } = wire(queue);
    expect([...handlers.keys()].sort()).toEqual(['approvals:resolve', 'command:run', 'project:write']);
    expect(() => invoke('project:write', { id: ids.command })).toThrow('That approval is a command approval, not a write one. It is still pending.');
    expect(() => invoke('project:write', { id: ids.studio })).toThrow('not a write one');
    expect(() => invoke('command:run', { id: ids.write })).toThrow('That approval is a write approval, not a command one. It is still pending.');
    expect(() => invoke('command:run', { id: ids.mcp })).toThrow('not a command one');
    expect(queue.list()).toHaveLength(5);
    expect((invoke('project:write', { id: ids.write }) as { kind: string }).kind).toBe('write');
    expect((invoke('command:run', { id: ids.command }) as { kind: string }).kind).toBe('command');
    expect(calls.slice(-2)).toEqual([
      [ids.write, 'approve', 'write'],
      [ids.command, 'approve', 'command'],
    ]);
  });

  it('approvals:resolve takes any kind and rejects a malformed decision', () => {
    const queue = new ApprovalQueue();
    const ids = stageAll(queue);
    const { calls, invoke } = wire(queue);
    for (const bad of [undefined, { id: ids.mcp }, { id: ids.mcp, decision: 'run' }, { id: 7, decision: 'approve' }]) {
      expect(() => invoke('approvals:resolve', bad)).toThrow('Approval decision is invalid.');
    }
    expect(calls).toEqual([]);
    expect((invoke('approvals:resolve', { id: ids.mcp, decision: 'reject' }) as { kind: string }).kind).toBe('mcp');
    expect(calls).toEqual([[ids.mcp, 'reject', undefined]]);
  });
});

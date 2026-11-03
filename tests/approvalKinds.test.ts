import { describe, expect, it } from 'vitest';
import { ApprovalQueue, type ApprovalKind } from '../electron/approvals';

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

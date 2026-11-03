import { describe, expect, it } from 'vitest';
import { approvalCopy } from '../src/labels';
import { diffLines } from '../src/textDiff';

describe('Studio approval cards', () => {
  const base = { id: 's', kind: 'studio' as const, createdAt: '', mcpServer: 'StudioMCP' };

  it('names the instance path, class and server for a write, and diffs against the last read', () => {
    const item = { ...base, studioOp: 'write' as const, studioPath: 'ServerScriptService.Shop', studioClass: 'Script', previous: 'print(1)', content: 'print(2)' };
    const copy = approvalCopy(item);
    expect(copy.title).toBe('Review Studio write: game.ServerScriptService.Shop (Script)');
    expect(copy.detail).toMatch(/^Changes this script in Studio on StudioMCP only if you approve/);
    expect(copy.approveLabel).toBe('Write to Studio');
    expect(diffLines(item.previous, item.content)).toEqual([{ kind: 'del', text: 'print(1)' }, { kind: 'add', text: 'print(2)' }]);
    expect(approvalCopy({ ...item, previous: undefined }).detail).toMatch(/^Creates this script/);
  });

  it('marks reads as read-only and lists snapshot services', () => {
    expect(approvalCopy({ ...base, studioOp: 'read', studioPath: 'Workspace.Door' })).toMatchObject({ title: 'Review Studio read: game.Workspace.Door', preview: 'game.Workspace.Door', approveLabel: 'Read script' });
    expect(approvalCopy({ ...base, studioOp: 'snapshot', studioPath: 'game', studioServices: ['Workspace', 'Teams'] })).toMatchObject({
      title: 'Review Studio read: place tree', detail: 'Read-only Luau runs in Studio on StudioMCP only if you approve', preview: 'Services: Workspace, Teams',
    });
  });
});

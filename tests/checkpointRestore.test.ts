import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { parseCheckpointRecord, planFileRestore, stageCheckpointRestore } from '../electron/checkpoints';

const root = path.resolve('C:/projects/obby');

function record(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    ts: '2026-01-01T00:00:00.000Z',
    projectRoot: root,
    relPath: 'src/init.lua',
    previous: 'print("old")',
    ...overrides,
  });
}

describe('checkpoint restoration', () => {
  it('parses a valid checkpoint record and keeps its previous text', () => {
    const parsed = parseCheckpointRecord(record(), root);
    expect(parsed.relPath).toBe('src/init.lua');
    expect(parsed.previous).toBe('print("old")');
  });

  it('rejects malformed records', () => {
    expect(() => parseCheckpointRecord('not json', root)).toThrow();
    expect(() => parseCheckpointRecord(JSON.stringify(null), root)).toThrow('invalid');
    expect(() => parseCheckpointRecord(JSON.stringify({ projectRoot: root, relPath: 42, previous: 'x' }), root)).toThrow('invalid');
    expect(() => parseCheckpointRecord(JSON.stringify({ projectRoot: root, relPath: 'src/init.lua' }), root)).toThrow('invalid');
  });

  it('rejects a checkpoint that belongs to another project', () => {
    const other = path.resolve('C:/projects/other');
    expect(() => parseCheckpointRecord(record({ projectRoot: other }), root)).toThrow('different project');
  });

  it('plans a staged write that swaps current content for checkpoint content', () => {
    const plan = planFileRestore(parseCheckpointRecord(record(), root), 'print("new")');
    expect(plan).toEqual({ relPath: 'src/init.lua', content: 'print("old")', previous: 'print("new")' });
  });

  it('refuses creation checkpoints instead of deleting without review', () => {
    const creation = parseCheckpointRecord(record({ previous: null }), root);
    expect(() => planFileRestore(creation, 'print("new")')).toThrow('delete');
  });

  it('stages the restore as a write approval instead of writing the file', () => {
    const queue = new ApprovalQueue();
    const staged = stageCheckpointRestore(queue, { projectRoot: root, record: parseCheckpointRecord(record(), root), current: 'print("new")' });
    expect(staged.kind).toBe('write');
    expect(staged.relPath).toBe('src/init.lua');
    expect(staged.content).toBe('print("old")');
    expect(staged.previous).toBe('print("new")');
    expect(queue.list()).toHaveLength(1);
  });

  it('refuses a creation checkpoint without staging anything', () => {
    const queue = new ApprovalQueue();
    const creation = parseCheckpointRecord(record({ previous: null }), root);
    expect(() => stageCheckpointRestore(queue, { projectRoot: root, record: creation, current: 'print("new")' })).toThrow('delete');
    expect(queue.list()).toHaveLength(0);
  });

  it('the IPC restore handler stages a write and never writes the target directly', () => {
    // Security-boundary invariant: reverting the handler to a direct write must
    // fail this test even though the pure planning module still passes.
    const source = readFileSync('electron/main.ts', 'utf8');
    const start = source.indexOf("ipcMain.handle('project:restore-checkpoint'");
    expect(start).toBeGreaterThan(-1);
    const end = source.indexOf('\n  });', start);
    const handler = source.slice(start, end);
    expect(handler).toContain('stageCheckpointRestore(');
    expect(handler).not.toMatch(/fs\.(writeFile|rm)\(/);
    expect(handler).not.toContain('JSON.parse(');
  });
});

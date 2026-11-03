import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import type { McpCaller } from '../electron/mcpTools';
import { luauToolCall, stageStudioTool, StudioReads } from '../electron/studioTools';
import type { McpToolSummary } from '../electron/types';

const executeLuau: McpToolSummary = {
  name: 'execute_luau',
  description: 'Runs Luau in Studio',
  inputSchema: { type: 'object', properties: { code: { type: 'string' }, datamodel_type: { type: 'string' }, studio_id: { type: 'string' } }, required: ['code', 'datamodel_type'] },
};

/** Records every call; replies with whatever the test queues. */
function fakeStudio(tools: McpToolSummary[] = [executeLuau]) {
  const calls: Array<{ name: string; args: Record<string, unknown> }> = [];
  const replies: string[] = [];
  const caller: McpCaller = {
    running: true,
    listTools: () => tools,
    callTool: async (name, args) => {
      calls.push({ name, args });
      return replies.shift() ?? '';
    },
  };
  return { caller, calls, replies };
}

describe('Studio Luau tool discovery', () => {
  it('fills code, Edit datamodel and studio id from the schema', () => {
    expect(luauToolCall([executeLuau], 'print(1)', 'S1')).toEqual({ tool: 'execute_luau', args: { code: 'print(1)', datamodel_type: 'Edit', studio_id: 'S1' } });
    const runCode = { name: 'run_code', description: '', inputSchema: { type: 'object', properties: { command: { type: 'string' } } } };
    expect(luauToolCall([runCode], 'x')).toEqual({ tool: 'run_code', args: { command: 'x' } });
  });

  it('refuses unclear schemas, a missing Luau tool and a required studio id', () => {
    const unclear = { name: 'execute_luau', description: '', inputSchema: { type: 'object', properties: { a: { type: 'string' }, b: { type: 'string' } } } };
    expect(() => luauToolCall([unclear], 'x')).toThrow(/Cannot tell which execute_luau argument/);
    expect(() => luauToolCall([{ name: 'script_read', description: '' }], 'x')).toThrow(/no execute_luau or run_code/);
    const needsId = { ...executeLuau, inputSchema: { ...executeLuau.inputSchema, required: ['code', 'studio_id'] } };
    expect(() => luauToolCall([needsId], 'x')).toThrow(/list_roblox_studios/);
  });
});

describe('staging Studio tools', () => {
  const root = '/project';
  const stage = (studio: ReturnType<typeof fakeStudio>, reads: StudioReads, approvals: ApprovalQueue, name: string, args: Record<string, unknown>) =>
    stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name, args });

  it('stages snapshot and read without calling Studio', () => {
    const studio = fakeStudio();
    const approvals = new ApprovalQueue();
    const snap = stage(studio, new StudioReads(), approvals, 'studio_snapshot', { services: ['Workspace', 'Teams'], maxDepth: 99 });
    const read = stage(studio, new StudioReads(), approvals, 'studio_read_script', { path: 'game.ServerScriptService.Shop' });
    expect([snap.ok, read.ok]).toEqual([true, true]);
    expect(studio.calls).toEqual([]);
    const [first, second] = approvals.list();
    expect(first).toMatchObject({ kind: 'studio', studioOp: 'snapshot', studioServices: ['Workspace', 'Teams'], mcpTool: 'execute_luau' });
    expect(JSON.parse(first.mcpArgs ?? '{}').code).toContain('maxDepth, maxNodes = 6, 2000');
    expect(second).toMatchObject({ studioOp: 'read', studioPath: 'ServerScriptService.Shop' });
    expect(stage(studio, new StudioReads(), approvals, 'studio_snapshot', { services: ['Bad Name'] }).ok).toBe(false);
  });

  it('a write needs a prior read, the right class and place, and server-owned currency', () => {
    const studio = fakeStudio();
    const approvals = new ApprovalQueue();
    const reads = new StudioReads();
    const write = (args: Record<string, unknown>) => stage(studio, reads, approvals, 'studio_write_script', args);
    const shop = { path: 'ServerScriptService.Shop', className: 'Script', source: 'print(2)' };
    expect(write(shop).output).toMatch(/studio_read_script first/);
    reads.set('ServerScriptService.Shop', { exists: true, className: 'ModuleScript', source: 'return {}' });
    expect(write(shop).output).toMatch(/is a ModuleScript, not a Script/);
    expect(write({ ...shop, path: 'StarterGui.Shop' }).output).toMatch(/does not run in StarterGui/);
    expect(write({ ...shop, source: 'p.leaderstats.Coins.Value += 1' }).output).toMatch(/--!strict/);
    expect(write({ ...shop, className: 'Part' }).ok).toBe(false);
    expect(approvals.list()).toEqual([]);
    reads.set('ServerScriptService.Shop', { exists: true, className: 'Script', source: 'print(1)' });
    expect(write(shop)).toMatchObject({ ok: true, pendingApproval: { kind: 'studio', path: 'ServerScriptService.Shop' } });
    expect(approvals.list()[0]).toMatchObject({ studioOp: 'write', studioClass: 'Script', previous: 'print(1)', content: 'print(2)' });
    expect(studio.calls).toEqual([]);
  });

  it('refuses when no server is connected', () => {
    const studio = fakeStudio();
    const offline = { ...studio.caller, running: false };
    expect(stageStudioTool(new ApprovalQueue(), offline, new StudioReads(), { projectRoot: root, name: 'studio_snapshot', args: {} }).ok).toBe(false);
  });
});

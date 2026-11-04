import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import type { McpCaller } from '../electron/mcpTools';
import { luauToolCall, runApprovedStudio, stageStudioRestore, stageStudioTool, studioReadKey, StudioReads } from '../electron/studioTools';
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
    identity: { command: 'StudioMCP', args: [], session: 'session-1' },
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
    reads.set(studioReadKey('session-1', undefined, 'ServerScriptService.Shop'), { exists: true, className: 'ModuleScript', source: 'return {}' });
    expect(write(shop).output).toMatch(/is a ModuleScript, not a Script/);
    expect(write({ ...shop, path: 'StarterGui.Shop' }).output).toMatch(/does not run in StarterGui/);
    expect(write({ ...shop, source: 'p.leaderstats.Coins.Value += 1' }).output).toMatch(/--!strict/);
    expect(write({ ...shop, className: 'Part' }).ok).toBe(false);
    expect(approvals.list()).toEqual([]);
    reads.set(studioReadKey('session-1', undefined, 'ServerScriptService.Shop'), { exists: true, className: 'Script', source: 'print(1)' });
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

describe('running approved Studio calls', () => {
  const root = '/project';
  const ok = (result: unknown) => `OPENRO:${JSON.stringify({ ok: true, result })}`;
  const setup = () => ({ studio: fakeStudio(), approvals: new ApprovalQueue(), reads: new StudioReads() });

  it('a snapshot reports missing services and never invents them', async () => {
    const { studio, approvals, reads } = setup();
    stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_snapshot', args: { services: ['Workspace', 'Teams'] } });
    studio.replies.push(ok({ truncated: false, services: [
      { name: 'Workspace', present: true, nodes: [{ path: 'Workspace.Baseplate', className: 'Part', depth: 1 }] },
      { name: 'Teams', present: true, nodes: 'garbage' },
    ] }));
    const result = await runApprovedStudio(approvals.list()[0], studio.caller, reads);
    expect(result.summary).toBe('Read 1 instances.');
    expect(result.output).toBe('Workspace.Baseplate (Part)');
    studio.replies.push(ok({ services: [{ name: 'Workspace', present: true, nodes: [] }] }));
    const again = await runApprovedStudio(approvals.list()[0], studio.caller, reads);
    expect(again.snapshot?.missing).toEqual(['Teams']);
    expect(again.summary).toMatch(/Missing services: Teams\./);
  });

  it('read, then write, then the read-back must match before a checkpoint is kept', async () => {
    const { studio, approvals, reads } = setup();
    const path = 'ServerScriptService.Shop';
    stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_read_script', args: { path } });
    studio.replies.push(ok({ exists: true, path, className: 'Script', source: 'print(1)' }));
    expect((await runApprovedStudio(approvals.take(approvals.list()[0].id), studio.caller, reads)).output).toBe('print(1)');
    const write = (source: string) => stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_write_script', args: { path, className: 'Script', source } });
    write('print(2)');
    studio.replies.push(ok({ path, className: 'Script', source: 'print(2) tampered' }));
    const bad = await runApprovedStudio(approvals.take(approvals.list()[0].id), studio.caller, reads);
    expect(bad).toMatchObject({ ok: false, output: 'Read-back source does not match the approved source.' });
    expect(bad.checkpoint).toBeUndefined();
    expect(write('print(2)').output).toMatch(/studio_read_script first/);
    reads.set(studioReadKey('session-1', undefined, path), { exists: true, className: 'Script', source: 'print(1)' });
    write('print(2)');
    studio.replies.push(ok({ path, className: 'Script', source: 'print(2)', undoRecorded: true }));
    const good = await runApprovedStudio(approvals.take(approvals.list()[0].id), studio.caller, reads);
    expect(good.summary).toMatch(/source\. Ctrl\+Z in Studio undoes it\.$/);
    expect(good).toMatchObject({ ok: true, checkpoint: { path, className: 'Script', previous: 'print(1)', content: 'print(2)' } });
    expect(studio.calls.map((call) => call.name)).toEqual(['execute_luau', 'execute_luau', 'execute_luau']);
    const restore = stageStudioRestore(approvals, studio.caller, good.checkpoint ?? { path, className: 'Script', previous: null, content: '' }, { projectRoot: root });
    expect(restore).toMatchObject({ studioOp: 'write', previous: 'print(2)', content: 'print(1)' });
    expect(String(JSON.parse(restore.mcpArgs ?? '{}').code)).toContain('local expectedPrevious = [[print(2)]]');
    expect(() => stageStudioRestore(approvals, studio.caller, { path, className: 'Script', previous: null, content: 'x' }, { projectRoot: root })).toThrow(/created this script/);
  });

  it('a read that Studio answers for another path is refused and authorizes nothing', async () => {
    const { studio, approvals, reads } = setup();
    stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_read_script', args: { path: 'ServerScriptService.Shop' } });
    studio.replies.push(ok({ exists: true, path: 'ServerScriptService.Other', className: 'Script', source: 'print(1)' }));
    const read = await runApprovedStudio(approvals.take(approvals.list()[0].id), studio.caller, reads);
    expect(read).toMatchObject({ ok: false, summary: 'Studio returned a different instance than game.ServerScriptService.Shop.' });
    expect(reads.size).toBe(0);
    const write = stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_write_script', args: { path: 'ServerScriptService.Shop', className: 'Script', source: 'x' } });
    expect(write.output).toMatch(/studio_read_script first/);
  });

  it('a read in one studio or server session never authorizes a write in another', async () => {
    const { studio, approvals, reads } = setup();
    const path = 'ServerScriptService.Shop';
    const stage = (name: string, args: Record<string, unknown>, caller = studio.caller) => stageStudioTool(approvals, caller, reads, { projectRoot: root, name, args: { path, ...args } });
    stage('studio_read_script', { studioId: 'A' });
    const read = approvals.list()[0];
    expect(read.studioId).toBe('A');
    expect(JSON.parse(read.mcpArgs ?? '{}').studio_id).toBe('A');
    studio.replies.push(ok({ exists: true, path, className: 'Script', source: 'print(1)' }));
    expect((await runApprovedStudio(approvals.take(read.id), studio.caller, reads)).ok).toBe(true);
    expect(stage('studio_write_script', { studioId: 'B', className: 'Script', source: 'x' }).output).toMatch(/studio_read_script first/);
    expect(stage('studio_write_script', { className: 'Script', source: 'x' }).output).toMatch(/studio_read_script first/);
    const restarted = { ...studio.caller, identity: { command: 'StudioMCP', args: [], session: 'session-2' } };
    expect(stage('studio_write_script', { studioId: 'A', className: 'Script', source: 'x' }, restarted).output).toMatch(/studio_read_script first/);
    expect(approvals.list()).toHaveLength(0);
    expect(stage('studio_write_script', { studioId: 'A', className: 'Script', source: 'print(2)' }).ok).toBe(true);
    const write = approvals.list()[0];
    expect(write).toMatchObject({ studioId: 'A', previous: 'print(1)' });
    expect(JSON.parse(write.mcpArgs ?? '{}').studio_id).toBe('A');
    studio.replies.push(ok({ path, className: 'Script', source: 'print(2)', undoRecorded: false }));
    const done = await runApprovedStudio(approvals.take(write.id), studio.caller, reads);
    expect(done.checkpoint).toMatchObject({ studioId: 'A' });
    expect(done.summary).toMatch(/Studio did not record an undo step, so Ctrl\+Z will not undo it\.$/);
    const restore = stageStudioRestore(approvals, studio.caller, done.checkpoint ?? { path, className: 'Script', previous: null, content: '' }, { projectRoot: root });
    expect(restore.studioId).toBe('A');
    expect(JSON.parse(restore.mcpArgs ?? '{}').studio_id).toBe('A');
  });

  it('a Studio error, a missing reply or a vanished tool is a failure', async () => {
    const { studio, approvals, reads } = setup();
    stageStudioTool(approvals, studio.caller, reads, { projectRoot: root, name: 'studio_read_script', args: { path: 'Workspace.X' } });
    const item = approvals.list()[0];
    studio.replies.push('OPENRO:{"ok":false,"error":"Two instances named X under Workspace"}');
    expect(await runApprovedStudio(item, studio.caller, reads)).toMatchObject({ ok: false, output: 'Two instances named X under Workspace' });
    studio.replies.push('Luau error: attempt to index nil');
    expect((await runApprovedStudio(item, studio.caller, reads)).ok).toBe(false);
    expect(reads.size).toBe(0);
    const gone = { ...studio.caller, listTools: () => [] };
    expect((await runApprovedStudio(item, gone, reads)).summary).toMatch(/no longer available/);
    const restarted = { ...studio.caller, identity: { command: 'StudioMCP', args: [], session: 'session-2' } };
    expect((await runApprovedStudio(item, restarted, reads)).summary).toMatch(/MCP server changed after this Studio read was staged/);
    expect(studio.calls).toHaveLength(2);
  });
});

describe('Studio playtest and console tools', () => {
  const root = '/project';
  const tools: McpToolSummary[] = [
    executeLuau,
    { name: 'start_stop_play', description: '' },
    { name: 'get_console_output', description: '' },
  ];
  const stage = (studio: ReturnType<typeof fakeStudio>, approvals: ApprovalQueue, name: string, args: Record<string, unknown>) =>
    stageStudioTool(approvals, studio.caller, new StudioReads(), { projectRoot: root, name, args });

  it('stages start, stop, and console with frozen args and no Studio call', () => {
    const studio = fakeStudio(tools);
    const approvals = new ApprovalQueue();
    const start = stage(studio, approvals, 'studio_playtest', { isStart: true, studioId: 'S1' });
    const stop = stage(studio, approvals, 'studio_playtest', { isStart: false, studioId: 'S1' });
    const consoleRead = stage(studio, approvals, 'studio_console', { studioId: 'S1' });
    expect([start.ok, stop.ok, consoleRead.ok]).toEqual([true, true, true]);
    expect(studio.calls).toEqual([]);
    const items = approvals.list();
    expect(items[0]).toMatchObject({ kind: 'studio', studioOp: 'play', mcpTool: 'start_stop_play', studioId: 'S1', studioPath: 'game' });
    expect(JSON.parse(items[0].mcpArgs ?? '{}')).toEqual({ is_start: true, studio_id: 'S1' });
    expect(JSON.parse(items[1].mcpArgs ?? '{}')).toEqual({ is_start: false, studio_id: 'S1' });
    expect(items[2]).toMatchObject({ studioOp: 'console', mcpTool: 'get_console_output' });
    expect(JSON.parse(items[2].mcpArgs ?? '{}')).toEqual({ studio_id: 'S1' });
  });

  it('refuses missing tools, a missing studio id, and a non-boolean isStart', () => {
    const approvals = new ApprovalQueue();
    const noTools = fakeStudio([]);
    expect(stage(noTools, approvals, 'studio_playtest', { isStart: true, studioId: 'S1' }).output).toMatch(/no start_stop_play/);
    expect(stage(noTools, approvals, 'studio_console', { studioId: 'S1' }).output).toMatch(/no get_console_output/);
    const studio = fakeStudio(tools);
    expect(stage(studio, approvals, 'studio_playtest', { isStart: 'yes', studioId: 'S1' }).output).toMatch(/must be true or false/);
    expect(stage(studio, approvals, 'studio_playtest', { isStart: true }).output).toMatch(/needs a studioId/);
    expect(stage(studio, approvals, 'studio_console', {}).output).toMatch(/needs a studioId/);
    expect(approvals.list()).toHaveLength(0);
  });

  it('runs approved playtest and console calls with bounded output', async () => {
    const studio = fakeStudio(tools);
    studio.replies.push('Edit started', 'x'.repeat(25_000));
    const approvals = new ApprovalQueue();
    const reads = new StudioReads();
    const start = stage(studio, approvals, 'studio_playtest', { isStart: true, studioId: 'S1' });
    const started = await runApprovedStudio(approvals.take(start.pendingApproval!.id, 'studio'), studio.caller, reads);
    expect(started).toMatchObject({ ok: true, summary: 'Started playtesting.' });
    expect(started.output).toBe('Edit started');
    const consoleRead = stage(studio, approvals, 'studio_console', { studioId: 'S1' });
    const output = await runApprovedStudio(approvals.take(consoleRead.pendingApproval!.id, 'studio'), studio.caller, reads);
    expect(output).toMatchObject({ ok: true, summary: 'Read the Studio output.' });
    expect(output.output).toHaveLength(20_000);
    expect(studio.calls).toEqual([
      { name: 'start_stop_play', args: { is_start: true, studio_id: 'S1' } },
      { name: 'get_console_output', args: { studio_id: 'S1' } },
    ]);
  });

  it('bounds play output and reports a stop with the frozen start flag', async () => {
    const studio = fakeStudio(tools);
    studio.replies.push('y'.repeat(5_000), 'stopped');
    const approvals = new ApprovalQueue();
    const reads = new StudioReads();
    const start = stage(studio, approvals, 'studio_playtest', { isStart: false, studioId: 'S1' });
    const stopped = await runApprovedStudio(approvals.take(start.pendingApproval!.id, 'studio'), studio.caller, reads);
    expect(stopped).toMatchObject({ ok: true, summary: 'Stopped playtesting.' });
    expect(stopped.output).toHaveLength(4_000);
    expect(studio.calls).toEqual([{ name: 'start_stop_play', args: { is_start: false, studio_id: 'S1' } }]);
  });

  it('refuses play and console approvals after the MCP server changes', async () => {
    const studio = fakeStudio(tools);
    const approvals = new ApprovalQueue();
    const reads = new StudioReads();
    const play = stage(studio, approvals, 'studio_playtest', { isStart: true, studioId: 'S1' });
    const consoleRead = stage(studio, approvals, 'studio_console', { studioId: 'S1' });
    const restarted = { ...studio.caller, identity: { command: 'StudioMCP', args: [], session: 'session-2' } };
    const playResult = await runApprovedStudio(approvals.take(play.pendingApproval!.id, 'studio'), restarted, reads);
    const consoleResult = await runApprovedStudio(approvals.take(consoleRead.pendingApproval!.id, 'studio'), restarted, reads);
    expect(playResult.summary).toMatch(/MCP server changed/);
    expect(consoleResult.summary).toMatch(/MCP server changed/);
    expect(studio.calls).toHaveLength(0);
  });
});

describe('Studio input simulation', () => {
  const root = '/project';
  const tools: McpToolSummary[] = [
    executeLuau,
    { name: 'user_keyboard_input', description: '' },
    { name: 'user_mouse_input', description: '' },
  ];
  const stage = (studio: ReturnType<typeof fakeStudio>, approvals: ApprovalQueue, name: string, args: Record<string, unknown>) =>
    stageStudioTool(approvals, studio.caller, new StudioReads(), { projectRoot: root, name, args });
  const keys = [{ action: 'keyPress', key_code: 'Space' }];
  const mouse = [{ action: 'mouseButtonClick', mouse_button: 'left', x: 640, y: 360 }];

  it('stages keyboard and mouse sequences with Client datamodel and frozen args', () => {
    const studio = fakeStudio(tools);
    const approvals = new ApprovalQueue();
    const keyboard = stage(studio, approvals, 'studio_keyboard', { actions: keys, studioId: 'S1' });
    const mouseInput = stage(studio, approvals, 'studio_mouse', { actions: mouse, studioId: 'S1' });
    expect([keyboard.ok, mouseInput.ok]).toEqual([true, true]);
    expect(studio.calls).toEqual([]);
    const items = approvals.list();
    expect(items[0]).toMatchObject({ kind: 'studio', studioOp: 'input', mcpTool: 'user_keyboard_input', studioId: 'S1' });
    expect(JSON.parse(items[0].mcpArgs ?? '{}')).toEqual({ actions: keys, datamodel_type: 'Client', studio_id: 'S1' });
    expect(items[1]).toMatchObject({ studioOp: 'input', mcpTool: 'user_mouse_input' });
    expect(JSON.parse(items[1].mcpArgs ?? '{}')).toEqual({ actions: mouse, datamodel_type: 'Client', studio_id: 'S1' });
  });

  it('refuses missing tools, a missing studio id, and invalid action lists', () => {
    const approvals = new ApprovalQueue();
    const noTools = fakeStudio([]);
    expect(stage(noTools, approvals, 'studio_keyboard', { actions: keys, studioId: 'S1' }).output).toMatch(/no user_keyboard_input/);
    expect(stage(noTools, approvals, 'studio_mouse', { actions: mouse, studioId: 'S1' }).output).toMatch(/no user_mouse_input/);
    const studio = fakeStudio(tools);
    expect(stage(studio, approvals, 'studio_keyboard', { actions: keys }).output).toMatch(/needs a studioId/);
    expect(stage(studio, approvals, 'studio_keyboard', { actions: [], studioId: 'S1' }).output).toMatch(/non-empty array/);
    expect(stage(studio, approvals, 'studio_keyboard', { actions: Array.from({ length: 41 }, () => ({ action: 'wait', wait_time_ms: 1 })), studioId: 'S1' }).output).toMatch(/at most 40/);
    expect(stage(studio, approvals, 'studio_keyboard', { actions: [{ key_code: 'A' }], studioId: 'S1' }).output).toMatch(/needs an action string/);
    expect(stage(studio, approvals, 'studio_keyboard', { actions: [{ action: '   ' }], studioId: 'S1' }).output).toMatch(/needs an action string/);
    expect(approvals.list()).toHaveLength(0);
  });

  it('runs an approved multi-step keyboard sequence with capped output', async () => {
    const studio = fakeStudio(tools);
    studio.replies.push('x'.repeat(5_000));
    const approvals = new ApprovalQueue();
    const sequence = [
      { action: 'keyDown', key_code: 'W' },
      { action: 'wait', wait_time_ms: 250 },
      { action: 'keyUp', key_code: 'W' },
    ];
    const staged = stage(studio, approvals, 'studio_keyboard', { actions: sequence, studioId: 'S1' });
    const result = await runApprovedStudio(approvals.take(staged.pendingApproval!.id, 'studio'), studio.caller, new StudioReads());
    expect(result).toMatchObject({ ok: true, summary: 'Sent 3 input action(s) to Studio.' });
    expect(result.output).toHaveLength(4_000);
    expect(studio.calls).toEqual([{ name: 'user_keyboard_input', args: { actions: sequence, datamodel_type: 'Client', studio_id: 'S1' } }]);
  });

  it('refuses input approvals after the MCP server changes', async () => {
    const studio = fakeStudio(tools);
    const approvals = new ApprovalQueue();
    const staged = stage(studio, approvals, 'studio_mouse', { actions: mouse, studioId: 'S1' });
    const restarted = { ...studio.caller, identity: { command: 'StudioMCP', args: [], session: 'session-2' } };
    const result = await runApprovedStudio(approvals.take(staged.pendingApproval!.id, 'studio'), restarted, new StudioReads());
    expect(result.summary).toMatch(/MCP server changed/);
    expect(studio.calls).toHaveLength(0);
  });
});

describe('Studio viewport capture', () => {
  const root = '/project';
  const tools: McpToolSummary[] = [executeLuau, { name: 'screen_capture', description: '' }];
  const stage = (studio: ReturnType<typeof fakeStudio>, approvals: ApprovalQueue, args: Record<string, unknown>) =>
    stageStudioTool(approvals, studio.caller, new StudioReads(), { projectRoot: root, name: 'studio_capture', args });

  function imageStudio(images: Array<{ mimeType: string; data: string }>, text = '') {
    const base = fakeStudio(tools);
    const caller = {
      ...base.caller,
      callToolResult: async (name: string, args: Record<string, unknown>) => {
        base.calls.push({ name, args });
        return { text, images, isError: false };
      },
    } as McpCaller;
    return { caller, calls: base.calls };
  }

  it('stages exact screen_capture arguments with a supplied or generated id', () => {
    const studio = fakeStudio(tools);
    const approvals = new ApprovalQueue();
    expect(stage(studio, approvals, { studioId: 'S1', captureId: 'shot-1' }).ok).toBe(true);
    expect(stage(studio, approvals, { studioId: 'S1' }).ok).toBe(true);
    expect(studio.calls).toHaveLength(0);
    const [supplied, generated] = approvals.list();
    expect(supplied).toMatchObject({ studioOp: 'capture', mcpTool: 'screen_capture', studioId: 'S1' });
    expect(JSON.parse(supplied.mcpArgs ?? '{}')).toEqual({ capture_id: 'shot-1', studio_id: 'S1' });
    expect(JSON.parse(generated.mcpArgs ?? '{}').capture_id).toMatch(/^openro-capture-\d+$/);
  });

  it('refuses a missing tool or studio id', () => {
    const approvals = new ApprovalQueue();
    const noTool = fakeStudio([]);
    expect(stage(noTool, approvals, { studioId: 'S1' }).output).toMatch(/no screen_capture/);
    const studio = fakeStudio(tools);
    expect(stage(studio, approvals, { captureId: 'x' }).output).toMatch(/needs a studioId/);
    expect(approvals.list()).toHaveLength(0);
  });

  it('saves exactly the decoded image bytes through the sink', async () => {
    const bytes = Buffer.from('abc');
    const image = { mimeType: 'image/png', data: bytes.toString('base64') };
    const capture = imageStudio([image]);
    const approvals = new ApprovalQueue();
    const staged = stage(fakeStudio(tools), approvals, { studioId: 'S1', captureId: 'shot-1' });
    const saved: Array<{ hex: string; mime: string; id: string }> = [];
    const result = await runApprovedStudio(approvals.take(staged.pendingApproval!.id, 'studio'), capture.caller, new StudioReads(), async (data, mime, id) => {
      saved.push({ hex: data.toString('hex'), mime, id });
      return '/captures/shot-1.png';
    });
    expect(result).toMatchObject({ ok: true, summary: 'Captured the Studio viewport to /captures/shot-1.png.', output: 'image/png, 3 bytes' });
    expect(saved).toEqual([{ hex: Buffer.from('abc').toString('hex'), mime: 'image/png', id: 'shot-1' }]);
    expect(capture.calls).toEqual([{ name: 'screen_capture', args: { capture_id: 'shot-1', studio_id: 'S1' } }]);
  });

  it('fails when Studio returns no image, the image is too large, or no sink/client is available', async () => {
    const approvals = new ApprovalQueue();
    const empty = imageStudio([]);
    const staged = stage(fakeStudio(tools), approvals, { studioId: 'S1' });
    expect((await runApprovedStudio(approvals.take(staged.pendingApproval!.id, 'studio'), empty.caller, new StudioReads(), async () => '/x.png')).summary).toMatch(/no image/);

    const huge = imageStudio([{ mimeType: 'image/png', data: 'a'.repeat(8_000_001) }]);
    const stagedHuge = stage(fakeStudio(tools), approvals, { studioId: 'S1' });
    expect((await runApprovedStudio(approvals.take(stagedHuge.pendingApproval!.id, 'studio'), huge.caller, new StudioReads(), async () => '/x.png')).summary).toMatch(/capture limit/);

    const ok = imageStudio([{ mimeType: 'image/png', data: Buffer.from('a').toString('base64') }]);
    const stagedNoSink = stage(fakeStudio(tools), approvals, { studioId: 'S1' });
    expect((await runApprovedStudio(approvals.take(stagedNoSink.pendingApproval!.id, 'studio'), ok.caller, new StudioReads())).summary).toMatch(/No capture sink/);

    const stagedNoClient = stage(fakeStudio(tools), approvals, { studioId: 'S1' });
    expect((await runApprovedStudio(approvals.take(stagedNoClient.pendingApproval!.id, 'studio'), fakeStudio(tools).caller, new StudioReads(), async () => '/x.png')).summary).toMatch(/cannot return image content/);
  });

  it('is wired to a userData sink and the executor', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("path.join(app.getPath('userData'), 'studio-captures')");
    expect(main).toContain('runApprovedStudio(item, mcp, studioReads, saveStudioCapture)');
    expect(main).toContain("case 'studio_capture'");
    expect(main).toContain('mode: 0o600');
    expect(main).toContain("captureId.replace(/[^\\w.-]+/g, '-')");
    expect(main).toContain("mimeType === 'image/png' ? '.png' : mimeType === 'image/jpeg' ? '.jpg' : '.bin'");
  });
});

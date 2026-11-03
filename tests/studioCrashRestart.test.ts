import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpStdioClient } from '../electron/mcp';
import { runApprovedStudio, stageStudioTool, StudioReads } from '../electron/studioTools';

// A Studio MCP stand-in. execute_luau logs each call to argv[2] and answers a read or a write of
// ServerScriptService.Shop; `crash` exits. tools/list waits for the file argv[3] when one is given.
const server = `
const fs = require('node:fs');
const [log, gate] = process.argv.slice(2);
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
const text = (id, value) => reply(id, { content: [{ type: 'text', text: 'OPENRO:' + JSON.stringify({ ok: true, result: value }) }] });
const tools = { tools: [{ name: 'execute_luau', description: '', inputSchema: { type: 'object', properties: { code: { type: 'string' } }, required: ['code'] } }, { name: 'crash', description: '', inputSchema: { type: 'object' } }] };
const whenOpen = (fn) => (!gate || fs.existsSync(gate) ? fn() : setTimeout(() => whenOpen(fn), 10));
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'studio', version: '1' } });
  if (msg.method === 'tools/list') whenOpen(() => reply(msg.id, tools));
  if (msg.method === 'tools/call' && msg.params.name === 'crash') process.exit(1);
  if (msg.method === 'tools/call' && msg.params.name === 'execute_luau') {
    const write = msg.params.arguments.code.includes('expectedPrevious');
    fs.appendFileSync(log, (write ? 'write' : 'read') + '\\n');
    text(msg.id, write ? { path: 'ServerScriptService.Shop', className: 'Script', source: 'print(2)' } : { exists: true, path: 'ServerScriptService.Shop', className: 'Script', source: 'print(1)' });
  }
});
`;

const waitFor = async (check: () => boolean) => {
  for (let i = 0; i < 300 && !check(); i++) await new Promise((resolve) => setTimeout(resolve, 10));
  expect(check()).toBe(true);
};

describe('a Studio MCP server that crashes and is started again with the same command line', () => {
  let dir = '';
  let client: McpStdioClient;
  const spec = (gate = '') => ({ command: process.execPath, args: [path.join(dir, 'studio.cjs'), path.join(dir, 'calls.log'), gate] });
  const calls = async () => (await readFile(path.join(dir, 'calls.log'), 'utf8').catch(() => '')).split('\n').filter(Boolean);
  const reads = new StudioReads();
  const approvals = new ApprovalQueue();
  const stage = (name: string, args: Record<string, unknown>) =>
    stageStudioTool(approvals, client, reads, { projectRoot: dir, name, args: { path: 'ServerScriptService.Shop', ...args } });

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-studio-crash-'));
    await writeFile(path.join(dir, 'studio.cjs'), server, 'utf8');
    client = new McpStdioClient(5_000);
    reads.clear();
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  /** Read Shop on the first process, stage a write there, then crash it. */
  const readStageCrash = async () => {
    await client.start(spec());
    stage('studio_read_script', {});
    expect((await runApprovedStudio(approvals.take(approvals.list()[0].id), client, reads)).ok).toBe(true);
    const write = stage('studio_write_script', { className: 'Script', source: 'print(2)' });
    expect(write.ok).toBe(true);
    await expect(client.callTool('crash', {})).rejects.toThrow(/exited unexpectedly/);
    await waitFor(() => !client.running);
    return approvals.take(write.pendingApproval?.id ?? '');
  };

  it('refuses the staged write while the new process is in its handshake', async () => {
    const write = await readStageCrash();
    const gate = path.join(dir, 'gate');
    const restart = client.start(spec(gate));
    await waitFor(() => client.running);
    const result = await runApprovedStudio(write, client, reads);
    expect(result.ok).toBe(false);
    expect(result.checkpoint).toBeUndefined();
    await writeFile(gate, '', 'utf8');
    await restart;
    expect(await calls()).toEqual(['read']);
  });

  it('refuses the staged write after the new process is up, and the old read authorizes nothing there', async () => {
    const write = await readStageCrash();
    await client.start(spec());
    const result = await runApprovedStudio(write, client, reads);
    expect(result).toMatchObject({ ok: false, summary: expect.stringMatching(/MCP server changed after this Studio write was staged/) });
    expect(stage('studio_write_script', { className: 'Script', source: 'print(2)' }).output).toMatch(/studio_read_script first/);
    expect(await calls()).toEqual(['read']);
  });
});

import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpStdioClient } from '../electron/mcp';
import { runApprovedMcpCall, stageMcpCall } from '../electron/mcpTools';

// Server named argv[3]. Logs each run_code call to argv[2]. The tool `crash` exits the process.
// tools/list is answered only once the file argv[4] exists, so a test can hold the handshake open.
const server = `
const fs = require('node:fs');
const [log, name, gate] = process.argv.slice(2);
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
const tools = { tools: [{ name: 'run_code', description: '', inputSchema: { type: 'object' } }, { name: 'crash', description: '', inputSchema: { type: 'object' } }] };
const whenOpen = (fn) => (!gate || fs.existsSync(gate) ? fn() : setTimeout(() => whenOpen(fn), 10));
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name, version: '1' } });
  if (msg.method === 'tools/list') whenOpen(() => reply(msg.id, tools));
  if (msg.method === 'tools/call' && msg.params.name === 'crash') process.exit(1);
  if (msg.method === 'tools/call' && msg.params.name === 'run_code') { fs.appendFileSync(log, name + ' ran\\n'); reply(msg.id, { content: [{ type: 'text', text: name + ' ran' }] }); }
});
`;

const waitFor = async (check: () => boolean) => {
  for (let i = 0; i < 300 && !check(); i++) await new Promise((resolve) => setTimeout(resolve, 10));
  expect(check()).toBe(true);
};

describe('an MCP server that crashes without stop()', () => {
  let dir = '';
  let client: McpStdioClient;
  const spec = (name: string, gate = '') => ({ command: process.execPath, args: [path.join(dir, 'server.cjs'), path.join(dir, 'ran.log'), name, gate] });
  const ran = async () => (await readFile(path.join(dir, 'ran.log'), 'utf8').catch(() => '')).split('\n').filter(Boolean);

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-crash-'));
    await writeFile(path.join(dir, 'server.cjs'), server, 'utf8');
    client = new McpStdioClient(5_000);
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  const crashA = async () => {
    const approvals = new ApprovalQueue();
    await client.start(spec('A'));
    const staged = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { staged_on: 'A' } });
    await expect(client.callTool('crash', {})).rejects.toThrow(/exited unexpectedly/);
    await waitFor(() => !client.running);
    return { approvals, id: staged.pendingApproval?.id ?? '' };
  };

  it('forgets the crashed server: no tools and no identity', async () => {
    await crashA();
    expect(client.listTools()).toEqual([]);
    expect(client.identity).toBeNull();
  });

  it('refuses a call staged on A while server B is still in its handshake', async () => {
    const { approvals, id } = await crashA();
    const gate = path.join(dir, 'gate');
    const startB = client.start(spec('B', gate));
    await waitFor(() => client.running);
    expect(client.identity).toBeNull();
    expect(client.listTools()).toEqual([]);
    const result = await runApprovedMcpCall(approvals.take(id), client);
    expect(result.ok).toBe(false);
    await writeFile(gate, '', 'utf8');
    await startB;
    expect(await ran()).toEqual([]);
  });

  it('refuses a call staged on A after server B finished its handshake', async () => {
    const { approvals, id } = await crashA();
    await client.start(spec('B'));
    expect(client.identity?.args).toEqual(spec('B').args);
    const result = await runApprovedMcpCall(approvals.take(id), client);
    expect(result).toMatchObject({ ok: false, summary: expect.stringMatching(/MCP server changed/) });
    expect(await ran()).toEqual([]);
    const fresh = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: {} });
    expect(await runApprovedMcpCall(approvals.take(fresh.pendingApproval?.id ?? ''), client)).toMatchObject({ ok: true, output: 'B ran' });
  });

  it('start() discards tools and identity left by any earlier process', async () => {
    const stale = client as unknown as { tools: unknown[]; serverIdentity: unknown };
    stale.tools = [{ name: 'run_code', description: '' }];
    stale.serverIdentity = { command: 'stale', args: [], session: 'stale' };
    const gate = path.join(dir, 'gate');
    const startB = client.start(spec('B', gate));
    await waitFor(() => client.running);
    expect(client.identity).toBeNull();
    expect(client.listTools()).toEqual([]);
    await writeFile(gate, '', 'utf8');
    await startB;
    expect(client.identity?.session).not.toBe('stale');
  });
});

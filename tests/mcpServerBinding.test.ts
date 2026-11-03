import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpStdioClient } from '../electron/mcp';
import { runApprovedMcpCall, sameServer, serverLabel, stageMcpCall } from '../electron/mcpTools';
import { approvalCopy } from '../src/labels';

// Server named by argv[3]; logs each tools/call to argv[2] with its own name.
const namedServer = `
const fs = require('node:fs');
const [log, name] = process.argv.slice(2);
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name, version: '1' } });
  if (msg.method === 'tools/list') reply(msg.id, { tools: [{ name: 'run_code', description: '', inputSchema: { type: 'object', properties: { command: { type: 'string' } } } }] });
  if (msg.method === 'tools/call') { fs.appendFileSync(log, name + ' ran\\n'); reply(msg.id, { content: [{ type: 'text', text: name + ' ran' }] }); }
});
`;

describe('a staged MCP call is bound to the server it was staged on', () => {
  let dir = '';
  let logPath = '';
  let client: McpStdioClient;
  const spec = (name: string) => ({ command: process.execPath, args: [path.join(dir, 'server.cjs'), logPath, name] });
  const ran = async () => (await readFile(logPath, 'utf8').catch(() => '')).split('\n').filter(Boolean);

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-bind-'));
    logPath = path.join(dir, 'ran.log');
    await writeFile(path.join(dir, 'server.cjs'), namedServer, 'utf8');
    client = new McpStdioClient();
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  it('refuses to run on a different server, or on a restart of the same one', async () => {
    const approvals = new ApprovalQueue();
    await client.start(spec('A'));
    const first = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { command: 'x' } });
    const second = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { command: 'y' } });
    expect(approvals.list()[0].mcpServer).toMatchObject({ command: process.execPath, args: spec('A').args, session: expect.any(String) });
    await client.stop();
    await client.start(spec('B'));
    const onB = await runApprovedMcpCall(approvals.take(first.pendingApproval?.id ?? ''), client);
    expect(onB).toMatchObject({ ok: false, summary: expect.stringMatching(/MCP server changed after run_code was staged\. It did not run/) });
    await client.stop();
    await client.start(spec('A'));
    const restarted = await runApprovedMcpCall(approvals.take(second.pendingApproval?.id ?? ''), client);
    expect(restarted.ok).toBe(false);
    expect(await ran()).toEqual([]);
    const fresh = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { command: 'z' } });
    expect(await runApprovedMcpCall(approvals.take(fresh.pendingApproval?.id ?? ''), client)).toMatchObject({ ok: true, output: 'A ran' });
    expect(await ran()).toEqual(['A ran']);
  });

  it('compares command, arguments and session', () => {
    const base = { command: 'node', args: ['a'], session: 's1' };
    expect(sameServer(base, { ...base })).toBe(true);
    expect(sameServer(base, { ...base, session: 's2' })).toBe(false);
    expect(sameServer(base, { ...base, args: ['b'] })).toBe(false);
    expect(sameServer(base, { ...base, args: ['a', 'b'] })).toBe(false);
    expect(sameServer(base, { ...base, command: 'deno' })).toBe(false);
    expect(sameServer(undefined, base)).toBe(false);
    expect(sameServer(base, null)).toBe(false);
  });

  it('names the target server on the approval card', () => {
    const label = serverLabel({ command: 'npx', args: ['-y', 'my server'] });
    expect(label).toBe('npx -y "my server"');
    const copy = approvalCopy({ id: '1', kind: 'mcp', createdAt: '', mcpTool: 'run_code', mcpArgs: '{}', mcpServer: label });
    expect(copy.detail).toBe('Runs on npx -y "my server" only if you approve these exact arguments. It will not run on any other server');
  });
});

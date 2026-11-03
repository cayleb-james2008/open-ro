import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpStdioClient } from '../electron/mcp';
import { mcpAgentTools, mcpToolSchemas, runApprovedMcpCall, stageMcpCall } from '../electron/mcpTools';

// Logs every method it receives (and tools/call arguments) to the file named in argv[2].
const loggingServer = `
const fs = require('node:fs');
const log = process.argv[2];
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  fs.appendFileSync(log, msg.method + (msg.method === 'tools/call' ? ' ' + JSON.stringify(msg.params) : '') + '\\n');
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'log', version: '1' } });
  if (msg.method === 'tools/list') reply(msg.id, { tools: [
    { name: 'run_code', description: 'Run Luau in Studio', inputSchema: { type: 'object', properties: { command: { type: 'string' } }, required: ['command'] } },
  ] });
  if (msg.method === 'tools/call') reply(msg.id, { content: [{ type: 'text', text: 'ran ' + msg.params.arguments.command }] });
});
`;

describe('agent MCP tool calls go through the approval queue', () => {
  let dir = '';
  let logPath = '';
  let client: McpStdioClient;
  let approvals: ApprovalQueue;

  const methods = async (): Promise<string[]> => (await readFile(logPath, 'utf8')).trim().split('\n');
  const toolCalls = async (): Promise<string[]> => (await methods()).filter((line) => line.startsWith('tools/call'));

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-approve-'));
    logPath = path.join(dir, 'calls.log');
    await writeFile(path.join(dir, 'server.cjs'), loggingServer, 'utf8');
    client = new McpStdioClient();
    approvals = new ApprovalQueue();
    await client.start({ command: process.execPath, args: [path.join(dir, 'server.cjs'), logPath] });
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  it('offers connected tools to the agent with their input schema', () => {
    const [schema] = mcpToolSchemas(client.listTools());
    expect(schema.name).toBe('mcp__run_code');
    expect(schema.parameters).toEqual({ type: 'object', properties: { command: { type: 'string' } }, required: ['command'] });
    expect(schema.description).toMatch(/must approve/);
  });

  it('never calls the server for a staged call that is not approved', async () => {
    const staged = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { command: 'workspace:ClearAllChildren()' } });
    expect(staged.ok).toBe(true);
    expect(staged.pendingApproval).toMatchObject({ kind: 'mcp', tool: 'run_code' });
    expect(approvals.list()).toHaveLength(1);
    // A round trip proves the server has processed everything sent before it.
    await client.refreshTools();
    expect(await toolCalls()).toEqual([]);

    approvals.take(staged.pendingApproval?.id ?? '');
    await client.refreshTools();
    expect(await toolCalls()).toEqual([]);
    expect(approvals.list()).toHaveLength(0);
  });

  it('runs exactly the staged arguments once after approval', async () => {
    const args: Record<string, unknown> = { command: 'print("hi")' };
    const staged = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args });
    args.command = 'workspace:ClearAllChildren()';

    const item = approvals.take(staged.pendingApproval?.id ?? '');
    const result = await runApprovedMcpCall(item, client);
    expect(result).toMatchObject({ ok: true, decision: 'approve', output: 'ran print("hi")' });
    expect(await toolCalls()).toEqual(['tools/call {"name":"run_code","arguments":{"command":"print(\\"hi\\")"}}']);
    expect(() => approvals.take(item.id)).toThrow(/no longer pending/);
  });

  it('refuses unknown tools and a disconnected server without staging', async () => {
    expect(stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__delete_place', args: {} })).toMatchObject({ ok: false });
    const staged = stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: { command: 'x' } });
    await client.stop();
    expect(stageMcpCall(approvals, client, { projectRoot: dir, agentName: 'mcp__run_code', args: {} })).toMatchObject({ ok: false, output: 'No MCP server is connected.' });
    const late = await runApprovedMcpCall(approvals.take(staged.pendingApproval?.id ?? ''), client);
    expect(late).toMatchObject({ ok: false, summary: 'MCP tool run_code is no longer available. It did not run.' });
    expect(approvals.list()).toHaveLength(0);
    expect(await toolCalls()).toEqual([]);
  });
});

describe('mcp agent tool names', () => {
  it('sanitizes names, caps them at 64 characters, and drops collisions', () => {
    const named = mcpAgentTools([
      { name: 'insert.model', description: '' },
      { name: 'insert model', description: '' },
      { name: 'x'.repeat(80), description: '' },
      { name: '', description: '' },
    ]);
    expect(named.map((entry) => entry.agentName)).toEqual(['mcp__insert_model', `mcp__${'x'.repeat(59)}`]);
    expect(named[0].tool.name).toBe('insert.model');
  });

  it('strips top-level $schema and $id, and gives schemaless tools an empty object schema', () => {
    const [withSchema, without] = mcpToolSchemas([
      { name: 'echo', description: 'Echo', inputSchema: { type: 'object', $schema: 'https://json-schema.org/draft/2020-12/schema', $id: 'x', properties: { message: { type: 'string' } } } },
      { name: 'ping', description: '' },
    ]);
    expect(withSchema.parameters).toEqual({ type: 'object', properties: { message: { type: 'string' } } });
    expect(without.parameters).toEqual({ type: 'object', properties: {} });
  });

  it('rejects oversized staged arguments', () => {
    const queue = new ApprovalQueue();
    expect(() => queue.stageMcp({ projectRoot: '/p', tool: 'run_code', args: { command: 'x'.repeat(100_001) } })).toThrow(/100,000/);
  });
});

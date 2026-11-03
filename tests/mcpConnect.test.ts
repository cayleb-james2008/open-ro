import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpStdioClient } from '../electron/mcp';
import { runApprovedMcpServer, stageMcpConnect } from '../electron/mcpTools';

// Appends its argv to the log as soon as it is spawned, then answers the MCP handshake.
const startLogger = `
const fs = require('node:fs');
fs.appendFileSync(process.argv[2], 'spawned ' + JSON.stringify(process.argv.slice(3)) + '\\n');
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'start', version: '1' } });
  if (msg.method === 'tools/list') reply(msg.id, { tools: [{ name: 'noop', description: '' }] });
});
`;

describe('connecting an MCP server needs approval', () => {
  let dir = '';
  let logPath = '';
  let client: McpStdioClient;
  const spawned = async (): Promise<string[]> => (await readFile(logPath, 'utf8').catch(() => '')).split('\n').filter(Boolean);

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-connect-'));
    logPath = path.join(dir, 'spawn.log');
    await writeFile(path.join(dir, 'server.cjs'), startLogger, 'utf8');
    client = new McpStdioClient();
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  it('staging spawns nothing, and a rejected start never spawns', async () => {
    const approvals = new ApprovalQueue();
    const item = stageMcpConnect(approvals, client, { command: process.execPath, args: [path.join(dir, 'server.cjs'), logPath, 'a b'] });
    expect(item).toMatchObject({ kind: 'mcp-server', command: process.execPath, serverArgs: [path.join(dir, 'server.cjs'), logPath, 'a b'] });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(client.running).toBe(false);
    approvals.take(item.id);
    expect(await spawned()).toEqual([]);
    expect(() => stageMcpConnect(approvals, client, { command: '' })).toThrow(/required/);
  });

  it('approval starts exactly the staged command and arguments, once', async () => {
    const approvals = new ApprovalQueue();
    const item = stageMcpConnect(approvals, client, { command: process.execPath, args: [path.join(dir, 'server.cjs'), logPath, 'a b', '--flag'] });
    const result = await runApprovedMcpServer(approvals.take(item.id), client);
    expect(result).toMatchObject({ ok: true, summary: 'MCP server started with 1 tools.' });
    expect(await spawned()).toEqual(['spawned ["a b","--flag"]']);
    expect(() => stageMcpConnect(approvals, client, { command: process.execPath })).toThrow(/already active/);
    expect(() => approvals.take(item.id)).toThrow(/no longer pending/);
  });
});

import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { McpStdioClient } from '../electron/mcp';

// Reads stdin with Node's readline, which also ends a line at U+2028 and U+2029. Lines that are not
// JSON are logged to argv[2]. The tool `echo` returns its `text` argument.
const server = `
const fs = require('node:fs');
const [log] = process.argv.slice(2);
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { fs.appendFileSync(log, JSON.stringify(line) + '\\n'); return; }
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'echo', version: '1' } });
  if (msg.method === 'tools/list') reply(msg.id, { tools: [{ name: 'echo', description: '', inputSchema: { type: 'object' } }] });
  if (msg.method === 'tools/call') reply(msg.id, { content: [{ type: 'text', text: msg.params.arguments.text }] });
});
`;

describe('outgoing MCP messages and line separators', () => {
  let dir = '';
  let client: McpStdioClient;

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-lines-'));
    await writeFile(path.join(dir, 'server.cjs'), server, 'utf8');
    client = new McpStdioClient(2_000);
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  it('escapes U+2028 and U+2029, so a readline server gets one intact message', async () => {
    await client.start({ command: process.execPath, args: [path.join(dir, 'server.cjs'), path.join(dir, 'broken.log')] });
    const text = 'local a = 1\u2028local b = 2\u2029print(a + b)';
    expect(await client.callTool('echo', { text })).toBe(text);
    expect(await readFile(path.join(dir, 'broken.log'), 'utf8').catch(() => '')).toBe('');
  });
});

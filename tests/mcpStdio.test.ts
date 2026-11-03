import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { McpStdioClient } from '../electron/mcp';

// A strict stdio server: it parses each stdin line as one JSON-RPC message, as the
// MCP stdio transport requires, and ignores anything else (such as Content-Length headers).
const strictServer = `
const readline = require('node:readline');
const seen = [];
const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { process.stderr.write('unparsed line\\n'); return; }
  seen.push(msg.method);
  if (msg.method === 'initialize') reply(msg.id, { protocolVersion: msg.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'strict', version: '1' } });
  if (msg.method === 'tools/list') reply(msg.id, { tools: [{ name: 'echo', description: 'Echo text' }, { name: 'fail', description: 'Always errors' }] });
  if (msg.method === 'tools/call' && msg.params.name === 'echo') reply(msg.id, { content: [{ type: 'text', text: msg.params.arguments.text }, { type: 'text', text: 'seen=' + seen.join(',') }] });
  if (msg.method === 'tools/call' && msg.params.name === 'fail') reply(msg.id, { content: [{ type: 'text', text: 'Studio is not open' }], isError: true });
});
`;

describe('mcp stdio client against a newline-delimited server', () => {
  let dir = '';
  let client: McpStdioClient;

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-'));
    await writeFile(path.join(dir, 'server.cjs'), strictServer, 'utf8');
    client = new McpStdioClient();
  });

  afterEach(async () => {
    await client.stop();
    await rm(dir, { recursive: true, force: true });
  });

  it('completes the handshake, lists tools, and round-trips a multi-line argument', async () => {
    await client.start({ command: process.execPath, args: [path.join(dir, 'server.cjs')] });
    expect(client.listTools().map((tool) => tool.name)).toEqual(['echo', 'fail']);
    const text = await client.callTool('echo', { text: 'line one\nline two\u2028line three' });
    expect(text).toBe('line one\nline two\u2028line three\nseen=initialize,notifications/initialized,tools/list,tools/call');
  });

  it('surfaces a tool error result as a rejected call', async () => {
    await client.start({ command: process.execPath, args: [path.join(dir, 'server.cjs')] });
    await expect(client.callTool('fail', {})).rejects.toThrow('Studio is not open');
  });
});

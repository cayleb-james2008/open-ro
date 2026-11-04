import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { McpStdioClient } from '../electron/mcp';

// Never answers initialize.
const silentServer = `require('node:readline').createInterface({ input: process.stdin }).on('line', () => {});`;

// Sends ping and roots/list to the client after initialize, and logs the client's answers.
const askingServer = `
const fs = require('node:fs');
const log = process.argv[2];
const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') {
    send({ jsonrpc: '2.0', id: 'srv-ping', method: 'ping' });
    send({ jsonrpc: '2.0', id: 7, method: 'roots/list', params: {} });
    send({ jsonrpc: '2.0', method: 'notifications/message', params: { level: 'info', data: 'hello' } });
    send({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: msg.params.protocolVersion, capabilities: {}, serverInfo: { name: 'ask', version: '1' } } });
    return;
  }
  if (msg.method === 'tools/list') { send({ jsonrpc: '2.0', id: msg.id, result: { tools: [] } }); return; }
  if (msg.method === undefined) fs.appendFileSync(log, JSON.stringify(msg) + '\\n');
});
`;

// Returns text and image content blocks; `fail: true` returns an isError result.
const imageServer = `
const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\\n');
require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') {
    send({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: msg.params.protocolVersion, capabilities: {}, serverInfo: { name: 'img', version: '1' } } });
    return;
  }
  if (msg.method === 'tools/list') {
    send({ jsonrpc: '2.0', id: msg.id, result: { tools: [{ name: 'screen_capture', description: 'capture', inputSchema: { type: 'object', properties: {} } }] } });
    return;
  }
  if (msg.method === 'tools/call') {
    if (msg.params.arguments && msg.params.arguments.fail) {
      send({ jsonrpc: '2.0', id: msg.id, result: { isError: true, content: [{ type: 'text', text: 'capture failed' }] } });
      return;
    }
    send({
      jsonrpc: '2.0', id: msg.id,
      result: { content: [ { type: 'text', text: 'captured' }, { type: 'image', data: 'aGVsbG8=', mimeType: 'image/png' }, { type: 'image', data: 'd29ybGQ=' } ] },
    });
  }
});
`;

describe('mcp client lifecycle', () => {
  let dir = '';
  const clients: McpStdioClient[] = [];

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-life-'));
    await writeFile(path.join(dir, 'silent.cjs'), silentServer, 'utf8');
    await writeFile(path.join(dir, 'ask.cjs'), askingServer, 'utf8');
    await writeFile(path.join(dir, 'image.cjs'), imageServer, 'utf8');
  });

  afterEach(async () => {
    await Promise.all(clients.map((client) => client.stop()));
    clients.length = 0;
    await rm(dir, { recursive: true, force: true });
  });

  it('stops the server when the handshake fails, so the next connect works', async () => {
    // 3 s: under parallel vitest workers a node spawn plus handshake can exceed 300 ms.
    const client = new McpStdioClient(3_000);
    clients.push(client);
    await expect(client.start({ command: process.execPath, args: [path.join(dir, 'silent.cjs')] })).rejects.toThrow(/initialize timed out/);
    expect(client.running).toBe(false);
    await client.start({ command: process.execPath, args: [path.join(dir, 'ask.cjs'), path.join(dir, 'answers.log')] });
    expect(client.running).toBe(true);
  });

  it('stops the server when the command cannot start', async () => {
    const client = new McpStdioClient(300);
    clients.push(client);
    await expect(client.start({ command: path.join(dir, 'missing-binary'), args: [] })).rejects.toThrow();
    expect(client.running).toBe(false);
  });

  it('answers ping with an empty result and unsupported server requests with method-not-found', async () => {
    const client = new McpStdioClient();
    clients.push(client);
    const logPath = path.join(dir, 'answers.log');
    await client.start({ command: process.execPath, args: [path.join(dir, 'ask.cjs'), logPath] });
    await client.refreshTools();
    const answers = (await readFile(logPath, 'utf8')).trim().split('\n').map((line) => JSON.parse(line) as Record<string, unknown>);
    expect(answers).toEqual([
      { jsonrpc: '2.0', id: 'srv-ping', result: {} },
      { jsonrpc: '2.0', id: 7, error: { code: -32601, message: 'Method not supported by Open-Ro: roots/list' } },
    ]);
  });

  it('returns text and base64 images from callToolResult while callTool stays text-only', async () => {
    const client = new McpStdioClient();
    clients.push(client);
    await client.start({ command: process.execPath, args: [path.join(dir, 'image.cjs')] });
    const result = await client.callToolResult('screen_capture', {});
    expect(result).toEqual({
      text: 'captured',
      isError: false,
      images: [
        { mimeType: 'image/png', data: 'aGVsbG8=' },
        { mimeType: 'image/png', data: 'd29ybGQ=' },
      ],
    });
    expect(await client.callTool('screen_capture', {})).toBe('captured');
    expect(await client.callToolResult('screen_capture', { fail: true })).toMatchObject({ isError: true, text: 'capture failed' });
    await expect(client.callTool('screen_capture', { fail: true })).rejects.toThrow('capture failed');
  });
});

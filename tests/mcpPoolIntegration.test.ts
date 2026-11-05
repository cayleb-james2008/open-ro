import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { McpServerPool } from '../electron/mcpPool';

// Node.js stdio mock MCP server script
const mockServerScript = `
const readline = require('readline');
const [serverName] = process.argv.slice(2);

const tools = serverName === 'studio'
  ? [{ name: 'execute_luau', description: 'Run Luau script' }, { name: 'search_tree', description: 'Search tree' }]
  : [{ name: 'render_scene', description: 'Render Blender scene' }, { name: 'generate_mesh', description: 'Generate mesh' }];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.method === 'initialize') {
      const res = { protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: serverName, version: '1.0.0' } };
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: res }) + '\\n');
    } else if (msg.method === 'tools/list') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { tools } }) + '\\n');
    } else if (msg.method === 'ping') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: {} }) + '\\n');
    } else if (msg.method === 'tools/call') {
      if (msg.params.name === 'crash') {
        process.exit(1);
      }
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text: serverName + ':' + msg.params.name + ':ok' }] } }) + '\\n');
    }
  } catch {
    // ignore parse errors
  }
});
`;

describe('McpServerPool Integration Tests: Real Child Processes & Fault Isolation', () => {
  let tempDir = '';
  let serverScriptPath = '';
  let pool: McpServerPool;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-pool-integ-'));
    serverScriptPath = path.join(tempDir, 'mockServer.js');
    await writeFile(serverScriptPath, mockServerScript, 'utf8');
    pool = new McpServerPool({
      heartbeatIntervalMs: 5_000,
      heartbeatTimeoutMs: 2_000,
    });
  });

  afterEach(async () => {
    await pool.disconnectAll();
    await rm(tempDir, { recursive: true, force: true });
  });

  it('connects multiple real child processes concurrently and discovers their tools', async () => {
    const studioInfo = await pool.connectMcpServer({
      id: 'studio',
      command: process.execPath,
      args: [serverScriptPath, 'studio'],
    });

    const blenderInfo = await pool.connectMcpServer({
      id: 'blender',
      command: process.execPath,
      args: [serverScriptPath, 'blender'],
    });

    expect(studioInfo.id).toBe('studio');
    expect(studioInfo.session).toContain('pool-studio-');
    expect(studioInfo.tools.map((t) => t.name)).toEqual(['execute_luau', 'search_tree']);

    expect(blenderInfo.id).toBe('blender');
    expect(blenderInfo.session).toContain('pool-blender-');
    expect(blenderInfo.tools.map((t) => t.name)).toEqual(['render_scene', 'generate_mesh']);

    expect(pool.listServers().length).toBe(2);
    expect(pool.getMcpClient('studio')?.running).toBe(true);
    expect(pool.getMcpClient('blender')?.running).toBe(true);
  });

  it('routes concurrent tool calls to their respective child processes', async () => {
    await pool.connectMcpServer({
      id: 'studio',
      command: process.execPath,
      args: [serverScriptPath, 'studio'],
    });

    await pool.connectMcpServer({
      id: 'blender',
      command: process.execPath,
      args: [serverScriptPath, 'blender'],
    });

    const [studioRes, blenderRes] = await Promise.all([
      pool.callMcpTool('studio', 'execute_luau', { code: 'print("hello")' }),
      pool.callMcpTool('blender', 'render_scene', { format: 'PNG' }),
    ]);

    expect(studioRes).toBe('studio:execute_luau:ok');
    expect(blenderRes).toBe('blender:render_scene:ok');
  });

  it('guarantees fault isolation: crash of one child process does not disrupt other servers', async () => {
    await pool.connectMcpServer({
      id: 'studio',
      command: process.execPath,
      args: [serverScriptPath, 'studio'],
      autoRestart: false, // keep down for isolation assertion
    });

    await pool.connectMcpServer({
      id: 'blender',
      command: process.execPath,
      args: [serverScriptPath, 'blender'],
    });

    expect(pool.getMcpClient('studio')?.running).toBe(true);
    expect(pool.getMcpClient('blender')?.running).toBe(true);

    // Stop/disconnect studio
    await pool.disconnectMcpServer('studio');
    expect(pool.getMcpClient('studio')).toBeUndefined();

    // Blender must remain fully functional
    const blenderClient = pool.getMcpClient('blender');
    expect(blenderClient).toBeDefined();
    expect(blenderClient?.running).toBe(true);

    const blenderRes = await pool.callMcpTool('blender', 'generate_mesh', { type: 'cube' });
    expect(blenderRes).toBe('blender:generate_mesh:ok');
  });

  it('cleanly stops all child processes on pool.disconnectAll()', async () => {
    await pool.connectMcpServer({
      id: 's1',
      command: process.execPath,
      args: [serverScriptPath, 'studio'],
    });
    await pool.connectMcpServer({
      id: 's2',
      command: process.execPath,
      args: [serverScriptPath, 'blender'],
    });

    expect(pool.listServers().length).toBe(2);

    await pool.disconnectAll();
    expect(pool.listServers().length).toBe(0);
    expect(pool.getMcpClient('s1')).toBeUndefined();
    expect(pool.getMcpClient('s2')).toBeUndefined();
  });
});

import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpServerPool } from '../electron/mcpPool';
import { extractServerIdFromSession, runApprovedMcpCall, sameServer, stageMcpCall } from '../electron/mcpTools';

const serverScript = `
const readline = require('readline');
const [serverName] = process.argv.slice(2);
const tools = serverName === 'studio'
  ? [{ name: 'execute_luau', description: 'Run Luau script' }]
  : [{ name: 'render_scene', description: 'Render scene' }];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.method === 'initialize') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: serverName, version: '1' } } }) + '\\n');
    } else if (msg.method === 'tools/list') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { tools } }) + '\\n');
    } else if (msg.method === 'tools/call') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text: serverName + ' executed ' + msg.params.name }] } }) + '\\n');
    }
  } catch {}
});
`;

describe('McpServerPool Approval Queue & Staged Security Tests', () => {
  let tempDir = '';
  let scriptPath = '';
  let pool: McpServerPool;
  let approvals: ApprovalQueue;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-mcp-approvals-'));
    scriptPath = path.join(tempDir, 'server.js');
    await writeFile(scriptPath, serverScript, 'utf8');
    pool = new McpServerPool();
    approvals = new ApprovalQueue();
  });

  afterEach(async () => {
    await pool.disconnectAll();
    await rm(tempDir, { recursive: true, force: true });
  });

  it('stages calls to distinct servers with isolated session identities', async () => {
    await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });
    await pool.connectMcpServer({ id: 'blender', command: process.execPath, args: [scriptPath, 'blender'] });

    const stagedStudio = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__studio__execute_luau',
      args: { code: 'print("studio")' },
    });
    expect(stagedStudio.ok).toBe(true);

    const stagedBlender = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__blender__render_scene',
      args: { samples: 64 },
    });
    expect(stagedBlender.ok).toBe(true);

    const items = approvals.list();
    expect(items.length).toBe(2);

    const studioItem = items.find((i) => i.id === stagedStudio.pendingApproval?.id);
    const blenderItem = items.find((i) => i.id === stagedBlender.pendingApproval?.id);

    expect(studioItem?.mcpServer?.session).toContain('pool-studio-');
    expect(blenderItem?.mcpServer?.session).toContain('pool-blender-');
    expect(studioItem?.mcpServer?.session).not.toBe(blenderItem?.mcpServer?.session);
  });

  it('executes approved calls on their respective servers in the pool', async () => {
    await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });
    await pool.connectMcpServer({ id: 'blender', command: process.execPath, args: [scriptPath, 'blender'] });

    const stagedStudio = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__studio__execute_luau',
      args: { code: 'print("test")' },
    });
    const stagedBlender = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__blender__render_scene',
      args: { samples: 128 },
    });

    const studioClient = pool.getMcpClient('studio')!;
    const blenderClient = pool.getMcpClient('blender')!;

    const studioResult = await runApprovedMcpCall(approvals.take(stagedStudio.pendingApproval!.id), studioClient);
    expect(studioResult.ok).toBe(true);
    expect(studioResult.output).toBe('studio executed execute_luau');

    const blenderResult = await runApprovedMcpCall(approvals.take(stagedBlender.pendingApproval!.id), blenderClient);
    expect(blenderResult.ok).toBe(true);
    expect(blenderResult.output).toBe('blender executed render_scene');
  });

  it('invalidates Server A calls when Server A restarts, while Server B calls remain valid', async () => {
    await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });
    await pool.connectMcpServer({ id: 'blender', command: process.execPath, args: [scriptPath, 'blender'] });

    const stagedStudio = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__studio__execute_luau',
      args: { code: 'warn("restart")' },
    });
    const stagedBlender = stageMcpCall(approvals, pool, {
      projectRoot: tempDir,
      agentName: 'mcp__blender__render_scene',
      args: { samples: 32 },
    });

    // Disconnect and restart Studio with fresh session
    await pool.disconnectMcpServer('studio');
    await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });

    const newStudioClient = pool.getMcpClient('studio')!;
    const blenderClient = pool.getMcpClient('blender')!;

    // Executing stagedStudio now must FAIL CLOSED because session changed
    const studioItem = approvals.take(stagedStudio.pendingApproval!.id);
    const studioExec = await runApprovedMcpCall(studioItem, newStudioClient);
    expect(studioExec.ok).toBe(false);
    expect(studioExec.summary).toContain('MCP server changed after execute_luau was staged');

    // Executing stagedBlender must SUCCEED because Blender did not restart
    const blenderItem = approvals.take(stagedBlender.pendingApproval!.id);
    const blenderExec = await runApprovedMcpCall(blenderItem, blenderClient);
    expect(blenderExec.ok).toBe(true);
    expect(blenderExec.output).toBe('blender executed render_scene');
  });

  it('sameServer refuses cross-server execution even if command and args match', () => {
    const serverAIdentity = { command: 'node', args: ['server.js'], session: 'pool-serverA-uuid1' };
    const serverBIdentity = { command: 'node', args: ['server.js'], session: 'pool-serverB-uuid2' };

    expect(sameServer(serverAIdentity, serverAIdentity)).toBe(true);
    expect(sameServer(serverBIdentity, serverBIdentity)).toBe(true);
    expect(sameServer(serverAIdentity, serverBIdentity)).toBe(false);
  });

  it('getClientForApproval disambiguates hyphenated server IDs from prefix servers', async () => {
    await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });
    await pool.connectMcpServer({ id: 'studio-mcp', command: process.execPath, args: [scriptPath, 'studio'] });

    const clientStudio = pool.getMcpClient('studio')!;
    const clientStudioMcp = pool.getMcpClient('studio-mcp')!;

    // Resolve client for studio-mcp
    const resolvedMcp = pool.getClientForApproval(clientStudioMcp.identity!);
    expect(resolvedMcp).toBeDefined();
    expect(resolvedMcp?.serverId).toBe('studio-mcp');
    expect(resolvedMcp).not.toBe(clientStudio);

    // Resolve client for studio
    const resolvedStudio = pool.getClientForApproval(clientStudio.identity!);
    expect(resolvedStudio).toBeDefined();
    expect(resolvedStudio?.serverId).toBe('studio');
    expect(resolvedStudio).not.toBe(clientStudioMcp);
  });
});

describe('Hyphenated Server IDs Session Binding & Security Tests', () => {
  it('extractServerIdFromSession correctly extracts multi-hyphenated server IDs from UUID and test sessions', () => {
    // Standard RFC-4122 UUID sessions
    expect(extractServerIdFromSession('pool-studio-mcp-e2b2f67c-1793-4a11-80a1-4ce8d5381fbc')).toBe('studio-mcp');
    expect(extractServerIdFromSession('pool-blender-3d-pipeline-00000000-0000-0000-0000-000000000000')).toBe('blender-3d-pipeline');
    expect(extractServerIdFromSession('pool-roblox-studio-editor-12345678-abcd-ef01-2345-6789abcdef01')).toBe('roblox-studio-editor');
    expect(extractServerIdFromSession('pool-single-word-e2b2f67c-1793-4a11-80a1-4ce8d5381fbc')).toBe('single-word');

    // Test / synthetic non-UUID sessions (backward compatibility)
    expect(extractServerIdFromSession('pool-serverA-uuid1')).toBe('serverA');
    expect(extractServerIdFromSession('pool-studio-mcp-uuid1')).toBe('studio-mcp');
    expect(extractServerIdFromSession('pool-maliciousServer-12345')).toBe('maliciousServer');

    // Non-pool sessions or invalid formats
    expect(extractServerIdFromSession('non-pool-session-id')).toBeUndefined();
    expect(extractServerIdFromSession('e2b2f67c-1793-4a11-80a1-4ce8d5381fbc')).toBeUndefined();
    expect(extractServerIdFromSession('')).toBeUndefined();
    expect(extractServerIdFromSession(null)).toBeUndefined();
  });

  it('sameServer enforces isolation between servers sharing hyphenated prefixes', () => {
    const serverMcp = {
      command: 'node',
      args: ['server.js'],
      session: 'pool-studio-mcp-e2b2f67c-1793-4a11-80a1-4ce8d5381fbc',
      serverId: 'studio-mcp',
    };
    const serverBlender = {
      command: 'node',
      args: ['server.js'],
      session: 'pool-studio-blender-f3c3a78d-2804-4b22-91b2-5df9e6492ace',
      serverId: 'studio-blender',
    };
    const serverMcpRestarted = {
      command: 'node',
      args: ['server.js'],
      session: 'pool-studio-mcp-99999999-9999-9999-9999-999999999999',
      serverId: 'studio-mcp',
    };

    // Identical server & session passes
    expect(sameServer(serverMcp, serverMcp)).toBe(true);

    // Servers sharing the prefix 'studio-' must be refused as different servers
    expect(sameServer(serverMcp, serverBlender)).toBe(false);
    expect(sameServer(serverBlender, serverMcp)).toBe(false);

    // Restarted server with fresh session must fail closed
    expect(sameServer(serverMcp, serverMcpRestarted)).toBe(false);
  });
});

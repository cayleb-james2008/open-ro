import * as crypto from 'node:crypto';
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { McpPoolClient, McpServerPool, type McpPoolClientOptions, type McpServerConfig } from '../electron/mcpPool';
import { runApprovedMcpServer, stageMcpPoolConnect } from '../electron/mcpTools';
import type { McpServerIdentity, McpToolSummary } from '../electron/types';

class MockIpcClient extends McpPoolClient {
  public mockRunning = false;
  public mockTools: McpToolSummary[] = [];
  public mockIdentity: McpServerIdentity | null = null;

  constructor(config: McpServerConfig, options: McpPoolClientOptions = {}) {
    super(config, options);
  }

  override get running(): boolean {
    return this.mockRunning;
  }

  override get identity(): McpServerIdentity | null {
    return this.mockIdentity;
  }

  override listTools(): McpToolSummary[] {
    return [...this.mockTools];
  }

  override async start(): Promise<void> {
    this.mockRunning = true;
    const session = `pool-${this.config.id}-${crypto.randomUUID()}`;
    this.mockIdentity = {
      command: this.config.command,
      args: [...this.config.args],
      session,
      serverId: this.config.id,
    };
    (this as unknown as { currentSession: string }).currentSession = session;
    (this as unknown as { currentStatus: string }).currentStatus = 'connected';
    this.lastUsed = Date.now();
  }

  override async stop(): Promise<void> {
    this.mockRunning = false;
    this.mockIdentity = null;
    (this as unknown as { currentStatus: string }).currentStatus = 'disconnected';
  }
}

describe('McpServerPool IPC & Preload Allowlist Tests', () => {
  it('registers all mcp:pool:* channels in electron/preload.ts', () => {
    const preloadText = readFileSync(path.resolve(__dirname, '../electron/preload.ts'), 'utf8');

    const expectedChannels = [
      'mcp:status',
      'mcp:connect',
      'mcp:disconnect',
      'mcp:pool:list',
      'mcp:pool:server-status',
      'mcp:pool:connect',
      'mcp:pool:disconnect',
      'mcp:pool:reconnect',
      'mcp:pool:save-config',
      'mcp:pool:delete-config',
      'mcp:pool:tools',
      'mcp:pool:health',
    ];

    for (const channel of expectedChannels) {
      expect(preloadText).toContain(`'${channel}'`);
    }

    expect(preloadText).toContain('onMcpServerEvent:');
  });

  it('registers pool IPC handlers in electron/main.ts', () => {
    const mainText = readFileSync(path.resolve(__dirname, '../electron/main.ts'), 'utf8');

    const expectedHandlers = [
      "ipcMain.handle('mcp:pool:list'",
      "ipcMain.handle('mcp:pool:server-status'",
      "ipcMain.handle('mcp:pool:connect'",
      "ipcMain.handle('mcp:pool:disconnect'",
      "ipcMain.handle('mcp:pool:reconnect'",
      "ipcMain.handle('mcp:pool:save-config'",
      "ipcMain.handle('mcp:pool:delete-config'",
      "ipcMain.handle('mcp:pool:tools'",
      "ipcMain.handle('mcp:pool:health'",
    ];

    for (const handler of expectedHandlers) {
      expect(mainText).toContain(handler);
    }
  });

  it('generates rich McpServerView and McpPoolHealthReport', async () => {
    const pool = new McpServerPool({
      maxActiveServers: 6,
    });

    await pool.saveServerConfig({
      id: 'custom-srv',
      name: 'Custom Tool Server',
      command: 'node',
      args: ['tool.js'],
    });

    const views = pool.listServerViews();
    expect(views.length).toBe(1);
    expect(views[0].id).toBe('custom-srv');
    expect(views[0].name).toBe('Custom Tool Server');
    expect(views[0].status).toBe('disconnected');
    expect(views[0].running).toBe(false);

    const singleView = pool.getServerView('custom-srv');
    expect(singleView).not.toBeNull();
    expect(singleView?.command).toBe('node');

    const health = pool.getHealthReport();
    expect(health.activeCount).toBe(0);
    expect(health.maxActive).toBe(6);
    expect(health.totalConfigured).toBe(1);
    expect(health.servers['custom-srv'].status).toBe('disconnected');

    await pool.deleteServerConfig('custom-srv');
    expect(pool.listServerViews().length).toBe(0);
  });

  it('stages pool connect approvals via stageMcpPoolConnect', () => {
    const approvals = new ApprovalQueue();
    const pending = stageMcpPoolConnect(approvals, {
      id: 'studio',
      command: 'npx',
      args: ['-y', 'roblox-studio-mcp'],
    });

    expect(pending.kind).toBe('mcp-server');
    expect(pending.command).toBe('npx');
    expect(pending.serverArgs).toEqual(['-y', 'roblox-studio-mcp']);
    expect(pending.mcpServerId).toBe('studio');
    expect(approvals.list().length).toBe(1);
  });

  it('connects multiple servers concurrently via approval queue resolution without replacing each other', async () => {
    const approvals = new ApprovalQueue();
    const pool = new McpServerPool({
      maxActiveServers: 6,
      clientFactory: (config, opts) => new MockIpcClient(config, opts),
    });

    // 1. Stage connection for serverA
    const pendingA = stageMcpPoolConnect(approvals, {
      id: 'serverA',
      command: 'node',
      args: ['serverA.js'],
    });
    expect(pendingA.kind).toBe('mcp-server');
    expect(pendingA.mcpServerId).toBe('serverA');
    expect(pendingA.command).toBe('node');
    expect(pendingA.serverArgs).toEqual(['serverA.js']);

    // 2. Stage connection for serverB
    const pendingB = stageMcpPoolConnect(approvals, {
      id: 'serverB',
      command: 'node',
      args: ['serverB.js'],
    });
    expect(pendingB.kind).toBe('mcp-server');
    expect(pendingB.mcpServerId).toBe('serverB');

    // Verify both approvals are pending concurrently with their distinct IDs preserved
    const queueItems = approvals.list();
    expect(queueItems.length).toBe(2);
    expect(queueItems.find((item) => item.id === pendingA.id)?.mcpServerId).toBe('serverA');
    expect(queueItems.find((item) => item.id === pendingB.id)?.mcpServerId).toBe('serverB');

    // 3. Resolve approval for serverA
    const itemA = approvals.take(pendingA.id);
    const resultA = await runApprovedMcpServer(itemA, pool);
    expect(resultA.ok).toBe(true);
    expect(resultA.summary).toContain('serverA');

    const clientA = pool.getMcpClient('serverA');
    expect(clientA).toBeDefined();
    expect(clientA?.running).toBe(true);
    expect(clientA?.identity?.session).toContain('pool-serverA-');

    // 4. Resolve approval for serverB
    const itemB = approvals.take(pendingB.id);
    const resultB = await runApprovedMcpServer(itemB, pool);
    expect(resultB.ok).toBe(true);
    expect(resultB.summary).toContain('serverB');

    const clientB = pool.getMcpClient('serverB');
    expect(clientB).toBeDefined();
    expect(clientB?.running).toBe(true);
    expect(clientB?.identity?.session).toContain('pool-serverB-');

    // 5. CRUCIAL VERIFICATION: Both servers are running simultaneously
    expect(pool.getMcpClient('serverA')?.running).toBe(true);
    expect(pool.getMcpClient('serverB')?.running).toBe(true);

    // Verify listServers() lists both servers and neither was renamed to 'default'
    const activeServers = pool.listServers();
    expect(activeServers.length).toBe(2);
    const activeIds = activeServers.map((s) => s.id);
    expect(activeIds).toContain('serverA');
    expect(activeIds).toContain('serverB');
    expect(activeIds).not.toContain('default');

    // Verify health report reflects exactly 2 active servers
    const health = pool.getHealthReport();
    expect(health.activeCount).toBe(2);
    expect(health.servers['serverA'].status).toBe('connected');
    expect(health.servers['serverB'].status).toBe('connected');
  });
});

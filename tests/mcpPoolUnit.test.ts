import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import {
  McpPoolClient,
  McpServerPool,
  type McpPoolClientOptions,
  type McpServerConfig,
} from '../electron/mcpPool';
import { runApprovedMcpCall, stageMcpCall } from '../electron/mcpTools';
import type { McpServerIdentity, McpToolSummary } from '../electron/types';

/**
 * Mock client for unit testing McpServerPool without spawning real processes.
 */
class MockUnitClient extends McpPoolClient {
  public mockRunning = false;
  public mockTools: McpToolSummary[] = [];
  public mockIdentity: McpServerIdentity | null = null;
  public startCalls = 0;
  public stopCalls = 0;
  public mockPingDelayMs = 4;
  public mockPingShouldFail = false;
  public readonly unitOptions: McpPoolClientOptions;

  constructor(
    config: McpServerConfig,
    options: McpPoolClientOptions = {},
  ) {
    super(config, options);
    this.unitOptions = options;
  }

  override get running(): boolean {
    return this.mockRunning;
  }

  override get identity(): McpServerIdentity | null {
    if (this.mockRunning) {
      return this.mockIdentity;
    }
    if (this.status === 'hibernating' && this.mockIdentity) {
      return this.mockIdentity;
    }
    return null;
  }

  override listTools(): McpToolSummary[] {
    return [...this.mockTools];
  }

  override async start(): Promise<void> {
    this.startCalls++;
    this.mockRunning = true;
    const isWaking = this.status === 'hibernating';
    const session = isWaking && this.mockIdentity ? this.mockIdentity.session : `pool-${this.config.id}-${crypto.randomUUID()}`;
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
    this.stopCalls++;
    this.mockRunning = false;
    this.mockIdentity = null;
    (this as unknown as { currentStatus: string }).currentStatus = 'disconnected';
  }

  override async hibernate(): Promise<void> {
    this.mockRunning = false;
    (this as unknown as { currentStatus: string }).currentStatus = 'hibernating';
  }

  override async ping(): Promise<number> {
    if (this.mockPingShouldFail) {
      throw new Error('Mock ping failure');
    }
    this.lastUsed = Date.now();
    return this.mockPingDelayMs;
  }

  override async callTool(name: string, args: Record<string, unknown>): Promise<string> {
    if (!this.mockRunning) {
      if (this.status === 'hibernating') {
        if (this.unitOptions.onBeforeWake) {
          await this.unitOptions.onBeforeWake(this);
        }
        await this.start();
      } else {
        throw new Error('MCP server is not running.');
      }
    }
    this.lastUsed = Date.now();
    return JSON.stringify({ echo: name, args });
  }

  public simulateCrash(err = new Error('Process exited unexpectedly')): void {
    (this as unknown as { onChildGone(err: Error): void }).onChildGone(err);
  }
}

describe('McpServerPool Unit Tests: Configuration & Boundaries', () => {
  it('throws an error when connecting server with empty or missing id', async () => {
    const pool = new McpServerPool();
    await expect(pool.connectMcpServer({ id: '', command: 'node', args: [] })).rejects.toThrow(/requires id/);
    await expect(pool.connectMcpServer({ id: '   ', command: 'node', args: [] })).rejects.toThrow(/requires id/);
  });

  it('throws an error when connecting server with empty or missing command', async () => {
    const pool = new McpServerPool();
    await expect(pool.connectMcpServer({ id: 'studio', command: '', args: [] })).rejects.toThrow(/requires id and command/);
    await expect(pool.connectMcpServer({ id: 'studio', command: '   ', args: [] })).rejects.toThrow(/requires id and command/);
  });

  it('throws an error when calling tool on unknown serverId', async () => {
    const pool = new McpServerPool();
    await expect(pool.callMcpTool('ghost-server', 'ping', {})).rejects.toThrow(/not found in pool/);
  });

  it('idempotently handles disconnect for non-existent serverId', async () => {
    const pool = new McpServerPool();
    await expect(pool.disconnectMcpServer('ghost-server')).resolves.toBeUndefined();
  });
});

describe('McpServerPool Unit Tests: Pooling & LRU Capacity', () => {
  it('manages up to 6 active servers and hibernates LRU when capacity is exceeded', async () => {
    const clients = new Map<string, MockUnitClient>();
    const pool = new McpServerPool({
      maxActiveServers: 3,
      clientFactory: (config, opts) => {
        const client = new MockUnitClient(config, opts);
        clients.set(config.id, client);
        return client;
      },
    });

    await pool.connectMcpServer({ id: 's1', command: 'cmd1', args: [] });
    await pool.connectMcpServer({ id: 's2', command: 'cmd2', args: [] });
    await pool.connectMcpServer({ id: 's3', command: 'cmd3', args: [] });

    expect(pool.listServers().length).toBe(3);

    // Touch s1 so s2 becomes the least recently used
    const c1 = clients.get('s1')!;
    const c2 = clients.get('s2')!;
    const c3 = clients.get('s3')!;
    c2.lastUsed = 100;
    c1.lastUsed = 200;
    c3.lastUsed = 300;

    // Connect 4th server, which exceeds capacity of 3
    await pool.connectMcpServer({ id: 's4', command: 'cmd4', args: [] });

    // s2 should have been hibernated
    expect(c2.status).toBe('hibernating');
    expect(c2.mockRunning).toBe(false);

    // s4 and others should be connected
    expect(clients.get('s4')?.status).toBe('connected');
    expect(c1.status).toBe('connected');
    expect(c3.status).toBe('connected');

    // Calling a tool on hibernated server s2 automatically wakes it up
    c2.mockTools = [{ name: 'inspect', description: 'test' }];
    const res = await pool.callMcpTool('s2', 'inspect', { query: 'test' });
    expect(res).toEqual({ echo: 'inspect', args: { query: 'test' } });
    expect(c2.status).toBe('connected');
    expect(c2.mockRunning).toBe(true);
  });

  it('reuses existing connection when spec is identical', async () => {
    const pool = new McpServerPool({
      clientFactory: (config, opts) => new MockUnitClient(config, opts),
    });

    const first = await pool.connectMcpServer({ id: 'studio', command: 'node', args: ['server.js'] });
    const second = await pool.connectMcpServer({ id: 'studio', command: 'node', args: ['server.js'] });

    expect(second.session).toBe(first.session);
    expect(pool.listServers().length).toBe(1);
  });

  it('replaces existing server with fresh session when command or args change', async () => {
    const pool = new McpServerPool({
      clientFactory: (config, opts) => new MockUnitClient(config, opts),
    });

    const first = await pool.connectMcpServer({ id: 'studio', command: 'node', args: ['v1.js'] });
    const second = await pool.connectMcpServer({ id: 'studio', command: 'node', args: ['v2.js'] });

    expect(second.session).not.toBe(first.session);
    expect(pool.listServers().length).toBe(1);
  });
});

describe('McpServerPool Unit Tests: Crash Recovery & Circuit Breaker', () => {
  it('triggers circuit breaker when restart limit is exceeded in sliding window', async () => {
    let mockClient: MockUnitClient | null = null;
    const pool = new McpServerPool({
      maxRestarts: 3,
      restartWindowMs: 60_000,
      clientFactory: (config, opts) => {
        mockClient = new MockUnitClient(config, opts);
        return mockClient;
      },
    });

    await pool.connectMcpServer({ id: 'fragile', command: 'node', args: ['fragile.js'] });
    expect(mockClient).not.toBeNull();
    const client = mockClient!;

    // 1st crash
    client.simulateCrash();
    expect(client.status).toBe('reconnecting');

    // 2nd crash
    client.simulateCrash();
    expect(client.status).toBe('reconnecting');

    // 3rd crash
    client.simulateCrash();
    expect(client.status).toBe('reconnecting');

    // 4th crash trips circuit breaker
    client.simulateCrash();
    expect(client.status).toBe('failed');
    expect(client.error).toContain('Circuit breaker tripped');
  });
});

describe('McpServerPool Unit Tests: Tool Namespacing & Disambiguation', () => {
  it('formats namespaced tools and direct aliases within 64-char provider limits', async () => {
    const pool = new McpServerPool({
      clientFactory: (config, opts) => {
        const client = new MockUnitClient(config, opts);
        if (config.id === 'studio') {
          client.mockTools = [
            { name: 'execute_luau', description: 'Run code in Studio' },
            { name: 'inspect', description: 'Inspect instance' },
          ];
        } else if (config.id === 'blender') {
          client.mockTools = [
            { name: 'generate_mesh', description: 'Create 3D mesh' },
            { name: 'inspect', description: 'Inspect mesh' },
          ];
        }
        return client;
      },
    });

    await pool.connectMcpServer({ id: 'studio', command: 'studio-cmd', args: [] });
    await pool.connectMcpServer({ id: 'blender', command: 'blender-cmd', args: [] });

    const tools = pool.listAllAgentTools();
    expect(tools.length).toBe(4);

    // Unique tool on studio: gets both namespaced and direct alias
    const luau = tools.find((t) => t.tool.name === 'execute_luau');
    expect(luau?.agentToolName).toBe('mcp__studio__execute_luau');
    expect(luau?.directAgentToolName).toBe('mcp__execute_luau');

    // Unique tool on blender: gets both namespaced and direct alias
    const mesh = tools.find((t) => t.tool.name === 'generate_mesh');
    expect(mesh?.agentToolName).toBe('mcp__blender__generate_mesh');
    expect(mesh?.directAgentToolName).toBe('mcp__generate_mesh');

    // Colliding tool "inspect": neither gets direct alias
    const studioInspect = tools.find((t) => t.serverId === 'studio' && t.tool.name === 'inspect');
    const blenderInspect = tools.find((t) => t.serverId === 'blender' && t.tool.name === 'inspect');
    expect(studioInspect?.agentToolName).toBe('mcp__studio__inspect');
    expect(studioInspect?.directAgentToolName).toBeUndefined();
    expect(blenderInspect?.agentToolName).toBe('mcp__blender__inspect');
    expect(blenderInspect?.directAgentToolName).toBeUndefined();

    // Direct resolution of unambiguous tools works
    const resolvedLuau = pool.resolveAgentTool('mcp__execute_luau');
    expect(resolvedLuau?.serverIdentity.session).toContain('pool-studio-');

    // Namespaced resolution works for both
    const resolvedStudioInspect = pool.resolveAgentTool('mcp__studio__inspect');
    expect(resolvedStudioInspect?.serverIdentity.session).toContain('pool-studio-');

    const resolvedBlenderInspect = pool.resolveAgentTool('mcp__blender__inspect');
    expect(resolvedBlenderInspect?.serverIdentity.session).toContain('pool-blender-');

    // All tool names must be <= 64 chars and match [A-Za-z0-9_-]
    for (const item of tools) {
      expect(item.agentToolName.length).toBeLessThanOrEqual(64);
      expect(item.agentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
      if (item.directAgentToolName) {
        expect(item.directAgentToolName.length).toBeLessThanOrEqual(64);
        expect(item.directAgentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
      }
    }
  });

  it('throws an ambiguity error when calling an ambiguous direct tool name', async () => {
    const pool = new McpServerPool({
      clientFactory: (config, opts) => {
        const client = new MockUnitClient(config, opts);
        client.mockTools = [{ name: 'duplicate_tool', description: 'Duplicated' }];
        return client;
      },
    });

    await pool.connectMcpServer({ id: 'server1', command: 'cmd1', args: [] });
    await pool.connectMcpServer({ id: 'server2', command: 'cmd2', args: [] });

    // Calling the ambiguous tool name should fail with a helpful error
    expect(pool.resolveAgentTool('mcp__duplicate_tool')).toBeNull();
  });
});

describe('McpServerPool Unit Tests: Capacity Auto-Wake & Hibernated Tools Discovery', () => {
  it('evicts LRU server when auto-waking a hibernated server and preserves maxActiveServers limit of 6', async () => {
    const clients = new Map<string, MockUnitClient>();
    const pool = new McpServerPool({
      maxActiveServers: 6,
      clientFactory: (config, opts) => {
        const client = new MockUnitClient(config, opts);
        clients.set(config.id, client);
        return client;
      },
    });

    // 1. Connect 6 servers (s1 through s6) to saturate max capacity
    for (let i = 1; i <= 6; i++) {
      const id = `s${i}`;
      await pool.connectMcpServer({ id, command: 'node', args: [`${id}.js`] });
      const client = clients.get(id)!;
      client.lastUsed = 1000 * i; // s1 = 1000, s2 = 2000, ..., s6 = 6000
      client.mockTools = [{ name: `tool_${id}`, description: `Tool for ${id}` }];
    }

    expect(pool.getHealthReport().activeCount).toBe(6);

    // 2. Connect 7th server (s7). Capacity enforcement must hibernate s1 (oldest lastUsed = 1000)
    await pool.connectMcpServer({ id: 's7', command: 'node', args: ['s7.js'] });
    const client7 = clients.get('s7')!;
    client7.lastUsed = 7000;
    client7.mockTools = [{ name: 'tool_s7', description: 'Tool for s7' }];

    const client1 = clients.get('s1')!;
    const client2 = clients.get('s2')!;

    expect(client1.status).toBe('hibernating');
    expect(client1.running).toBe(false);
    expect(pool.getHealthReport().activeCount).toBe(6);

    // 3. Make s2 the least recently used active server
    client2.lastUsed = 2000;
    for (let i = 3; i <= 7; i++) {
      clients.get(`s${i}`)!.lastUsed = 10000 + i;
    }

    // 4. Trigger auto-wake on hibernated server s1 by calling its tool
    const result = await pool.callMcpTool('s1', 'tool_s1', { action: 'ping' });
    expect(result).toEqual({ echo: 'tool_s1', args: { action: 'ping' } });

    // 5. VERIFICATION:
    // s1 woke up to 'connected'
    expect(client1.status).toBe('connected');
    expect(client1.running).toBe(true);

    // s2 was evicted to 'hibernating'
    expect(client2.status).toBe('hibernating');
    expect(client2.running).toBe(false);

    // Total active server count MUST NOT EXCEED 6
    const healthAfterWake = pool.getHealthReport();
    expect(healthAfterWake.activeCount).toBe(6);

    // The active servers are s1, s3, s4, s5, s6, s7
    const activeViews = pool.listServerViews().filter((v) => v.running);
    expect(activeViews.length).toBe(6);
    const activeIds = activeViews.map((v) => v.id);
    expect(activeIds).toContain('s1');
    expect(activeIds).not.toContain('s2');
    expect(activeIds).toContain('s3');
    expect(activeIds).toContain('s4');
    expect(activeIds).toContain('s5');
    expect(activeIds).toContain('s6');
    expect(activeIds).toContain('s7');
  });

  it('preserves tool schemas for hibernated servers, enables agent discovery and auto-wakes upon execution', async () => {
    const approvals = new ApprovalQueue();
    const clients = new Map<string, MockUnitClient>();

    const pool = new McpServerPool({
      maxActiveServers: 2, // Low capacity to easily trigger hibernation
      clientFactory: (config, opts) => {
        const client = new MockUnitClient(config, opts);
        clients.set(config.id, client);
        return client;
      },
    });

    // 1. Connect studio and blender servers
    await pool.connectMcpServer({ id: 'studio', command: 'node', args: ['studio.js'] });
    await pool.connectMcpServer({ id: 'blender', command: 'node', args: ['blender.js'] });

    const studioClient = clients.get('studio')!;
    const blenderClient = clients.get('blender')!;

    studioClient.mockTools = [
      { name: 'execute_luau', description: 'Run Luau script', inputSchema: { type: 'object', properties: { code: { type: 'string' } } } },
    ];
    blenderClient.mockTools = [
      { name: 'render_scene', description: 'Render Blender scene', inputSchema: { type: 'object', properties: { samples: { type: 'number' } } } },
    ];

    studioClient.lastUsed = 1000;
    blenderClient.lastUsed = 2000;

    // 2. Connect 3rd server to force 'studio' (LRU) into hibernation
    await pool.connectMcpServer({ id: 'audio', command: 'node', args: ['audio.js'] });
    const audioClient = clients.get('audio')!;
    audioClient.mockTools = [{ name: 'generate_sound', description: 'Sound FX generator' }];

    expect(studioClient.status).toBe('hibernating');
    expect(studioClient.running).toBe(false);

    // 3. DISCOVERY VERIFICATION:
    // Tools on hibernated server 'studio' MUST STILL BE DISCOVERABLE
    const allAgentTools = pool.listAllAgentTools();
    const luauTool = allAgentTools.find((t) => t.tool.name === 'execute_luau');
    expect(luauTool).toBeDefined();
    expect(luauTool?.serverId).toBe('studio');
    expect(luauTool?.agentToolName).toBe('mcp__studio__execute_luau');

    // Schemas passed to provider API must contain execute_luau
    const schemas = pool.getAgentToolSchemas();
    const luauSchema = schemas.find((s) => s.name === 'mcp__studio__execute_luau');
    expect(luauSchema).toBeDefined();
    expect(luauSchema?.description).toContain('execute_luau');

    // 4. RESOLUTION & STAGING VERIFICATION:
    // resolveAgentTool must resolve execute_luau even though studio is hibernating
    const resolved = pool.resolveAgentTool('mcp__studio__execute_luau');
    expect(resolved).not.toBeNull();
    expect(resolved?.tool.name).toBe('execute_luau');
    expect(resolved?.serverIdentity).toBeDefined();

    // Stage tool call through approval queue
    const staged = stageMcpCall(approvals, pool, {
      projectRoot: '/test/project',
      agentName: 'mcp__studio__execute_luau',
      args: { code: 'print("waking up")' },
    });
    expect(staged.ok).toBe(true);
    expect(staged.pendingApproval).toBeDefined();

    // 5. EXECUTION & AUTO-WAKE VERIFICATION:
    const approvalItem = approvals.take(staged.pendingApproval!.id);
    const targetClient = pool.getClientForApproval(approvalItem.mcpServer);
    expect(targetClient).toBeDefined();

    // Executing the tool wakes the hibernated client
    const execResult = await runApprovedMcpCall(approvalItem, targetClient!);
    expect(execResult.ok).toBe(true);
    expect(execResult.output).toContain('execute_luau');

    // Verify studio is now running and connected
    expect(studioClient.status).toBe('connected');
    expect(studioClient.running).toBe(true);
  });
});

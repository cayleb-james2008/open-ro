import * as crypto from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import {
  McpPoolClient,
  McpServerPool,
  type McpPoolClientOptions,
  type McpServerConfig,
} from '../electron/mcpPool';
import {
  runApprovedMcpCall,
  runApprovedMcpServer,
  stageMcpCall,
  stageMcpPoolConnect,
} from '../electron/mcpTools';
import type { McpServerIdentity, McpToolSummary } from '../electron/types';

// ============================================================================
// Real Child Process MCP Server Script for Process Lifecycle Verification
// ============================================================================
const realServerScript = `
const readline = require('readline');
const [serverName] = process.argv.slice(2);

const tools = [
  {
    name: 'exec_' + serverName.replace(/[^a-zA-Z0-9_]/g, '_'),
    description: 'Execute tool on ' + serverName,
    inputSchema: { type: 'object', properties: { payload: { type: 'string' } } },
  },
  {
    name: 'status_check',
    description: 'Report status for ' + serverName,
  },
];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.method === 'initialize') {
      const res = {
        protocolVersion: '2024-11-05',
        capabilities: { tools: {} },
        serverInfo: { name: serverName, version: '1.0.0' },
      };
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: res }) + '\\n');
    } else if (msg.method === 'tools/list') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { tools } }) + '\\n');
    } else if (msg.method === 'ping') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: {} }) + '\\n');
    } else if (msg.method === 'tools/call') {
      const toolName = msg.params.name;
      const toolArgs = msg.params.arguments || {};
      const outputText = serverName + ':' + toolName + ':' + JSON.stringify(toolArgs);
      process.stdout.write(
        JSON.stringify({
          jsonrpc: '2.0',
          id: msg.id,
          result: { content: [{ type: 'text', text: outputText }] },
        }) + '\\n'
      );
    }
  } catch {}
});
`;

// ============================================================================
// Mock Client with High-Frequency Capacity Invariant Auditing
// ============================================================================
class InstrumentedMockClient extends McpPoolClient {
  public mockRunning = false;
  public mockTools: McpToolSummary[] = [];
  public mockIdentity: McpServerIdentity | null = null;
  public startCount = 0;
  public hibernateCount = 0;
  public stopCount = 0;
  public readonly clientOpts: McpPoolClientOptions;
  private onStatusChangeCallback?: () => void;

  constructor(
    config: McpServerConfig,
    options: McpPoolClientOptions = {},
    onStatusChange?: () => void,
  ) {
    super(config, options);
    this.clientOpts = options;
    this.onStatusChangeCallback = onStatusChange;
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
    this.startCount++;
    this.mockRunning = true;
    const isWaking = this.status === 'hibernating';
    const session =
      isWaking && this.mockIdentity
        ? this.mockIdentity.session
        : `pool-${this.config.id}-${crypto.randomUUID()}`;

    this.mockIdentity = {
      command: this.config.command,
      args: [...this.config.args],
      session,
      serverId: this.config.id,
    };
    (this as unknown as { currentSession: string }).currentSession = session;
    (this as unknown as { currentStatus: string }).currentStatus = 'connected';
    this.lastUsed = Date.now();
    this.onStatusChangeCallback?.();
  }

  override async hibernate(): Promise<void> {
    this.hibernateCount++;
    this.mockRunning = false;
    (this as unknown as { currentStatus: string }).currentStatus = 'hibernating';
    this.onStatusChangeCallback?.();
  }

  override async stop(): Promise<void> {
    this.stopCount++;
    this.mockRunning = false;
    this.mockIdentity = null;
    (this as unknown as { currentStatus: string }).currentStatus = 'disconnected';
    this.onStatusChangeCallback?.();
  }

  override async ping(): Promise<number> {
    this.lastUsed = Date.now();
    return 1;
  }

  override async callTool(name: string, args: Record<string, unknown>): Promise<string> {
    if (!this.mockRunning) {
      if (this.status === 'hibernating') {
        if (this.clientOpts.onBeforeWake) {
          await this.clientOpts.onBeforeWake(this);
        }
        await this.start();
      } else {
        throw new Error(`MCP server ${this.config.id} is not running.`);
      }
    }
    this.lastUsed = Date.now();
    return JSON.stringify({ serverId: this.config.id, tool: name, args });
  }
}

describe('Challenger 1 (Iteration 2): Multi-Server Approval Queue & Capacity Auto-Wake', () => {
  let tempDir = '';
  let scriptPath = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-it2-challenger1-'));
    scriptPath = path.join(tempDir, 'server.js');
    await writeFile(scriptPath, realServerScript, 'utf8');
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  // =========================================================================
  // Challenge 1: Multi-Server Approval Queue Connection
  // =========================================================================
  describe('Challenge 1: Multi-Server Approval Queue Connection', () => {
    it('stages 3 distinct servers, resolves all 3, and verifies concurrent execution with distinct IDs', async () => {
      const approvals = new ApprovalQueue();
      const pool = new McpServerPool({
        maxActiveServers: 6,
        clientFactory: (config, opts) => {
          const client = new InstrumentedMockClient(config, opts);
          client.mockTools = [
            { name: `tool_${config.id}`, description: `Tool for ${config.id}` },
          ];
          return client;
        },
      });

      // Step 1: Stage approvals for 3 distinct servers using stageMcpPoolConnect
      const pendingStudio = stageMcpPoolConnect(approvals, {
        id: 'studio-server',
        command: 'node',
        args: ['studio-srv.js'],
      });
      const pendingBlender = stageMcpPoolConnect(approvals, {
        id: 'blender-server',
        command: 'node',
        args: ['blender-srv.js'],
      });
      const pendingCustom = stageMcpPoolConnect(approvals, {
        id: 'custom-server',
        command: 'node',
        args: ['custom-srv.js'],
      });

      // Verify all 3 pending items are recorded in the queue
      const stagedList = approvals.list();
      expect(stagedList.length).toBe(3);

      expect(pendingStudio.kind).toBe('mcp-server');
      expect(pendingStudio.mcpServerId).toBe('studio-server');
      expect(pendingStudio.command).toBe('node');
      expect(pendingStudio.serverArgs).toEqual(['studio-srv.js']);

      expect(pendingBlender.kind).toBe('mcp-server');
      expect(pendingBlender.mcpServerId).toBe('blender-server');
      expect(pendingBlender.command).toBe('node');
      expect(pendingBlender.serverArgs).toEqual(['blender-srv.js']);

      expect(pendingCustom.kind).toBe('mcp-server');
      expect(pendingCustom.mcpServerId).toBe('custom-server');
      expect(pendingCustom.command).toBe('node');
      expect(pendingCustom.serverArgs).toEqual(['custom-srv.js']);

      // Step 2: Resolve all 3 approvals in arbitrary/mixed order
      const itemCustom = approvals.take(pendingCustom.id);
      const resCustom = await runApprovedMcpServer(itemCustom, pool);
      expect(resCustom.ok).toBe(true);
      expect(resCustom.summary).toContain('custom-server');

      const itemStudio = approvals.take(pendingStudio.id);
      const resStudio = await runApprovedMcpServer(itemStudio, pool);
      expect(resStudio.ok).toBe(true);
      expect(resStudio.summary).toContain('studio-server');

      const itemBlender = approvals.take(pendingBlender.id);
      const resBlender = await runApprovedMcpServer(itemBlender, pool);
      expect(resBlender.ok).toBe(true);
      expect(resBlender.summary).toContain('blender-server');

      // Approval queue is now empty
      expect(approvals.list().length).toBe(0);

      // Step 3: Verify all 3 servers are running concurrently in mcpPool, each with distinct ID
      const clientStudio = pool.getMcpClient('studio-server');
      const clientBlender = pool.getMcpClient('blender-server');
      const clientCustom = pool.getMcpClient('custom-server');

      expect(clientStudio).toBeDefined();
      expect(clientBlender).toBeDefined();
      expect(clientCustom).toBeDefined();

      expect(clientStudio?.running).toBe(true);
      expect(clientBlender?.running).toBe(true);
      expect(clientCustom?.running).toBe(true);

      // Verify sessions are distinct and contain proper server IDs
      expect(clientStudio?.identity?.session).toContain('pool-studio-server-');
      expect(clientBlender?.identity?.session).toContain('pool-blender-server-');
      expect(clientCustom?.identity?.session).toContain('pool-custom-server-');

      const sessions = new Set([
        clientStudio?.identity?.session,
        clientBlender?.identity?.session,
        clientCustom?.identity?.session,
      ]);
      expect(sessions.size).toBe(3);

      // Verify none has replaced or disconnected the others
      const listed = pool.listServers();
      expect(listed.length).toBe(3);
      const listedIds = listed.map((s) => s.id);
      expect(listedIds).toContain('studio-server');
      expect(listedIds).toContain('blender-server');
      expect(listedIds).toContain('custom-server');
      expect(listedIds).not.toContain('default');

      // Health report reflects all 3 running
      const health = pool.getHealthReport();
      expect(health.activeCount).toBe(3);
      expect(health.servers['studio-server'].status).toBe('connected');
      expect(health.servers['blender-server'].status).toBe('connected');
      expect(health.servers['custom-server'].status).toBe('connected');
    });

    it('handles multi-server approval queue with real child processes and verifies tool execution', async () => {
      const approvals = new ApprovalQueue();
      const pool = new McpServerPool({ maxActiveServers: 6 });

      try {
        // Stage 3 distinct servers spawning real node child processes
        const s1 = stageMcpPoolConnect(approvals, {
          id: 'studio-server',
          command: process.execPath,
          args: [scriptPath, 'studio-server'],
        });
        const s2 = stageMcpPoolConnect(approvals, {
          id: 'blender-server',
          command: process.execPath,
          args: [scriptPath, 'blender-server'],
        });
        const s3 = stageMcpPoolConnect(approvals, {
          id: 'custom-server',
          command: process.execPath,
          args: [scriptPath, 'custom-server'],
        });

        // Resolve in order
        for (const staged of [s1, s2, s3]) {
          const item = approvals.take(staged.id);
          const res = await runApprovedMcpServer(item, pool);
          expect(res.ok).toBe(true);
        }

        // All 3 real processes are running concurrently
        expect(pool.listServers().length).toBe(3);
        expect(pool.getMcpClient('studio-server')?.running).toBe(true);
        expect(pool.getMcpClient('blender-server')?.running).toBe(true);
        expect(pool.getMcpClient('custom-server')?.running).toBe(true);

        // Execute tools on all 3 concurrently
        const results = await Promise.all([
          pool.callMcpTool('studio-server', 'exec_studio_server', { a: 1 }),
          pool.callMcpTool('blender-server', 'exec_blender_server', { b: 2 }),
          pool.callMcpTool('custom-server', 'exec_custom_server', { c: 3 }),
        ]);

        expect(results[0]).toBe('studio-server:exec_studio_server:{"a":1}');
        expect(results[1]).toBe('blender-server:exec_blender_server:{"b":2}');
        expect(results[2]).toBe('custom-server:exec_custom_server:{"c":3}');
      } finally {
        await pool.disconnectAll();
      }
    });

    it('rejects approval cleanly without affecting other staged or running servers', async () => {
      const approvals = new ApprovalQueue();
      const pool = new McpServerPool({
        clientFactory: (config, opts) => new InstrumentedMockClient(config, opts),
      });

      const s1 = stageMcpPoolConnect(approvals, { id: 'srv-approved', command: 'node', args: ['srv.js'] });
      const s2 = stageMcpPoolConnect(approvals, { id: 'srv-rejected', command: 'node', args: ['srv.js'] });

      // Approve s1
      await runApprovedMcpServer(approvals.take(s1.id), pool);
      expect(pool.getMcpClient('srv-approved')?.running).toBe(true);

      // Reject s2
      const rejectedItem = approvals.take(s2.id);
      expect(rejectedItem.mcpServerId).toBe('srv-rejected');
      // No start call made for rejectedItem

      expect(pool.getMcpClient('srv-rejected')).toBeUndefined();
      expect(pool.getMcpClient('srv-approved')?.running).toBe(true);
      expect(pool.listServers().length).toBe(1);
    });
  });

  // =========================================================================
  // Challenge 2: Capacity Auto-Wake Enforcement (Strict Active Count Invariant)
  // =========================================================================
  describe('Challenge 2: Capacity Auto-Wake Enforcement', () => {
    it('connects 6 servers, connects 7th to hibernate oldest, auto-wakes hibernated server, and asserts active count NEVER exceeds 6', async () => {
      const clients = new Map<string, InstrumentedMockClient>();
      let maxObservedActiveCount = 0;
      let violationDetected = false;

      // Pool with capacity limit = 6
      const pool = new McpServerPool({
        maxActiveServers: 6,
        clientFactory: (config, opts) => {
          const client = new InstrumentedMockClient(config, opts, () => {
            // High-frequency assertion: on every start/hibernate/stop, audit active count
            const active = Array.from(clients.values()).filter(
              (c) => c.status === 'connected' || c.status === 'connecting',
            ).length;
            if (active > maxObservedActiveCount) {
              maxObservedActiveCount = active;
            }
            if (active > 6) {
              violationDetected = true;
            }
          });
          clients.set(config.id, client);
          return client;
        },
      });

      // Step 1: Connect 6 servers so pool is at capacity limit (maxActiveServers = 6)
      for (let i = 1; i <= 6; i++) {
        await pool.connectMcpServer({
          id: `server-${i}`,
          command: 'node',
          args: [`server-${i}.js`],
        });
        clients.get(`server-${i}`)!.lastUsed = 1000 * i;
      }

      expect(pool.getHealthReport().activeCount).toBe(6);
      expect(maxObservedActiveCount).toBe(6);
      expect(violationDetected).toBe(false);

      // Verify all 6 are running and connected
      for (let i = 1; i <= 6; i++) {
        expect(clients.get(`server-${i}`)?.status).toBe('connected');
        expect(clients.get(`server-${i}`)?.running).toBe(true);
      }

      // Step 2: Connect a 7th server, verify oldest (server-1) was hibernated
      await pool.connectMcpServer({
        id: 'server-7',
        command: 'node',
        args: ['server-7.js'],
      });
      clients.get('server-7')!.lastUsed = 7000;

      // Oldest was server-1 (lastUsed = 1000)
      const c1 = clients.get('server-1')!;
      expect(c1.status).toBe('hibernating');
      expect(c1.running).toBe(false);

      // Active count is strictly 6
      expect(pool.getHealthReport().activeCount).toBe(6);
      expect(maxObservedActiveCount).toBe(6);
      expect(violationDetected).toBe(false);

      // Step 3: Invoke a tool call on the hibernated server (server-1)
      // Before invoking, configure lastUsed such that server-2 is the LRU among active servers
      clients.get('server-2')!.lastUsed = 2000; // oldest active
      for (let i = 3; i <= 7; i++) {
        clients.get(`server-${i}`)!.lastUsed = 10000 + i;
      }

      const c2 = clients.get('server-2')!;
      expect(c2.status).toBe('connected');

      // Call tool on hibernated server-1
      const callResult = await pool.callMcpTool('server-1', 'compute', { x: 42 });
      expect(callResult).toEqual({
        serverId: 'server-1',
        tool: 'compute',
        args: { x: 42 },
      });

      // Step 4: VERIFICATIONS:
      // a) server-1 auto-wakes to connected
      expect(c1.status).toBe('connected');
      expect(c1.running).toBe(true);

      // b) server-2 was hibernated
      expect(c2.status).toBe('hibernating');
      expect(c2.running).toBe(false);

      // c) Active count NEVER exceeded 6 at ANY point during or after the wake
      expect(violationDetected).toBe(false);
      expect(maxObservedActiveCount).toBe(6);
      expect(pool.getHealthReport().activeCount).toBe(6);

      // Check views: exactly 6 running
      const runningViews = pool.listServerViews().filter((v) => v.running);
      expect(runningViews.length).toBe(6);
      const runningIds = runningViews.map((v) => v.id);
      expect(runningIds).toContain('server-1');
      expect(runningIds).not.toContain('server-2');
      expect(runningIds).toContain('server-3');
      expect(runningIds).toContain('server-4');
      expect(runningIds).toContain('server-5');
      expect(runningIds).toContain('server-6');
      expect(runningIds).toContain('server-7');
    });

    it('enforces capacity limit during auto-wake with REAL spawned child processes', async () => {
      const pool = new McpServerPool({
        maxActiveServers: 6,
        heartbeatIntervalMs: 15_000,
        heartbeatTimeoutMs: 3_000,
      });

      try {
        // Connect 6 real processes
        for (let i = 1; i <= 6; i++) {
          await pool.connectMcpServer({
            id: `real-${i}`,
            command: process.execPath,
            args: [scriptPath, `real-${i}`],
          });
          const client = pool.getMcpClient(`real-${i}`)!;
          client.lastUsed = 1000 * i;
        }

        expect(pool.getHealthReport().activeCount).toBe(6);

        // Connect 7th real process: real-1 (LRU) must be evicted to hibernating
        await pool.connectMcpServer({
          id: 'real-7',
          command: process.execPath,
          args: [scriptPath, 'real-7'],
        });

        const v1 = pool.getServerView('real-1');
        expect(v1?.status).toBe('hibernating');
        expect(v1?.running).toBe(false);
        expect(pool.getHealthReport().activeCount).toBe(6);

        // Make real-2 the LRU of the remaining active servers
        const c2 = pool.getMcpClient('real-2')!;
        c2.lastUsed = 2000;
        for (let i = 3; i <= 7; i++) {
          pool.getMcpClient(`real-${i}`)!.lastUsed = 20000 + i;
        }

        // Call tool on hibernated real-1 to trigger auto-wake
        const toolRes = await pool.callMcpTool('real-1', 'exec_real_1', { data: 'test-wake' });
        expect(toolRes).toBe('real-1:exec_real_1:{"data":"test-wake"}');

        // real-1 is now connected
        const v1After = pool.getServerView('real-1');
        expect(v1After?.status).toBe('connected');
        expect(v1After?.running).toBe(true);

        // real-2 was evicted to hibernating
        const v2After = pool.getServerView('real-2');
        expect(v2After?.status).toBe('hibernating');
        expect(v2After?.running).toBe(false);

        // Total active count strictly 6
        expect(pool.getHealthReport().activeCount).toBe(6);
      } finally {
        await pool.disconnectAll();
      }
    }, 25_000);

    it('serializes concurrent auto-wake bursts without race conditions or capacity overflows', async () => {
      const clients = new Map<string, InstrumentedMockClient>();
      let maxActiveObserved = 0;
      let capacityOverflow = false;

      const pool = new McpServerPool({
        maxActiveServers: 6,
        clientFactory: (config, opts) => {
          const client = new InstrumentedMockClient(config, opts, () => {
            const count = Array.from(clients.values()).filter(
              (c) => c.status === 'connected' || c.status === 'connecting',
            ).length;
            if (count > maxActiveObserved) maxActiveObserved = count;
            if (count > 6) capacityOverflow = true;
          });
          clients.set(config.id, client);
          return client;
        },
      });

      // Connect 8 servers: s1 to s8
      for (let i = 1; i <= 8; i++) {
        await pool.connectMcpServer({ id: `s${i}`, command: 'node', args: [`s${i}.js`] });
        clients.get(`s${i}`)!.lastUsed = 1000 * i;
      }

      // 6 active, s1 and s2 hibernated
      expect(pool.getHealthReport().activeCount).toBe(6);
      expect(clients.get('s1')?.status).toBe('hibernating');
      expect(clients.get('s2')?.status).toBe('hibernating');

      // Set access times for s3..s8
      clients.get('s3')!.lastUsed = 3000;
      clients.get('s4')!.lastUsed = 4000;
      clients.get('s5')!.lastUsed = 5000;
      clients.get('s6')!.lastUsed = 6000;
      clients.get('s7')!.lastUsed = 7000;
      clients.get('s8')!.lastUsed = 8000;

      // Burst: Call tools on BOTH hibernated servers (s1 and s2) simultaneously
      const [r1, r2] = await Promise.all([
        pool.callMcpTool('s1', 'tool_s1', { req: 1 }),
        pool.callMcpTool('s2', 'tool_s2', { req: 2 }),
      ]);

      expect(r1).toEqual({ serverId: 's1', tool: 'tool_s1', args: { req: 1 } });
      expect(r2).toEqual({ serverId: 's2', tool: 'tool_s2', args: { req: 2 } });

      // Both s1 and s2 are now active
      expect(clients.get('s1')?.status).toBe('connected');
      expect(clients.get('s2')?.status).toBe('connected');

      // The two LRU servers among s3..s8 (s3 and s4) must have been evicted to hibernating
      expect(clients.get('s3')?.status).toBe('hibernating');
      expect(clients.get('s4')?.status).toBe('hibernating');

      // Active count must remain strictly <= 6 throughout
      expect(capacityOverflow).toBe(false);
      expect(maxActiveObserved).toBe(6);
      expect(pool.getHealthReport().activeCount).toBe(6);
    });

    it('stages a tool call on a hibernated server, approves it, auto-wakes with preserved session UUID, and executes tool', async () => {
      const approvals = new ApprovalQueue();
      const clients = new Map<string, InstrumentedMockClient>();

      const pool = new McpServerPool({
        maxActiveServers: 2, // low capacity limit to test auto-wake
        clientFactory: (config, opts) => {
          const client = new InstrumentedMockClient(config, opts);
          clients.set(config.id, client);
          return client;
        },
      });

      // Connect serverA and serverB
      await pool.connectMcpServer({ id: 'serverA', command: 'node', args: ['serverA.js'] });
      await pool.connectMcpServer({ id: 'serverB', command: 'node', args: ['serverB.js'] });

      const cA = clients.get('serverA')!;
      const cB = clients.get('serverB')!;
      cA.mockTools = [{ name: 'run_task', description: 'Run serverA task' }];
      cB.mockTools = [{ name: 'render', description: 'Render task' }];

      cA.lastUsed = 100;
      cB.lastUsed = 200;

      // Connect serverC to evict serverA (LRU) into hibernation
      await pool.connectMcpServer({ id: 'serverC', command: 'node', args: ['serverC.js'] });
      expect(cA.status).toBe('hibernating');
      expect(cA.running).toBe(false);

      // Verify tools on hibernated serverA are still discoverable by the agent
      const tools = pool.listAllAgentTools();
      const stagedTool = tools.find((t) => t.tool.name === 'run_task');
      expect(stagedTool).toBeDefined();
      expect(stagedTool?.serverId).toBe('serverA');
      expect(stagedTool?.agentToolName).toBe('mcp__serverA__run_task');

      // Stage tool call while serverA is hibernating
      const stageRes = stageMcpCall(approvals, pool, {
        projectRoot: '/test',
        agentName: 'mcp__serverA__run_task',
        args: { param: 'test-val' },
      });
      expect(stageRes.ok).toBe(true);
      expect(stageRes.pendingApproval).toBeDefined();

      const item = approvals.take(stageRes.pendingApproval!.id);
      expect(item.mcpTool).toBe('run_task');

      // Resolve approval using the electron/main.ts resolution flow:
      const targetClient = pool.getClientForApproval(item.mcpServer);
      expect(targetClient).toBeDefined();
      expect(targetClient?.config.id).toBe('serverA');

      if (targetClient && targetClient.status === 'hibernating') {
        await pool.wakeServer(targetClient.config.id);
      }

      // Execute approved call
      const execRes = await runApprovedMcpCall(item, targetClient!);
      expect(execRes.ok).toBe(true);
      expect(execRes.output).toContain('run_task');

      // serverA is woken and connected
      expect(cA.status).toBe('connected');
      expect(cA.running).toBe(true);

      // Capacity invariant preserved: active count is 2 (serverB was LRU so evicted, or serverC)
      expect(pool.getHealthReport().activeCount).toBe(2);
    });
  });
});

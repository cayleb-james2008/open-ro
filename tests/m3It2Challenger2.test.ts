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
  extractServerIdFromIdentity,
  extractServerIdFromSession,
  runApprovedMcpCall,
  sameServer,
  stageMcpCall,
} from '../electron/mcpTools';
import type { McpServerIdentity, McpToolSummary } from '../electron/types';

// ============================================================================
// Real Child Process Server Script with 'echo' and 'calc' Tools
// ============================================================================
const mathEchoServerScript = `
const readline = require('readline');
const [serverName] = process.argv.slice(2);

const tools = [
  {
    name: 'echo',
    description: 'Echo back input payload',
    inputSchema: { type: 'object', properties: { text: { type: 'string' } } },
  },
  {
    name: 'calc',
    description: 'Perform arithmetic calculation',
    inputSchema: { type: 'object', properties: { expr: { type: 'string' }, a: { type: 'number' }, b: { type: 'number' } } },
  },
];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.method === 'initialize') {
      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0',
        id: msg.id,
        result: {
          protocolVersion: '2024-11-05',
          capabilities: { tools: {} },
          serverInfo: { name: serverName, version: '1.0.0' },
        },
      }) + '\\n');
    } else if (msg.method === 'notifications/initialized') {
      // acknowledged
    } else if (msg.method === 'tools/list') {
      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0',
        id: msg.id,
        result: { tools },
      }) + '\\n');
    } else if (msg.method === 'tools/call') {
      const toolName = msg.params.name;
      const args = msg.params.arguments || {};
      let outText = '';
      if (toolName === 'echo') {
        outText = serverName + ': ' + (args.text || '');
      } else if (toolName === 'calc') {
        const a = Number(args.a) || 0;
        const b = Number(args.b) || 0;
        outText = serverName + ' result: ' + (a + b);
      } else {
        outText = 'unknown tool';
      }
      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0',
        id: msg.id,
        result: { content: [{ type: 'text', text: outText }] },
      }) + '\\n');
    } else if (msg.method === 'ping') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: {} }) + '\\n');
    }
  } catch (err) {}
});
`;

// ============================================================================
// Mock Client for In-Memory Deterministic Capacity & Auto-Wake Stress
// ============================================================================
class MockEmpiricalClient extends McpPoolClient {
  public mockRunning = false;
  public mockTools: McpToolSummary[] = [];
  public mockIdentity: McpServerIdentity | null = null;
  public startCalls = 0;
  public hibernateCalls = 0;
  public stopCalls = 0;
  public readonly unitOptions: McpPoolClientOptions;

  constructor(config: McpServerConfig, options: McpPoolClientOptions = {}) {
    super(config, options);
    this.unitOptions = options;
  }

  override get running(): boolean {
    return this.mockRunning;
  }

  override get identity(): McpServerIdentity | null {
    if (this.mockRunning || (this.status === 'hibernating' && this.mockIdentity)) {
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
    const session = isWaking && this.mockIdentity
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
  }

  override async stop(): Promise<void> {
    this.stopCalls++;
    this.mockRunning = false;
    this.mockIdentity = null;
    (this as unknown as { currentStatus: string }).currentStatus = 'disconnected';
  }

  override async hibernate(): Promise<void> {
    this.hibernateCalls++;
    this.mockRunning = false;
    (this as unknown as { currentStatus: string }).currentStatus = 'hibernating';
  }

  override async ping(): Promise<number> {
    this.lastUsed = Date.now();
    return 1;
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
    return JSON.stringify({ server: this.config.id, tool: name, args });
  }
}

describe('Challenger 2 Empirical Stress Test: Hibernated Tool Discovery & Hyphenated Server IDs', () => {
  let tempDir = '';
  let scriptPath = '';
  let pool: McpServerPool;
  let approvals: ApprovalQueue;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-m3it2-c2-'));
    scriptPath = path.join(tempDir, 'math-echo.js');
    await writeFile(scriptPath, mathEchoServerScript, 'utf8');
    pool = new McpServerPool();
    approvals = new ApprovalQueue();
  });

  afterEach(async () => {
    await pool.disconnectAll();
    await rm(tempDir, { recursive: true, force: true });
  });

  // ==========================================================================
  // Section 1: Hibernated Tool Discovery & Transparent Execution
  // ==========================================================================
  describe('Mission 1: Hibernated Tool Discovery & Transparent Auto-Wake Execution', () => {
    it('discovers tools on a hibernated real server, stages a call, auto-wakes via wakeServer, and verifies sameServer passes', async () => {
      // 1. Connect a real child process server exposing echo and calc
      const serverInfo = await pool.connectMcpServer({
        id: 'math-server',
        command: process.execPath,
        args: [scriptPath, 'math-server'],
      });

      expect(serverInfo.tools.length).toBe(2);
      const client = pool.getMcpClient('math-server') as McpPoolClient;
      expect(client).toBeDefined();
      expect(client.running).toBe(true);

      // Verify active tool schemas and discovery
      const initialTools = pool.listAllAgentTools();
      expect(initialTools.length).toBe(2);
      expect(initialTools.some((t) => t.tool.name === 'echo')).toBe(true);
      expect(initialTools.some((t) => t.tool.name === 'calc')).toBe(true);

      const initialSchemas = pool.getAgentToolSchemas();
      expect(initialSchemas.some((s) => s.name === 'mcp__math_server__echo' || s.name === 'mcp__echo')).toBe(true);
      expect(initialSchemas.some((s) => s.name === 'mcp__math_server__calc' || s.name === 'mcp__calc')).toBe(true);

      const sessionBeforeHibernate = client.session;
      const identityBeforeHibernate = client.identity;
      expect(sessionBeforeHibernate).toBeTruthy();

      // 2. Hibernate the server
      await client.hibernate();
      expect(client.status).toBe('hibernating');
      expect(client.running).toBe(false);

      // 3. Verify mcp.listAllAgentTools() and mcp.getAgentToolSchemas() continue to advertise tools
      const hibernatedTools = pool.listAllAgentTools();
      expect(hibernatedTools.length).toBe(2);
      const echoTool = hibernatedTools.find((t) => t.tool.name === 'echo');
      const calcTool = hibernatedTools.find((t) => t.tool.name === 'calc');
      expect(echoTool).toBeDefined();
      expect(echoTool?.serverId).toBe('math-server');
      expect(calcTool).toBeDefined();
      expect(calcTool?.serverId).toBe('math-server');

      const hibernatedSchemas = pool.getAgentToolSchemas();
      expect(hibernatedSchemas.length).toBeGreaterThanOrEqual(2);
      expect(hibernatedSchemas.some((s) => s.name.includes('echo'))).toBe(true);
      expect(hibernatedSchemas.some((s) => s.name.includes('calc'))).toBe(true);

      // Verify resolveAgentTool resolves while hibernating
      const resolvedCalc = pool.resolveAgentTool('mcp__math-server__calc');
      expect(resolvedCalc).not.toBeNull();
      expect(resolvedCalc?.tool.name).toBe('calc');
      expect(resolvedCalc?.serverIdentity.session).toBe(sessionBeforeHibernate);

      // 4. Stage a tool call on the hibernated server's tool
      const stageRes = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__math-server__calc',
        args: { a: 15, b: 27 },
      });
      expect(stageRes.ok).toBe(true);
      expect(stageRes.pendingApproval).toBeDefined();

      const approvalItem = approvals.take(stageRes.pendingApproval!.id);
      expect(approvalItem.kind).toBe('mcp');
      expect(approvalItem.mcpTool).toBe('calc');
      expect(approvalItem.mcpServer?.session).toBe(sessionBeforeHibernate);

      // 5. Approve the tool call: execute via the application path (wakeServer + runApprovedMcpCall)
      const targetClient = pool.getClientForApproval(approvalItem.mcpServer);
      expect(targetClient).toBeDefined();
      expect(targetClient?.serverId).toBe('math-server');
      expect(targetClient?.status).toBe('hibernating');

      // Mimic electron/main.ts resolveApproval flow
      if (targetClient && targetClient.status === 'hibernating') {
        await pool.wakeServer(targetClient.serverId);
      }

      // Verify sameServer returns true after wake
      expect(targetClient?.running).toBe(true);
      expect(targetClient?.session).toBe(sessionBeforeHibernate);
      expect(sameServer(approvalItem.mcpServer, targetClient?.identity ?? null)).toBe(true);

      const runResult = await runApprovedMcpCall(approvalItem, targetClient!);
      expect(runResult.ok).toBe(true);
      expect(runResult.output).toBe('math-server result: 42');

      // Subsequent call on 'echo' executes cleanly without re-waking
      const stageEcho = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__math-server__echo',
        args: { text: 'verification successful' },
      });
      expect(stageEcho.ok).toBe(true);
      const echoApprovalItem = approvals.take(stageEcho.pendingApproval!.id);
      const echoRunResult = await runApprovedMcpCall(echoApprovalItem, targetClient!);
      expect(echoRunResult.ok).toBe(true);
      expect(echoRunResult.output).toBe('math-server: verification successful');
    });

    it('demonstrates that runApprovedMcpCall directly auto-waking a client executes the tool with preserved session UUID', async () => {
      // Test direct execution where runApprovedMcpCall triggers line 109 auto-wake
      await pool.connectMcpServer({
        id: 'math-direct',
        command: process.execPath,
        args: [scriptPath, 'math-direct'],
      });
      const client = pool.getMcpClient('math-direct') as McpPoolClient;
      await client.hibernate();
      expect(client.status).toBe('hibernating');

      const stageRes = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__math-direct__echo',
        args: { text: 'direct auto-wake' },
      });
      expect(stageRes.ok).toBe(true);
      const item = approvals.take(stageRes.pendingApproval!.id);

      // Directly calling runApprovedMcpCall without prior wakeServer
      const runResult = await runApprovedMcpCall(item, client);
      expect(runResult.ok).toBe(true);
      expect(runResult.output).toBe('math-direct: direct auto-wake');
      expect(client.running).toBe(true);
    });

    it('EMPIRICAL BUG DETECTION: direct runApprovedMcpCall on hibernated client bypasses capacity enforcement', async () => {
      // Build a pool with capacity = 2 using MockEmpiricalClient
      const mockClients = new Map<string, MockEmpiricalClient>();
      const smallPool = new McpServerPool({
        maxActiveServers: 2,
        clientFactory: (cfg, opts) => {
          const c = new MockEmpiricalClient(cfg, opts);
          mockClients.set(cfg.id, c);
          return c;
        },
      });

      // 1. Connect s1 and s2 (active count = 2)
      await smallPool.connectMcpServer({ id: 's1', command: 'node', args: ['s1.js'] });
      await smallPool.connectMcpServer({ id: 's2', command: 'node', args: ['s2.js'] });
      mockClients.get('s1')!.mockTools = [{ name: 'task1', description: 't1' }];
      mockClients.get('s2')!.mockTools = [{ name: 'task2', description: 't2' }];

      mockClients.get('s1')!.lastUsed = 100;
      mockClients.get('s2')!.lastUsed = 200;

      // 2. Connect s3 -> s1 is evicted to hibernating
      await smallPool.connectMcpServer({ id: 's3', command: 'node', args: ['s3.js'] });
      mockClients.get('s3')!.mockTools = [{ name: 'task3', description: 't3' }];

      expect(mockClients.get('s1')!.status).toBe('hibernating');
      expect(smallPool.getHealthReport().activeCount).toBe(2);

      // 3. Stage call for hibernated s1
      const stageRes = stageMcpCall(approvals, smallPool, {
        projectRoot: '/test',
        agentName: 'mcp__s1__task1',
        args: {},
      });
      const item = approvals.take(stageRes.pendingApproval!.id);
      const targetClient = smallPool.getClientForApproval(item.mcpServer)!;

      // 4. If runApprovedMcpCall(item, targetClient) is called directly (as done by Challenger 1):
      // In lines 109-111 of electron/mcpTools.ts, it calls statusHolder.start().
      // This bypasses pool.enforceCapacity(targetClient.config.id)!
      await runApprovedMcpCall(item, targetClient);

      // EMPIRICAL OBSERVATION:
      // When woken via runApprovedMcpCall directly, activeCount is 3, which exceeds maxActive of 2!
      const activeCountAfterDirectRun = smallPool.getHealthReport().activeCount;
      // This confirms Challenger 1's test failure: direct runApprovedMcpCall causes activeCount = 3 > 2.
      expect(activeCountAfterDirectRun).toBe(3);

      await smallPool.disconnectAll();
    });
  });

  // ==========================================================================
  // Section 2: Hyphenated Server ID Disambiguation
  // ==========================================================================
  describe('Mission 2: Hyphenated Server ID Disambiguation & Cross-Server Isolation', () => {
    it('connects studio-mcp-roblox, studio-mcp-blender, and studio and verifies getClientForApproval resolves each accurately without prefix collision', async () => {
      // Connect all 3 servers
      await pool.connectMcpServer({ id: 'studio', command: process.execPath, args: [scriptPath, 'studio'] });
      await pool.connectMcpServer({ id: 'studio-mcp-roblox', command: process.execPath, args: [scriptPath, 'studio-mcp-roblox'] });
      await pool.connectMcpServer({ id: 'studio-mcp-blender', command: process.execPath, args: [scriptPath, 'studio-mcp-blender'] });

      const clientStudio = pool.getMcpClient('studio')!;
      const clientRoblox = pool.getMcpClient('studio-mcp-roblox')!;
      const clientBlender = pool.getMcpClient('studio-mcp-blender')!;

      expect(clientStudio).toBeDefined();
      expect(clientRoblox).toBeDefined();
      expect(clientBlender).toBeDefined();

      // Check sessions and identities
      const idStudio = clientStudio.identity!;
      const idRoblox = clientRoblox.identity!;
      const idBlender = clientBlender.identity!;

      expect(idStudio.serverId).toBe('studio');
      expect(idRoblox.serverId).toBe('studio-mcp-roblox');
      expect(idBlender.serverId).toBe('studio-mcp-blender');

      expect(idStudio.session).toMatch(/^pool-studio-[0-9a-fA-F-]{36}$/);
      expect(idRoblox.session).toMatch(/^pool-studio-mcp-roblox-[0-9a-fA-F-]{36}$/);
      expect(idBlender.session).toMatch(/^pool-studio-mcp-blender-[0-9a-fA-F-]{36}$/);

      // Verify getClientForApproval with full identity (canonical serverId present)
      const resolvedStudio = pool.getClientForApproval(idStudio);
      const resolvedRoblox = pool.getClientForApproval(idRoblox);
      const resolvedBlender = pool.getClientForApproval(idBlender);

      expect(resolvedStudio).toBe(clientStudio);
      expect(resolvedRoblox).toBe(clientRoblox);
      expect(resolvedBlender).toBe(clientBlender);

      // STRESS TEST: Verify getClientForApproval when serverId is stripped (session-only parsing fallback)
      const legacyIdStudio: McpServerIdentity = { command: idStudio.command, args: idStudio.args, session: idStudio.session };
      const legacyIdRoblox: McpServerIdentity = { command: idRoblox.command, args: idRoblox.args, session: idRoblox.session };
      const legacyIdBlender: McpServerIdentity = { command: idBlender.command, args: idBlender.args, session: idBlender.session };

      const resolvedLegacyStudio = pool.getClientForApproval(legacyIdStudio);
      const resolvedLegacyRoblox = pool.getClientForApproval(legacyIdRoblox);
      const resolvedLegacyBlender = pool.getClientForApproval(legacyIdBlender);

      expect(resolvedLegacyStudio).toBe(clientStudio);
      expect(resolvedLegacyRoblox).toBe(clientRoblox);
      expect(resolvedLegacyBlender).toBe(clientBlender);
    });

    it('verifies sameServer prevents cross-server execution between studio-mcp-roblox and studio-mcp-blender', () => {
      const robloxIdentity: McpServerIdentity = {
        command: process.execPath,
        args: ['math-echo.js', 'studio-mcp-roblox'],
        session: 'pool-studio-mcp-roblox-11111111-2222-3333-4444-555555555555',
        serverId: 'studio-mcp-roblox',
      };
      const blenderIdentity: McpServerIdentity = {
        command: process.execPath,
        args: ['math-echo.js', 'studio-mcp-blender'],
        session: 'pool-studio-mcp-blender-66666666-7777-8888-9999-000000000000',
        serverId: 'studio-mcp-blender',
      };
      const studioIdentity: McpServerIdentity = {
        command: process.execPath,
        args: ['math-echo.js', 'studio'],
        session: 'pool-studio-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
        serverId: 'studio',
      };

      // 1. Same server with exact match passes
      expect(sameServer(robloxIdentity, robloxIdentity)).toBe(true);
      expect(sameServer(blenderIdentity, blenderIdentity)).toBe(true);
      expect(sameServer(studioIdentity, studioIdentity)).toBe(true);

      // 2. Cross-server between studio-mcp-roblox and studio-mcp-blender MUST be rejected
      expect(sameServer(robloxIdentity, blenderIdentity)).toBe(false);
      expect(sameServer(blenderIdentity, robloxIdentity)).toBe(false);

      // 3. Cross-server between studio and studio-mcp-roblox MUST be rejected
      expect(sameServer(studioIdentity, robloxIdentity)).toBe(false);
      expect(sameServer(robloxIdentity, studioIdentity)).toBe(false);

      // 4. Staged with studio-mcp-roblox but malicious current client claiming same command/args
      const forgedRobloxWithBlenderSession: McpServerIdentity = {
        command: robloxIdentity.command,
        args: robloxIdentity.args,
        session: blenderIdentity.session,
        serverId: 'studio-mcp-roblox',
      };
      expect(sameServer(robloxIdentity, forgedRobloxWithBlenderSession)).toBe(false);

      // 5. Fallback session-only parsing (when serverId is omitted):
      // Prefix collision stress: 'studio' vs 'studio-mcp-roblox'
      const sessionOnlyRoblox: McpServerIdentity = {
        command: 'node',
        args: ['server.js'],
        session: 'pool-studio-mcp-roblox-11111111-2222-3333-4444-555555555555',
      };
      const sessionOnlyStudio: McpServerIdentity = {
        command: 'node',
        args: ['server.js'],
        session: 'pool-studio-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
      };
      expect(sameServer(sessionOnlyRoblox, sessionOnlyStudio)).toBe(false);
      expect(sameServer(sessionOnlyStudio, sessionOnlyRoblox)).toBe(false);
    });

    it('prevents cross-server execution in full staging and approval pipeline across hyphenated servers', async () => {
      await pool.connectMcpServer({ id: 'studio-mcp-roblox', command: process.execPath, args: [scriptPath, 'roblox'] });
      await pool.connectMcpServer({ id: 'studio-mcp-blender', command: process.execPath, args: [scriptPath, 'blender'] });

      const robloxClient = pool.getMcpClient('studio-mcp-roblox')!;
      const blenderClient = pool.getMcpClient('studio-mcp-blender')!;

      // Stage tool call intended for studio-mcp-roblox
      const stageRes = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__studio-mcp-roblox__echo',
        args: { text: 'should only run on roblox' },
      });
      expect(stageRes.ok).toBe(true);

      const stagedItem = approvals.take(stageRes.pendingApproval!.id);

      // Attempt to execute on blenderClient (cross-server attack)
      const crossResult = await runApprovedMcpCall(stagedItem, blenderClient);
      expect(crossResult.ok).toBe(false);
      expect(crossResult.summary).toContain('MCP server changed after echo was staged');

      // Executing on the legitimate robloxClient succeeds
      const legitResult = await runApprovedMcpCall(stagedItem, robloxClient);
      expect(legitResult.ok).toBe(true);
      expect(legitResult.output).toBe('roblox: should only run on roblox');
    });

    it('extractServerIdFromSession fuzzer: handles arbitrary multi-hyphenated server IDs and rejects malformed strings', () => {
      const validCases = [
        { session: 'pool-a-b-c-d-e2b2f67c-1793-4a11-80a1-4ce8d5381fbc', expected: 'a-b-c-d' },
        { session: 'pool-studio-mcp-roblox-editor-00000000-0000-0000-0000-000000000000', expected: 'studio-mcp-roblox-editor' },
        { session: 'pool-x-y-11111111-2222-3333-4444-555555555555', expected: 'x-y' },
        { session: 'pool-singleword-e2b2f67c-1793-4a11-80a1-4ce8d5381fbc', expected: 'singleword' },
        // Token format fallback
        { session: 'pool-studio-mcp-token1', expected: 'studio-mcp' },
        { session: 'pool-server1-sessionX', expected: 'server1' },
      ];

      for (const { session, expected } of validCases) {
        expect(extractServerIdFromSession(session)).toBe(expected);
      }

      const invalidCases = [
        'invalid-prefix-session',
        'pool-',
        'pool-no-trailing-token-', // trailing hyphen
        '',
        null,
        undefined,
      ];

      for (const invalid of invalidCases) {
        expect(extractServerIdFromSession(invalid as unknown as string)).toBeUndefined();
      }
    });
  });
});

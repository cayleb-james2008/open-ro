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
  type McpServerPoolEvent,
} from '../electron/mcpPool';
import {
  MCP_TOOL_PREFIX,
  runApprovedMcpCall,
  sameServer,
  stageMcpCall,
} from '../electron/mcpTools';
import type { McpServerIdentity, McpToolSummary } from '../electron/types';

/**
 * Adversarial Server Script for Real Child Process Tests.
 * Features:
 * - Configurable crash on specific tool call
 * - Configurable hang on 'ping' (for deadlock & heartbeat testing)
 * - Exposes configurable tools including long names & colliding names
 */
const adversarialServerScript = `
const readline = require('readline');
const [serverRole, hangOnPingArg] = process.argv.slice(2);
const shouldHangOnPing = hangOnPingArg === 'true';

let tools = [];
if (serverRole === 'serverA') {
  tools = [
    { name: 'execute', description: 'Server A execute tool' },
    { name: 'search', description: 'Server A search tool' },
    { name: 'a_very_long_tool_name_designed_to_test_provider_limits_and_namespacing_boundary_conditions_99_chars', description: 'Long tool A' },
    { name: 'special.chars/tool:name@v1', description: 'Special chars tool' },
    { name: 'crash_me', description: 'Triggers process exit' },
  ];
} else if (serverRole === 'serverB') {
  tools = [
    { name: 'execute', description: 'Server B execute tool' },
    { name: 'search', description: 'Server B search tool' },
    { name: 'b_unique_tool', description: 'Server B unique tool' },
  ];
} else if (serverRole === 'hang_on_ping') {
  tools = [
    { name: 'echo', description: 'Echo tool' },
  ];
}

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.method === 'initialize') {
      const res = {
        protocolVersion: '2024-11-05',
        capabilities: { tools: {} },
        serverInfo: { name: serverRole, version: '1.0.0' },
      };
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: res }) + '\\n');
    } else if (msg.method === 'tools/list') {
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { tools } }) + '\\n');
    } else if (msg.method === 'ping') {
      if (shouldHangOnPing || serverRole === 'hang_on_ping') {
        // Intentionally hang: never answer ping
        return;
      }
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: {} }) + '\\n');
    } else if (msg.method === 'tools/call') {
      if (msg.params.name === 'crash_me') {
        process.exit(1);
      }
      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0',
        id: msg.id,
        result: { content: [{ type: 'text', text: serverRole + ' executed ' + msg.params.name }] },
      }) + '\\n');
    }
  } catch {}
});
`;

describe('Milestone 3 Challenger 2: Adversarial Session & Invalidation Stress Harness', () => {
  let tempDir: string;
  let scriptPath: string;
  let pool: McpServerPool;
  let approvals: ApprovalQueue;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-m3-challenger2-'));
    scriptPath = path.join(tempDir, 'adversarialServer.js');
    await writeFile(scriptPath, adversarialServerScript, 'utf8');
    pool = new McpServerPool({
      heartbeatIntervalMs: 10_000,
      heartbeatTimeoutMs: 3_000,
    });
    approvals = new ApprovalQueue();
  });

  afterEach(async () => {
    await pool.disconnectAll();
    await rm(tempDir, { recursive: true, force: true });
  });

  // =========================================================================
  // TEST 1: Session UUID Cross-Injection Attack
  // =========================================================================
  describe('Test 1: Session UUID cross-injection attack & security binding', () => {
    it('refuses execution when staged approval for Server A is passed to Server B', async () => {
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
      });
      await pool.connectMcpServer({
        id: 'serverB',
        command: process.execPath,
        args: [scriptPath, 'serverB'],
      });

      const stagedA = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__serverA__execute',
        args: { param: 'cross-inject-test' },
      });
      expect(stagedA.ok).toBe(true);

      const approvalItem = approvals.take(stagedA.pendingApproval!.id);
      expect(approvalItem.mcpServer?.session).toContain('pool-serverA-');

      const clientB = pool.getMcpClient('serverB')!;
      expect(clientB).toBeDefined();

      // sameServer MUST evaluate false between Server A staged identity and Server B current identity
      expect(sameServer(approvalItem.mcpServer, clientB.identity)).toBe(false);

      // runApprovedMcpCall MUST refuse execution
      const crossExecResult = await runApprovedMcpCall(approvalItem, clientB);
      expect(crossExecResult.ok).toBe(false);
      expect(crossExecResult.summary).toContain('The MCP server changed after execute was staged. It did not run.');
    });

    it('refuses execution when staged approval has its UUID manipulated or forged', async () => {
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
      });

      const stagedA = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__serverA__execute',
        args: { param: 'tamper-uuid-test' },
      });
      expect(stagedA.ok).toBe(true);

      const approvalItem = approvals.take(stagedA.pendingApproval!.id);
      const clientA = pool.getMcpClient('serverA')!;

      // 1. Attack vector: Inject fake UUID with valid prefix
      const fakeUuidItem = {
        ...approvalItem,
        mcpServer: {
          ...approvalItem.mcpServer!,
          session: 'pool-serverA-00000000-0000-0000-0000-000000000000',
        },
      };
      expect(sameServer(fakeUuidItem.mcpServer, clientA.identity)).toBe(false);
      const fakeRes = await runApprovedMcpCall(fakeUuidItem, clientA);
      expect(fakeRes.ok).toBe(false);
      expect(fakeRes.summary).toContain('The MCP server changed after execute was staged.');

      // 2. Attack vector: Strip pool prefix completely
      const strippedPrefixItem = {
        ...approvalItem,
        mcpServer: {
          ...approvalItem.mcpServer!,
          session: 'arbitrary-injected-session-id',
        },
      };
      expect(sameServer(strippedPrefixItem.mcpServer, clientA.identity)).toBe(false);
      const strippedRes = await runApprovedMcpCall(strippedPrefixItem, clientA);
      expect(strippedRes.ok).toBe(false);
      expect(strippedRes.summary).toContain('The MCP server changed after execute was staged.');

      // 3. Attack vector: Undefined or missing server identity
      expect(sameServer(undefined, clientA.identity)).toBe(false);
      expect(sameServer(approvalItem.mcpServer, null)).toBe(false);
    });

    it('refuses cross-server execution even if command and args are 100% identical', async () => {
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
      });
      await pool.connectMcpServer({
        id: 'serverB',
        command: process.execPath,
        args: [scriptPath, 'serverB'],
      });

      const clientA = pool.getMcpClient('serverA')!;
      const clientB = pool.getMcpClient('serverB')!;

      // Forge an identity that matches server A command and args but uses server B session
      const forgedIdentity: McpServerIdentity = {
        command: clientA.identity!.command,
        args: [...clientA.identity!.args],
        session: clientB.identity!.session,
      };

      // sameServer must refuse because server prefixes differ ('serverB' vs 'serverA')
      expect(sameServer(forgedIdentity, clientA.identity)).toBe(false);
      expect(sameServer(clientA.identity, forgedIdentity)).toBe(false);
    });

    it('routes through getClientForApproval and rejects fabricated server IDs', () => {
      const forgedServer: McpServerIdentity = {
        command: 'fake-cmd',
        args: ['arg1'],
        session: 'pool-maliciousServer-12345',
      };
      const resolvedClient = pool.getClientForApproval(forgedServer);
      // Malicious server is not in pool, so client should either be undefined or default fallback
      if (resolvedClient) {
        // Even if default client is returned, sameServer will refuse execution
        expect(sameServer(forgedServer, resolvedClient.identity)).toBe(false);
      } else {
        expect(resolvedClient).toBeUndefined();
      }
    });
  });

  // =========================================================================
  // TEST 2: Crash-Invalidation Race Condition
  // =========================================================================
  describe('Test 2: Crash-invalidation race condition & auto-restart backoff', () => {
    it('fails closed during auto-restart backoff window and rejects with changed session after restart', async () => {
      // Connect Server A with autoRestart enabled
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
        autoRestart: true,
        maxRestarts: 3,
      });

      // Also connect Server B to verify fault isolation
      await pool.connectMcpServer({
        id: 'serverB',
        command: process.execPath,
        args: [scriptPath, 'serverB'],
      });

      // Stage tool calls on both servers
      const stagedA = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__serverA__execute',
        args: { query: 'test-crash-race' },
      });
      expect(stagedA.ok).toBe(true);

      const stagedB = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__serverB__b_unique_tool',
        args: { query: 'server-b-safe' },
      });
      expect(stagedB.ok).toBe(true);

      const clientA = pool.getMcpClient('serverA')!;
      const clientB = pool.getMcpClient('serverB')!;
      const originalSessionA = clientA.identity!.session;

      const itemA = approvals.take(stagedA.pendingApproval!.id);
      const itemB = approvals.take(stagedB.pendingApproval!.id);

      // Trigger crash on Server A by calling crash_me tool
      let crashedEventReceived = false;
      let restartedEventReceived = false;

      pool.on('event', (evt: McpServerPoolEvent) => {
        if (evt.serverId === 'serverA') {
          if (evt.type === 'server-crashed') crashedEventReceived = true;
          if (evt.type === 'server-restarted') restartedEventReceived = true;
        }
      });

      // Trigger crash
      await expect(clientA.callTool('crash_me', {})).rejects.toThrow();

      // Verify server A is now crashed or reconnecting
      expect(['crashed', 'reconnecting']).toContain(clientA.status);
      expect(clientA.running).toBe(false);
      expect(clientA.identity).toBeNull();

      // RACE CONDITION ATTACK 1: Attempt to approve and execute call during backoff window
      // Must FAIL CLOSED immediately
      const backoffAttempt = await runApprovedMcpCall(itemA, clientA);
      expect(backoffAttempt.ok).toBe(false);
      expect(backoffAttempt.summary).toMatch(/(no longer available|changed)/);

      // While Server A is crashed, Server B must remain 100% operational and executable
      const serverBExec = await runApprovedMcpCall(itemB, clientB);
      expect(serverBExec.ok).toBe(true);
      expect(serverBExec.output).toBe('serverB executed b_unique_tool');

      // Wait for auto-restart to complete (backoff delay is 500ms for 1st restart)
      const startTime = Date.now();
      while (!clientA.running && Date.now() - startTime < 4_000) {
        await new Promise((r) => setTimeout(r, 50));
      }

      expect(clientA.running).toBe(true);
      expect(clientA.status).toBe('connected');
      expect(clientA.identity).not.toBeNull();
      expect(clientA.identity!.session).not.toBe(originalSessionA);
      expect(clientA.identity!.session).toContain('pool-serverA-');

      // RACE CONDITION ATTACK 2: Attempt to execute the staged call on the restarted server
      // Must FAIL with verbatim message "The MCP server changed after execute was staged. It did not run."
      const postRestartAttempt = await runApprovedMcpCall(itemA, clientA);
      expect(postRestartAttempt.ok).toBe(false);
      expect(postRestartAttempt.summary).toBe(
        'The MCP server changed after execute was staged. It did not run. Ask for the call again.',
      );
    });
  });

  // =========================================================================
  // TEST 3: Tool Collision & 64-Character Limit Fuzzing
  // =========================================================================
  describe('Test 3: Tool collision & 64-character limit fuzzing', () => {
    it('disambiguates identical tool names across servers without silent collisions', async () => {
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
      });
      await pool.connectMcpServer({
        id: 'serverB',
        command: process.execPath,
        args: [scriptPath, 'serverB'],
      });

      const allTools = pool.listAllAgentTools();

      // Both servers expose 'execute' and 'search'
      const executeA = allTools.find((t) => t.serverId === 'serverA' && t.tool.name === 'execute');
      const executeB = allTools.find((t) => t.serverId === 'serverB' && t.tool.name === 'execute');
      const searchA = allTools.find((t) => t.serverId === 'serverA' && t.tool.name === 'search');
      const searchB = allTools.find((t) => t.serverId === 'serverB' && t.tool.name === 'search');

      expect(executeA).toBeDefined();
      expect(executeB).toBeDefined();
      expect(searchA).toBeDefined();
      expect(searchB).toBeDefined();

      // Namespaced names must disambiguate
      expect(executeA?.agentToolName).toBe('mcp__serverA__execute');
      expect(executeB?.agentToolName).toBe('mcp__serverB__execute');
      expect(searchA?.agentToolName).toBe('mcp__serverA__search');
      expect(searchB?.agentToolName).toBe('mcp__serverB__search');

      // Colliding tools MUST NOT have direct unnamespaced aliases
      expect(executeA?.directAgentToolName).toBeUndefined();
      expect(executeB?.directAgentToolName).toBeUndefined();
      expect(searchA?.directAgentToolName).toBeUndefined();
      expect(searchB?.directAgentToolName).toBeUndefined();

      // Resolving via namespaced names routes to exact target server
      const resolvedExecA = pool.resolveAgentTool('mcp__serverA__execute');
      const resolvedExecB = pool.resolveAgentTool('mcp__serverB__execute');
      expect(resolvedExecA?.serverIdentity.session).toContain('pool-serverA-');
      expect(resolvedExecB?.serverIdentity.session).toContain('pool-serverB-');

      // Resolving ambiguous direct name mcp__execute must return null / throw
      expect(pool.resolveAgentTool('mcp__execute')).toBeNull();

      // Staging call with ambiguous unnamespaced name must be rejected
      const ambiguousStage = stageMcpCall(approvals, pool, {
        projectRoot: tempDir,
        agentName: 'mcp__execute',
        args: {},
      });
      expect(ambiguousStage.ok).toBe(false);
      expect(ambiguousStage.output).toContain('Unknown MCP tool: mcp__execute');
    });

    it('enforces <= 64 characters format and valid regex for 100-character tool names', async () => {
      await pool.connectMcpServer({
        id: 'serverA',
        command: process.execPath,
        args: [scriptPath, 'serverA'],
      });

      const allTools = pool.listAllAgentTools();

      // Find the ultra long tool
      const longTool = allTools.find((t) =>
        t.tool.name.includes('a_very_long_tool_name_designed_to_test_provider_limits'),
      );
      expect(longTool).toBeDefined();
      expect(longTool!.tool.name.length).toBeGreaterThan(90);

      // Verify <= 64 characters limit
      expect(longTool!.agentToolName.length).toBeLessThanOrEqual(64);
      expect(longTool!.agentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
      expect(longTool!.agentToolName.startsWith(MCP_TOOL_PREFIX)).toBe(true);

      // Find special chars tool
      const specialTool = allTools.find((t) => t.tool.name.includes('special.chars'));
      expect(specialTool).toBeDefined();
      expect(specialTool!.agentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
      expect(specialTool!.agentToolName).not.toContain('.');
      expect(specialTool!.agentToolName).not.toContain('/');
      expect(specialTool!.agentToolName).not.toContain(':');
      expect(specialTool!.agentToolName).not.toContain('@');

      // Verify all tools satisfy provider constraints
      for (const item of allTools) {
        expect(item.agentToolName.length).toBeLessThanOrEqual(64);
        expect(item.agentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
        if (item.directAgentToolName) {
          expect(item.directAgentToolName.length).toBeLessThanOrEqual(64);
          expect(item.directAgentToolName).toMatch(/^[A-Za-z0-9_-]+$/);
        }
      }

      // Verify tool schemas generated for model consumption satisfy provider rules
      const schemas = pool.getAgentToolSchemas();
      for (const s of schemas) {
        expect(s.name.length).toBeLessThanOrEqual(64);
        expect(s.name).toMatch(/^[A-Za-z0-9_-]+$/);
        expect(s.description.length).toBeLessThanOrEqual(1000);
      }
    });

    it('fuzzes tool resolution with arbitrary 100-character adversarial strings', () => {
      // Generate various boundary and fuzz strings
      const fuzzedInputs = [
        'mcp__' + 'x'.repeat(100),
        'mcp__' + 'serverA__' + 'y'.repeat(100),
        'mcp__!@#$%^&*()_+~`',
        'mcp__<script>alert(1)</script>',
        'mcp__..\\..\\windows\\system32',
        'mcp__\u0000\u001f\u007f',
        '',
        '   ',
        'mcp__',
        'mcp____',
      ];

      for (const fuzz of fuzzedInputs) {
        // Must never throw unhandled exception or crash
        expect(() => pool.resolveAgentTool(fuzz)).not.toThrow();
        const res = pool.resolveAgentTool(fuzz);
        expect(res).toBeNull();
      }
    });
  });

  // =========================================================================
  // TEST 4: Heartbeat Timeout & Deadlock Recovery
  // =========================================================================
  describe('Test 4: Heartbeat timeout & deadlock recovery', () => {
    it('detects ping timeout on a hung server within specified timeout and marks failure', async () => {
      // Connect server configured to hang indefinitely on ping
      const info = await pool.connectMcpServer({
        id: 'hung_server',
        command: process.execPath,
        args: [scriptPath, 'hang_on_ping'],
        requestTimeoutMs: 1_500,
      });
      expect(info.id).toBe('hung_server');

      const client = pool.getMcpClient('hung_server') as McpPoolClient;
      expect(client).toBeDefined();
      expect(client.running).toBe(true);

      const start = Date.now();
      // ping() has default timeout of 5,000ms (or options.heartbeatTimeoutMs)
      // Call ping directly: must time out and reject
      await expect(client.ping()).rejects.toThrow(/timed out/);
      const elapsed = Date.now() - start;

      // Pool timeout must trigger within 5 seconds (with small buffer for timer scheduling)
      expect(elapsed).toBeLessThanOrEqual(5_500);
      expect(elapsed).toBeGreaterThanOrEqual(1_000);
    });

    it('triggers automated deadlock recovery after consecutive ping heartbeat failures', async () => {
      const events: McpServerPoolEvent[] = [];
      const testPool = new McpServerPool({
        heartbeatIntervalMs: 400,
        heartbeatTimeoutMs: 400,
        onEvent: (evt) => events.push(evt),
      });

      try {
        await testPool.connectMcpServer({
          id: 'deadlock_server',
          command: process.execPath,
          args: [scriptPath, 'hang_on_ping'],
          autoRestart: true,
          maxRestarts: 3,
        });

        const client = testPool.getMcpClient('deadlock_server') as McpPoolClient;
        expect(client.running).toBe(true);

        // Wait for heartbeat failures to accumulate and trigger deadlock recovery
        // 2 consecutive failures: ~400ms * 2 + timeouts ~1200ms + process tree kill ~500ms
        const startWait = Date.now();
        while (Date.now() - startWait < 8_000) {
          if (events.some((e) => e.type === 'server-crashed' && e.serverId === 'deadlock_server')) {
            break;
          }
          await new Promise((r) => setTimeout(r, 100));
        }

        // Verify health was updated to unhealthy
        const healthEvent = events.find((e) => e.type === 'health-updated' && !e.isHealthy);
        expect(healthEvent).toBeDefined();
        expect(client.health.consecutiveFailures).toBeGreaterThanOrEqual(2);
        expect(client.health.isHealthy).toBe(false);

        // Verify deadlock handler terminated the hung process and signaled crash
        const crashedEvent = events.find(
          (e) => e.type === 'server-crashed' && e.serverId === 'deadlock_server',
        );
        expect(crashedEvent).toBeDefined();
        expect(crashedEvent?.type).toBe('server-crashed');

        // Check health report reflects degradation
        const report = testPool.getHealthReport();
        expect(report.servers['deadlock_server'].isHealthy).toBe(false);
      } finally {
        await testPool.disconnectAll();
      }
    });
  });
});

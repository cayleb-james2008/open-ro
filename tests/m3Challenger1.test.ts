import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { McpServerPool, type McpServerPoolEvent } from '../electron/mcpPool';

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

const mockServerScript = `
const readline = require('readline');
const { spawn } = require('child_process');
const [serverName] = process.argv.slice(2);

const tools = [
  { name: 'echo', description: 'Echo input text' },
  { name: 'delayed_compute', description: 'Compute with delay' },
  { name: 'spawn_grandchild', description: 'Spawn grandchild process' },
  { name: 'crash', description: 'Trigger sudden process crash' },
];

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
      const toolName = msg.params.name;
      const toolArgs = msg.params.arguments || {};
      if (toolName === 'crash') {
        process.exit(1);
      } else if (toolName === 'echo') {
        const text = serverName + ':echo:' + (toolArgs.text || '');
        process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text }] } }) + '\\n');
      } else if (toolName === 'delayed_compute') {
        const delay = Number(toolArgs.delayMs || 10);
        setTimeout(() => {
          const text = serverName + ':delayed_compute:' + (toolArgs.input || '');
          process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text }] } }) + '\\n');
        }, delay);
      } else if (toolName === 'spawn_grandchild') {
        const grandchild = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
          stdio: 'ignore',
          detached: false,
          windowsHide: true,
        });
        const payload = JSON.stringify({ childPid: process.pid, grandchildPid: grandchild.pid });
        process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text: payload }] } }) + '\\n');
      }
    }
  } catch {
    // Ignore parse errors
  }
});
`;

describe('Challenger 1 Adversarial Suite: Multi-Server Concurrency & Connection Pooling', () => {
  let tempDir = '';
  let serverScriptPath = '';
  let pool: McpServerPool;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-m3-challenger1-'));
    serverScriptPath = path.join(tempDir, 'mockServer.js');
    await writeFile(serverScriptPath, mockServerScript, 'utf8');
    pool = new McpServerPool({
      maxActiveServers: 6,
      heartbeatIntervalMs: 15_000,
      heartbeatTimeoutMs: 3_000,
    });
  });

  afterEach(async () => {
    await pool.disconnectAll();
    await rm(tempDir, { recursive: true, force: true });
  });

  // =========================================================================
  // Test 1: Concurrency Saturation & LRU Hibernation with Auto-Wake
  // =========================================================================
  it('enforces capacity limit during 8-server saturation, hibernates LRU servers, and auto-wakes on call', async () => {
    const serverIds = ['srv-1', 'srv-2', 'srv-3', 'srv-4', 'srv-5', 'srv-6', 'srv-7', 'srv-8'];

    // Sequentially connect 8 servers (max capacity = 6)
    for (let i = 0; i < serverIds.length; i++) {
      const id = serverIds[i];
      await pool.connectMcpServer({
        id,
        command: process.execPath,
        args: [serverScriptPath, id],
      });

      const activeCount = pool.getHealthReport().activeCount;
      // Throughout connection saturation, active count must never exceed maxActiveServers (6)
      expect(activeCount).toBeLessThanOrEqual(6);
    }

    // At this stage, exactly 6 servers must be active and total configured is 8
    const report = pool.getHealthReport();
    expect(report.totalConfigured).toBe(8);
    expect(report.activeCount).toBe(6);

    // The first two servers (srv-1 and srv-2) must be the LRU hibernated servers
    const srv1View = pool.getServerView('srv-1');
    const srv2View = pool.getServerView('srv-2');
    expect(srv1View?.status).toBe('hibernating');
    expect(srv1View?.running).toBe(false);
    expect(srv2View?.status).toBe('hibernating');
    expect(srv2View?.running).toBe(false);

    // Servers 3 to 8 must remain connected and active
    for (const id of ['srv-3', 'srv-4', 'srv-5', 'srv-6', 'srv-7', 'srv-8']) {
      const view = pool.getServerView(id);
      expect(view?.status).toBe('connected');
      expect(view?.running).toBe(true);
    }

    // Calling a tool on hibernated server srv-1 must auto-wake it
    const wakeResult = await pool.callMcpTool('srv-1', 'echo', { text: 'awake' });
    expect(wakeResult).toBe('srv-1:echo:awake');

    const wokenView = pool.getServerView('srv-1');
    expect(wokenView?.status).toBe('connected');
    expect(wokenView?.running).toBe(true);

    // srv-2 must still remain hibernated
    expect(pool.getServerView('srv-2')?.status).toBe('hibernating');
  }, 25_000);

  // =========================================================================
  // Test 2: Concurrent Tool Execution Across 4 Servers with Random Delays
  // =========================================================================
  it('executes concurrent tool calls across 4 servers simultaneously with random delays without cross-talk', async () => {
    const servers = ['alpha', 'beta', 'gamma', 'delta'];

    // Connect all 4 servers
    for (const s of servers) {
      await pool.connectMcpServer({
        id: s,
        command: process.execPath,
        args: [serverScriptPath, s],
      });
    }

    expect(pool.listServers().length).toBe(4);

    // Launch 32 interleaved concurrent calls across the 4 servers with random delays
    const callPromises: Array<Promise<{ expected: string; actual: unknown }>> = [];

    for (let i = 0; i < 32; i++) {
      const targetServer = servers[i % servers.length];
      const delayMs = Math.floor(Math.random() * 40) + 5;
      const input = `input-${i}-${targetServer}-${delayMs}`;
      const expected = `${targetServer}:delayed_compute:${input}`;

      const p = pool
        .callMcpTool(targetServer, 'delayed_compute', { input, delayMs })
        .then((actual) => ({ expected, actual }));

      callPromises.push(p);
    }

    const results = await Promise.all(callPromises);

    // Validate that every single response matches its expected server and input without cross-talk
    for (const res of results) {
      expect(res.actual).toBe(res.expected);
    }
  }, 20_000);

  // =========================================================================
  // Test 3: Rapid Crash-Restart Loop, Circuit Breaker & Fault Isolation
  // =========================================================================
  it('trips circuit breaker on rapid crash loop while maintaining uninterrupted service on peer servers', async () => {
    const events: McpServerPoolEvent[] = [];
    pool.on('event', (e) => events.push(e));

    // Connect fragile server with circuit breaker (3 maxRestarts in 60s)
    await pool.connectMcpServer({
      id: 'fragile',
      command: process.execPath,
      args: [serverScriptPath, 'fragile'],
      maxRestarts: 3,
    });

    // Connect two stable companion servers
    await pool.connectMcpServer({
      id: 'stable-1',
      command: process.execPath,
      args: [serverScriptPath, 'stable-1'],
    });

    await pool.connectMcpServer({
      id: 'stable-2',
      command: process.execPath,
      args: [serverScriptPath, 'stable-2'],
    });

    expect(pool.getServerView('fragile')?.status).toBe('connected');
    expect(pool.getServerView('stable-1')?.status).toBe('connected');
    expect(pool.getServerView('stable-2')?.status).toBe('connected');

    let stableCallsCompleted = 0;
    let stableCallsFailed = 0;
    let shouldStopStableCalls = false;

    // Concurrently keep sending tool calls to stable-1 and stable-2
    const stableWorker = (async () => {
      let idx = 0;
      while (!shouldStopStableCalls) {
        idx++;
        const target = idx % 2 === 0 ? 'stable-1' : 'stable-2';
        try {
          const res = await pool.callMcpTool(target, 'echo', { text: `ping-${idx}` });
          if (res === `${target}:echo:ping-${idx}`) {
            stableCallsCompleted++;
          } else {
            stableCallsFailed++;
          }
        } catch {
          stableCallsFailed++;
        }
        await new Promise((r) => setTimeout(r, 60));
      }
    })();

    // Helper to wait for event
    const waitForEvent = (predicate: (e: McpServerPoolEvent) => boolean, timeoutMs = 8_000) => {
      return new Promise<McpServerPoolEvent>((resolve, reject) => {
        const timer = setTimeout(() => {
          pool.off('event', listener);
          reject(new Error(`Timeout waiting for event (${timeoutMs}ms)`));
        }, timeoutMs);

        const listener = (e: McpServerPoolEvent) => {
          if (predicate(e)) {
            clearTimeout(timer);
            pool.off('event', listener);
            resolve(e);
          }
        };

        pool.on('event', listener);
      });
    };

    // Crash 1 (triggers 500ms backoff restart)
    const restart1Promise = waitForEvent((e) => e.type === 'server-restarted' && e.serverId === 'fragile');
    await pool.callMcpTool('fragile', 'crash', {}).catch(() => {});
    await restart1Promise;
    expect(pool.getServerView('fragile')?.status).toBe('connected');

    // Crash 2 (triggers 1000ms backoff restart)
    const restart2Promise = waitForEvent((e) => e.type === 'server-restarted' && e.serverId === 'fragile');
    await pool.callMcpTool('fragile', 'crash', {}).catch(() => {});
    await restart2Promise;
    expect(pool.getServerView('fragile')?.status).toBe('connected');

    // Crash 3 (triggers 2000ms backoff restart)
    const restart3Promise = waitForEvent((e) => e.type === 'server-restarted' && e.serverId === 'fragile');
    await pool.callMcpTool('fragile', 'crash', {}).catch(() => {});
    await restart3Promise;
    expect(pool.getServerView('fragile')?.status).toBe('connected');

    // Crash 4: Exceeds 3 restarts within 60s sliding window -> Circuit breaker trips!
    const circuitTripPromise = waitForEvent((e) => e.type === 'server-stopped' && e.serverId === 'fragile' && e.reason === 'Circuit breaker tripped');
    await pool.callMcpTool('fragile', 'crash', {}).catch(() => {});
    await circuitTripPromise;

    // Verify circuit breaker tripped status and message
    const fragileView = pool.getServerView('fragile');
    expect(fragileView?.status).toBe('failed');
    expect(fragileView?.error).toMatch(/circuit breaker tripped/i);

    // Stop background stable calls and verify complete fault isolation
    shouldStopStableCalls = true;
    await stableWorker;

    expect(stableCallsCompleted).toBeGreaterThan(15);
    expect(stableCallsFailed).toBe(0);

    // Verify stable servers are still 100% functional
    const s1Final = await pool.callMcpTool('stable-1', 'echo', { text: 'after-circuit-breaker' });
    const s2Final = await pool.callMcpTool('stable-2', 'echo', { text: 'after-circuit-breaker' });
    expect(s1Final).toBe('stable-1:echo:after-circuit-breaker');
    expect(s2Final).toBe('stable-2:echo:after-circuit-breaker');
  }, 30_000);

  // =========================================================================
  // Test 4: Tree-Kill Verification: Elimination of Child & Grandchild Processes
  // =========================================================================
  it('terminates entire process hierarchy (child and grandchild) upon server disconnect', async () => {
    await pool.connectMcpServer({
      id: 'srv-tree',
      command: process.execPath,
      args: [serverScriptPath, 'srv-tree'],
    });

    // Request child process to spawn a grandchild process
    const rawResult = await pool.callMcpTool('srv-tree', 'spawn_grandchild', {});
    const { childPid, grandchildPid } = (typeof rawResult === 'string' ? JSON.parse(rawResult) : rawResult) as {
      childPid: number;
      grandchildPid: number;
    };

    expect(typeof childPid).toBe('number');
    expect(typeof grandchildPid).toBe('number');
    expect(childPid).toBeGreaterThan(0);
    expect(grandchildPid).toBeGreaterThan(0);

    // Verify that both child and grandchild processes are initially active
    expect(isPidAlive(childPid)).toBe(true);
    expect(isPidAlive(grandchildPid)).toBe(true);

    // Disconnect the server, triggering killProcessTree
    await pool.disconnectMcpServer('srv-tree');

    // Allow brief OS scheduling interval for process termination cleanup
    await new Promise((r) => setTimeout(r, 400));

    // Verify both child and grandchild processes are dead (no zombie processes)
    expect(isPidAlive(childPid)).toBe(false);
    expect(isPidAlive(grandchildPid)).toBe(false);
  }, 15_000);
});

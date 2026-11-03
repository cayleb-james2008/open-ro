import { readFileSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import { runAgentTurn, testConnection } from '../electron/agent';
import type { ToolCallRequest, ToolCallResult, ToolExecutor, ToolSchema } from '../electron/types';

interface CapturedRequest {
  method: string;
  url: string;
  headers: IncomingHttpHeaders;
  body: string;
}

type Respond = (status: number, payload: unknown, headers?: Record<string, string>) => void;
type Responder = (request: CapturedRequest, respond: Respond) => void;

const servers: Server[] = [];

/** Start an HTTP server on an ephemeral loopback port and record every request. */
async function mockProvider(responder: Responder): Promise<{ baseUrl: string; requests: CapturedRequest[]; close: () => Promise<void> }> {
  const requests: CapturedRequest[] = [];
  const server = createServer((request, response) => {
    let body = '';
    request.on('data', (chunk: Buffer) => {
      body += chunk.toString('utf8');
    });
    request.on('end', () => {
      const captured: CapturedRequest = { method: request.method ?? '', url: request.url ?? '', headers: request.headers, body };
      requests.push(captured);
      responder(captured, (status, payload, headers) => {
        response.writeHead(status, { 'Content-Type': 'application/json', ...headers });
        response.end(JSON.stringify(payload));
      });
    });
  });
  servers.push(server);
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const address = server.address() as AddressInfo;
  const close = (): Promise<void> => new Promise((resolve) => server.close(() => resolve()));
  return { baseUrl: `http://127.0.0.1:${address.port}`, requests, close };
}

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => server.close(() => resolve()))));
});

describe('openai-compatible routes over real HTTP', () => {
  it('tests a connection with the documented path, auth header, and model', async () => {
    const provider = await mockProvider((_request, respond) => respond(200, { choices: [{ message: { content: 'open-ro connection check' } }] }));
    const result = await testConnection({ kind: 'openai-api', model: 'mock-model', baseUrl: provider.baseUrl, apiKey: 'test-key' }, process.cwd());
    expect(result.ok).toBe(true);
    expect(result.detail).toContain('open-ro connection check');
    expect(provider.requests).toHaveLength(1);
    const sent = provider.requests[0];
    expect(sent.method).toBe('POST');
    expect(sent.url).toBe('/chat/completions');
    expect(sent.headers.authorization).toBe('Bearer test-key');
    expect(JSON.parse(sent.body)).toMatchObject({ model: 'mock-model', max_tokens: 30 });
  });

  it('reports a rejected key without throwing', async () => {
    const provider = await mockProvider((_request, respond) => respond(401, { error: { message: 'bad key' } }));
    const result = await testConnection({ kind: 'openai-api', model: 'mock-model', baseUrl: provider.baseUrl, apiKey: 'bad-key' }, process.cwd());
    expect(result.ok).toBe(false);
    expect(result.detail).toContain('HTTP 401');
  });

  it('runs a full agent turn with a tool call and result round trip', async () => {
    let round = 0;
    const provider = await mockProvider((_request, respond) => {
      round += 1;
      if (round === 1) {
        respond(200, {
          choices: [{ message: { content: '', tool_calls: [{ id: 'call_1', function: { name: 'read_file', arguments: JSON.stringify({ path: 'src/init.lua' }) } }] } }],
        });
        return;
      }
      respond(200, { choices: [{ message: { content: 'Done: the file prints hello' } }] });
    });
    const executed: ToolCallRequest[] = [];
    const schema: ToolSchema = {
      name: 'read_file',
      description: 'Read one project file',
      parameters: { type: 'object', properties: { path: { type: 'string', description: 'Project-relative path' } }, required: ['path'] },
    };
    const executor: ToolExecutor = {
      schemas: () => [schema],
      execute: async (call): Promise<ToolCallResult> => {
        executed.push(call);
        return { ok: true, output: 'print("hello")' };
      },
    };
    const result = await runAgentTurn({
      connection: { kind: 'openai-api', model: 'mock-model', baseUrl: provider.baseUrl, apiKey: 'test-key' },
      messages: [{ role: 'user', content: 'Read src/init.lua' }],
      projectRoot: process.cwd(),
      executor,
    });
    expect(result.ok).toBe(true);
    expect(result.assistantText).toBe('Done: the file prints hello');
    expect(executed).toEqual([{ id: 'call_1', name: 'read_file', args: { path: 'src/init.lua' } }]);
    expect(provider.requests).toHaveLength(2);
    const firstBody = JSON.parse(provider.requests[0].body) as { tools?: Array<{ function?: { name?: string } }> };
    expect(firstBody.tools?.[0]?.function?.name).toBe('read_file');
    const secondBody = JSON.parse(provider.requests[1].body) as { messages?: Array<Record<string, unknown>> };
    expect(secondBody.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ role: 'assistant' }),
        expect.objectContaining({ role: 'tool', tool_call_id: 'call_1', content: 'print("hello")' }),
      ]),
    );
  });
});

describe('anthropic route over real HTTP', () => {
  it('tests the connection through GET /v1/models with the required headers', async () => {
    const provider = await mockProvider((_request, respond) => respond(200, { data: [] }));
    const result = await testConnection({ kind: 'anthropic-api', model: 'claude-mock', baseUrl: provider.baseUrl, apiKey: 'test-key' }, process.cwd());
    expect(result.ok).toBe(true);
    expect(result.detail).toContain('Auth OK');
    const sent = provider.requests[0];
    expect(sent.method).toBe('GET');
    expect(sent.url).toBe('/v1/models');
    expect(sent.headers['x-api-key']).toBe('test-key');
    expect(sent.headers['anthropic-version']).toBe('2023-06-01');
  });
});

describe('network failure path', () => {
  it('reports a refused connection instead of throwing', async () => {
    const provider = await mockProvider((_request, respond) => respond(200, {}));
    await provider.close();
    const result = await testConnection({ kind: 'openai-api', model: 'mock-model', baseUrl: provider.baseUrl }, process.cwd());
    expect(result.ok).toBe(false);
    expect(result.detail.length).toBeGreaterThan(0);
  });
});

describe('transient provider failures', () => {
  const executor: ToolExecutor = { schemas: () => [], execute: async () => ({ ok: true, output: '' }) };
  const turn = (baseUrl: string, signal?: AbortSignal) => runAgentTurn({
    connection: { kind: 'openai-api', model: 'mock-model', baseUrl },
    messages: [{ role: 'user', content: 'hi' }],
    projectRoot: process.cwd(),
    executor,
    signal,
  });

  it('retries two 5xx responses and succeeds on the third attempt', async () => {
    let calls = 0;
    const provider = await mockProvider((_request, respond) => {
      calls += 1;
      if (calls < 3) {
        respond(502, { error: 'upstream' });
      } else {
        respond(200, { choices: [{ message: { content: 'recovered' } }] });
      }
    });
    const result = await turn(provider.baseUrl);
    expect(result.ok).toBe(true);
    expect(result.assistantText).toBe('recovered');
    expect(provider.requests).toHaveLength(3);
  });

  it('honors Retry-After on a 429 and retries', async () => {
    let calls = 0;
    const provider = await mockProvider((_request, respond) => {
      calls += 1;
      if (calls === 1) {
        respond(429, { error: 'slow down' }, { 'Retry-After': '0' });
      } else {
        respond(200, { choices: [{ message: { content: 'after wait' } }] });
      }
    });
    const result = await turn(provider.baseUrl);
    expect(result.ok).toBe(true);
    expect(result.assistantText).toBe('after wait');
    expect(provider.requests).toHaveLength(2);
  });

  it('does not retry a non-429 4xx response', async () => {
    const provider = await mockProvider((_request, respond) => respond(400, { error: 'bad request' }));
    const result = await turn(provider.baseUrl);
    expect(result.ok).toBe(false);
    expect(result.error).toContain('HTTP 400');
    expect(provider.requests).toHaveLength(1);
  });

  it('aborts during the retry wait', async () => {
    const provider = await mockProvider((_request, respond) => respond(500, { error: 'down' }));
    const controller = new AbortController();
    const started = Date.now();
    const pending = turn(provider.baseUrl, controller.signal);
    setTimeout(() => controller.abort(), 100);
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(Date.now() - started).toBeLessThan(2_000);
    expect(provider.requests).toHaveLength(1);
  });
});

describe('turn cancellation', () => {
  it('aborts an in-flight agent turn when its signal fires', async () => {
    const provider = await mockProvider(() => {
      // Hold the request open; the abort must end it.
    });
    const controller = new AbortController();
    const executor: ToolExecutor = { schemas: () => [], execute: async () => ({ ok: true, output: '' }) };
    const started = Date.now();
    const pending = runAgentTurn({
      connection: { kind: 'openai-api', model: 'mock-model', baseUrl: provider.baseUrl, apiKey: 'test-key' },
      messages: [{ role: 'user', content: 'hang' }],
      projectRoot: process.cwd(),
      executor,
      signal: controller.signal,
    });
    setTimeout(() => controller.abort(), 120);
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(Date.now() - started).toBeLessThan(3_000);
    expect(provider.requests).toHaveLength(1);
  });

  it('wires cancellation through the handler, preload allowlist, and adapters', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    const start = main.indexOf("ipcMain.handle('agent:send'");
    expect(start).toBeGreaterThan(-1);
    const handler = main.slice(start, main.indexOf('\n  });', start));
    expect(handler).toContain('new AbortController()');
    expect(handler).toContain('signal: controller.signal');
    expect(handler).toContain('A turn is already running');
    const guardAt = handler.indexOf('A turn is already running');
    const rootAt = handler.indexOf('const requestedRoot');
    expect(guardAt).toBeGreaterThan(-1);
    expect(rootAt).toBeGreaterThan(guardAt);
    expect(main).toContain("ipcMain.handle('agent:cancel'");
    expect(main).toContain('activeTurn.abort()');
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'agent:cancel'");
    const agent = readFileSync('electron/agent.ts', 'utf8');
    expect(agent).toContain('AbortSignal.any([signal, timeout])');
    expect(agent).toContain('signal: turnSignal(signal)');
  });
});

import { spawn } from 'node:child_process';
import { getProviderOption, type ProviderOption, type ProviderKind } from './providerCatalog';
import type {
  AgentTurnResult,
  ChatMessage,
  ConnectionTestResult,
  ProviderConnection,
  ToolCallRequest,
  ToolCallResult,
  ToolExecutor,
  ToolSchema,
} from './types';

/**
 * Agent runtime (docs/ARCHITECTURE.md §1, §3): routes a conversation to the
 * selected provider adapter and executes tool calls through the executor the
 * main process injects. Approval-gated actions come back as staged results.
 */

const MAX_TOOL_ROUNDS = 6;
const API_TIMEOUT_MS = 120_000;

export function toolSchemas(): ToolSchema[] {
  return [
    {
      name: 'list_dir',
      description: 'List the files and directories relative to the selected project root.',
      parameters: {
        type: 'object',
        properties: { path: { type: 'string', description: 'Directory relative to the project root. Empty means the root.' } },
        required: [],
      },
    },
    {
      name: 'read_file',
      description: 'Read a text file relative to the selected project root.',
      parameters: {
        type: 'object',
        properties: { path: { type: 'string', description: 'File path relative to the project root.' } },
        required: ['path'],
      },
    },
    {
      name: 'write_file',
      description: 'Propose writing a file relative to the project root. The user must approve the change before it is written.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'File path relative to the project root.' },
          content: { type: 'string', description: 'Full new file content.' },
        },
        required: ['path', 'content'],
      },
    },
    {
      name: 'run_command',
      description: 'Propose a shell command in the project. The user must approve the exact command before it runs.',
      parameters: {
        type: 'object',
        properties: { command: { type: 'string', description: 'The command to run, with its arguments in the same string.' } },
        required: ['command'],
      },
    },
    {
      name: 'report_playtest',
      description:
        'Record a playtest result. It is PASSED only with at least one assertion, one state check and one PNG screenshot in the project, and every expected value equals the actual value.',
      parameters: {
        type: 'object',
        properties: {
          scenario: { type: 'string', description: 'What the playtest exercised.' },
          assertions: { type: 'array', description: 'Checks as {description, expected, actual}.', items: { type: 'object' } },
          stateChecks: { type: 'array', description: 'Instance state as {instancePath, property, expected, actual}.', items: { type: 'object' } },
          screenshots: { type: 'array', description: 'PNG paths relative to the project root.', items: { type: 'string' } },
        },
        required: ['scenario', 'assertions', 'stateChecks', 'screenshots'],
      },
    },
  ];
}

function sanitizeMessages(messages: ChatMessage[]): ChatMessage[] {
  return messages
    .filter((message) => typeof message.content === 'string')
    .map((message) => ({ role: message.role, content: message.content }));
}

function systemPrompt(): string {
  return [
    'You are Open-Ro, a development agent inside a local Roblox workspace.',
    'Work on the selected local project. Read files before proposing changes.',
    'write_file and run_command only stage an approval. They do not change the project until the user approves that exact action in Open-Ro.',
    'Tools named mcp__<tool> call the connected MCP server, such as Roblox Studio. They also only stage an approval.',
    'Never claim a file was written, a command ran, or an MCP tool ran unless the result says it completed.',
    'Never say a playtest passed unless report_playtest returned PASSED.',
    'Report exactly what you inspected, staged, or left unverified.',
  ].join(' ');
}

/** OpenAI-compatible adapter (openai, openrouter, ollama, custom endpoints). */
async function callOpenAiCompatible(
  connection: ProviderConnection,
  option: ProviderOption,
  messages: ChatMessage[],
  executor: ToolExecutor,
): Promise<AgentTurnResult> {
  const base = (connection.baseUrl ?? option.defaultBaseUrl ?? '').replace(/\/+$/, '');
  if (!base) {
    return { ok: false, assistantText: '', toolTrace: [], error: 'This provider requires a base URL.' };
  }
  const trace: AgentTurnResult['toolTrace'] = [];
  const convo: unknown[] = [
    { role: 'system', content: systemPrompt() },
    ...sanitizeMessages(messages).filter((m) => m.role !== 'system'),
  ];
  let assistantText = '';

  for (let round = 0; round <= MAX_TOOL_ROUNDS; round += 1) {
    const response = await fetch(`${base}/chat/completions`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(connection.apiKey ? { Authorization: `Bearer ${connection.apiKey}` } : {}),
      },
      body: JSON.stringify({
        model: connection.model,
        messages: convo,
        tools: executor.schemas().map((schema) => ({ type: 'function', function: schema })),
      }),
      signal: AbortSignal.timeout(API_TIMEOUT_MS),
    });
    if (!response.ok) {
      return { ok: false, assistantText, toolTrace: trace, error: `Provider request failed: HTTP ${response.status} ${(await response.text()).slice(0, 300)}` };
    }
    const payload = (await response.json()) as {
      choices?: Array<{ message?: { content?: string; tool_calls?: Array<{ id: string; function?: { name: string; arguments: string } }> } }>;
    };
    const choice = payload.choices?.[0]?.message;
    assistantText = choice?.content ?? assistantText;
    const calls = (choice?.tool_calls ?? []).map((call, index) => ({
      id: call.id ?? `call-${round}-${index}`,
      name: call.function?.name ?? '',
      args: safeParseArgs(call.function?.arguments),
    }));
    if (calls.length === 0) {
      break;
    }
    convo.push({ role: 'assistant', content: choice?.content ?? '', tool_calls: calls.map((call) => ({ id: call.id, type: 'function', function: { name: call.name, arguments: JSON.stringify(call.args) } })) });
    for (const call of calls) {
      const executed = await runTool(call, executor, trace);
      convo.push({ role: 'tool', tool_call_id: call.id, content: executed.output });
    }
  }
  return { ok: true, assistantText, toolTrace: trace };
}

/** Anthropic adapter (/v1/messages). */
async function callAnthropic(
  connection: ProviderConnection,
  messages: ChatMessage[],
  executor: ToolExecutor,
): Promise<AgentTurnResult> {
  const base = (connection.baseUrl ?? 'https://api.anthropic.com').replace(/\/+$/, '');
  const trace: AgentTurnResult['toolTrace'] = [];
  const baseURL = `${base}/v1/messages`;
  const system = systemPrompt();
  const turns: Array<Record<string, unknown>> = sanitizeMessages(messages)
    .filter((message) => message.role !== 'system')
    .map((message) => ({ role: message.role === 'assistant' ? 'assistant' : 'user', content: message.content }));
  let assistantText = '';

  for (let round = 0; round <= MAX_TOOL_ROUNDS; round += 1) {
    const response = await fetch(baseURL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'x-api-key': connection.apiKey ?? '',
        'anthropic-version': '2023-06-01',
      },
      body: JSON.stringify({
        model: connection.model,
        max_tokens: 4096,
        system,
        messages: turns,
        tools: executor.schemas().map((schema) => ({
          name: schema.name,
          description: schema.description,
          input_schema: schema.parameters,
        })),
      }),
      signal: AbortSignal.timeout(API_TIMEOUT_MS),
    });
    if (!response.ok) {
      return { ok: false, assistantText, toolTrace: trace, error: `Provider request failed: HTTP ${response.status} ${(await response.text()).slice(0, 300)}` };
    }
    const payload = (await response.json()) as {
      content?: Array<{ type: string; text?: string; id?: string; name?: string; input?: Record<string, unknown> }>;
      error?: { message?: string };
    };
    if (payload.error) {
      return { ok: false, assistantText, toolTrace: trace, error: payload.error.message ?? 'Provider reported an error.' };
    }
    const toolCalls = (payload.content ?? [])
      .filter((block) => block.type === 'tool_use')
      .map((block, index) => ({ id: block.id ?? `tool-${round}-${index}`, name: block.name ?? '', args: block.input ?? {} }));
    assistantText = (payload.content ?? []).filter((block) => block.type === 'text').map((block) => block.text ?? '').join('\n').trim() || assistantText;
    if (toolCalls.length === 0) {
      break;
    }
    turns.push({ role: 'assistant', content: payload.content ?? [] });
    const results: Array<Record<string, unknown>> = [];
    for (const call of toolCalls) {
      const executed = await runTool(call, executor, trace);
      results.push({ type: 'tool_result', tool_use_id: call.id, content: executed.output, is_error: !executed.ok });
    }
    turns.push({ role: 'user', content: results });
  }
  return { ok: true, assistantText, toolTrace: trace };
}

function safeParseArgs(raw: string | undefined): Record<string, unknown> {
  if (!raw) {
    return {};
  }
  try {
    const parsed = JSON.parse(raw) as unknown;
    return typeof parsed === 'object' && parsed !== null ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

async function runTool(
  call: { id: string; name: string; args: Record<string, unknown> },
  executor: ToolExecutor,
  trace: AgentTurnResult['toolTrace'],
): Promise<ToolCallResult> {
  const name = call.name as ToolCallRequest['name'];
  try {
    const result = await executor.execute({ id: call.id, name, args: call.args });
    const tracedArgs = { ...call.args };
    if (typeof tracedArgs.content === 'string') {
      tracedArgs.content = `[${tracedArgs.content.length} characters staged for approval]`;
    }
    trace.push({
      id: call.id,
      name,
      args: tracedArgs,
      status: result.pendingApproval ? 'staged' : result.ok ? 'executed' : 'error',
      detail: result.pendingApproval
        ? `Waiting for user approval: ${result.pendingApproval.kind}${result.pendingApproval.path ? ` ${result.pendingApproval.path}` : result.pendingApproval.command ? ` ${result.pendingApproval.command}` : result.pendingApproval.tool ? ` ${result.pendingApproval.tool}` : ''}`
        : result.output.slice(0, 500),
    });
    return result;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const tracedArgs = { ...call.args };
    if (typeof tracedArgs.content === 'string') {
      tracedArgs.content = `[${tracedArgs.content.length} characters]`;
    }
    trace.push({ id: call.id, name, args: tracedArgs, status: 'error', detail: message });
    return { ok: false, output: `Tool failed: ${message}` };
  }
}

/** Subscription CLI routes: launch the official, unmodified CLI (docs/AUTH.md). */
async function callCli(
  connection: ProviderConnection,
  prompt: string,
  projectRoot: string,
): Promise<AgentTurnResult> {
  const option = getProviderOption(connection.kind);
  const isWindows = process.platform === 'win32';
  const exe = isWindows ? `${option.cliCommand}.cmd` : option.cliCommand!;
  const commandArgs =
    connection.kind === 'codex-cli'
      ? ['exec', '--skip-git-repo-check', prompt]
      : connection.kind === 'claude-code-cli'
        ? ['-p', prompt]
        : ['run', prompt]; // opencode
  const cwd = projectRoot || process.env.HOME || process.cwd();
  const result = await new Promise<AgentTurnResult>((resolve) => {
    const child = spawn(exe, commandArgs, {
      cwd,
      // Subscription processes must not inherit unrelated API key variables.
      env: {
        PATH: process.env.PATH ?? '',
        HOME: process.env.HOME ?? '',
        APPDATA: process.env.APPDATA ?? '',
        NO_COLOR: '1',
        TERM: 'dumb',
      },
      shell: isWindows,
    });
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => child.kill(), 10 * 60_000);
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', (error) => {
      clearTimeout(timer);
      resolve({ ok: false, assistantText: '', toolTrace: [], error: `Could not launch ${option.cliCommand}: ${error.message}` });
    });
    child.on('exit', (code) => {
      clearTimeout(timer);
      if (code === 0) {
        resolve({ ok: true, assistantText: stdout.trim() || '(command completed with no output)', toolTrace: [] });
      } else {
        resolve({ ok: false, assistantText: stdout.trim(), toolTrace: [], error: `${option.cliCommand} exited with code ${code}: ${stderr.trim().slice(0, 400)}` });
      }
    });
  });
  return result;
}

export async function runAgentTurn(args: {
  connection: ProviderConnection;
  messages: ChatMessage[];
  projectRoot: string;
  executor: ToolExecutor;
}): Promise<AgentTurnResult> {
  const { connection, messages, projectRoot, executor } = args;
  const option = getProviderOption(connection.kind);
  const lastPrompt = [...messages].reverse().find((message) => message.role === 'user')?.content ?? '';

  if (option.connection === 'cli') {
    return callCli(connection, lastPrompt, projectRoot);
  }
  if (option.api === 'anthropic-messages') {
    return callAnthropic(connection, messages, executor);
  }
  return callOpenAiCompatible(connection, option, sanitizeMessages(messages), executor);
}

/** Connection test per route (docs/AUTH.md: a CLI route is validated by running its status command). */
export async function testConnection(connection: ProviderConnection, projectRoot: string): Promise<ConnectionTestResult> {
  const option = getProviderOption(connection.kind);
  try {
    if (option.connection === 'cli') {
      const result = await callCli(connection, 'Reply with exactly: open-ro connection check', projectRoot);
      return result.ok
        ? { ok: true, detail: `${option.cliCommand} responded: ${result.assistantText.slice(0, 200)}` }
        : { ok: false, detail: result.error ?? 'CLI check failed.' };
    }
    if (option.api === 'anthropic-messages') {
      return await (async () => {
        const response = await fetch(`${(connection.baseUrl ?? option.defaultBaseUrl ?? '').replace(/\/+$/, '')}/v1/models`, {
          headers: { 'x-api-key': connection.apiKey ?? '', 'anthropic-version': '2023-06-01' },
          signal: AbortSignal.timeout(20_000),
        });
        return response.ok
          ? { ok: true, detail: `Auth OK (HTTP ${response.status}).` }
          : { ok: false, detail: `Key rejected: HTTP ${response.status}.` };
      })();
    }
    const base = (connection.baseUrl ?? option.defaultBaseUrl ?? '').replace(/\/+$/, '');
    const response = await fetch(`${base}/chat/completions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(connection.apiKey ? { Authorization: `Bearer ${connection.apiKey}` } : {}) },
      body: JSON.stringify({ model: connection.model, messages: [{ role: 'user', content: 'Reply with exactly: open-ro connection check' }], max_tokens: 30 }),
      signal: AbortSignal.timeout(30_000),
    });
    if (response.ok) {
      const payload = (await response.json()) as { choices?: Array<{ message?: { content?: string } }> };
      return { ok: true, detail: `Model responded: ${(payload.choices?.[0]?.message?.content ?? '').slice(0, 120)}` };
    }
    return { ok: false, detail: `Endpoint rejected the request: HTTP ${response.status}.` };
  } catch (error) {
    return { ok: false, detail: error instanceof Error ? error.message : String(error) };
  }
}

export type { ProviderKind };

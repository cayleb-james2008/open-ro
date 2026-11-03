import type { ApprovalQueue, PendingApproval } from './approvals';
import { assertMcpSpec } from './mcpFraming';
import type { ApprovalResult, McpServerIdentity, McpToolSummary, ToolCallResult, ToolSchema } from './types';

/**
 * Connected MCP tools for the agent. The agent can only stage a call. The
 * call runs after the user approves that exact tool and argument JSON.
 */

export const MCP_TOOL_PREFIX = 'mcp__';
const MAX_AGENT_TOOL_NAME = 64;

export interface McpCaller {
  readonly running: boolean;
  /** Command, arguments and per-start session of the running server. */
  readonly identity: McpServerIdentity | null;
  listTools(): McpToolSummary[];
  callTool(name: string, args: Record<string, unknown>): Promise<string>;
}

/** Agent-facing names: provider APIs accept [A-Za-z0-9_-] up to 64 characters. Collisions keep the first tool. */
export function mcpAgentTools(tools: McpToolSummary[]): Array<{ agentName: string; tool: McpToolSummary }> {
  const seen = new Set<string>();
  const out: Array<{ agentName: string; tool: McpToolSummary }> = [];
  for (const tool of tools) {
    const agentName = `${MCP_TOOL_PREFIX}${tool.name.replace(/[^A-Za-z0-9_-]/g, '_')}`.slice(0, MAX_AGENT_TOOL_NAME);
    if (!tool.name || seen.has(agentName)) {
      continue;
    }
    seen.add(agentName);
    out.push({ agentName, tool });
  }
  return out;
}

export function mcpToolSchemas(tools: McpToolSummary[]): ToolSchema[] {
  return mcpAgentTools(tools).map(({ agentName, tool }) => ({
    name: agentName as ToolSchema['name'],
    description: `Propose calling the MCP tool "${tool.name}". The user must approve the exact arguments before it runs. ${tool.description}`.trim().slice(0, 1_000),
    parameters: providerSafeSchema(tool.inputSchema),
  }));
}

/** Model APIs expect a plain object schema; `$schema` and `$id` at the top level are not needed and some providers reject them. */
function providerSafeSchema(schema: Record<string, unknown> | undefined): ToolSchema['parameters'] {
  if (!schema) {
    return { type: 'object', properties: {} };
  }
  const { $schema: _schema, $id: _id, ...rest } = schema;
  return { properties: {}, ...rest, type: 'object' } as ToolSchema['parameters'];
}

export function isMcpAgentTool(name: string): boolean {
  return name.startsWith(MCP_TOOL_PREFIX);
}

/** Stage an agent MCP call. Never calls the server. */
export function stageMcpCall(
  approvals: ApprovalQueue,
  caller: McpCaller,
  input: { projectRoot: string; agentName: string; args: Record<string, unknown> },
): ToolCallResult {
  const server = caller.identity;
  if (!caller.running || !server) {
    return { ok: false, output: 'No MCP server is connected.' };
  }
  const match = mcpAgentTools(caller.listTools()).find((entry) => entry.agentName === input.agentName);
  if (!match) {
    return { ok: false, output: `Unknown MCP tool: ${input.agentName}` };
  }
  const pending = approvals.stageMcp({ projectRoot: input.projectRoot, tool: match.tool.name, args: input.args, server });
  return {
    ok: true,
    output: `Staged MCP tool ${match.tool.name} for approval. It has not run.`,
    pendingApproval: { kind: 'mcp', tool: match.tool.name, id: pending.id },
  };
}

/** Run an MCP call the user approved. The tool must still be offered by the connected server. */
export async function runApprovedMcpCall(item: PendingApproval, caller: McpCaller): Promise<ApprovalResult> {
  const tool = item.mcpTool ?? '';
  if (item.kind !== 'mcp' || !tool) {
    throw new Error('Not an MCP approval.');
  }
  if (!caller.running || !caller.listTools().some((entry) => entry.name === tool)) {
    return { ok: false, decision: 'approve', summary: `MCP tool ${tool} is no longer available. It did not run.` };
  }
  if (!sameServer(item.mcpServer, caller.identity)) {
    return { ok: false, decision: 'approve', summary: `The MCP server changed after ${tool} was staged. It did not run. Ask for the call again.` };
  }
  const args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
  try {
    const output = await caller.callTool(tool, args);
    return { ok: true, decision: 'approve', summary: `MCP tool ${tool} ran.`, output };
  } catch (error) {
    return { ok: false, decision: 'approve', summary: `MCP tool ${tool} failed.`, output: error instanceof Error ? error.message : String(error) };
  }
}

/** Same command, same arguments and the same process session. A restart of the same command is a different server. */
export function sameServer(staged: McpServerIdentity | undefined, current: McpServerIdentity | null): boolean {
  return !!staged && !!current && staged.session === current.session && staged.command === current.command &&
    staged.args.length === current.args.length && staged.args.every((arg, index) => arg === current.args[index]);
}

/** One-line label of a server for approval cards. */
export function serverLabel(server: { command: string; args: string[] }): string {
  return [server.command, ...server.args.map((arg) => (/[\s"']/.test(arg) ? JSON.stringify(arg) : arg))].join(' ');
}

export interface McpServerStarter {
  readonly running: boolean;
  start(spec: { command: string; args: string[] }): Promise<void>;
  listTools(): McpToolSummary[];
}

/** Stage starting an MCP server from the renderer's form. Never starts anything: starting a server runs a local program. */
export function stageMcpConnect(approvals: ApprovalQueue, client: McpServerStarter, spec: unknown): PendingApproval {
  const clean = assertMcpSpec((spec ?? {}) as { command?: unknown; args?: unknown });
  if (client.running) {
    throw new Error('An MCP server is already active. Stop it before connecting another.');
  }
  return approvals.stageMcpServer({ projectRoot: '', command: clean.command, args: clean.args });
}

/** Start exactly the command and arguments the user approved. */
export async function runApprovedMcpServer(item: PendingApproval, client: McpServerStarter): Promise<ApprovalResult> {
  if (item.kind !== 'mcp-server' || !item.command) {
    throw new Error('Not an MCP server approval.');
  }
  await client.start({ command: item.command, args: [...(item.serverArgs ?? [])] });
  return { ok: true, decision: 'approve', summary: `MCP server started with ${client.listTools().length} tools.` };
}

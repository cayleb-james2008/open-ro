import type { ApprovalQueue, PendingApproval } from './approvals';
import type { ApprovalResult, McpToolSummary, ToolCallResult, ToolSchema } from './types';

/**
 * Connected MCP tools for the agent. The agent can only stage a call. The
 * call runs after the user approves that exact tool and argument JSON.
 */

export const MCP_TOOL_PREFIX = 'mcp__';
const MAX_AGENT_TOOL_NAME = 64;

export interface McpCaller {
  readonly running: boolean;
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
  if (!caller.running) {
    return { ok: false, output: 'No MCP server is connected.' };
  }
  const match = mcpAgentTools(caller.listTools()).find((entry) => entry.agentName === input.agentName);
  if (!match) {
    return { ok: false, output: `Unknown MCP tool: ${input.agentName}` };
  }
  const pending = approvals.stageMcp({ projectRoot: input.projectRoot, tool: match.tool.name, args: input.args });
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
  const args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
  try {
    const output = await caller.callTool(tool, args);
    return { ok: true, decision: 'approve', summary: `MCP tool ${tool} ran.`, output };
  } catch (error) {
    return { ok: false, decision: 'approve', summary: `MCP tool ${tool} failed.`, output: error instanceof Error ? error.message : String(error) };
  }
}

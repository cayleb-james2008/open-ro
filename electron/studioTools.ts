import type { ApprovalQueue, PendingApproval } from './approvals';
import type { McpCaller } from './mcpTools';
import {
  currencyProblems,
  formatPath,
  isScriptClass,
  normalizeSnapshot,
  parseInstancePath,
  parseLuauReply,
  placementProblem,
  readbackProblems,
  type ScriptClass,
  type TreeSnapshot,
} from './studioChecks';
import { buildReadLuau, buildSnapshotLuau, buildWriteLuau, DEFAULT_SNAPSHOT_SERVICES } from './studioLuau';
import type { ApprovalResult, McpToolSummary, ToolCallResult } from './types';

/**
 * Open-Ro's Studio tools. Each one stages a call to the Studio MCP server's
 * Luau tool; nothing reaches Studio until the user approves it. A write needs
 * an approved read of the same path first, so the review shows a real diff.
 */

const CODE_NAMES = ['code', 'source', 'luau', 'command', 'script'];

/** Find the connected Luau tool and fill its arguments from its own input schema. Refuses when the schema is not clear. */
export function luauToolCall(tools: McpToolSummary[], code: string, studioId?: string): { tool: string; args: Record<string, unknown> } {
  const tool = tools.find((item) => item.name === 'execute_luau') ?? tools.find((item) => item.name === 'run_code');
  if (!tool) {
    throw new Error('The connected MCP server has no execute_luau or run_code tool. Connect the Roblox Studio MCP server.');
  }
  const schema = (tool.inputSchema ?? {}) as { properties?: Record<string, { type?: unknown }>; required?: unknown };
  const props = schema.properties ?? {};
  const required = Array.isArray(schema.required) ? schema.required : [];
  const args: Record<string, unknown> = {};
  if ('datamodel_type' in props) {
    args.datamodel_type = 'Edit';
  }
  if ('studio_id' in props && studioId) {
    args.studio_id = studioId;
  } else if (required.includes('studio_id')) {
    throw new Error('Studio needs a studio_id. Ask to run list_roblox_studios, then pass studioId.');
  }
  const strings = Object.keys(props).filter((key) => props[key]?.type === 'string' && key !== 'datamodel_type' && key !== 'studio_id');
  const named = strings.filter((key) => CODE_NAMES.includes(key));
  const codeKey = strings.length === 1 ? strings[0] : named.length === 1 ? named[0] : '';
  if (!codeKey) {
    throw new Error(`Cannot tell which ${tool.name} argument takes the Luau code (string arguments: ${strings.join(', ') || 'none'}).`);
  }
  args[codeKey] = code;
  return { tool: tool.name, args };
}

/** What approved reads saw, by dot path. `source: null` means the instance exists but is not a script; a missing entry means it was never read. */
export class StudioReads extends Map<string, { exists: boolean; className?: string; source: string | null }> {}

const SERVICE_NAME = /^[A-Za-z]{1,40}$/;

export function stageStudioTool(
  approvals: ApprovalQueue,
  caller: McpCaller,
  reads: StudioReads,
  input: { projectRoot: string; name: string; args: Record<string, unknown> },
): ToolCallResult {
  if (!caller.running) {
    return { ok: false, output: 'No MCP server is connected. Connect the Roblox Studio MCP server first.' };
  }
  try {
    const { args } = input;
    const studioId = typeof args.studioId === 'string' && args.studioId ? args.studioId : undefined;
    const tools = caller.listTools();
    const stage = (op: 'snapshot' | 'read' | 'write', path: string, code: string, extra: Partial<Parameters<ApprovalQueue['stageStudio']>[0]>) => {
      const call = luauToolCall(tools, code, studioId);
      const item = approvals.stageStudio({ projectRoot: input.projectRoot, op, path, tool: call.tool, args: call.args, ...extra });
      return { ok: true, output: `Staged Studio ${op} of ${path} for approval. Nothing has run in Studio.`, pendingApproval: { kind: 'studio' as const, tool: call.tool, path, id: item.id } };
    };
    if (input.name === 'studio_snapshot') {
      const services = Array.isArray(args.services) && args.services.length > 0 ? args.services.map(String) : DEFAULT_SNAPSHOT_SERVICES;
      if (services.length > 30 || services.some((name) => !SERVICE_NAME.test(name))) {
        return { ok: false, output: 'services must be up to 30 Roblox service names, such as Workspace or ServerScriptService.' };
      }
      const maxDepth = Math.min(Math.max(Math.trunc(Number(args.maxDepth) || 3), 1), 6);
      return stage('snapshot', 'game', buildSnapshotLuau({ services, maxDepth, maxNodes: 2000 }), { services });
    }
    const segments = parseInstancePath(String(args.path ?? ''));
    const path = segments.join('.');
    if (input.name === 'studio_read_script') {
      return stage('read', path, buildReadLuau(segments), {});
    }
    if (input.name !== 'studio_write_script') {
      return { ok: false, output: `Unknown Studio tool: ${input.name}` };
    }
    const className = args.className;
    const source = typeof args.source === 'string' ? args.source : '';
    if (!isScriptClass(className)) {
      return { ok: false, output: 'className must be Script, LocalScript or ModuleScript.' };
    }
    const problems = [placementProblem(segments, className), ...currencyProblems(segments, className, source)].filter((item): item is string => !!item);
    if (problems.length > 0) {
      return { ok: false, output: `Not staged. ${problems.join(' ')}` };
    }
    const seen = reads.get(path);
    if (!seen) {
      return { ok: false, output: `Read ${formatPath(segments)} with studio_read_script first, so the review shows what changes.` };
    }
    if (seen.exists && (seen.source === null || seen.className !== className)) {
      return { ok: false, output: `${formatPath(segments)} is a ${seen.className ?? 'non-script'}, not a ${className}. Nothing was staged.` };
    }
    const previous = seen.exists ? seen.source : null;
    return stage('write', path, buildWriteLuau({ segments, className, source, expectedPrevious: previous }), { className, previous: previous ?? undefined, content: source });
  } catch (error) {
    return { ok: false, output: error instanceof Error ? error.message : String(error) };
  }
}

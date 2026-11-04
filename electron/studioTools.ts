import type { ApprovalQueue, PendingApproval } from './approvals';
import { sameServer, type McpCaller } from './mcpTools';
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

/**
 * What approved reads saw, keyed by studioReadKey. `source: null` means the instance exists but is not a script;
 * a missing entry means it was never read. A read authorizes writes only on the same server session and studio.
 */
export class StudioReads extends Map<string, { exists: boolean; className?: string; source: string | null }> {}

export function studioReadKey(session: string, studioId: string | undefined, path: string): string {
  return JSON.stringify([session, studioId ?? null, path]);
}

const SERVICE_NAME = /^[A-Za-z]{1,40}$/;

export function stageStudioTool(
  approvals: ApprovalQueue,
  caller: McpCaller,
  reads: StudioReads,
  input: { projectRoot: string; name: string; args: Record<string, unknown> },
): ToolCallResult {
  const server = caller.identity;
  if (!caller.running || !server) {
    return { ok: false, output: 'No MCP server is connected. Connect the Roblox Studio MCP server first.' };
  }
  try {
    const { args } = input;
    const studioId = typeof args.studioId === 'string' && args.studioId ? args.studioId : undefined;
    const tools = caller.listTools();
    const stage = (op: 'snapshot' | 'read' | 'write', path: string, code: string, extra: Partial<Parameters<ApprovalQueue['stageStudio']>[0]>) => {
      const call = luauToolCall(tools, code, studioId);
      const item = approvals.stageStudio({ projectRoot: input.projectRoot, op, path, tool: call.tool, args: call.args, server, studioId, ...extra });
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
    if (input.name === 'studio_playtest') {
      if (!caller.listTools().some((tool) => tool.name === 'start_stop_play')) {
        return { ok: false, output: 'The connected MCP server has no start_stop_play tool.' };
      }
      if (typeof args.isStart !== 'boolean') {
        return { ok: false, output: 'isStart must be true or false.' };
      }
      if (!studioId) {
        return { ok: false, output: 'Studio needs a studioId. Run list_roblox_studios first.' };
      }
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'play',
        path: 'game',
        tool: 'start_stop_play',
        args: { is_start: args.isStart, studio_id: studioId },
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged ${args.isStart ? 'starting' : 'stopping'} playtest for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: 'start_stop_play', path: 'game', id: item.id },
      };
    }
    if (input.name === 'studio_console') {
      if (!caller.listTools().some((tool) => tool.name === 'get_console_output')) {
        return { ok: false, output: 'The connected MCP server has no get_console_output tool.' };
      }
      if (!studioId) {
        return { ok: false, output: 'Studio needs a studioId. Run list_roblox_studios first.' };
      }
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'console',
        path: 'game',
        tool: 'get_console_output',
        args: { studio_id: studioId },
        server,
        studioId,
      });
      return {
        ok: true,
        output: 'Staged reading the Studio output for approval. Nothing has run in Studio.',
        pendingApproval: { kind: 'studio' as const, tool: 'get_console_output', path: 'game', id: item.id },
      };
    }
    if (input.name === 'studio_keyboard' || input.name === 'studio_mouse') {
      const toolName = input.name === 'studio_keyboard' ? 'user_keyboard_input' : 'user_mouse_input';
      if (!caller.listTools().some((tool) => tool.name === toolName)) {
        return { ok: false, output: `The connected MCP server has no ${toolName} tool.` };
      }
      if (!studioId) {
        return { ok: false, output: 'Studio needs a studioId. Run list_roblox_studios first.' };
      }
      const actions = args.actions;
      if (!Array.isArray(actions) || actions.length === 0) {
        return { ok: false, output: 'actions must be a non-empty array.' };
      }
      if (actions.length > 40) {
        return { ok: false, output: 'actions must contain at most 40 steps.' };
      }
      const clean = actions.map((action) => (action && typeof action === 'object' ? (action as Record<string, unknown>) : null));
      if (clean.some((action) => !action || typeof action.action !== 'string' || !action.action.trim())) {
        return { ok: false, output: 'Every input action needs an action string.' };
      }
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'input',
        path: 'game',
        tool: toolName,
        args: { actions: clean, datamodel_type: 'Client', studio_id: studioId },
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged ${clean.length} input action(s) for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: toolName, path: 'game', id: item.id },
      };
    }
    if (input.name === 'studio_capture') {
      if (!caller.listTools().some((tool) => tool.name === 'screen_capture')) {
        return { ok: false, output: 'The connected MCP server has no screen_capture tool.' };
      }
      if (!studioId) {
        return { ok: false, output: 'Studio needs a studioId. Run list_roblox_studios first.' };
      }
      const supplied = typeof args.captureId === 'string' ? args.captureId.trim().slice(0, 80) : '';
      const captureId = supplied || `openro-capture-${Date.now()}`;
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'capture',
        path: 'game',
        tool: 'screen_capture',
        args: { capture_id: captureId, studio_id: studioId },
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged capturing the Studio viewport as ${captureId} for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: 'screen_capture', path: 'game', id: item.id },
      };
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
    const seen = reads.get(studioReadKey(server.session, studioId, path));
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

/** What a confirmed Studio write replaced. `previous: null` means Open-Ro created the script. */
export interface StudioCheckpoint { path: string; className: ScriptClass; previous: string | null; content: string; studioId?: string }

type ImageCapableCaller = McpCaller & {
  callToolResult?(name: string, args: Record<string, unknown>): Promise<{ text: string; images: Array<{ mimeType: string; data: string }>; isError: boolean }>;
};

/** Run an approved Studio call and check what Studio reports back. */
export async function runApprovedStudio(
  item: PendingApproval,
  caller: McpCaller,
  reads: StudioReads,
  saveCapture?: (data: Buffer, mimeType: string, captureId: string) => Promise<string>,
): Promise<ApprovalResult & { snapshot?: TreeSnapshot; checkpoint?: StudioCheckpoint }> {
  const tool = item.mcpTool ?? '';
  const path = item.studioPath ?? '';
  if (item.kind !== 'studio' || !tool || !item.studioOp) {
    throw new Error('Not a Studio approval.');
  }
  const label = path === 'game' ? 'the place tree' : `game.${path}`;
  const key = studioReadKey(item.mcpServer?.session ?? '', item.studioId, path);
  const fail = (summary: string, output?: string) => ({ ok: false, decision: 'approve' as const, summary, output });
  if (!caller.running || !caller.listTools().some((entry) => entry.name === tool)) {
    return fail(`Studio tool ${tool} is no longer available. Nothing ran.`);
  }
  if (!sameServer(item.mcpServer, caller.identity)) {
    return fail(`The MCP server changed after this Studio ${item.studioOp} was staged. Nothing ran. Ask for it again.`);
  }
  if (item.studioOp === 'capture') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail('The staged Studio capture request was malformed. Nothing ran.');
    }
    const imageCaller = caller as ImageCapableCaller;
    if (!imageCaller.callToolResult) {
      return fail('This MCP client cannot return image content, so the capture was not saved.');
    }
    if (!saveCapture) {
      return fail('No capture sink is configured, so the image was not saved.');
    }
    try {
      const result = await imageCaller.callToolResult(tool, args);
      if (result.isError) {
        return fail('Studio capture failed.', result.text);
      }
      const image = result.images[0];
      if (!image) {
        return fail('Studio returned no image for the capture.', result.text);
      }
      if (image.data.length > 8_000_000) {
        return fail('Studio returned an image larger than the 8,000,000-character capture limit.');
      }
      const bytes = Buffer.from(image.data, 'base64');
      const captureId = String(args.capture_id ?? 'capture');
      const saved = await saveCapture(bytes, image.mimeType, captureId);
      return {
        ok: true,
        decision: 'approve',
        summary: `Captured the Studio viewport to ${saved}.`,
        output: `${image.mimeType}, ${bytes.length} bytes`,
      };
    } catch (error) {
      return fail('Studio capture failed.', error instanceof Error ? error.message : String(error));
    }
  }
  if (item.studioOp === 'play' || item.studioOp === 'console' || item.studioOp === 'input') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail(`The staged Studio ${item.studioOp} request was malformed. Nothing ran.`);
    }
    try {
      const text = await caller.callTool(tool, args);
      if (item.studioOp === 'play') {
        return {
          ok: true,
          decision: 'approve',
          summary: `${args.is_start === true ? 'Started' : 'Stopped'} playtesting.`,
          output: text.slice(0, 4_000),
        };
      }
      if (item.studioOp === 'input') {
        const count = Array.isArray(args.actions) ? args.actions.length : 0;
        return { ok: true, decision: 'approve', summary: `Sent ${count} input action(s) to Studio.`, output: text.slice(0, 4_000) };
      }
      return { ok: true, decision: 'approve', summary: 'Read the Studio output.', output: text.slice(0, 20_000) };
    } catch (error) {
      return fail(`Studio ${item.studioOp} failed.`, error instanceof Error ? error.message : String(error));
    }
  }
  let text: string;
  try {
    text = await caller.callTool(tool, JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>);
  } catch (error) {
    return fail(`Studio ${item.studioOp} of ${label} failed.`, error instanceof Error ? error.message : String(error));
  }
  const reply = parseLuauReply(text);
  if (!reply.ok) {
    return fail(`Studio ${item.studioOp} of ${label} failed.`, reply.error);
  }
  const result = reply.result;
  if (item.studioOp === 'snapshot') {
    const snapshot = normalizeSnapshot(result, item.studioServices ?? []);
    const lines = snapshot.services.flatMap((service) => service.nodes.map((node) => `${'  '.repeat(node.depth - 1)}${node.path} (${node.className})`));
    const missing = snapshot.missing.length > 0 ? ` Missing services: ${snapshot.missing.join(', ')}.` : '';
    const cut = snapshot.truncated ? ' The tree was cut at 2,000 instances.' : '';
    return { ok: true, decision: 'approve', summary: `Read ${snapshot.nodeCount} instances.${missing}${cut}`, output: lines.slice(0, 400).join('\n'), snapshot };
  }
  if (item.studioOp === 'read') {
    if (result.exists !== true) {
      reads.set(key, { exists: false, source: null });
      return { ok: true, decision: 'approve', summary: `${label} does not exist. Missing: game.${String(result.missing ?? path)}.` };
    }
    if (result.path !== path || typeof result.className !== 'string') {
      return fail(`Studio returned a different instance than ${label}.`, JSON.stringify(result.path ?? null));
    }
    const source = typeof result.source === 'string' ? result.source : null;
    reads.set(key, { exists: true, className: result.className, source });
    return { ok: true, decision: 'approve', summary: `Read ${label} (${result.className}).`, output: source ?? '(not a script)' };
  }
  const className = item.studioClass as ScriptClass;
  const content = item.content ?? '';
  const problems = readbackProblems(parseInstancePath(path), className, content, result);
  if (problems.length > 0) {
    reads.delete(key);
    return fail(`Studio did not confirm the write to ${label}. Read it again before another write.`, problems.join('\n'));
  }
  reads.set(key, { exists: true, className, source: content });
  return {
    ok: true,
    decision: 'approve',
    summary: `Wrote ${label} (${className}). Studio read back the same path, class and source. ${result.undoRecorded === true ? 'Ctrl+Z in Studio undoes it.' : 'Studio did not record an undo step, so Ctrl+Z will not undo it.'}`,
    checkpoint: { path, className, previous: item.previous ?? null, content, studioId: item.studioId },
  };
}

/** Stage putting a checkpoint's previous source back. It only applies if the script still holds what Open-Ro wrote. */
export function stageStudioRestore(approvals: ApprovalQueue, caller: McpCaller, checkpoint: StudioCheckpoint, input: { projectRoot: string }): PendingApproval {
  if (!caller.running || !caller.identity) {
    throw new Error('No MCP server is connected. Connect the Roblox Studio MCP server first.');
  }
  if (checkpoint.previous === null) {
    throw new Error('Open-Ro created this script, so there is no earlier source. Delete it in Studio, or use Ctrl+Z if the write said it recorded an undo step.');
  }
  const segments = parseInstancePath(checkpoint.path);
  const code = buildWriteLuau({ segments, className: checkpoint.className, source: checkpoint.previous, expectedPrevious: checkpoint.content });
  const call = luauToolCall(caller.listTools(), code, checkpoint.studioId);
  return approvals.stageStudio({
    projectRoot: input.projectRoot, op: 'write', path: checkpoint.path, tool: call.tool, args: call.args, studioId: checkpoint.studioId,
    className: checkpoint.className, previous: checkpoint.content, content: checkpoint.previous, server: caller.identity,
  });
}

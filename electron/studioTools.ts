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
import type {
  ApprovalResult,
  McpToolSummary,
  StudioAssetParams,
  StudioAssetResult,
  StudioCaptureParams,
  StudioCaptureResult,
  StudioConsoleParams,
  StudioConsoleResult,
  StudioInspectParams,
  StudioInspectResult,
  StudioKeyboardParams,
  StudioKeyboardResult,
  StudioMouseParams,
  StudioMouseResult,
  StudioMultiEditItem,
  StudioMultiEditParams,
  StudioMultiEditResult,
  StudioNavigateParams,
  StudioNavigateResult,
  StudioPlaytestParams,
  StudioPlaytestResult,
  StudioSearchTreeParams,
  StudioSearchTreeResult,
  ToolCallResult,
} from './types';

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
    if (input.name === 'studio_navigate') {
      if (!caller.listTools().some((tool) => tool.name === 'character_navigation')) {
        return { ok: false, output: 'The connected MCP server has no character_navigation tool.' };
      }
      if (!studioId) {
        return { ok: false, output: 'Studio needs a studioId. Run list_roblox_studios first.' };
      }
      const instancePath = typeof args.instance_path === 'string' ? args.instance_path.trim() : (typeof args.instancePath === 'string' ? args.instancePath.trim() : '');
      const hasCoords = typeof args.x === 'number' && typeof args.y === 'number' && typeof args.z === 'number';
      if (!instancePath && !hasCoords) {
        return { ok: false, output: 'character_navigation requires instance_path or coordinates (x, y, z).' };
      }
      const speedMultiplier = args.speed_multiplier !== undefined || args.speedMultiplier !== undefined
        ? Math.min(Math.max(Number(args.speed_multiplier ?? args.speedMultiplier) || 1.0, 0.1), 10.0)
        : undefined;
      const mcpArgs: Record<string, unknown> = {
        datamodel_type: typeof args.datamodel_type === 'string' ? args.datamodel_type : 'Client',
        studio_id: studioId,
        ...(instancePath ? { instance_path: instancePath } : { x: args.x, y: args.y, z: args.z }),
        ...(speedMultiplier !== undefined ? { speed_multiplier: speedMultiplier } : {}),
      };
      const targetLabel = instancePath ? instancePath : `(${args.x}, ${args.y}, ${args.z})`;
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'input',
        path: instancePath || 'game',
        tool: 'character_navigation',
        args: mcpArgs,
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged character navigation to ${targetLabel} for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: 'character_navigation', path: instancePath || 'game', id: item.id },
      };
    }
    if (input.name === 'studio_search_tree') {
      if (!caller.listTools().some((tool) => tool.name === 'search_game_tree')) {
        return { ok: false, output: 'The connected MCP server has no search_game_tree tool.' };
      }
      const maxDepth = Math.min(Math.max(Math.trunc(Number(args.max_depth ?? args.maxDepth) || 3), 1), 10);
      const headLimit = Math.min(Math.max(Math.trunc(Number(args.head_limit ?? args.headLimit) || 200), 1), 1000);
      const targetPath = args.path ? String(args.path) : undefined;
      const mcpArgs: Record<string, unknown> = {
        datamodel_type: typeof args.datamodel_type === 'string' ? args.datamodel_type : 'Edit',
        max_depth: maxDepth,
        head_limit: headLimit,
        ...(studioId ? { studio_id: studioId } : {}),
        ...(targetPath ? { path: targetPath } : {}),
        ...(args.instance_type ? { instance_type: String(args.instance_type) } : {}),
        ...(args.keywords ? { keywords: String(args.keywords) } : {}),
      };
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'search_tree',
        path: targetPath ?? 'game',
        tool: 'search_game_tree',
        args: mcpArgs,
        server,
        studioId,
      });
      return {
        ok: true,
        output: 'Staged searching game tree for approval. Nothing has run in Studio.',
        pendingApproval: { kind: 'studio' as const, tool: 'search_game_tree', path: targetPath ?? 'game', id: item.id },
      };
    }
    if (input.name === 'studio_inspect') {
      if (!caller.listTools().some((tool) => tool.name === 'inspect_instance')) {
        return { ok: false, output: 'The connected MCP server has no inspect_instance tool.' };
      }
      const targetPath = typeof args.path === 'string' ? args.path.trim() : '';
      if (!targetPath) {
        return { ok: false, output: 'path is required for studio_inspect.' };
      }
      const mcpArgs: Record<string, unknown> = {
        path: targetPath,
        ...(studioId ? { studio_id: studioId } : {}),
      };
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'inspect',
        path: targetPath,
        tool: 'inspect_instance',
        args: mcpArgs,
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged inspecting ${targetPath} for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: 'inspect_instance', path: targetPath, id: item.id },
      };
    }
    if (input.name === 'studio_multi_edit') {
      if (!caller.listTools().some((tool) => tool.name === 'multi_edit')) {
        return { ok: false, output: 'The connected MCP server has no multi_edit tool.' };
      }
      const edits = args.edits;
      if (!Array.isArray(edits) || edits.length === 0) {
        return { ok: false, output: 'edits must be a non-empty array.' };
      }
      const targetPath = typeof args.path === 'string' && args.path
        ? args.path
        : (typeof args.file_path === 'string' && args.file_path
          ? args.file_path
          : (edits[0] && typeof edits[0] === 'object' && 'path' in edits[0] && edits[0].path ? String(edits[0].path) : ''));
      if (!targetPath) {
        return { ok: false, output: 'A script path is required for studio_multi_edit.' };
      }
      const segments = parseInstancePath(targetPath);
      const path = segments.join('.');
      const firstEdit = edits[0] && typeof edits[0] === 'object' ? edits[0] as Record<string, unknown> : {};
      const classNameArg = typeof args.className === 'string' ? args.className : (typeof firstEdit.className === 'string' ? firstEdit.className : undefined);
      const seen = reads.get(studioReadKey(server.session, studioId, path));
      if (!seen) {
        return { ok: false, output: `Read ${formatPath(segments)} with studio_read_script first, so the review shows what changes.` };
      }
      if (seen.exists && classNameArg && seen.className && seen.className !== classNameArg) {
        return { ok: false, output: `${formatPath(segments)} is a ${seen.className}, not a ${classNameArg}. Nothing was staged.` };
      }
      const actualClass = (classNameArg ?? seen.className) as ScriptClass;
      if (!isScriptClass(actualClass)) {
        return { ok: false, output: 'className must be Script, LocalScript or ModuleScript.' };
      }
      let current = seen.exists && typeof seen.source === 'string' ? seen.source : '';
      const previous = seen.exists ? seen.source : null;
      for (const edit of edits) {
        if (!edit || typeof edit !== 'object') {
          return { ok: false, output: 'Each edit must be an object with old_string and new_string.' };
        }
        const oldStr = typeof edit.old_string === 'string' ? edit.old_string : '';
        const newStr = typeof edit.new_string === 'string' ? edit.new_string : '';
        if (oldStr.length > 0 && !current.includes(oldStr)) {
          return { ok: false, output: `old_string not found in ${path}. Read or inspect the script first to obtain current source.` };
        }
        if (edit.replace_all === true && oldStr.length > 0) {
          current = current.split(oldStr).join(newStr);
        } else if (oldStr.length > 0) {
          current = current.replace(oldStr, newStr);
        } else if (!previous) {
          current = newStr;
        }
      }
      const problems = [placementProblem(segments, actualClass), ...currencyProblems(segments, actualClass, current)].filter((item): item is string => !!item);
      if (problems.length > 0) {
        return { ok: false, output: `Not staged. ${problems.join(' ')}` };
      }
      const cleanEdits = edits.map((e) => ({
        old_string: typeof e.old_string === 'string' ? e.old_string : '',
        new_string: typeof e.new_string === 'string' ? e.new_string : '',
        ...(typeof e.replace_all === 'boolean' ? { replace_all: e.replace_all } : {}),
      }));
      const mcpArgs: Record<string, unknown> = {
        datamodel_type: 'Edit',
        file_path: formatPath(segments),
        edits: cleanEdits,
        ...(studioId ? { studio_id: studioId } : {}),
        ...(actualClass ? { className: actualClass } : {}),
      };
      const item = approvals.stageStudio({
        projectRoot: input.projectRoot,
        op: 'multi_edit',
        path,
        tool: 'multi_edit',
        args: mcpArgs,
        className: actualClass,
        previous: previous ?? undefined,
        content: current,
        server,
        studioId,
      });
      return {
        ok: true,
        output: `Staged Studio multi_edit of ${path} for approval. Nothing has run in Studio.`,
        pendingApproval: { kind: 'studio' as const, tool: 'multi_edit', path, id: item.id },
      };
    }
    if (input.name === 'studio_asset') {
      const action = args.action;
      if (action !== 'search' && action !== 'insert') {
        return { ok: false, output: 'action must be "search" or "insert".' };
      }
      if (action === 'search') {
        if (!caller.listTools().some((tool) => tool.name === 'search_asset')) {
          return { ok: false, output: 'The connected MCP server has no search_asset tool.' };
        }
        const query = typeof args.keyword === 'string' ? args.keyword : (typeof args.query === 'string' ? args.query : '');
        const maxResults = Math.min(Math.max(Number(args.max_results ?? args.maxResults) || 5, 1), 20);
        const mcpArgs: Record<string, unknown> = {
          query,
          maxResults,
          ...(studioId ? { studio_id: studioId } : {}),
          ...(args.asset_type ? { assetType: String(args.asset_type) } : {}),
          ...(args.scope ? { scope: String(args.scope) } : {}),
        };
        const item = approvals.stageStudio({
          projectRoot: input.projectRoot,
          op: 'asset',
          path: 'CreatorStore',
          tool: 'search_asset',
          args: mcpArgs,
          server,
          studioId,
        });
        return {
          ok: true,
          output: 'Staged searching assets for approval. Nothing has run in Studio.',
          pendingApproval: { kind: 'studio' as const, tool: 'search_asset', path: 'CreatorStore', id: item.id },
        };
      } else {
        if (!caller.listTools().some((tool) => tool.name === 'insert_asset')) {
          return { ok: false, output: 'The connected MCP server has no insert_asset tool.' };
        }
        const assetId = args.asset_id ?? args.assetId;
        if (assetId === undefined || assetId === null || String(assetId).trim() === '') {
          return { ok: false, output: 'asset_id is required for inserting an asset.' };
        }
        const parentPath = typeof args.target_parent === 'string' ? args.target_parent : (typeof args.parentPath === 'string' ? args.parentPath : 'Workspace');
        const mcpArgs: Record<string, unknown> = {
          assetId: String(assetId),
          parentPath,
          ...(studioId ? { studio_id: studioId } : {}),
          ...(args.asset_type ? { assetType: String(args.asset_type) } : {}),
          ...(args.asset_name ? { assetName: String(args.asset_name) } : {}),
        };
        const item = approvals.stageStudio({
          projectRoot: input.projectRoot,
          op: 'asset',
          path: parentPath,
          tool: 'insert_asset',
          args: mcpArgs,
          server,
          studioId,
        });
        return {
          ok: true,
          output: `Staged inserting asset ${assetId} under ${parentPath} for approval. Nothing has run in Studio.`,
          pendingApproval: { kind: 'studio' as const, tool: 'insert_asset', path: parentPath, id: item.id },
        };
      }
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
        if (tool === 'character_navigation') {
          return { ok: true, decision: 'approve', summary: 'Navigated character in Studio.', output: text.slice(0, 4_000) };
        }
        const count = Array.isArray(args.actions) ? args.actions.length : 0;
        return { ok: true, decision: 'approve', summary: `Sent ${count} input action(s) to Studio.`, output: text.slice(0, 4_000) };
      }
      return { ok: true, decision: 'approve', summary: 'Read the Studio output.', output: text.slice(0, 20_000) };
    } catch (error) {
      return fail(`Studio ${item.studioOp} failed.`, error instanceof Error ? error.message : String(error));
    }
  }
  if (item.studioOp === 'search_tree') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail('The staged Studio search_tree request was malformed. Nothing ran.');
    }
    try {
      const text = await caller.callTool(tool, args);
      let count = 0;
      try {
        const parsed = JSON.parse(text);
        if (Array.isArray(parsed)) {
          count = parsed.length;
        } else if (parsed && typeof parsed === 'object') {
          const arr = (parsed as Record<string, unknown>).instances ?? (parsed as Record<string, unknown>).nodes;
          if (Array.isArray(arr)) {
            count = arr.length;
          }
        }
      } catch {
        // text was not json
      }
      return {
        ok: true,
        decision: 'approve',
        summary: `Found ${count} instance(s) in DataModel.`,
        output: text,
      };
    } catch (error) {
      return fail('Studio search_tree failed.', error instanceof Error ? error.message : String(error));
    }
  }
  if (item.studioOp === 'inspect') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail('The staged Studio inspect request was malformed. Nothing ran.');
    }
    try {
      const text = await caller.callTool(tool, args);
      try {
        const parsed = JSON.parse(text) as Record<string, unknown>;
        const data = Array.isArray(parsed.matches) && parsed.matches[0] && typeof parsed.matches[0] === 'object'
          ? (parsed.matches[0] as Record<string, unknown>)
          : parsed;
        const className = typeof data.className === 'string' ? data.className : undefined;
        const props = data.properties && typeof data.properties === 'object' ? (data.properties as Record<string, unknown>) : {};
        const source = typeof props.Source === 'string' ? props.Source : (typeof props.source === 'string' ? props.source : null);
        reads.set(key, { exists: true, className, source });
      } catch {
        // non-json or partial response
      }
      return {
        ok: true,
        decision: 'approve',
        summary: `Inspected ${label}.`,
        output: text,
      };
    } catch (error) {
      return fail('Studio inspect failed.', error instanceof Error ? error.message : String(error));
    }
  }
  if (item.studioOp === 'multi_edit') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail('The staged Studio multi_edit request was malformed. Nothing ran.');
    }
    const className = (item.studioClass ?? 'Script') as ScriptClass;
    const content = item.content ?? '';
    try {
      const text = await caller.callTool(tool, args);
      try {
        const parsed = JSON.parse(text);
        if (parsed && typeof parsed === 'object' && parsed.success === false) {
          return fail(`Studio multi_edit of ${label} failed.`, String(parsed.message ?? text));
        }
      } catch {
        if (text.toLowerCase().includes('error:')) {
          return fail(`Studio multi_edit of ${label} failed.`, text);
        }
      }
      reads.set(key, { exists: true, className, source: content });
      return {
        ok: true,
        decision: 'approve',
        summary: `Wrote batch edits to ${label} (${className}).`,
        output: text,
        checkpoint: { path, className, previous: item.previous ?? null, content, studioId: item.studioId },
      };
    } catch (error) {
      return fail(`Studio multi_edit of ${label} failed.`, error instanceof Error ? error.message : String(error));
    }
  }
  if (item.studioOp === 'asset') {
    let args: Record<string, unknown>;
    try {
      args = JSON.parse(item.mcpArgs ?? '{}') as Record<string, unknown>;
    } catch {
      return fail('The staged Studio asset request was malformed. Nothing ran.');
    }
    try {
      const text = await caller.callTool(tool, args);
      if (tool === 'search_asset') {
        let count = 0;
        try {
          const parsed = JSON.parse(text);
          if (Array.isArray(parsed?.results)) {
            count = parsed.results.length;
          }
        } catch {
          // non-json
        }
        return {
          ok: true,
          decision: 'approve',
          summary: `Found ${count} asset(s) in Creator Store.`,
          output: text,
        };
      }
      return {
        ok: true,
        decision: 'approve',
        summary: `Inserted asset ${String(args.assetId ?? '')} under ${String(args.parentPath ?? 'Workspace')}.`,
        output: text,
      };
    } catch (error) {
      return fail(`Studio asset tool ${tool} failed.`, error instanceof Error ? error.message : String(error));
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

function isMcpCaller(candidate: unknown): candidate is McpCaller {
  return typeof candidate === 'object' && candidate !== null && 'callTool' in candidate && typeof (candidate as McpCaller).callTool === 'function';
}

function extractCallerAndParams<P extends object>(
  arg1: unknown,
  arg2?: unknown,
  arg3?: unknown,
): { caller: McpCaller; params: P; approvals?: ApprovalQueue } {
  let caller: McpCaller | undefined;
  let params: P | undefined;
  let approvals: ApprovalQueue | undefined;

  if (isMcpCaller(arg1)) {
    caller = arg1;
    params = (arg2 ?? {}) as P;
    if (arg3 && typeof arg3 === 'object' && 'stageMcp' in arg3) {
      approvals = arg3 as ApprovalQueue;
    }
  } else {
    params = (arg1 ?? {}) as P;
    if (isMcpCaller(arg2)) {
      caller = arg2;
    } else if (arg2 && typeof arg2 === 'object' && 'stageMcp' in arg2) {
      approvals = arg2 as ApprovalQueue;
      if (isMcpCaller(arg3)) {
        caller = arg3;
      }
    } else if (isMcpCaller(arg3)) {
      caller = arg3;
    }
    if (!caller && (params as { caller?: McpCaller }).caller) {
      caller = (params as { caller?: McpCaller }).caller;
    }
  }

  if (!caller) {
    throw new Error('No MCP caller provided.');
  }
  if (!caller.running) {
    throw new Error('Studio MCP server is disconnected.');
  }
  return { caller, params, approvals };
}

export async function studio_search_tree(
  caller: McpCaller,
  params?: StudioSearchTreeParams,
): Promise<StudioSearchTreeResult>;
export async function studio_search_tree(
  params: StudioSearchTreeParams,
  caller?: McpCaller,
): Promise<StudioSearchTreeResult>;
export async function studio_search_tree(
  arg1: unknown,
  arg2?: unknown,
): Promise<StudioSearchTreeResult> {
  const { caller, params } = extractCallerAndParams<StudioSearchTreeParams>(arg1, arg2);
  const args: Record<string, unknown> = {
    datamodel_type: params.datamodel_type ?? 'Edit',
    ...(params.max_depth !== undefined ? { max_depth: Math.min(Math.max(Math.trunc(Number(params.max_depth)) || 1, 1), 10) } : {}),
    ...(params.head_limit !== undefined ? { head_limit: Math.min(Math.max(Math.trunc(Number(params.head_limit)) || 1, 1), 1000) } : {}),
    ...(params.studioId ? { studio_id: params.studioId } : {}),
    ...(params.path ? { path: params.path } : {}),
    ...(params.instance_type ? { instance_type: params.instance_type } : {}),
    ...(params.keywords ? { keywords: params.keywords } : {}),
  };
  const raw = await caller.callTool('search_game_tree', args);
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    parsed = [];
  }
  let items: Array<Record<string, unknown>> = [];
  if (Array.isArray(parsed)) {
    items = parsed;
  } else if (parsed && typeof parsed === 'object') {
    const list = (parsed as Record<string, unknown>).instances ?? (parsed as Record<string, unknown>).nodes;
    if (Array.isArray(list)) {
      items = list as Array<Record<string, unknown>>;
    }
  }
  return {
    instances: items.map((i) => ({
      path: String(i.fullPath ?? i.path ?? ''),
      className: String(i.className ?? ''),
      name: String(i.name ?? ''),
    })),
  };
}

export async function studio_inspect(
  caller: McpCaller,
  params: StudioInspectParams,
): Promise<StudioInspectResult>;
export async function studio_inspect(
  params: StudioInspectParams,
  caller?: McpCaller,
): Promise<StudioInspectResult>;
export async function studio_inspect(
  arg1: unknown,
  arg2?: unknown,
): Promise<StudioInspectResult> {
  const { caller, params } = extractCallerAndParams<StudioInspectParams>(arg1, arg2);
  if (!params.path || typeof params.path !== 'string') {
    throw new Error('Missing path parameter for studio_inspect.');
  }
  const args: Record<string, unknown> = {
    path: params.path,
    ...(params.max_depth !== undefined ? { max_depth: params.max_depth } : {}),
    ...(params.studioId ? { studio_id: params.studioId } : {}),
  };
  const raw = await caller.callTool('inspect_instance', args);
  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    throw new Error(`Failed to parse inspect_instance output: ${raw}`);
  }
  const data = Array.isArray(parsed.matches) && parsed.matches[0] && typeof parsed.matches[0] === 'object'
    ? (parsed.matches[0] as Record<string, unknown>)
    : parsed;
  const childrenData = data.children;
  const children = Array.isArray((childrenData as Record<string, unknown>)?.immediateChildren)
    ? (childrenData as Record<string, unknown>).immediateChildren as Array<unknown>
    : (Array.isArray(childrenData) ? childrenData : []);
  return {
    path: String(data.path ?? params.path),
    className: String(data.className ?? ''),
    name: typeof data.name === 'string' ? data.name : undefined,
    uniqueId: typeof data.uniqueId === 'string' ? data.uniqueId : undefined,
    properties: (data.properties && typeof data.properties === 'object' ? data.properties : {}) as Record<string, unknown>,
    attributes: (data.attributes && typeof data.attributes === 'object' ? data.attributes : {}) as Record<string, unknown>,
    children,
    matches: Array.isArray(parsed.matches) ? (parsed.matches as StudioInspectResult['matches']) : undefined,
    warning: typeof parsed.warning === 'string' ? parsed.warning : undefined,
  };
}

export async function studio_multi_edit(
  caller: McpCaller,
  params: StudioMultiEditParams,
): Promise<StudioMultiEditResult>;
export async function studio_multi_edit(
  params: StudioMultiEditParams,
  approvals?: ApprovalQueue,
  caller?: McpCaller,
): Promise<StudioMultiEditResult>;
export async function studio_multi_edit(
  arg1: unknown,
  arg2?: unknown,
  arg3?: unknown,
): Promise<StudioMultiEditResult> {
  const { caller, params, approvals } = extractCallerAndParams<StudioMultiEditParams>(arg1, arg2, arg3);
  if (!Array.isArray(params.edits)) {
    throw new Error('Edits must be an array.');
  }
  for (const edit of params.edits) {
    if (edit.className && !['Script', 'LocalScript', 'ModuleScript'].includes(edit.className)) {
      throw new Error(`Invalid script class: ${edit.className}`);
    }
  }

  const edits = params.edits;
  const targetPath = params.path ?? params.file_path ?? (edits[0]?.path ? String(edits[0].path) : '');
  const cleanEdits = edits.map((e) => ({
    path: e.path ?? targetPath,
    old_string: e.old_string,
    new_string: typeof e.new_string === 'string' ? e.new_string : '',
    ...(e.className ? { className: e.className } : {}),
    ...(typeof e.replace_all === 'boolean' ? { replace_all: e.replace_all } : {}),
  }));

  if (approvals && caller.identity) {
    const staged = approvals.stageMcp({
      projectRoot: '/mock-project',
      tool: 'multi_edit',
      args: { edits: cleanEdits },
      server: caller.identity,
    });
    approvals.take(staged.id, 'mcp');
  }

  const className = params.className ?? edits.find((e) => e.className)?.className;
  const args: Record<string, unknown> = {
    datamodel_type: 'Edit',
    file_path: targetPath,
    edits: cleanEdits,
    ...(params.studioId ? { studio_id: params.studioId } : {}),
    ...(className ? { className } : {}),
  };
  const raw = await caller.callTool('multi_edit', args);
  let parsed: Record<string, unknown> = {};
  try {
    parsed = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    // raw might be non-JSON
  }
  if (parsed.success === false) {
    return { success: false, modified_scripts: [] };
  }
  const modified = Array.isArray(parsed.modified_scripts)
    ? parsed.modified_scripts.map(String)
    : (targetPath ? [targetPath] : []);
  return { success: true, modified_scripts: modified };
}

export async function studio_asset(
  caller: McpCaller,
  params: StudioAssetParams,
): Promise<StudioAssetResult>;
export async function studio_asset(
  params: StudioAssetParams,
  approvals?: ApprovalQueue,
  caller?: McpCaller,
): Promise<StudioAssetResult>;
export async function studio_asset(
  arg1: unknown,
  arg2?: unknown,
  arg3?: unknown,
): Promise<StudioAssetResult> {
  const { caller, params, approvals } = extractCallerAndParams<StudioAssetParams>(arg1, arg2, arg3);
  if (params.action !== 'search' && params.action !== 'insert') {
    throw new Error(`Unsupported studio_asset action: ${String(params.action)}`);
  }

  if (params.action === 'search') {
    const query = params.keyword ?? params.query ?? '';
    const maxResults = Math.min(Math.max(Number(params.max_results ?? params.maxResults) || 5, 1), 20);
    const args: Record<string, unknown> = {
      query,
      keyword: query,
      maxResults,
      ...(params.studioId ? { studio_id: params.studioId } : {}),
      ...(params.asset_type ? { assetType: params.asset_type, asset_type: params.asset_type } : {}),
      ...(params.scope ? { scope: params.scope } : {}),
      ...(params.priceFilter ? { priceFilter: params.priceFilter } : {}),
    };
    const raw = await caller.callTool('search_asset', args);
    let parsed: Record<string, unknown> = {};
    try {
      parsed = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      parsed = {};
    }
    return { results: Array.isArray(parsed.results) ? parsed.results : [] };
  }

  if (params.action === 'insert') {
    if (params.asset_id === undefined || Number(params.asset_id) <= 0) {
      throw new Error(`Invalid asset ID: ${String(params.asset_id)}`);
    }
    const assetId = String(params.asset_id);
    const parentPath = params.target_parent ?? params.parentPath ?? 'Workspace';
    const args: Record<string, unknown> = {
      assetId,
      asset_id: Number(assetId) || assetId,
      parentPath,
      target_parent: parentPath,
      ...(params.studioId ? { studio_id: params.studioId } : {}),
      ...(params.asset_type ? { assetType: params.asset_type, asset_type: params.asset_type } : {}),
    };
    if (approvals && caller.identity) {
      const staged = approvals.stageMcp({
        projectRoot: '/mock-project',
        tool: 'insert_asset',
        args,
        server: caller.identity,
      });
      approvals.take(staged.id, 'mcp');
    }
    const raw = await caller.callTool('insert_asset', args);
    let insertedInstance = raw;
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      if (typeof parsed.inserted_instance === 'string') {
        insertedInstance = parsed.inserted_instance;
      } else if (typeof parsed.instancePath === 'string') {
        insertedInstance = parsed.instancePath;
      }
    } catch {
      // raw is string
    }
    return { inserted_instance: insertedInstance };
  }

  throw new Error(`Unsupported studio_asset action: ${String(params.action)}`);
}

export async function studio_navigate(
  caller: McpCaller,
  params: StudioNavigateParams,
): Promise<StudioNavigateResult>;
export async function studio_navigate(
  params: StudioNavigateParams,
  caller?: McpCaller,
): Promise<StudioNavigateResult>;
export async function studio_navigate(
  arg1: unknown,
  arg2?: unknown,
): Promise<StudioNavigateResult> {
  const { caller, params } = extractCallerAndParams<StudioNavigateParams>(arg1, arg2);
  const instancePath = typeof params.instance_path === 'string' ? params.instance_path.trim() : undefined;
  const hasCoords = typeof params.x === 'number' && typeof params.y === 'number' && typeof params.z === 'number';
  if (!instancePath && !hasCoords) {
    throw new Error('character_navigation requires instance_path or coordinates (x, y, z).');
  }
  const speedMultiplier = params.speed_multiplier !== undefined
    ? Math.min(Math.max(Number(params.speed_multiplier) || 1.0, 0.1), 10.0)
    : undefined;
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    datamodel_type: params.datamodel_type ?? 'Client',
    ...(studioId ? { studio_id: studioId } : {}),
    ...(instancePath ? { instance_path: instancePath } : { x: params.x, y: params.y, z: params.z }),
    ...(speedMultiplier !== undefined ? { speed_multiplier: speedMultiplier } : {}),
  };
  const raw = await caller.callTool('character_navigation', args);
  return { success: true, output: raw };
}

export async function studio_keyboard(
  caller: McpCaller,
  params: StudioKeyboardParams,
): Promise<StudioKeyboardResult>;
export async function studio_keyboard(
  params: StudioKeyboardParams,
  caller?: McpCaller,
): Promise<StudioKeyboardResult>;
export async function studio_keyboard(
  arg1: unknown,
  arg2?: unknown,
): Promise<StudioKeyboardResult> {
  const { caller, params } = extractCallerAndParams<StudioKeyboardParams>(arg1, arg2);
  if (!Array.isArray(params.actions) || params.actions.length === 0) {
    throw new Error('actions must be a non-empty array.');
  }
  const cleanActions = params.actions.map((item) => {
    const act: Record<string, unknown> = { ...item };
    if (act.action === 'textInput' && act.text && !act.text_inputs) {
      act.text_inputs = act.text;
    }
    return act;
  });
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    datamodel_type: params.datamodel_type ?? 'Client',
    actions: cleanActions,
    ...(studioId ? { studio_id: studioId } : {}),
  };
  const raw = await caller.callTool('user_keyboard_input', args);
  return { success: true, output: raw };
}

export async function studio_mouse(
  caller: McpCaller,
  params: StudioMouseParams,
): Promise<StudioMouseResult>;
export async function studio_mouse(
  params: StudioMouseParams,
  caller?: McpCaller,
): Promise<StudioMouseResult>;
export async function studio_mouse(
  arg1: unknown,
  arg2?: unknown,
): Promise<StudioMouseResult> {
  const { caller, params } = extractCallerAndParams<StudioMouseParams>(arg1, arg2);
  if (!Array.isArray(params.actions) || params.actions.length === 0) {
    throw new Error('actions must be a non-empty array.');
  }
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    datamodel_type: params.datamodel_type ?? 'Client',
    actions: params.actions,
    ...(studioId ? { studio_id: studioId } : {}),
  };
  const raw = await caller.callTool('user_mouse_input', args);
  return { success: true, output: raw };
}

export async function studio_playtest(
  caller: McpCaller,
  params?: StudioPlaytestParams,
): Promise<StudioPlaytestResult>;
export async function studio_playtest(
  params: StudioPlaytestParams,
  caller?: McpCaller,
): Promise<StudioPlaytestResult>;
export async function studio_playtest(
  arg1?: unknown,
  arg2?: unknown,
): Promise<StudioPlaytestResult> {
  const { caller, params } = extractCallerAndParams<StudioPlaytestParams>(arg1 ?? {}, arg2);
  const isStop = params.mode === 'Stop' || params.isStart === false || params.is_start === false;
  const isStart = !isStop;
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    is_start: isStart,
    mode: isStart ? (params.mode ?? 'Play') : 'Stop',
    ...(studioId ? { studio_id: studioId } : {}),
  };
  const raw = await caller.callTool('start_stop_play', args);
  return { success: true, output: raw };
}

export async function studio_console(
  caller: McpCaller,
  params?: StudioConsoleParams,
): Promise<StudioConsoleResult>;
export async function studio_console(
  params: StudioConsoleParams,
  caller?: McpCaller,
): Promise<StudioConsoleResult>;
export async function studio_console(
  arg1?: unknown,
  arg2?: unknown,
): Promise<StudioConsoleResult> {
  const { caller, params } = extractCallerAndParams<StudioConsoleParams>(arg1 ?? {}, arg2);
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    ...(studioId ? { studio_id: studioId } : {}),
  };
  const raw = await caller.callTool('get_console_output', args);
  const logs = typeof raw === 'string' ? raw.split('\n').filter((l) => l.trim().length > 0) : [];
  return { logs, rawText: raw };
}

export async function studio_capture(
  caller: McpCaller,
  params?: StudioCaptureParams,
  saveCapture?: (data: Buffer, mimeType: string, captureId: string) => Promise<string>,
): Promise<StudioCaptureResult>;
export async function studio_capture(
  params: StudioCaptureParams,
  caller?: McpCaller,
  saveCapture?: (data: Buffer, mimeType: string, captureId: string) => Promise<string>,
): Promise<StudioCaptureResult>;
export async function studio_capture(
  arg1?: unknown,
  arg2?: unknown,
  arg3?: unknown,
): Promise<StudioCaptureResult> {
  let saveSink: ((data: Buffer, mimeType: string, captureId: string) => Promise<string>) | undefined;
  if (typeof arg2 === 'function') {
    saveSink = arg2 as (data: Buffer, mimeType: string, captureId: string) => Promise<string>;
  } else if (typeof arg3 === 'function') {
    saveSink = arg3 as (data: Buffer, mimeType: string, captureId: string) => Promise<string>;
  }
  const { caller, params } = extractCallerAndParams<StudioCaptureParams>(arg1 ?? {}, arg2, arg3);
  const captureId = params.captureId ?? params.capture_id ?? `capture-${Date.now()}`;
  const studioId = params.studioId ?? params.studio_id;
  const args: Record<string, unknown> = {
    capture_id: captureId,
    ...(studioId ? { studio_id: studioId } : {}),
    ...(params.cameraPosition ? { camera_position: params.cameraPosition } : {}),
    ...(params.lookAtPosition ? { look_at_position: params.lookAtPosition } : {}),
  };

  const imageCaller = caller as McpCaller & {
    callToolResult?(name: string, args: Record<string, unknown>): Promise<{ text: string; images: Array<{ mimeType: string; data: string }>; isError: boolean }>;
  };
  if (typeof imageCaller.callToolResult === 'function') {
    const res = await imageCaller.callToolResult('screen_capture', args);
    const img = res.images?.[0];
    if (!img) {
      throw new Error(`screen_capture returned no images: ${res.text}`);
    }
    const buf = Buffer.from(img.data, 'base64');
    let savedPath = '';
    if (saveSink) {
      savedPath = await saveSink(buf, img.mimeType, captureId);
    }
    return { savedPath, bytes: buf.length, mimeType: img.mimeType };
  }

  const raw = await caller.callTool('screen_capture', args);
  let savedPath = '';
  let bytes = 0;
  let mimeType = 'image/png';
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const b64 = typeof parsed.base64 === 'string' ? parsed.base64 : (typeof parsed.data === 'string' ? parsed.data : '');
    if (b64) {
      const buf = Buffer.from(b64, 'base64');
      bytes = buf.length;
      if (saveSink) {
        savedPath = await saveSink(buf, mimeType, captureId);
      }
    }
  } catch {
    bytes = Buffer.byteLength(raw, 'utf8');
  }
  return { savedPath, bytes, mimeType };
}



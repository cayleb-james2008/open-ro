/**
 * Checks for Open-Ro's Roblox Studio edits: instance paths, script placement,
 * server-owned currency, and the replies Studio sends back. Replies are
 * untrusted data; nothing is filled in that Studio did not report.
 */

export const SCRIPT_CLASSES = ['Script', 'LocalScript', 'ModuleScript'] as const;
export type ScriptClass = (typeof SCRIPT_CLASSES)[number];

/** Where each script class does its job. A server Script in StarterGui, or a LocalScript in ServerScriptService, never runs. */
const PLACEMENT: Record<ScriptClass, string[]> = {
  Script: ['ServerScriptService', 'Workspace', 'ServerStorage'],
  LocalScript: ['StarterGui', 'StarterPlayer', 'StarterPack', 'ReplicatedFirst'],
  ModuleScript: ['ReplicatedStorage', 'ServerScriptService', 'ServerStorage', 'StarterPlayer', 'StarterGui', 'ReplicatedFirst'],
};
const CLIENT_SERVICES = new Set(['StarterGui', 'StarterPlayer', 'StarterPack', 'ReplicatedFirst']);
/** Prefix of the one line Open-Ro's Luau prints and returns. */
export const MARKER = 'OPENRO:';

/** "game.ServerScriptService.Shop" or "ServerScriptService.Shop" to name segments. Names containing dots are refused because the path would be ambiguous. */
export function parseInstancePath(path: string): string[] {
  const parts = path.trim().split('.');
  if (parts[0] === 'game') {
    parts.shift();
  }
  if (parts.length === 0 || parts.some((part) => part.length === 0 || part.length > 100 || /[\u0000-\u001f]/.test(part))) {
    throw new Error(`Not a usable instance path: ${JSON.stringify(path)}. Use dot paths like game.ServerScriptService.Shop.`);
  }
  return parts;
}

export function formatPath(segments: string[]): string {
  return ['game', ...segments].join('.');
}

export function isScriptClass(value: unknown): value is ScriptClass {
  return typeof value === 'string' && (SCRIPT_CLASSES as readonly string[]).includes(value);
}

/** Refuse a script that would sit where it cannot run. */
export function placementProblem(segments: string[], className: ScriptClass): string | null {
  if (segments.length < 2) {
    return 'A script needs a name under a service, for example game.ServerScriptService.Shop.';
  }
  const allowed = PLACEMENT[className];
  return allowed.includes(segments[0]) ? null : `A ${className} does not run in ${segments[0]}. Put it in ${allowed.join(', ')}.`;
}

const CURRENCY = /\b(leaderstats|coins?|cash|gems?|gold|currency|money|points|rewards?|credits?)\b/i;
const VALUE_WRITE = /\.Value\s*(?:[-+*/]?=)(?!=)|:(?:IncrementAsync|SetAsync|UpdateAsync)\s*\(/;

/** Server-owned currency rules (S03): clients never change it, the server types and checks it. Heuristic, line-based. */
export function currencyProblems(segments: string[], className: ScriptClass, source: string): string[] {
  const writes = source.split(/\r?\n/).filter((line) => !/^\s*--/.test(line) && CURRENCY.test(line) && VALUE_WRITE.test(line));
  if (writes.length === 0) {
    return [];
  }
  if (className === 'LocalScript' || CLIENT_SERVICES.has(segments[0])) {
    return [`Client code changes currency or rewards (${writes[0].trim()}). The server must own those values; send a RemoteEvent and change them in ServerScriptService.`];
  }
  const problems: string[] = [];
  if (!/^\s*--!strict\b/.test(source)) {
    problems.push('Server code that changes currency or rewards must start with --!strict so Luau type-checks it.');
  }
  if (/OnServerEvent/.test(source) && !/typeof\s*\(/.test(source)) {
    problems.push('This handler changes currency from a client request without checking argument types. Check them with typeof() first.');
  }
  return problems;
}

export type LuauReply = { ok: true; result: Record<string, unknown> } | { ok: false; error: string };

/** Find Open-Ro's marked reply in a tool's text output, raw or JSON-quoted. */
export function parseLuauReply(text: string): LuauReply {
  const candidates = [text];
  try {
    const unquoted: unknown = JSON.parse(text);
    if (typeof unquoted === 'string') {
      candidates.push(unquoted);
    }
  } catch {
    // Not a JSON string; the raw text is the only candidate.
  }
  for (const candidate of candidates) {
    const start = candidate.indexOf(MARKER);
    if (start < 0) {
      continue;
    }
    const line = candidate.slice(start + MARKER.length).split(/\r?\n/)[0];
    try {
      const reply = JSON.parse(line) as { ok?: unknown; result?: unknown; error?: unknown };
      if (reply.ok === true && reply.result && typeof reply.result === 'object' && !Array.isArray(reply.result)) {
        return { ok: true, result: reply.result as Record<string, unknown> };
      }
      if (reply.ok === false) {
        return { ok: false, error: typeof reply.error === 'string' ? reply.error : 'Studio reported an error without a message.' };
      }
    } catch {
      // Try the next candidate.
    }
  }
  return { ok: false, error: `Studio did not return Open-Ro's reply. Output began: ${text.slice(0, 200)}` };
}

export interface SnapshotNode { path: string; className: string; depth: number }
export interface TreeSnapshot {
  services: Array<{ name: string; present: boolean; nodes: SnapshotNode[] }>;
  missing: string[];
  truncated: boolean;
  nodeCount: number;
}

/** Validate a snapshot reply. Only services that were asked for are kept, and a service is present only when Studio said so. */
export function normalizeSnapshot(result: Record<string, unknown>, requested: string[]): TreeSnapshot {
  const reported = Array.isArray(result.services) ? result.services : [];
  const services = requested.map((name) => {
    const entry = reported.find((item): item is Record<string, unknown> => !!item && typeof item === 'object' && (item as Record<string, unknown>).name === name);
    const rawNodes = entry && Array.isArray(entry.nodes) ? entry.nodes : [];
    const nodes = rawNodes.filter((node): node is SnapshotNode =>
      !!node && typeof node === 'object' && typeof (node as SnapshotNode).path === 'string' &&
      typeof (node as SnapshotNode).className === 'string' && Number.isInteger((node as SnapshotNode).depth));
    return { name, present: entry?.present === true, nodes: entry?.present === true ? nodes : [] };
  });
  return {
    services,
    missing: services.filter((service) => !service.present).map((service) => service.name),
    truncated: result.truncated === true,
    nodeCount: services.reduce((sum, service) => sum + service.nodes.length, 0),
  };
}

/** After a write: the instance Studio reports must be the one that was reviewed, with the reviewed source. */
export function readbackProblems(segments: string[], className: ScriptClass, source: string, result: Record<string, unknown>): string[] {
  const problems: string[] = [];
  if (result.path !== segments.join('.')) {
    problems.push(`Read-back path is ${JSON.stringify(result.path ?? null)}, expected ${segments.join('.')}.`);
  }
  if (result.className !== className) {
    problems.push(`Read-back class is ${JSON.stringify(result.className ?? null)}, expected ${className}.`);
  }
  if (result.source !== source) {
    problems.push('Read-back source does not match the approved source.');
  }
  return problems;
}

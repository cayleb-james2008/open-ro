import * as fs from 'node:fs/promises';
import * as path from 'node:path';

/** Directory names that must never be listed, read, or written through project tools. */
export const IGNORED_SEGMENTS = new Set(['.git', 'node_modules', 'dist', 'dist-electron', 'release', 'coverage']);

const SECRET_SEGMENT = /^(?:\.env(?:\..*)?|.*\.(?:pem|key|p12|pfx)|id_rsa|id_ed25519|credentials\.json)$/i;

export function isIgnoredName(name: string): boolean {
  return IGNORED_SEGMENTS.has(name);
}

export function isSecretName(name: string): boolean {
  return SECRET_SEGMENT.test(name);
}

/**
 * Two project roots name the same folder when their resolved forms match.
 * Windows compares case-insensitively; POSIX paths are case-sensitive.
 */
export function sameProjectRoot(left: string, right: string): boolean {
  const a = path.resolve(left);
  const b = path.resolve(right);
  return process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
}

/** True when `target` is `root` or a descendant. Prefix tricks such as `/proj` vs `/proj-evil` fail this check. */
export function isInsideRoot(root: string, target: string): boolean {
  const relative = path.relative(path.resolve(root), path.resolve(target));
  return relative === '' || (relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative));
}

export function assertProjectRelative(rel: string): void {
  const parts = (rel || '.').split(/[/\\]+/).filter((part) => part.length > 0 && part !== '.');
  for (const part of parts) {
    if (part === '..') {
      throw new Error('Path escapes the selected project root.');
    }
    if (IGNORED_SEGMENTS.has(part)) {
      throw new Error(`Path is inside an ignored folder (${part}).`);
    }
    if (SECRET_SEGMENT.test(part)) {
      throw new Error('Refusing to read or write a likely secret file.');
    }
  }
}

async function nearestExisting(start: string): Promise<{ existing: string; missing: string[] }> {
  const missing: string[] = [];
  let cursor = start;
  for (;;) {
    try {
      const real = await fs.realpath(cursor);
      return { existing: real, missing };
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== 'ENOENT') {
        throw error;
      }
      const parent = path.dirname(cursor);
      if (parent === cursor) {
        throw new Error('Path escapes the selected project root.');
      }
      missing.push(path.basename(cursor));
      cursor = parent;
    }
  }
}

/**
 * Resolve `rel` inside `root` and reject traversal, prefix siblings, and symlink escapes.
 * Missing files are allowed when every existing ancestor stays inside the root.
 */
export async function resolveWithinRoot(root: string, rel: string): Promise<string> {
  if (!root || !root.trim()) {
    throw new Error('No project folder selected.');
  }
  assertProjectRelative(rel);
  let rootReal: string;
  try {
    rootReal = await fs.realpath(root);
  } catch {
    throw new Error('Project folder is not available.');
  }
  const requested = path.resolve(rootReal, (rel || '.').replace(/^[/\\]+/, ''));
  if (!isInsideRoot(rootReal, requested)) {
    throw new Error('Path escapes the selected project root.');
  }
  const { existing, missing } = await nearestExisting(requested);
  if (!isInsideRoot(rootReal, existing)) {
    throw new Error('Path escapes the selected project root.');
  }
  if (missing.length === 0) {
    return existing;
  }
  return path.join(existing, ...missing.reverse());
}

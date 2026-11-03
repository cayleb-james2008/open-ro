import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { isIgnoredName, isSecretName } from './pathSafety';

export interface ProjectIndexEntry {
  /** Project-relative path with forward slashes. */
  relPath: string;
  bytes: number;
}

export interface ProjectIndex {
  root: string;
  entries: ProjectIndexEntry[];
  /** True when a cap or the depth limit stopped the scan early. */
  truncated: boolean;
}

export const DEFAULT_INDEX_LIMITS = {
  maxFiles: 2_000,
  maxBytes: 4_000_000,
  maxDepth: 12,
} as const;

interface IndexLimits {
  maxFiles: number;
  maxBytes: number;
  maxDepth: number;
}

function compareNames(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/**
 * Walk a project tree without reading file contents. Ignored segments, secret
 * names, and symlinks/junctions are skipped; caps make the result bounded and
 * deterministic. Unreadable subdirectories are skipped rather than failing the
 * whole index.
 */
export async function indexProject(root: string, limits: Partial<IndexLimits> = {}): Promise<ProjectIndex> {
  const maxFiles = limits.maxFiles ?? DEFAULT_INDEX_LIMITS.maxFiles;
  const maxBytes = limits.maxBytes ?? DEFAULT_INDEX_LIMITS.maxBytes;
  const maxDepth = limits.maxDepth ?? DEFAULT_INDEX_LIMITS.maxDepth;
  const entries: ProjectIndexEntry[] = [];
  let bytes = 0;
  let truncated = false;

  async function walk(directory: string, depth: number, prefix: string): Promise<void> {
    if (truncated) {
      return;
    }
    if (depth > maxDepth) {
      truncated = true;
      return;
    }
    let items;
    try {
      items = await fs.readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    items.sort((left, right) => compareNames(left.name, right.name));
    for (const item of items) {
      if (truncated) {
        return;
      }
      if (item.isSymbolicLink() || isIgnoredName(item.name) || isSecretName(item.name)) {
        continue;
      }
      const relPath = prefix ? `${prefix}/${item.name}` : item.name;
      if (item.isDirectory()) {
        await walk(path.join(directory, item.name), depth + 1, relPath);
        continue;
      }
      if (!item.isFile()) {
        continue;
      }
      if (entries.length >= maxFiles) {
        truncated = true;
        return;
      }
      let size = 0;
      try {
        size = (await fs.stat(path.join(directory, item.name))).size;
      } catch {
        continue;
      }
      if (bytes + size > maxBytes) {
        truncated = true;
        return;
      }
      bytes += size;
      entries.push({ relPath, bytes: size });
    }
  }

  await walk(root, 0, '');
  return { root, entries, truncated };
}

export interface ProjectContextOptions {
  maxEntries?: number;
  maxChars?: number;
}

/**
 * Compact, bounded project map for the agent prompt. Metadata only; the caller
 * must label it as untrusted data. Returns '' for an empty index.
 */
export function buildProjectContext(index: ProjectIndex, options: ProjectContextOptions = {}): string {
  const maxEntries = options.maxEntries ?? 150;
  const maxChars = options.maxChars ?? 6_000;
  if (index.entries.length === 0) {
    return '';
  }
  const noteReserve = 160;
  const pick = (budget: number): { lines: string[]; included: number } => {
    const lines: string[] = [];
    let chars = 0;
    let included = 0;
    for (const entry of index.entries.slice(0, maxEntries)) {
      const line = `${entry.relPath} (${entry.bytes} B)`;
      if (chars + line.length + 1 > budget) {
        break;
      }
      lines.push(line);
      chars += line.length + 1;
      included += 1;
    }
    return { lines, included };
  };

  const full = pick(maxChars);
  if (full.included === index.entries.length && !index.truncated) {
    return full.lines.join('\n');
  }
  // Truncation is real: reserve room so the note survives the character cap.
  const reserved = pick(Math.max(0, maxChars - noteReserve));
  const omitted = index.entries.length - reserved.included;
  const note = `... ${omitted > 0 ? `${omitted} more file(s); ` : ''}index${index.truncated ? ' was truncated by caps' : ''}. Use list_dir/read_file for specifics.`;
  let text = reserved.lines.join('\n');
  const room = maxChars - text.length - (text ? 1 : 0);
  if (room > 0) {
    const clipped = note.length > room ? note.slice(0, room) : note;
    text = text ? `${text}\n${clipped}` : clipped;
  }
  return text.length > maxChars ? text.slice(0, maxChars) : text;
}

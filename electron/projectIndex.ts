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

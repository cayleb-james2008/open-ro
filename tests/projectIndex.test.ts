import { readFileSync } from 'node:fs';
import { mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { indexProject } from '../electron/projectIndex';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(tempDirs.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
});

async function tempDir(): Promise<string> {
  const dir = await mkdtemp(path.join(tmpdir(), 'openro-index-'));
  tempDirs.push(dir);
  return dir;
}

async function fixture(): Promise<string> {
  const root = await tempDir();
  const outside = await tempDir();
  await mkdir(path.join(root, 'src', 'nested'), { recursive: true });
  await mkdir(path.join(root, '.git'), { recursive: true });
  await mkdir(path.join(root, 'node_modules', 'pkg'), { recursive: true });
  await mkdir(path.join(root, 'dist'), { recursive: true });
  await writeFile(path.join(outside, 'secret.txt'), 'outside');
  await symlink(outside, path.join(root, 'link'), process.platform === 'win32' ? 'junction' : 'dir');
  await writeFile(path.join(root, 'README.md'), 'hello');
  await writeFile(path.join(root, 'src', 'init.lua'), 'print(1)');
  await writeFile(path.join(root, 'src', 'nested', 'deep.lua'), 'x');
  await writeFile(path.join(root, '.env'), 'SECRET=1');
  await writeFile(path.join(root, 'credentials.json'), '{}');
  await writeFile(path.join(root, '.git', 'config'), 'x');
  await writeFile(path.join(root, 'node_modules', 'pkg', 'index.js'), 'x');
  await writeFile(path.join(root, 'dist', 'bundle.js'), 'x');
  return root;
}

describe('project index', () => {
  it('returns sorted posix paths with byte sizes and skips ignored, secret, and linked content', async () => {
    const root = await fixture();
    const index = await indexProject(root);
    expect(index.truncated).toBe(false);
    expect(index.entries).toEqual([
      { relPath: 'README.md', bytes: 5 },
      { relPath: 'src/init.lua', bytes: 8 },
      { relPath: 'src/nested/deep.lua', bytes: 1 },
    ]);
    expect(index.entries.some((entry) => entry.relPath.includes('secret'))).toBe(false);
  });

  it('truncates at the file cap', async () => {
    const root = await fixture();
    const index = await indexProject(root, { maxFiles: 2 });
    expect(index.entries).toHaveLength(2);
    expect(index.truncated).toBe(true);
  });

  it('truncates at the byte cap', async () => {
    const root = await fixture();
    const index = await indexProject(root, { maxBytes: 6 });
    expect(index.entries).toEqual([{ relPath: 'README.md', bytes: 5 }]);
    expect(index.truncated).toBe(true);
  });

  it('truncates at the depth limit', async () => {
    const root = await fixture();
    const index = await indexProject(root, { maxDepth: 0 });
    expect(index.entries).toEqual([{ relPath: 'README.md', bytes: 5 }]);
    expect(index.truncated).toBe(true);
  });

  it('is deterministic across runs', async () => {
    const root = await fixture();
    const first = await indexProject(root);
    const second = await indexProject(root);
    expect(second).toEqual(first);
  });

  it('is exposed through the project:index IPC handler and allowlisted channel', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    const start = main.indexOf("ipcMain.handle('project:index'");
    expect(start).toBeGreaterThan(-1);
    const handler = main.slice(start, main.indexOf('\n  });', start));
    expect(handler).toContain('indexProject(projectRoot)');
    expect(handler).toContain('No project folder selected');
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'project:index'");
  });
});

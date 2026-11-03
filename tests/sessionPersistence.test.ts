import { readFileSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { writeTextAtomic } from '../electron/atomicWrite';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(tempDirs.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
});

async function tempDir(): Promise<string> {
  const dir = await mkdtemp(path.join(tmpdir(), 'openro-atomic-'));
  tempDirs.push(dir);
  return dir;
}

function functionSource(source: string, startMarker: string): string {
  const start = source.indexOf(startMarker);
  expect(start).toBeGreaterThan(-1);
  const end = source.indexOf('\n}', start);
  return source.slice(start, end);
}

describe('atomic persistence', () => {
  it('writes the full content and leaves no temp file', async () => {
    const dir = await tempDir();
    const file = path.join(dir, 'sessions.json');
    await writeTextAtomic(file, '{"activeId":null,"sessions":[]}');
    expect(await readFile(file, 'utf8')).toBe('{"activeId":null,"sessions":[]}');
    expect(await readdir(dir)).toEqual(['sessions.json']);
  });

  it('replaces an existing file instead of appending to it', async () => {
    const dir = await tempDir();
    const file = path.join(dir, 'state.json');
    await writeFile(file, 'x'.repeat(10_000));
    await writeTextAtomic(file, 'short');
    expect(await readFile(file, 'utf8')).toBe('short');
    expect(await readdir(dir)).toEqual(['state.json']);
  });

  it('survives concurrent writes with one complete winner', async () => {
    const dir = await tempDir();
    const file = path.join(dir, 'sessions.json');
    const payloads = Array.from({ length: 12 }, (_, index) => JSON.stringify({ write: index, blob: 'y'.repeat(2_000 + index) }));
    await Promise.all(payloads.map((payload) => writeTextAtomic(file, payload)));
    expect(payloads).toContain(await readFile(file, 'utf8'));
    expect(await readdir(dir)).toEqual(['sessions.json']);
  });

  it('creates missing parent directories', async () => {
    const dir = await tempDir();
    const file = path.join(dir, 'nested', 'deeper', 'state.json');
    await writeTextAtomic(file, 'ok');
    expect(await readFile(file, 'utf8')).toBe('ok');
  });

  it('cleans up the temp file when the rename fails', async () => {
    const dir = await tempDir();
    const target = path.join(dir, 'state.json');
    await mkdir(path.join(target, 'inner'), { recursive: true });
    await expect(writeTextAtomic(target, 'data')).rejects.toThrow();
    const leftovers = (await readdir(dir)).filter((name) => name.endsWith('.tmp'));
    expect(leftovers).toEqual([]);
  });

  it('persists sessions and app state through the atomic writer', () => {
    const source = readFileSync('electron/main.ts', 'utf8');
    const saveStart = source.indexOf("ipcMain.handle('sessions:save'");
    expect(saveStart).toBeGreaterThan(-1);
    const sessionSave = source.slice(saveStart, source.indexOf('\n  });', saveStart));
    expect(sessionSave).toContain('writeTextAtomic(');
    expect(sessionSave).not.toMatch(/fs\.writeFile\(/);
    const appState = functionSource(source, 'async function writeAppState');
    expect(appState).toContain('writeTextAtomic(');
    expect(appState).not.toMatch(/fs\.writeFile\(/);
  });

  it('restores the saved project root before the window is created', () => {
    const source = readFileSync('electron/main.ts', 'utf8');
    const readyStart = source.indexOf('app.whenReady()');
    expect(readyStart).toBeGreaterThan(-1);
    const ready = source.slice(readyStart);
    const loadAt = ready.indexOf('loadSavedProjectRoot()');
    const windowAt = ready.indexOf('createWindow()');
    expect(loadAt).toBeGreaterThan(-1);
    expect(windowAt).toBeGreaterThan(loadAt);
  });
});

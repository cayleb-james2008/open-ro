import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import {
  assertProjectRelative,
  isReservedDeviceName,
  ProjectScope,
} from '../electron/pathSafety';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(tempDirs.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
});

async function tempDir(prefix: string): Promise<string> {
  const dir = await mkdtemp(path.join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

describe('Project Scope Isolation & Write Boundary Hardening', () => {
  describe('Device Names, Null Bytes & Alternate Data Streams', () => {
    it('detects and blocks null-byte path injections', () => {
      expect(() => assertProjectRelative('valid/path\0/evil.lua')).toThrow(/null byte/);
      expect(() => assertProjectRelative('\0evil.lua')).toThrow(/null byte/);
    });

    it('identifies reserved Windows device names', () => {
      if (process.platform === 'win32') {
        expect(isReservedDeviceName('CON')).toBe(true);
        expect(isReservedDeviceName('con.txt')).toBe(true);
        expect(isReservedDeviceName('PRN')).toBe(true);
        expect(isReservedDeviceName('AUX')).toBe(true);
        expect(isReservedDeviceName('NUL')).toBe(true);
        expect(isReservedDeviceName('nul.lua')).toBe(true);
        expect(isReservedDeviceName('COM1')).toBe(true);
        expect(isReservedDeviceName('LPT9')).toBe(true);
        expect(isReservedDeviceName('normal_file.lua')).toBe(false);

        expect(() => assertProjectRelative('con.txt')).toThrow(/reserved device name/);
        expect(() => assertProjectRelative('src/nul')).toThrow(/reserved device name/);
      } else {
        expect(isReservedDeviceName('CON')).toBe(false);
      }
    });

    it('rejects NTFS alternate data streams on Windows', () => {
      if (process.platform === 'win32') {
        expect(() => assertProjectRelative('script.lua:hidden')).toThrow(/reserved character/);
        expect(() => assertProjectRelative('nested/dir/test.txt::$DATA')).toThrow(/reserved character/);
      }
    });
  });

  describe('ProjectScope Boundary Enforcement', () => {
    it('enforces read-only locks preventing any write operations', async () => {
      const root = await tempDir('openro-scope-readonly-');
      const scope = new ProjectScope({ root, readOnly: true });

      expect(scope.isReadOnly()).toBe(true);
      const writeResult = scope.validateWrite('src/main.lua');
      expect(writeResult.ok).toBe(false);
      expect(writeResult.reason).toContain('read-only mode');

      // Reads are still permitted
      const readResult = scope.validateRead('src/main.lua');
      expect(readResult.ok).toBe(true);
    });

    it('restricts writes strictly to allowed subpaths when configured', async () => {
      const root = await tempDir('openro-scope-subpaths-');
      const scope = new ProjectScope({
        root,
        allowedWritePrefixes: ['src', 'assets'],
      });

      expect(scope.validateWrite('src/index.lua').ok).toBe(true);
      expect(scope.validateWrite('src/components/ui.lua').ok).toBe(true);
      expect(scope.validateWrite('assets/textures/mesh.png').ok).toBe(true);

      const rootWrite = scope.validateWrite('root_file.lua');
      expect(rootWrite.ok).toBe(false);
      expect(rootWrite.reason).toContain('outside the user-selected write scope');

      const docsWrite = scope.validateWrite('docs/README.md');
      expect(docsWrite.ok).toBe(false);
      expect(docsWrite.reason).toContain('outside the user-selected write scope');
    });

    it('blocks writes targeting explicitly denied prefixes', async () => {
      const root = await tempDir('openro-scope-denied-');
      const scope = new ProjectScope({
        root,
        deniedWritePrefixes: ['.omni-loop', 'build', 'private'],
      });

      expect(scope.validateWrite('src/safe.lua').ok).toBe(true);

      const omniWrite = scope.validateWrite('.omni-loop/state.json');
      expect(omniWrite.ok).toBe(false);
      expect(omniWrite.reason).toContain("restricted prefix '.omni-loop'");

      const buildWrite = scope.validateWrite('build/bundle.js');
      expect(buildWrite.ok).toBe(false);
      expect(buildWrite.reason).toContain("restricted prefix 'build'");
    });

    it('rejects path traversal attempts outside project scope', async () => {
      const root = await tempDir('openro-scope-traversal-');
      const scope = new ProjectScope({ root });

      const traversal = scope.validateWrite('../outside.lua');
      expect(traversal.ok).toBe(false);
      expect(traversal.reason).toContain('escapes');

      const readTraversal = scope.validateRead('../../secret.pem');
      expect(readTraversal.ok).toBe(false);
      expect(readTraversal.reason).toContain('escapes');
    });

    it('resolves valid paths within scope root', async () => {
      const root = await tempDir('openro-scope-resolve-');
      await mkdir(path.join(root, 'src'));
      await writeFile(path.join(root, 'src', 'main.lua'), 'print("hi")');

      const scope = new ProjectScope({ root });
      const resolved = await scope.resolve('src/main.lua');
      expect(resolved).toBe(path.join(root, 'src', 'main.lua'));
    });
  });

  describe('Multi-Project Approval Queue Isolation', () => {
    it('refuses taking approvals staged under a different project root', async () => {
      const queue = new ApprovalQueue();
      const projA = await tempDir('openro-projA-');
      const projB = await tempDir('openro-projB-');

      const staged = queue.stageWrite({
        projectRoot: projA,
        relPath: 'src/module.lua',
        content: 'return {}',
        previous: '',
      });

      // Taking with wrong project root throws and leaves approval pending
      expect(() => queue.take(staged.id, undefined, projB)).toThrow(
        /which does not match active project/,
      );

      // Verify approval is still pending
      expect(queue.list()).toHaveLength(1);

      // Taking with matching project root succeeds
      const taken = queue.take(staged.id, undefined, projA);
      expect(taken.id).toBe(staged.id);
      expect(queue.list()).toHaveLength(0);
    });

    it('refuses rejecting approvals staged under a different project root', async () => {
      const queue = new ApprovalQueue();
      const projA = await tempDir('openro-projA2-');
      const projB = await tempDir('openro-projB2-');

      const staged = queue.stageCommand({
        projectRoot: projA,
        command: 'npm test',
        cwd: projA,
      });

      expect(() => queue.reject(staged.id, projB)).toThrow(
        /which does not match active project/,
      );
      expect(queue.list()).toHaveLength(1);

      const rejected = queue.reject(staged.id, projA);
      expect(rejected.id).toBe(staged.id);
      expect(queue.list()).toHaveLength(0);
    });
  });
});

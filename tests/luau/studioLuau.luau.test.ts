import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { parseLuauReply, readbackProblems } from '../../electron/studioChecks';
import { buildReadLuau, buildWriteLuau } from '../../electron/studioLuau';

// Runs the generated Luau against tests/fixtures/studio-datamodel.luau with the Luau CLI named by OPENRO_LUAU_BIN.
const luau = process.env.OPENRO_LUAU_BIN ?? '';
const mock = readFileSync(join(__dirname, '../fixtures/studio-datamodel.luau'), 'utf8');
const dir = mkdtempSync(join(tmpdir(), 'openro-luau-'));
const SHOP = "print('old')\n-- ]] tricky ]=] end";

/** Run one snippet; `after` runs once the snippet has finished and may print lines prefixed AFTER:. */
function run(snippet: string, after = '') {
  const file = join(dir, 'case.luau');
  writeFileSync(file, `${mock}\n${snippet.replace(/\nreturn finish\(/, '\nlocal __reply = finish(')}\n${after}\n`);
  const out = execFileSync(luau, [file], { encoding: 'utf8' });
  return { reply: parseLuauReply(out), after: out.split('\n').filter((line) => line.startsWith('AFTER:')).map((line) => line.slice(6)) };
}
const write = (segments: string[], source: string, expectedPrevious: string | null, className: 'Script' | 'ModuleScript' = 'Script') =>
  buildWriteLuau({ segments, className, source, expectedPrevious });
/** Prints Shop's Source as hex so any byte change shows. */
const shopSource = 'print("AFTER:" .. (game:GetService("ServerScriptService"):FindFirstChild("Shop").Source:gsub(".", function(c) return string.format("%02x", c:byte()) end)))';
const hex = (text: string) => Buffer.from(text, 'utf8').toString('hex');

describe('generated Studio Luau, run by the Luau CLI', () => {
  it('a write that matches the review lands and reads back exactly, CRLF and NUL included', () => {
    for (const source of ['--!strict\n-- ]==] ]] edge\nprint("new")\n', 'print(1)\r\nprint(2)\r\n', 'a\0b\n\rc']) {
      const { reply } = run(write(['ServerScriptService', 'Shop'], source, SHOP));
      expect(reply.ok).toBe(true);
      expect(reply.ok && readbackProblems(['ServerScriptService', 'Shop'], 'Script', source, reply.result)).toEqual([]);
    }
  });

  it('refuses a stale write and leaves the script as it was', () => {
    const { reply, after } = run(write(['ServerScriptService', 'Shop'], 'x', 'something else'), shopSource);
    expect(reply).toEqual({ ok: false, error: 'ServerScriptService.Shop changed after the review. Read it again.' });
    expect(after).toEqual([hex(SHOP)]);
  });

  it('refuses a CRLF/LF difference as stale', () => {
    expect(run(write(['ServerScriptService', 'Shop'], 'x', SHOP.replace('\n', '\r\n'))).reply.ok).toBe(false);
  });

  it('refuses a create when the script appeared after the review', () => {
    const { reply, after } = run(write(['ServerScriptService', 'Shop'], 'x', null), shopSource);
    expect(reply).toEqual({ ok: false, error: 'ServerScriptService.Shop appeared after the review. Read it again.' });
    expect(after).toEqual([hex(SHOP)]);
  });

  it('refuses duplicate names on the path and at the target, for reads and writes', () => {
    expect(run(write(['ReplicatedStorage', 'Dup', 'X'], 'x', null)).reply).toEqual({ ok: false, error: 'Two instances named Dup under ReplicatedStorage' });
    expect(run(write(['ServerScriptService', 'Twin'], 'x', '')).reply).toEqual({ ok: false, error: 'Two instances named Twin under ServerScriptService' });
    expect(run(buildReadLuau(['ServerScriptService', 'Twin'])).reply).toEqual({ ok: false, error: 'Two instances named Twin under ServerScriptService' });
  });

  it('refuses a removed script and a class change', () => {
    expect(run(write(['ServerScriptService', 'Gone'], 'x', 'old')).reply).toEqual({ ok: false, error: 'The script was removed after the review.' });
    expect(run(write(['ReplicatedStorage', 'Shared', 'Util'], 'x', 'return {}')).reply).toEqual({ ok: false, error: 'ReplicatedStorage.Shared.Util is a ModuleScript, not a Script' });
  });
});

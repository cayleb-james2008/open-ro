import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { summarizePlaytestLedger, type PlaytestLedgerRecord } from '../electron/playtestLedger';

const root = 'C:\\Work\\Game';

function record(file: string, modifiedMs: number, payload: unknown): PlaytestLedgerRecord {
  return { file, modifiedMs, raw: typeof payload === 'string' ? payload : JSON.stringify(payload) };
}

function payload(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    projectRoot: root,
    report: { scenario: 'Player touches the coin' },
    verdict: { status: 'PASSED', reason: '1 assertions, 1 state checks and 1 screenshots matched.', screenshots: [{}, {}] },
    ...overrides,
  };
}

describe('playtest ledger summaries', () => {
  it('summarizes matching records newest first with evidence counts', () => {
    const entries = summarizePlaytestLedger(
      [
        record('old-PASSED.json', 1_000, payload()),
        record('new-FAILED.json', 3_000, payload({ verdict: { status: 'FAILED', reason: 'An assertion did not match.', failingCheck: 'assertion "coin": expected 1, got 0', screenshots: [{}] } })),
        record('mid-NO_EVIDENCE.json', 2_000, payload({ verdict: { status: 'NO_EVIDENCE', reason: 'no assertions', screenshots: [] } })),
      ],
      root,
    );
    expect(entries.map((entry) => [entry.file, entry.status, entry.screenshots])).toEqual([
      ['new-FAILED.json', 'FAILED', 1],
      ['mid-NO_EVIDENCE.json', 'NO_EVIDENCE', 0],
      ['old-PASSED.json', 'PASSED', 2],
    ]);
    expect(entries[0].failingCheck).toContain('expected 1, got 0');
    expect(entries[0].reason).toBe('An assertion did not match.');
    expect(entries[1].reason).toBe('no assertions');
    expect(entries[2].createdAt).toBe(new Date(1_000).toISOString());
    expect(entries[2].scenario).toBe('Player touches the coin');
  });

  it('matches Windows roots case-insensitively and tolerates an invalid mtime', () => {
    const entries = summarizePlaytestLedger(
      [
        record('case.json', 1, payload({ projectRoot: 'c:/WORK/game/' })),
        record('bad-time.json', Number.NaN, payload()),
      ],
      root,
    );
    expect(entries.map((entry) => entry.file)).toEqual(['case.json', 'bad-time.json']);
    expect(entries[1].createdAt).toBe(new Date(0).toISOString());
  });

  it('skips malformed, non-object, foreign-project, and unknown-status records', () => {
    const entries = summarizePlaytestLedger(
      [
        record('bad.json', 5, 'not json'),
        record('scalar.json', 5, '42'),
        record('foreign.json', 5, payload({ projectRoot: 'C:\\Work\\Other' })),
        record('unknown.json', 5, payload({ verdict: { status: 'MAYBE', reason: '' } })),
        record('good.json', 5, payload()),
      ],
      root,
    );
    expect(entries.map((entry) => entry.file)).toEqual(['good.json']);
  });

  it('normalizes separators and trailing slashes in project roots', () => {
    const entries = summarizePlaytestLedger([record('a.json', 1, payload({ projectRoot: 'C:/Work/Game/' }))], root);
    expect(entries).toHaveLength(1);
  });

  it('caps the ledger at 50 entries', () => {
    const records = Array.from({ length: 55 }, (_, index) => record(`run-${index}.json`, index, payload()));
    const entries = summarizePlaytestLedger(records, root);
    expect(entries).toHaveLength(50);
    expect(entries[0].file).toBe('run-54.json');
    expect(entries[49].file).toBe('run-5.json');
  });

  it('is wired through IPC, the preload allowlist, and the api', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("ipcMain.handle('playtest:ledger'");
    expect(main).toContain('summarizePlaytestLedger(');
    expect(main).toContain("'playtests'");
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'playtest:ledger'");
    expect(readFileSync('src/api.ts', 'utf8')).toContain('playtestLedger');
  });
});

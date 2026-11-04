import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { deriveRepairPlan, summarizePlaytestLedger, type PlaytestLedgerEntry, type PlaytestLedgerRecord } from '../electron/playtestLedger';

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

describe('playtest repair plans', () => {
  const entry = (overrides: Partial<PlaytestLedgerEntry> = {}): PlaytestLedgerEntry => ({
    file: 'run.json',
    scenario: 'Player touches the coin',
    status: 'FAILED',
    reason: 'An assertion did not match.',
    failingCheck: 'assertion "coin": expected 1, got 0',
    screenshots: 1,
    createdAt: new Date(1_000).toISOString(),
    ...overrides,
  });

  it('selects the newest open failure and counts every attempt since the last pass', () => {
    const plan = deriveRepairPlan([
      entry({ file: 'n4.json', status: 'NO_EVIDENCE', reason: 'no screenshot' }),
      entry({ file: 'f3.json', failingCheck: 'assertion "coin": expected 1, got 0' }),
      entry({ file: 'n2.json', status: 'NO_EVIDENCE', reason: 'no screenshot' }),
      entry({ file: 'f1.json', failingCheck: 'state Coins.Value expected 1, got 0' }),
      entry({ file: 'pass.json', status: 'PASSED' }),
      entry({ file: 'old.json', failingCheck: 'before the pass' }),
    ]);
    expect(plan).toMatchObject({
      scenario: 'Player touches the coin',
      failingCheck: 'assertion "coin": expected 1, got 0',
      attempt: 4,
      screenshots: 1,
      lastFailedFile: 'f3.json',
    });
    expect(plan?.instruction).toContain('Repair "Player touches the coin" (attempt 4)');
    expect(plan?.instruction).toContain('assertion "coin": expected 1, got 0');
    expect(plan?.instruction).toContain('record report_playtest with the same scenario');
  });

  it('closes older failures when the same scenario passes later', () => {
    expect(deriveRepairPlan([entry({ status: 'PASSED' }), entry({ file: 'older.json' })])).toBeNull();
  });

  it('does not let a pass in another scenario close the failure', () => {
    const plan = deriveRepairPlan([
      entry({ file: 'coin-pass.json', scenario: 'Coin', status: 'PASSED' }),
      entry({ file: 'door-fail.json', scenario: 'Door', failingCheck: 'state Door.Open expected true, got false' }),
    ]);
    expect(plan?.scenario).toBe('Door');
    expect(plan?.attempt).toBe(1);
  });

  it('selects the newest open failure across scenarios', () => {
    const plan = deriveRepairPlan([
      entry({ file: 'door.json', scenario: 'Door' }),
      entry({ file: 'coin.json', scenario: 'Coin' }),
    ]);
    expect(plan?.scenario).toBe('Door');
  });

  it('ignores evidence-free runs as plan sources and falls back to the reason', () => {
    expect(deriveRepairPlan([])).toBeNull();
    expect(deriveRepairPlan([entry({ status: 'NO_EVIDENCE' })])).toBeNull();
    const noCheck = entry({ reason: 'An assertion did not match.' });
    delete noCheck.failingCheck;
    const plan = deriveRepairPlan([noCheck]);
    expect(plan?.failingCheck).toBe('An assertion did not match.');
    expect(plan?.attempt).toBe(1);
  });

  it('wires the plan through IPC, the preload allowlist, the api, and failed report output', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("ipcMain.handle('playtest:repair-plan'");
    expect(main).toContain('deriveRepairPlan(await readPlaytestLedger(projectRoot))');
    expect(main).toContain("verdict.status === 'FAILED' ? deriveRepairPlan(await readPlaytestLedger(root).catch(() => [])) : null");
    expect(main).toContain('Repair loop: attempt ${plan.attempt}');
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'playtest:repair-plan'");
    expect(readFileSync('src/api.ts', 'utf8')).toContain('playtestRepairPlan');
  });
});

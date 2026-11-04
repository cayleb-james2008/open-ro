import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  ACCEPTANCE_SCENARIO_MAX,
  acceptanceForScenario,
  acceptanceFromRead,
  acceptanceInstruction,
  acceptanceText,
  applyAcceptanceChecks,
  parseAcceptanceDefinition,
  verifyAcceptanceChecks,
  verifyAcceptanceFiles,
} from '../electron/acceptanceChecks';

const valid = {
  version: 1,
  checks: [
    { id: 'probe', scenario: 'Repair loop live probe', type: 'assertion', description: 'repair probe', expected: 'true' },
    { id: 'enabled', scenario: 'Repair loop live probe', type: 'state', instancePath: 'Workspace.RepairProbe', property: 'Enabled', expected: 'true' },
    { id: 'other', scenario: 'Door opens', type: 'assertion', description: 'door opens', expected: 'true' },
    { id: 'probe-file', scenario: 'Repair loop live probe', type: 'file', path: 'repair-probe.txt', contains: 'fixed by the repair' },
  ],
  screenshots: ['shots/after.png'],
  repairHint: 'Create repair-probe.txt with write_file, then re-run the probe.',
};

describe('acceptance definitions', () => {
  it('parses a BOM-prefixed definition and filters by scenario', () => {
    const definition = parseAcceptanceDefinition(`\uFEFF${JSON.stringify(valid)}`);
    expect(definition.checks).toHaveLength(4);
    expect(definition.screenshots).toEqual(['shots/after.png']);
    expect(acceptanceForScenario(definition, 'Repair loop live probe')).toMatchObject({
      assertions: [{ id: 'probe', description: 'repair probe', expected: 'true' }],
      stateChecks: [{ id: 'enabled', instancePath: 'Workspace.RepairProbe', property: 'Enabled', expected: 'true' }],
      fileChecks: [{ id: 'probe-file', path: 'repair-probe.txt', contains: 'fixed by the repair' }],
      screenshots: ['shots/after.png'],
    });
    expect(acceptanceForScenario(definition, 'missing')).toBeNull();
    expect(acceptanceForScenario(definition, 'Door opens')?.assertions).toHaveLength(1);
  });

  it('builds a repair instruction with expected values and evidence', () => {
    const definition = parseAcceptanceDefinition(JSON.stringify(valid));
    const text = acceptanceInstruction(definition, 'Repair loop live probe');
    expect(text).toContain('Acceptance definition for "Repair loop live probe"');
    expect(text).toContain('assertion "repair probe": expected "true"');
    expect(text).toContain('state Workspace.RepairProbe.Enabled: expected "true"');
    expect(text).toContain('file repair-probe.txt contains "fixed by the repair"');
    expect(text).toContain('Evidence screenshot path(s): shots/after.png');
    expect(text).toContain('Repair hint: Create repair-probe.txt with write_file');
    expect(text).toContain('call report_playtest');
    expect(text).toContain('Do not claim PASSED until the gate returns PASSED.');
    expect(acceptanceInstruction(definition, 'Door opens')).toContain('"door opens"');
    expect(acceptanceInstruction(definition, 'missing')).toBe('');
    const both = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [{ id: 'both', scenario: 'Both', type: 'file', path: 'both.txt', contains: 'fixed', expected: 'fixed by the loop' }],
    }));
    expect(acceptanceInstruction(both, 'Both')).toContain('file both.txt contains "fixed" and equals "fixed by the loop"');
  });

  it('rejects malformed, unsafe, duplicate and oversized definitions', () => {
    const bad = (payload: unknown) => () => parseAcceptanceDefinition(JSON.stringify(payload));
    expect(() => parseAcceptanceDefinition('not json')).toThrow(/valid JSON/);
    expect(() => parseAcceptanceDefinition('x'.repeat(256_001))).toThrow(/larger than 256 KB/);
    expect(bad({ version: 2, checks: valid.checks })).toThrow(/version/);
    expect(bad({ version: 1, checks: [] })).toThrow(/1-50/);
    expect(bad({ version: 1, checks: Array.from({ length: 51 }, (_, index) => ({ ...valid.checks[0], id: `id-${index}` })) })).toThrow(/1-50/);
    expect(bad({ version: 1, checks: [valid.checks[0], { ...valid.checks[0] }] })).toThrow(/Duplicate/);
    expect(bad({ version: 1, checks: [{ ...valid.checks[0], type: 'guess' }] })).toThrow(/unknown type/);
    expect(bad({ version: 1, checks: [{ ...valid.checks[1], instancePath: '' }] })).toThrow(/instancePath/);
    expect(bad({ version: 1, checks: valid.checks, screenshots: ['C:/outside.png'] })).toThrow(/relative/);
    expect(bad({ version: 1, checks: valid.checks, screenshots: ['../escape.png'] })).toThrow(/relative/);
    expect(bad({ version: 1, checks: valid.checks, screenshots: Array.from({ length: 11 }, (_, index) => `shot-${index}.png`) })).toThrow(/at most 10/);
    expect(bad({ version: 1, checks: [{ id: 'f1', scenario: 'x', type: 'file', path: 'probe.txt' }] })).toThrow(/contains or expected/);
    expect(bad({ version: 1, checks: [{ id: 'f2', scenario: 'x', type: 'file', path: '../probe.txt', contains: 'x' }] })).toThrow(/relative/);
    expect(bad({ version: 1, checks: valid.checks, repairHint: 42 })).toThrow(/repairHint/);
    expect(bad({ version: 1, checks: valid.checks, repairHint: 'x'.repeat(1_001) })).toThrow(/repairHint/);
    expect(bad({ version: 1, checks: [{ ...valid.checks[0], scenario: 's'.repeat(201) }] })).toThrow(/longer than 200/);
    expect(bad({ version: 1, checks: [{ id: 'f3', scenario: 'x', type: 'file', path: 'node_modules/probe.txt', contains: 'x' }] })).toThrow(/ignored folder/);
    expect(bad({ version: 1, checks: [{ id: 'f4', scenario: 'x', type: 'file', path: '.env', contains: 'x' }] })).toThrow(/secret file/);
    expect(bad({ version: 1, checks: Array.from({ length: 50 }, (_, index) => ({ id: `big-${index}`, scenario: 'x', type: 'assertion', description: 'd'.repeat(500), expected: 'e'.repeat(500) })) })).toThrow(/larger than 12000/);
    expect(bad({ version: 1, checks: Array.from({ length: 8 }, (_, index) => ({ id: `ctrl-${index}`, scenario: 'x', type: 'assertion', description: '\u0001'.repeat(250), expected: '\u0001'.repeat(250) })) })).toThrow(/instruction is larger than 15000/);
  });

  it('enforces file checks with missing, contains and exact-content cases', async () => {
    const definition = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [
        { id: 'probe-file', scenario: 'Repair loop live probe', type: 'file', path: 'repair-probe.txt', contains: 'fixed' },
        { id: 'exact-file', scenario: 'Repair loop live probe', type: 'file', path: 'exact.txt', expected: 'exact text' },
        { id: 'other-file', scenario: 'Door opens', type: 'file', path: 'other.txt', contains: 'other' },
      ],
    }));
    const files = new Map<string, string>([
      ['repair-probe.txt', 'fixed by the loop'],
      ['exact.txt', 'exact text'],
    ]);
    const read = async (path: string) => files.get(path) ?? null;
    expect(await verifyAcceptanceFiles(definition, 'Repair loop live probe', read)).toBeNull();
    expect(await verifyAcceptanceFiles(definition, 'Door opens', read)).toContain('other.txt is missing');
    expect(await verifyAcceptanceFiles(definition, 'unknown', read)).toBeNull();
    files.delete('repair-probe.txt');
    expect(await verifyAcceptanceFiles(definition, 'Repair loop live probe', read)).toContain('repair-probe.txt is missing');
    files.set('repair-probe.txt', 'something else');
    expect(await verifyAcceptanceFiles(definition, 'Repair loop live probe', read)).toContain('does not contain "fixed"');
    files.set('repair-probe.txt', 'fixed by the loop');
    files.set('exact.txt', 'exact text ');
    expect(await verifyAcceptanceFiles(definition, 'Repair loop live probe', read)).toContain('does not equal the expected content');
  });

  it('applies acceptance checks without weakening an existing failure', async () => {
    const definition = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [{ id: 'probe-file', scenario: 'Repair', type: 'file', path: 'probe.txt', contains: 'fixed' }],
    }));
    const missing = async () => null;
    const present = async () => 'fixed by the loop';
    expect(await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, definition, null, 'Repair', present)).toEqual({ status: 'PASSED', reason: 'all matched' });
    expect(await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, definition, null, 'Repair', missing)).toMatchObject({
      status: 'FAILED',
      reason: 'Acceptance file check failed: acceptance file probe.txt is missing or unreadable.',
      failingCheck: 'acceptance file probe.txt is missing or unreadable',
    });
    const earlier = { status: 'FAILED' as const, reason: '1 assertion did not match.', failingCheck: 'assertion "probe": expected true, got false' };
    const combined = await applyAcceptanceChecks(earlier, definition, null, 'Repair', missing);
    expect(combined.status).toBe('FAILED');
    expect(combined.failingCheck).toBe(earlier.failingCheck);
    expect(combined.reason).toContain('Acceptance file check failed');
    const noEvidence = await applyAcceptanceChecks({ status: 'NO_EVIDENCE', reason: 'no assertions' }, definition, null, 'Repair', missing);
    expect(noEvidence).toMatchObject({
      status: 'FAILED',
      failingCheck: 'acceptance file probe.txt is missing or unreadable',
    });
    expect(noEvidence.reason).toContain('no assertions');
    expect(noEvidence.reason).toContain('Acceptance file check failed');
    const invalid = await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, null, 'bad json', 'Repair', present);
    expect(invalid).toMatchObject({ status: 'FAILED', failingCheck: 'acceptance definition' });
    expect(invalid.reason).toContain('invalid: bad json');
    const noDefinition = await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, null, null, 'Repair', missing);
    expect(noDefinition.status).toBe('PASSED');
  });

  it('uses the same 200-character scenario key as the ledger', async () => {
    const longScenario = 's'.repeat(ACCEPTANCE_SCENARIO_MAX);
    expect(ACCEPTANCE_SCENARIO_MAX).toBe(200);
    const definition = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [{ id: 'probe-file', scenario: longScenario, type: 'file', path: 'probe.txt', contains: 'fixed' }],
    }));
    const suffixed = await applyAcceptanceChecks({ status: 'PASSED', reason: 'ok' }, definition, null, `${longScenario}suffix`, async () => null);
    expect(suffixed).toMatchObject({ status: 'FAILED' });
    expect(acceptanceInstruction(definition, `${longScenario}suffix`)).toContain(`Acceptance definition for ${JSON.stringify(longScenario)}`);
  });

  it('offers a strict and a silent parse path', () => {
    expect(acceptanceText('not json', false)).toBeNull();
    expect(() => acceptanceText('not json', true)).toThrow(/valid JSON/);
    expect(acceptanceText(JSON.stringify(valid), true)?.checks).toHaveLength(4);
  });

  it('requires the exact declared assertion and state multiset', async () => {
    const definition = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [
        { id: 'coin', scenario: 'Coin pickup', type: 'assertion', description: 'coin count', expected: '1' },
        { id: 'door', scenario: 'Coin pickup', type: 'state', instancePath: 'Workspace.Door', property: 'Enabled', expected: 'true' },
        { id: 'file', scenario: 'Coin pickup', type: 'file', path: 'coin.txt', contains: 'collected' },
      ],
    }));
    const report = {
      assertions: [{ description: 'coin count', expected: '1', actual: '1' }],
      stateChecks: [{ instancePath: 'Workspace.Door', property: 'Enabled', expected: 'true', actual: 'true' }],
    };
    const extraAssertion = { ...report, assertions: [...report.assertions, { description: 'extra', expected: 'true', actual: 'true' }] };
    const extraState = { ...report, stateChecks: [...report.stateChecks, { instancePath: 'Workspace.Door', property: 'Locked', expected: 'false', actual: 'false' }] };
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', report)).toBeNull();
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', extraAssertion)).toContain('unexpected');
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', extraState)).toContain('unexpected');
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', { assertions: [], stateChecks: report.stateChecks })).toContain('missing');
    const wrong = { ...report, assertions: [{ description: 'coin count', expected: '2', actual: '2' }] };
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', wrong)).toContain('missing');
    expect(verifyAcceptanceChecks(definition, 'Coin pickup', wrong)).toContain('unexpected');
    const fileOnly = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [{ id: 'file', scenario: 'Coin pickup', type: 'file', path: 'coin.txt', contains: 'collected' }],
    }));
    expect(verifyAcceptanceChecks(fileOnly, 'Coin pickup', extraAssertion)).toBeNull();

    const failed = await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, definition, null, 'Coin pickup', async () => 'collected', extraAssertion);
    expect(failed.status).toBe('FAILED');
    expect(failed.reason).toContain('Acceptance assertion/state set failed');
    expect(failed.reason).toContain('unexpected');
    expect(failed.failingCheck).toContain('acceptance set:');
    const earlier = { status: 'FAILED' as const, reason: '1 assertion did not match.', failingCheck: 'assertion "coin count": expected 1, got 0' };
    const preserved = await applyAcceptanceChecks(earlier, definition, null, 'Coin pickup', async () => 'collected', extraAssertion);
    expect(preserved.failingCheck).toBe(earlier.failingCheck);
    expect(preserved.reason).toContain('Acceptance assertion/state set failed');
    const exact = await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, definition, null, 'Coin pickup', async () => 'collected', report);
    expect(exact.status).toBe('PASSED');
  });

  it('counts duplicate declarations and trims only for comparison', () => {
    const duplicate = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [
        { id: 'a1', scenario: 'Coin pickup', type: 'assertion', description: 'coin count', expected: '1' },
        { id: 'a2', scenario: 'Coin pickup', type: 'assertion', description: 'coin count', expected: '1' },
      ],
    }));
    const one = { assertions: [{ description: 'coin count', expected: '1', actual: '1' }], stateChecks: [] };
    const two = { assertions: [...one.assertions, ...one.assertions], stateChecks: [] };
    const three = { assertions: [...two.assertions, ...one.assertions], stateChecks: [] };
    expect(verifyAcceptanceChecks(duplicate, 'Coin pickup', one)).toContain('missing');
    expect(verifyAcceptanceChecks(duplicate, 'Coin pickup', two)).toBeNull();
    expect(verifyAcceptanceChecks(duplicate, 'Coin pickup', three)).toContain('unexpected');
    const single = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [{ id: 'a', scenario: 'Coin pickup', type: 'assertion', description: 'coin count', expected: '1' }],
    }));
    expect(verifyAcceptanceChecks(single, 'Coin pickup', {
      assertions: [{ description: '  coin count  ', expected: '  1  ', actual: '1' }],
      stateChecks: [],
    })).toBeNull();
  });

  it('reports both a set mismatch and a file failure', async () => {
    const definition = parseAcceptanceDefinition(JSON.stringify({
      version: 1,
      checks: [
        { id: 'a', scenario: 'Coin pickup', type: 'assertion', description: 'coin count', expected: '1' },
        { id: 'file', scenario: 'Coin pickup', type: 'file', path: 'coin.txt', contains: 'collected' },
      ],
    }));
    const extra = {
      assertions: [
        { description: 'coin count', expected: '1', actual: '1' },
        { description: 'extra', expected: 'true', actual: 'true' },
      ],
      stateChecks: [],
    };
    const result = await applyAcceptanceChecks({ status: 'PASSED', reason: 'all matched' }, definition, null, 'Coin pickup', async () => null, extra);
    expect(result.status).toBe('FAILED');
    expect(result.reason).toContain('Acceptance assertion/state set failed');
    expect(result.reason).toContain('Acceptance file check failed');
    expect(result.failingCheck).toContain('acceptance set:');
  });

  it('maps read outcomes to strict or silent definitions', () => {
    const raw = JSON.stringify(valid);
    expect(acceptanceFromRead({ kind: 'missing' }, true)).toBeNull();
    expect(acceptanceFromRead({ kind: 'text', raw }, true)?.checks).toHaveLength(4);
    expect(acceptanceFromRead({ kind: 'text', raw: 'bad' }, false)).toBeNull();
    expect(() => acceptanceFromRead({ kind: 'text', raw: 'bad' }, true)).toThrow(/valid JSON/);
    expect(() => acceptanceFromRead({ kind: 'error', error: new Error('denied') }, true)).toThrow(/denied/);
    expect(acceptanceFromRead({ kind: 'error', error: new Error('denied') }, false)).toBeNull();
  });

  it('is wired into the repair plan, preload and api', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("ipcMain.handle('acceptance:read'");
    expect(main).toContain("'.openro/acceptance.json'");
    expect(main).toContain('acceptanceInstruction(definition, plan.scenario)');
    expect(main).toContain('readAcceptanceDefinition(projectRoot, true)');
    expect(main).toContain('applyAcceptanceChecks(verdict, definition, definitionError, report.scenario');
    expect(main).toContain('}, report);');
    expect(main).toContain('stat.size > 256_000');
    expect(main).toContain('acceptanceFromRead(outcome, strict)');
    expect(main).toContain('readAcceptanceDefinition(root, true)');
    expect(main).toContain("code === 'ENOENT' || code === 'ENOTDIR'");
    const module = readFileSync('electron/acceptanceChecks.ts', 'utf8');
    expect(module).toContain('Acceptance file check failed');
    expect(module).toContain('Acceptance definition is invalid');
    expect(module).toContain('ACCEPTANCE_SCENARIO_MAX');
    expect(module).toContain('if (strict) {');
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'acceptance:read'");
    const api = readFileSync('src/api.ts', 'utf8');
    expect(api).toContain('acceptanceChecks');
    expect(api).toContain("import type { AcceptanceDefinition } from '../electron/acceptanceChecks';");
    expect(api).not.toContain("import { AcceptanceDefinition }");
  });
});

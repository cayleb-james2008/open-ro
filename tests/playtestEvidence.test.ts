import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { resolveWithinRoot } from '../electron/pathSafety';
import { gradePlaytest, parsePlaytestReport } from '../electron/playtestEvidence';

const PNG = Buffer.from('89504e470d0a1a0a0000000d4948445200000001000000010806000000', 'hex');

describe('playtest evidence gate (S04)', () => {
  let root = '';
  const grade = async (raw: unknown) => gradePlaytest(parsePlaytestReport(raw), (reported) => resolveWithinRoot(root, reported));
  const full = {
    scenario: 'Player touches the coin',
    assertions: [{ description: 'coin collected message shown', expected: 'true', actual: 'true' }],
    stateChecks: [{ instancePath: 'Players.Tester.leaderstats.Coins', property: 'Value', expected: '1', actual: '1' }],
    screenshots: ['shots/after.png'],
  };

  beforeEach(async () => {
    root = await mkdtemp(path.join(tmpdir(), 'openro-playtest-'));
    await writeFile(path.join(root, 'after.txt'), 'not an image', 'utf8');
    await mkdir(path.join(root, 'shots'));
    await writeFile(path.join(root, 'shots', 'after.png'), PNG);
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it('passes only with assertions, state checks and a real PNG that all match', async () => {
    const verdict = await grade(full);
    expect(verdict.status).toBe('PASSED');
    expect(verdict.screenshots).toEqual([{ path: 'shots/after.png', sha256: expect.stringMatching(/^[0-9a-f]{64}$/), bytes: PNG.length }]);
  });

  it.each([
    ['assertions', { ...full, assertions: [] }, /no assertions/],
    ['state checks', { ...full, stateChecks: [] }, /no state checks/],
    ['screenshots', { ...full, screenshots: [] }, /no readable PNG screenshot/],
    ['a non-PNG screenshot', { ...full, screenshots: ['after.txt'] }, /after\.txt \(not a PNG\)/],
    ['a missing screenshot', { ...full, screenshots: ['shots/none.png'] }, /shots\/none\.png/],
    ['a screenshot outside the project', { ...full, screenshots: ['../outside.png'] }, /escapes/],
  ])('is never PASSED without %s', async (_label, report, reason) => {
    const verdict = await grade(report);
    expect(verdict.status).toBe('NO_EVIDENCE');
    expect(verdict.reason).toMatch(reason);
  });

  it('is never PASSED with an empty or junk report', async () => {
    expect((await grade({})).status).toBe('NO_EVIDENCE');
    await expect(grade('passed')).rejects.toThrow(/must be an object/);
    await expect(grade({ ...full, assertions: [{ description: 'x', expected: {}, actual: {} }] })).rejects.toThrow(/expected/);
  });

  it('ignores a self-declared passed flag and records the failing check', async () => {
    const verdict = await grade({ ...full, assertions: [{ description: 'door opens', expected: 'Open', actual: 'Closed', passed: true }] });
    expect(verdict).toMatchObject({ status: 'FAILED', failingCheck: 'assertion "door opens": expected Open, got Closed' });
  });

  it('records a failing state check even without a screenshot', async () => {
    const verdict = await grade({ ...full, screenshots: [], stateChecks: [{ instancePath: 'Workspace.Door', property: 'CanCollide', expected: 'false', actual: 'true' }] });
    expect(verdict).toMatchObject({ status: 'FAILED', failingCheck: 'state Workspace.Door.CanCollide: expected false, got true' });
  });

  it('never passes over-limit input: a failing 51st assertion fails, 51 matching items are not evidence', async () => {
    const match = { description: 'ok', expected: 'Open', actual: 'Open' };
    const failing51 = await grade({ ...full, assertions: [...Array.from({ length: 50 }, () => match), { description: 'door 51', expected: 'Open', actual: 'Closed' }] });
    expect(failing51).toMatchObject({ status: 'FAILED', failingCheck: 'assertion "door 51": expected Open, got Closed' });
    const many = await grade({ ...full, stateChecks: Array.from({ length: 51 }, () => full.stateChecks[0]) });
    expect(many).toMatchObject({ status: 'NO_EVIDENCE', reason: expect.stringMatching(/stateChecks has 51 items, over the limit of 50/) });
  });

  it('never passes values that differ after 2,000 characters, and refuses over-long values', async () => {
    const long = 'x'.repeat(2_000);
    const differs = await grade({ ...full, assertions: [{ description: 'long', expected: `${long}A`, actual: `${long}B` }] });
    expect(differs.status).toBe('FAILED');
    const same = await grade({ ...full, assertions: [{ description: 'long', expected: `${long}A`, actual: `${long}A` }] });
    expect(same).toMatchObject({ status: 'NO_EVIDENCE', reason: expect.stringMatching(/longer than 2000 characters/) });
  });
});

import * as crypto from 'node:crypto';
import { readFileSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { resolveWithinRoot } from '../electron/pathSafety';
import { gradePlaytest, parsePlaytestReport, resolvePlaytestScreenshot } from '../electron/playtestEvidence';

const PNG = Buffer.from('89504e470d0a1a0a0000000d4948445200000001000000010806000000', 'hex');
const JPEG = Buffer.from('ffd8ffe000104a46494600010100000100010000ffdb', 'hex');

describe('playtest evidence gate (S04)', () => {
  let root = '';
  let captures = '';
  const grade = async (raw: unknown) => gradePlaytest(parsePlaytestReport(raw), (reported) => resolveWithinRoot(root, reported));
  const gradeWithCaptures = async (raw: unknown) =>
    gradePlaytest(parsePlaytestReport(raw), (reported) => resolvePlaytestScreenshot(root, reported, captures));
  const full = {
    scenario: 'Player touches the coin',
    assertions: [{ description: 'coin collected message shown', expected: 'true', actual: 'true' }],
    stateChecks: [{ instancePath: 'Players.Tester.leaderstats.Coins', property: 'Value', expected: '1', actual: '1' }],
    screenshots: ['shots/after.png'],
  };

  beforeEach(async () => {
    root = await mkdtemp(path.join(tmpdir(), 'openro-playtest-'));
    captures = await mkdtemp(path.join(tmpdir(), 'openro-captures-'));
    await writeFile(path.join(root, 'after.txt'), 'not an image', 'utf8');
    await mkdir(path.join(root, 'shots'));
    await writeFile(path.join(root, 'shots', 'after.png'), PNG);
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
    await rm(captures, { recursive: true, force: true });
  });

  it('passes only with assertions, state checks and a real PNG that all match', async () => {
    const verdict = await grade(full);
    expect(verdict.status).toBe('PASSED');
    expect(verdict.screenshots).toEqual([{ path: 'shots/after.png', sha256: expect.stringMatching(/^[0-9a-f]{64}$/), bytes: PNG.length }]);
  });

  it.each([
    ['assertions', { ...full, assertions: [] }, /no assertions/],
    ['state checks', { ...full, stateChecks: [] }, /no state checks/],
    ['screenshots', { ...full, screenshots: [] }, /no readable image screenshot/],
    ['a non-PNG screenshot', { ...full, screenshots: ['after.txt'] }, /after\.txt \(not a PNG or JPEG\)/],
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

  it('fails on a later assertion or state check, not only the first', async () => {
    const good = full.assertions[0];
    const laterAssertion = await grade({ ...full, assertions: [good, good, { description: 'third', expected: '3', actual: '2' }] });
    expect(laterAssertion).toMatchObject({ status: 'FAILED', failingCheck: 'assertion "third": expected 3, got 2' });
    const state = full.stateChecks[0];
    const laterState = await grade({ ...full, stateChecks: [state, { ...state, instancePath: 'Workspace.Door', property: 'Open', expected: 'true', actual: 'false' }] });
    expect(laterState).toMatchObject({ status: 'FAILED', failingCheck: 'state Workspace.Door.Open: expected true, got false' });
  });

  it('accepts a JPEG saved in the managed capture folder as evidence', async () => {
    const file = path.join(captures, 'shot-1.jpg');
    await writeFile(file, JPEG);
    const verdict = await gradeWithCaptures({ ...full, screenshots: [file] });
    expect(verdict.status).toBe('PASSED');
    const expected = crypto.createHash('sha256').update(JPEG).digest('hex');
    expect(verdict.screenshots).toEqual([{ path: file, sha256: expected, bytes: JPEG.length }]);
  });

  it('still resolves project-relative images through the capture-aware resolver', async () => {
    const verdict = await gradeWithCaptures(full);
    expect(verdict.status).toBe('PASSED');
    expect(verdict.screenshots[0].path).toBe('shots/after.png');
  });

  it('rejects capture paths outside the managed folder, prefix siblings, traversal, and links out', async () => {
    const outside = await mkdtemp(path.join(tmpdir(), 'openro-outside-capture-'));
    const outsideFile = path.join(outside, 'x.jpg');
    await writeFile(outsideFile, JPEG);
    expect((await gradeWithCaptures({ ...full, screenshots: [outsideFile] })).status).toBe('NO_EVIDENCE');

    const sibling = `${captures}-evil`;
    await mkdir(sibling, { recursive: true });
    const siblingFile = path.join(sibling, 'x.jpg');
    await writeFile(siblingFile, JPEG);
    expect((await gradeWithCaptures({ ...full, screenshots: [siblingFile] })).status).toBe('NO_EVIDENCE');

    const escapeFile = path.join(captures, '..', 'escape.jpg');
    await writeFile(escapeFile, JPEG);
    expect((await gradeWithCaptures({ ...full, screenshots: [escapeFile] })).status).toBe('NO_EVIDENCE');

    const outsideDir = await mkdtemp(path.join(tmpdir(), 'openro-link-target-'));
    await writeFile(path.join(outsideDir, 'secret.jpg'), JPEG);
    await symlink(outsideDir, path.join(captures, 'link'), process.platform === 'win32' ? 'junction' : 'dir');
    expect((await gradeWithCaptures({ ...full, screenshots: [path.join(captures, 'link', 'secret.jpg')] })).status).toBe('NO_EVIDENCE');

    await rm(outside, { recursive: true, force: true });
    await rm(sibling, { recursive: true, force: true });
    await rm(escapeFile, { force: true });
    await rm(outsideDir, { recursive: true, force: true });
  });

  it('matches capture-dir casing like the platform filesystem', async () => {
    const file = path.join(captures, 'case.jpg');
    await writeFile(file, JPEG);
    const verdict = await gradeWithCaptures({ ...full, screenshots: [file.toUpperCase()] });
    if (process.platform === 'win32') {
      expect(verdict.status).toBe('PASSED');
    } else {
      expect(verdict.status).toBe('NO_EVIDENCE');
    }
  });

  it('rejects a valid image larger than 25 MB', async () => {
    const file = path.join(captures, 'huge.jpg');
    const huge = Buffer.alloc(25_000_001);
    PNG.copy(huge);
    await writeFile(file, huge);
    const verdict = await gradeWithCaptures({ ...full, screenshots: [file] });
    expect(verdict.status).toBe('NO_EVIDENCE');
    expect(verdict.reason).toMatch(/not a file under 25 MB/);
  });

  it('wires the managed capture folder into report_playtest and the agent schema', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain('resolvePlaytestScreenshot(root, reported');
    expect(main).toContain("'studio-captures'");
    const agent = readFileSync('electron/agent.ts', 'utf8');
    expect(agent).toContain('saved Studio capture');
  });
});

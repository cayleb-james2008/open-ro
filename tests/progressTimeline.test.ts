import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { buildProgressTimeline, progressSummary } from '../src/progressTimeline';
import type { PlaytestLedgerEntry, PlaytestRepairPlan } from '../electron/playtestLedger';
import type { StoredChatSession } from '../electron/sessions';

const root = 'C:\\Users\\KK\\Documents\\Default Project\\open-ro';

function entry(overrides: Partial<PlaytestLedgerEntry>): PlaytestLedgerEntry {
  return {
    file: '2026-11-05T00-00-00-000Z-failed.json',
    scenario: 'Door opens',
    status: 'FAILED',
    reason: '1 assertion did not match.',
    failingCheck: 'assertion "door": expected true, got false',
    screenshots: 1,
    createdAt: '2026-11-05T00:00:00.000Z',
    ...overrides,
  };
}

function session(overrides: Partial<StoredChatSession>): StoredChatSession {
  return {
    id: 'chat-1',
    title: 'Door repair',
    messages: [
      { role: 'user', content: 'fix the door' },
      { role: 'assistant', content: 'staging a write' },
    ],
    updatedAt: '2026-11-05T01:00:00.000Z',
    projectRoot: root,
    ...overrides,
  };
}

const plan: PlaytestRepairPlan = {
  scenario: 'Door opens',
  failingCheck: 'assertion "door": expected true, got false',
  reason: '1 assertion did not match.',
  attempt: 2,
  screenshots: 1,
  lastFailedAt: '2026-11-05T02:00:00.000Z',
  lastFailedFile: '2026-11-05T00-00-00-000Z-failed.json',
  instruction: 'Repair "Door opens" (attempt 2).',
};

describe('progress timeline', () => {
  it('orders repair, playtest and chat events newest first', () => {
    const timeline = buildProgressTimeline({
      repairPlan: plan,
      playtestHistory: [
        entry({}),
        entry({ file: 'passed.json', status: 'PASSED', createdAt: '2026-11-04T23:00:00.000Z', failingCheck: undefined }),
      ],
      sessions: [session({})],
      projectRoot: root,
    });
    expect(timeline.map((event) => event.kind)).toEqual(['repair', 'chat', 'playtest', 'playtest']);
    expect(timeline[0]).toMatchObject({ title: 'Repair attempt 2 — Door opens', status: 'OPEN', detail: plan.failingCheck });
    expect(timeline[1]).toMatchObject({ title: 'Door repair', detail: '2 messages', status: 'CHAT' });
    expect(timeline[2].title).toBe('FAILED — Door opens');
    expect(timeline[3].title).toBe('PASSED — Door opens');
  });

  it('summarizes runs, passes and the open attempt', () => {
    const stats = progressSummary({
      repairPlan: plan,
      playtestHistory: [entry({}), entry({ file: 'p.json', status: 'PASSED' }), entry({ file: 'n.json', status: 'NO_EVIDENCE' })],
      sessions: [session({})],
      projectRoot: root,
    });
    expect(stats).toEqual({ runs: 3, passed: 1, failed: 1, openAttempt: 2, scenario: 'Door opens', chats: 1 });
  });

  it('excludes chats from other projects and handles empty input', () => {
    const other = session({ id: 'chat-2', title: 'Other', projectRoot: 'C:\\Other' });
    const timeline = buildProgressTimeline({ repairPlan: null, playtestHistory: [], sessions: [other], projectRoot: root });
    expect(timeline).toEqual([]);
    expect(progressSummary({ repairPlan: null, playtestHistory: [], sessions: [other], projectRoot: root })).toMatchObject({ runs: 0, chats: 0, openAttempt: null });
    const legacy = session({ id: 'chat-3', title: 'Legacy', projectRoot: undefined });
    expect(buildProgressTimeline({ repairPlan: null, playtestHistory: [], sessions: [legacy], projectRoot: root })).toHaveLength(1);
  });

  it('sorts invalid timestamps last without throwing', () => {
    const timeline = buildProgressTimeline({
      repairPlan: null,
      playtestHistory: [entry({ createdAt: 'not-a-date' }), entry({ file: 'p.json', createdAt: '2026-11-05T00:00:00.000Z' })],
      sessions: [],
      projectRoot: root,
    });
    expect(timeline.map((event) => event.id)).toEqual(['playtest-p.json', 'playtest-2026-11-05T00-00-00-000Z-failed.json']);
  });

  it('is wired into the Workspace above Local media', () => {
    const app = readFileSync('src/App.tsx', 'utf8');
    expect(app).toContain('buildProgressTimeline(progressInput)');
    expect(app).toContain('progressSummary(progressInput)');
    expect(app).toContain('<span>Plan &amp; progress</span>');
    expect(app).toContain('progressStats.passed}/${progressStats.runs} passed');
    expect(app.indexOf('<span>Plan &amp; progress</span>')).toBeLessThan(app.indexOf('<span>Local media</span>'));
    expect(app).toContain('progressEvents.map((event)');
    expect(app).toContain('No progress yet');
  });
});

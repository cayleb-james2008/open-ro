import { describe, expect, it } from 'vitest';
import { sanitizeSessions } from '../electron/sessions';
import { isSessionVisibleForProject, normalizeProjectPath } from '../src/sessionScope';

function session(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return { id: 'chat-1', title: 'Door logic', messages: [], updatedAt: '2026-11-03T00:00:00.000Z', ...overrides };
}

describe('project path normalization', () => {
  it('ignores slashes, case, whitespace, and trailing separators', () => {
    expect(normalizeProjectPath('  C:\\Work\\Obby\\  ')).toBe('c:/work/obby');
    expect(normalizeProjectPath('c:/work/obby')).toBe('c:/work/obby');
  });
});

describe('session visibility per project', () => {
  const active = 'C:\\Work\\Obby';

  it('shows every chat while no project is open', () => {
    expect(isSessionVisibleForProject({ projectRoot: 'C:\\Other' }, null)).toBe(true);
    expect(isSessionVisibleForProject({}, null)).toBe(true);
  });

  it('shows chats that belong to the open project, including path variants', () => {
    expect(isSessionVisibleForProject({ projectRoot: 'C:\\Work\\Obby' }, active)).toBe(true);
    expect(isSessionVisibleForProject({ projectRoot: 'c:/work/obby/' }, active)).toBe(true);
  });

  it('hides chats that belong to a different project', () => {
    expect(isSessionVisibleForProject({ projectRoot: 'C:\\Work\\Other' }, active)).toBe(false);
  });

  it('never hides legacy or project-less chats', () => {
    expect(isSessionVisibleForProject({}, active)).toBe(true);
    expect(isSessionVisibleForProject({ projectRoot: '' }, active)).toBe(true);
  });
});

describe('session sanitizer project field', () => {
  it('keeps a trimmed projectRoot', () => {
    const snapshot = sanitizeSessions({ activeId: 'chat-1', sessions: [session({ projectRoot: '  C:\\Work\\Obby  ' })] });
    expect(snapshot.sessions[0].projectRoot).toBe('C:\\Work\\Obby');
  });

  it('drops non-string and blank projectRoot values without dropping the session', () => {
    const snapshot = sanitizeSessions({
      activeId: 'chat-1',
      sessions: [session({ id: 'chat-1', projectRoot: 42 }), session({ id: 'chat-2', projectRoot: '   ' })],
    });
    expect(snapshot.sessions).toHaveLength(2);
    expect(snapshot.sessions[0].projectRoot).toBeUndefined();
    expect(snapshot.sessions[1].projectRoot).toBeUndefined();
  });

  it('caps an oversized projectRoot', () => {
    const snapshot = sanitizeSessions({ activeId: 'chat-1', sessions: [session({ projectRoot: 'x'.repeat(5_000) })] });
    expect(snapshot.sessions[0].projectRoot).toHaveLength(1_000);
  });

  it('still drops non-schema fields alongside the new project field', () => {
    const snapshot = sanitizeSessions({ activeId: 'chat-1', sessions: [session({ projectRoot: 'C:\\Work\\Obby', apiKey: 'sk-nope' })] });
    expect(JSON.stringify(snapshot)).not.toContain('sk-nope');
    expect(snapshot.sessions[0].projectRoot).toBe('C:\\Work\\Obby');
  });
});

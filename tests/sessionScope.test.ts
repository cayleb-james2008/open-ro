import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { sanitizeSessions } from '../electron/sessions';
import { isSessionVisibleForProject, normalizeProjectPath, resolveActiveSessionId } from '../src/sessionScope';

function session(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return { id: 'chat-1', title: 'Door logic', messages: [], updatedAt: '2026-11-03T00:00:00.000Z', ...overrides };
}

describe('project path normalization', () => {
  it('ignores slashes, case, whitespace, and trailing separators for Windows paths', () => {
    expect(normalizeProjectPath('  C:\\Work\\Obby\\  ')).toBe('c:/work/obby');
    expect(normalizeProjectPath('c:/work/obby')).toBe('c:/work/obby');
  });

  it('keeps POSIX paths case-sensitive', () => {
    expect(normalizeProjectPath('/Work/Obby/')).toBe('/Work/Obby');
    expect(normalizeProjectPath('/work/obby')).toBe('/work/obby');
    expect(isSessionVisibleForProject({ projectRoot: '/work/obby' }, '/Work/Obby')).toBe(false);
    expect(isSessionVisibleForProject({ projectRoot: '/Work/Obby/' }, '/Work/Obby')).toBe(true);
  });

  it('treats forward-slash UNC shares as Windows paths', () => {
    expect(normalizeProjectPath('//Server/Share/')).toBe('//server/share');
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

describe('active chat after a project change', () => {
  const active = 'C:\\Work\\Obby';
  const sessions = [
    { id: 'a', projectRoot: 'C:\\Work\\Obby' },
    { id: 'b', projectRoot: 'C:\\Work\\Other' },
    { id: 'legacy' },
  ];

  it('keeps the active chat when it is visible', () => {
    expect(resolveActiveSessionId(sessions, 'a', active)).toBe('a');
    expect(resolveActiveSessionId(sessions, 'legacy', active)).toBe('legacy');
  });

  it('replaces a hidden active chat with the first visible chat', () => {
    expect(resolveActiveSessionId(sessions, 'b', active)).toBe('a');
  });

  it('clears the active chat when nothing visible remains', () => {
    expect(resolveActiveSessionId([{ id: 'b', projectRoot: 'C:\\Work\\Other' }], 'b', active)).toBeNull();
  });

  it('never auto-selects a chat when none was open', () => {
    expect(resolveActiveSessionId(sessions, null, active)).toBeNull();
  });
});

describe('agent turn project coherence', () => {
  it('agent:send validates and switches to the chat project before running tools', () => {
    const source = readFileSync('electron/main.ts', 'utf8');
    const start = source.indexOf("ipcMain.handle('agent:send'");
    expect(start).toBeGreaterThan(-1);
    const handler = source.slice(start, source.indexOf('\n  });', start));
    expect(handler).toContain('projectRoot?: string');
    expect(handler).toContain('args.projectRoot');
    expect(handler).toContain('isDirectory()');
    expect(handler).toContain('approvals.clear()');
    expect(handler).toContain('studioReads.clear()');
    expect(handler).toContain('saveProjectRoot(');
    const switchAt = handler.indexOf('projectRoot = next;');
    const turnAt = handler.indexOf('runAgentTurn({');
    expect(switchAt).toBeGreaterThan(-1);
    expect(turnAt).toBeGreaterThan(switchAt);
    expect(handler).toContain('projectRoot: turnRoot,');
    expect(handler).toContain('buildExecutor(turnRoot)');
  });

  it('the renderer sends the chat project and auto-switches a hidden chat', () => {
    const source = readFileSync('src/App.tsx', 'utf8');
    expect(source).toContain('api.sendPrompt({ sessionId, messages: history, ...(turnProjectRoot ? { projectRoot: turnProjectRoot } : {}) })');
    expect(source).toContain('const turnProjectRoot = prior.find((session) => session.id === sessionId)?.projectRoot ?? projectRoot ?? undefined;');
    expect(source).toContain('resolveActiveSessionId(sessions, activeSession, projectRoot)');
    expect(source).toContain("const showsDiff = item.kind === 'write' || item.studioOp === 'write' || item.kind === 'delete';");
  });

  it('treats the same folder under another spelling as no project switch', () => {
    const source = readFileSync('electron/main.ts', 'utf8');
    const selectStart = source.indexOf("ipcMain.handle('project:select-root'");
    const select = source.slice(selectStart, source.indexOf('\n  });', selectStart));
    expect(select).toContain('if (projectRoot && sameProjectRoot(projectRoot, next)) {');
    expect(select).toContain('return projectRoot;');
    const keepAt = select.indexOf('return projectRoot;');
    const assignAt = select.indexOf('projectRoot = next;');
    expect(keepAt).toBeGreaterThan(-1);
    expect(assignAt).toBeGreaterThan(keepAt);
    expect(select).not.toContain('path.resolve(next)');
    const resolveStart = source.indexOf('async function resolveApproval');
    const resolve = source.slice(resolveStart, source.indexOf('\nfunction registerIpc', resolveStart));
    expect(resolve).toContain('!sameProjectRoot(item.projectRoot, projectRoot)');
    const turnStart = source.indexOf("ipcMain.handle('agent:send'");
    const turn = source.slice(turnStart, source.indexOf('\n  });', turnStart));
    expect(turn).toContain('let turnRoot = currentRoot;');
    expect(turn).toContain('!sameProjectRoot(currentRoot, next)');
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

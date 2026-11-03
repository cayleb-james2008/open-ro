import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// The helpers are unit-tested elsewhere; these checks pin that the app actually uses them.
const read = (file: string) => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');

describe('approval wiring in main.ts and App.tsx', () => {
  it('main.ts registers the approval channels only through registerApprovalChannels', () => {
    const main = read('electron/main.ts');
    expect(main).toContain('registerApprovalChannels((channel, listener) => ipcMain.handle(channel, listener), resolveApproval);');
    for (const channel of ['project:write', 'command:run', 'approvals:resolve']) {
      expect(main).not.toContain(`'${channel}'`);
    }
  });

  it('App.tsx sends follow-ups through the guarded helpers and has no manual resend button', () => {
    const app = read('src/App.tsx');
    expect(app).toContain('const next = nextFollowup(readyFollowups, activeSession, sending);');
    expect(app).toContain('setDraft((current) => draftAfterSend(current, textOverride));');
    expect(app).not.toMatch(/setDraft\(''\)/);
    expect(app).toContain('showsResultIn(lastResult.sessionId, activeSession)');
    expect(app).toContain('const owner = resultSession(followupsRef.current, id);');
    expect(app).not.toContain('Send result to the agent');
    expect(app.match(/sendDraft\(followupText\(/g)).toHaveLength(1);
  });
});

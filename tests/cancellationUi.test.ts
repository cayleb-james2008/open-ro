import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('cancellation UI', () => {
  it('exposes a typed cancel call over the allowlisted channel', () => {
    const api = readFileSync('src/api.ts', 'utf8');
    expect(api).toContain('cancelAgent()');
    expect(api).toContain("call<{ ok: boolean; cancelled: boolean }>('agent:cancel')");
  });

  it('renders a Stop control only while a turn is running, and it calls the cancel API', () => {
    const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
    expect(app).toContain('async function stopTurn()');
    expect(app).toContain('await api.cancelAgent()');
    expect(app).toContain(
      '{sending && (\n                <button type="button" className="outline-btn" onClick={() => void stopTurn()} aria-label="Stop the current agent turn">Stop</button>\n              )}',
    );
    expect(app).toContain("current === 'Stopping the current turn…'");
    const stopAt = app.indexOf('aria-label="Stop the current agent turn"');
    const sendAt = app.indexOf('aria-label="Send message"');
    expect(stopAt).toBeGreaterThan(-1);
    expect(sendAt).toBeGreaterThan(stopAt);
  });

  it('assigns the abort controller before project validation so a cancel cannot race setup', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    const controllerAt = main.indexOf('const controller = new AbortController()');
    const rootAt = main.indexOf('const requestedRoot');
    expect(controllerAt).toBeGreaterThan(-1);
    expect(rootAt).toBeGreaterThan(controllerAt);
  });
});

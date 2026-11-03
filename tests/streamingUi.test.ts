import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
const api = readFileSync('src/api.ts', 'utf8');

describe('streaming chat UI', () => {
  it('types the delta subscription on Window', () => {
    expect(api).toContain('onAgentDelta?(listener: (payload: { sessionId: string; text: string }) => void): () => void;');
  });

  it('subscribes, appends only matching-session deltas, and returns the unsubscribe', () => {
    expect(app).toContain('!window.openro?.onAgentDelta');
    expect(app).toContain('return window.openro.onAgentDelta(({ sessionId, text }) => {');
    expect(app).toContain('prev && prev.sessionId === sessionId ? { sessionId, text: prev.text + text } : { sessionId, text }');
  });

  it('starts an empty stream entry before sending and clears it in finally', () => {
    expect(app).toContain("setStreaming({ sessionId, text: '' });");
    expect(app).toContain('setStreaming(null);');
    expect(app.indexOf("setStreaming({ sessionId, text: '' })")).toBeLessThan(app.indexOf('setSending(true)'));
  });

  it('renders streamed text in the sending bubble instead of thinking dots', () => {
    expect(app).toContain('streaming.sessionId === activeSession && streaming.text');
    expect(app).toContain('<div className="message-text">{streaming.text}</div>');
    expect(app).toContain('<div className="thinking">');
  });
});

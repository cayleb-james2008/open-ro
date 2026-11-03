// Dev-only design comps at /comps/<state>. main.tsx loads this behind import.meta.env.DEV, so
// production builds drop it. Each comp renders the real App against a fixture IPC bridge.
import { StrictMode } from 'react';
import type { Root } from 'react-dom/client';
import App from '../App';
import { ErrorBoundary } from '../ErrorBoundary';
import './comps.css';

type Fixture = Record<string, unknown>;
const base: Fixture = {
  'providers:catalog': [], 'providers:list': [], 'providers:storage-state': { secure: true, services: 0, store: 'fixture' },
  'project:root': '/home/dev/obby', 'project:list': [], 'project:checkpoints': [], 'studio:checkpoints': [],
  'capabilities:detect': [], 'mcp:status': { running: false, tools: [] }, 'approvals:list': [], 'sessions:load': { activeId: null, sessions: [] },
};
const approvals: Fixture = {
  ...base,
  'approvals:list': [
    { id: 'w1', kind: 'write', createdAt: '2026-11-03T15:40:00.000Z', relPath: 'src/Shop.server.lua', previous: 'local price = 10\nprint(price)', content: '--!strict\nlocal price = 12\nprint(price)' },
    { id: 's1', kind: 'studio', createdAt: '2026-11-03T15:41:00.000Z', mcpServer: 'StudioMCP', studioOp: 'read', studioPath: 'Workspace.Door' },
  ],
  'sessions:load': { activeId: 'c1', sessions: [{ id: 'c1', title: 'Shop price', updatedAt: '2026-11-03T15:39:00.000Z', messages: [
    { role: 'user', content: 'Raise the shop price to 12 and keep it server-owned.' },
    { role: 'assistant', content: 'I staged a change that sets the price on the server. Review the diff below.', trace: [{ id: 't1', name: 'write_file', args: {}, status: 'staged', approvalId: 'w1' }] },
  ] }] },
};

const COMPS: Record<string, (channel: string) => Promise<unknown>> = {
  'chat-empty': async (channel) => base[channel] ?? null,
  'chat-approvals': async (channel) => approvals[channel] ?? null,
  loading: () => new Promise(() => undefined),
  error: async () => { throw new Error('Fixture: the main process did not answer.'); },
};

export function mountComp(root: Root, name: string) {
  const invoke = COMPS[name];
  if (!invoke) {
    root.render(<nav className="comps-index">{Object.keys(COMPS).map((key) => <a key={key} href={`/comps/${key}`}>{key}</a>)}</nav>);
    return;
  }
  window.openro = { invoke };
  root.render(<StrictMode><ErrorBoundary><App /></ErrorBoundary></StrictMode>);
}

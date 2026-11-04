import { contextBridge, ipcRenderer } from 'electron';

/**
 * Narrow renderer bridge (docs/ARCHITECTURE.md §1): only allowlisted channels
 * cross this boundary, and no Node objects or secrets are exposed.
 */

const INVOKE_CHANNELS = new Set([
  'providers:catalog',
  'providers:list',
  'providers:save',
  'providers:set-active',
  'providers:delete',
  'providers:storage-state',
  'providers:test',
  'agent:send',
  'agent:cancel',
  'project:select-root',
  'project:root',
  'project:index',
  'playtest:ledger',
  'project:list',
  'project:read',
  'project:preview-write',
  'project:write',
  'project:checkpoints',
  'project:restore-checkpoint',
  'command:preview',
  'command:run',
  'mcp:status',
  'mcp:connect',
  'mcp:disconnect',
  'capabilities:detect',
  'approvals:list',
  'approvals:resolve',
  'sessions:load',
  'sessions:save',
  'shell:open-external',
]);

const api = {
  invoke: (channel: string, payload?: unknown): Promise<unknown> => {
    if (!INVOKE_CHANNELS.has(channel)) {
      return Promise.reject(new Error(`Blocked preload bridge channel: ${channel}`));
    }
    return ipcRenderer.invoke(channel, payload);
  },
  /** Subscribe to live agent text deltas. Returns an unsubscribe function. */
  onAgentDelta: (listener: (payload: { sessionId: string; text: string }) => void): (() => void) => {
    const handler = (_event: unknown, payload: unknown): void => {
      listener(payload as { sessionId: string; text: string });
    };
    ipcRenderer.on('agent:delta', handler);
    return () => {
      ipcRenderer.removeListener('agent:delta', handler);
    };
  },
};

contextBridge.exposeInMainWorld('openro', api);

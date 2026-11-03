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
  'project:select-root',
  'project:root',
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
  'mcp:call',
  'capabilities:detect',
]);

const api = {
  invoke: (channel: string, payload?: unknown): Promise<unknown> => {
    if (!INVOKE_CHANNELS.has(channel)) {
      return Promise.reject(new Error(`Blocked preload bridge channel: ${channel}`));
    }
    return ipcRenderer.invoke(channel, payload);
  },
};

contextBridge.exposeInMainWorld('openro', api);

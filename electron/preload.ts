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
  'playtest:repair-plan',
  'acceptance:read',
  'media:hardware',
  'media:comfy-status',
  'media:prepare',
  'media:generate',
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
  'mcp:pool:list',
  'mcp:pool:server-status',
  'mcp:pool:connect',
  'mcp:pool:disconnect',
  'mcp:pool:reconnect',
  'mcp:pool:save-config',
  'mcp:pool:delete-config',
  'mcp:pool:tools',
  'mcp:pool:health',
  'capabilities:detect',
  'approvals:list',
  'approvals:resolve',
  'sessions:load',
  'sessions:save',
  'shell:open-external',
  'blender:status',
  'blender:detect',
  'blender:run-task',
  'blender:cancel-task',
  'blender:history',
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
  /** Subscribe to live MCP server pool events. Returns an unsubscribe function. */
  onMcpServerEvent: (listener: (payload: unknown) => void): (() => void) => {
    const handler = (_event: unknown, payload: unknown): void => {
      listener(payload);
    };
    ipcRenderer.on('mcp:pool:event', handler);
    return () => {
      ipcRenderer.removeListener('mcp:pool:event', handler);
    };
  },
  /** Subscribe to live Blender headless task events. Returns an unsubscribe function. */
  onBlenderEvent: (listener: (payload: unknown) => void): (() => void) => {
    const handler = (_event: unknown, payload: unknown): void => {
      listener(payload);
    };
    ipcRenderer.on('blender:event', handler);
    return () => {
      ipcRenderer.removeListener('blender:event', handler);
    };
  },
};

contextBridge.exposeInMainWorld('openro', api);

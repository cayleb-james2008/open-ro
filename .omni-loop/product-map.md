# Product map

## Entry points

- `electron/main.ts` starts the window, registers IPC, and owns the project root and approval queue.
- `electron/preload.ts` exposes an allow-list of channels as `window.openro`.
- `src/main.tsx` mounts `src/App.tsx`. Vite serves the renderer at `http://127.0.0.1:5173` in development.
- Packaged mode loads `dist/index.html`. That path was not launched here.

## State

- Provider records: `userData/state.json` via `electron/providerState.ts`.
- Secrets: `userData/credentials.json` encrypted with `safeStorage`, or not written.
- Chats: `userData/sessions.json`.
- Checkpoints: `userData/checkpoints/*.json`.
- Approvals: in-memory `ApprovalQueue` only.

## UI

- One stylesheet, `src/styles.css`. Tokens are CSS variables on `:root`. There is no separate DTCG token file.
- Screens are Chat, Models, Workspace, and Integrations inside `App.tsx`.

## Verification

- `npm run typecheck`
- `npm run lint` (suppression guard plus v4 record validation)
- `npm test` (Vitest)
- `npm run build`
- `npm run verify:ui` when the renderer is already on port 5173 and Chrome is installed

## Blast radius

- `electron/secureStore.ts` (credentials)
- `electron/main.ts` (filesystem, shell, MCP spawn)
- `electron/agent.ts` (outbound model calls)

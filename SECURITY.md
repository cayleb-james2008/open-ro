# Security policy

Open-Ro is a local desktop agent. Treat model providers, MCP servers, repositories and generated code as untrusted inputs.

## Reporting vulnerabilities

Please report vulnerabilities privately to the repository maintainer before opening a public issue. Include the affected version, operating system, reproduction steps and impact. Do not include API keys, OAuth tokens, Roblox cookies or other secrets in reports.

## Current security boundaries

- Provider API keys are encrypted with Electron `safeStorage`. If `safeStorage.isEncryptionAvailable()` is false, Open-Ro refuses to persist the key. There is no plaintext fallback.
- Subscription sign-in is delegated to official, unmodified provider CLIs. Open-Ro does not parse their auth files or collect their subscription tokens. Sending a chat message is what launches that CLI.
- Project tools use only the folder selected in the main process. The renderer cannot point a tool at a different root. Paths are resolved with `realpath`, so a symlink inside the project cannot be used to read outside it. Likely secret names (`.env`, private keys, `credentials.json`) and ignored folders (`node_modules`, `.git`, build output) are refused.
- Agent file writes and shell commands are stored in the main process and run only after the user approves that approval id. The renderer cannot substitute a different path, body, or command. There is no OS-native approval dialog and no Blender job runner.
- Approved shell commands and MCP servers inherit a short environment allow-list. Provider API keys in the parent environment are not passed through.
- MCP tools execute with the privileges of the configured MCP process. Only connect servers you trust and review their documentation/source.

These safeguards are not a security certification. Review prompts and local permissions carefully, especially when opening untrusted projects.

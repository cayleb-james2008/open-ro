# Security policy

Open-Ro is a local desktop agent. Treat model providers, MCP servers, repositories and generated code as untrusted inputs.

## Reporting vulnerabilities

Please report vulnerabilities privately to the repository maintainer before opening a public issue. Include the affected version, operating system, reproduction steps and impact. Do not include API keys, OAuth tokens, Roblox cookies or other secrets in reports.

## Current security boundaries

- Provider API keys are encrypted with Electron `safeStorage`; on Linux, Open-Ro refuses API-key persistence when Electron selects the `basic_text` backend.
- Subscription sign-in is delegated to official, unmodified provider CLIs. Open-Ro does not parse their auth files or collect their subscription tokens.
- Project file access is limited to the selected directory, with path and symlink checks.
- Each agent-initiated project file write, shell command, Blender job and official CLI task requires an OS-native approval dialog.
- MCP tools execute with the privileges of the configured MCP process. Only connect servers you trust and review their documentation/source.

These safeguards are not a security certification. Review prompts and local permissions carefully, especially when opening untrusted projects.

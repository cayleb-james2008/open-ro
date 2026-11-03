# Platform support policy

## Support targets

| Component | Windows 10/11 x64 | Omarchy / Arch Linux x64 | Other Linux |
|---|---|---|---|
| Open-Ro desktop app | Primary native target | Primary native target | Best-effort AppImage/deb target |
| Provider API routes | Target | Target | Target |
| Official CLI routes | Target where vendor CLI supports Windows | Target where vendor CLI supports Linux | Depends on vendor CLI |
| Local project tools | Target | Target | Target |
| MCP stdio | Target | Target | Target |
| Roblox Studio | Official platform | Wine/Vinegar experimental | Wine/other compatibility layer experimental |
| Blender | Supported where installed | Supported where installed | Supported where installed |
| Rojo | CLI/plugin dependent | CLI/plugin dependent | CLI/plugin dependent |

## Windows baseline

- Windows 10/11 x64.
- NSIS installer.
- Windows command shims may require PowerShell-aware process launching.
- Validate secure credential storage, file locks, child-process teardown and upgrade/uninstall behavior.
- Roblox Studio integration is the first hardware-validation baseline.

## Omarchy / Arch baseline

- Native Electron desktop client.
- pacman package and AppImage fallback.
- A usable Secret Service/keyring is required for persistent API-key storage. If Electron reports insecure `basic_text` storage, Open-Ro must refuse persistence rather than write keys in plaintext.
- Document required shared libraries and desktop-session assumptions.
- Test under the user's active Wayland/compositor session and standard user permissions.
- Detect Blender, Git, Rojo and other optional tools without requiring them at application startup.

## Roblox Studio on Linux

Roblox Studio is not officially supported on Linux. Open-Ro's Linux client support does not imply official Studio support. Wine/Vinegar compatibility depends on local Studio build, Wine/Proton environment, MCP availability, graphics stack, and user configuration.

Linux Studio claims must therefore be labeled experimental until a reproducible test matrix passes on actual hardware. Never claim official Roblox support or guaranteed feature parity for the Wine/Vinegar route.

## Support-state vocabulary

- **Supported:** automated CI and real-machine acceptance passed for stated versions.
- **Best effort:** expected to work but not part of blocking release acceptance.
- **Experimental:** known compatibility risks; user testing and logs are required.
- **Planned:** not implemented yet.

Every release should publish the tested OS version, architecture, desktop environment, package format and known caveats.

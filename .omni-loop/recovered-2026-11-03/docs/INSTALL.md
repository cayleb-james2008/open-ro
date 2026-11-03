# Install Open-Ro Studio

Open-Ro Studio is currently distributed as source code. Native installers are configured in the build pipeline but have not yet been published or tested on physical Windows/Omarchy machines.

## Windows 10/11 x64

### Build from source

1. Install Node.js 22.12+ and Git.
2. Download or clone the Open-Ro source.
3. In PowerShell, run:

   ```powershell
   npm install
   npm run dev
   ```

### Build the installer

```powershell
npm run package:win
```

The NSIS installer is written to `release/`. Windows packaging should run on a Windows host (the CI job uses `windows-latest`).

## Omarchy Linux (Arch)

### Runtime prerequisites

- Node.js 22.12+ and npm (source build only)
- A working desktop secret service such as GNOME Keyring or KWallet for API-key storage
- Optional: `git`, `rojo`, `blender`, and a working Roblox Studio MCP bridge

For an Omarchy terminal:

```bash
sudo pacman -S --needed nodejs npm git
npm install
npm run dev
```

### Build an Arch package and portable AppImage

```bash
npm run package:linux-omarchy
```

The pacman package and AppImage are written to `release/`. To install a generated pacman artifact:

```bash
sudo pacman -U ./release/open-ro-*.pacman
```

To use an AppImage:

```bash
chmod +x ./release/open-ro-*.AppImage
./release/open-ro-*.AppImage
```

The AppImage and pacman targets are configured but not yet validated on an Omarchy device. If Electron's keyring detection reports `basic_text`, Open-Ro refuses to save API keys; install and unlock a desktop keyring, then restart the app.

## First launch

1. Open **Models & providers**.
2. Connect exactly one model route: direct API key, official provider CLI sign-in, OpenRouter, OpenCode CLI, Ollama, or a custom OpenAI-compatible endpoint.
3. Test the connection and make it active.
4. Start chatting. A local project folder is optional; selecting one enables file tools.
5. Install or connect only the optional tools required by your workflow.

## Roblox Studio caveat

The desktop app is designed to run natively on Windows and Omarchy. Roblox's published platform support does not include Linux, so Roblox Studio under Wine/Vinegar and its MCP transport remain experimental on Linux. Do not treat the Linux bridge as a supported Roblox configuration.

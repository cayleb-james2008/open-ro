<div align="center">
  <img src="docs/assets/open-ro-wordmark.svg" alt="Open-Ro Studio" width="300" />
  <p><strong>Your Roblox workspace. Your model. Your tools.</strong></p>
  <p>
    <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-7c8cff.svg"></a>
    <a href="https://github.com/cayleb-james2008/open-ro/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/cayleb-james2008/open-ro/actions/workflows/ci.yml/badge.svg"></a>
    <img alt="Windows and Linux" src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-35b9a5.svg">
    <img alt="Status" src="https://img.shields.io/badge/status-early%20development-e4a84c.svg">
    <img alt="Local first" src="https://img.shields.io/badge/design-local--first-6574cd.svg">
  </p>
  <p><em>Open-Ro is an independent, open-source AI development studio for creating, editing, testing, and shipping Roblox experiences.</em></p>
</div>

---

## The project in one sentence

**Open-Ro turns a natural-language idea into an editable Roblox project through a model-agnostic desktop agent, optional tools, and a repeatable build → test → inspect → repair loop.**

Open-Ro is being built for **Windows and native Linux desktops, including Omarchy (Arch Linux)**. Users bring a model connection; everything else is an optional capability they can install, connect, or skip.

> **Current release state: 0.1.0 foundation.** This repository contains an early Electron desktop client, provider routes, local project tools, approval-gated commands, an MCP client foundation, and Windows/Linux packaging configuration. Windows NSIS and Linux AppImage/deb/pacman packages build in CI; install/uninstall acceptance is still pending. It is not yet a complete Ropilot-equivalent product. The long-term target and the unfinished work are documented below and in [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Interface

These screenshots are the renderer at 1280×800, opened in a browser without the Electron preload. The green notice on the chat screen is the missing-bridge state. Inside `npm run dev` the preload is present and the notice is not shown.

![Open-Ro welcome screen with chat, models, workspace, and integrations navigation](docs/assets/ui-chat.png)

![Models screen listing the local Ollama and llama.cpp routes first](docs/assets/ui-models.png)

![Workspace screen before a project folder is selected](docs/assets/ui-workspace.png)

## What we are building

Open-Ro is not just a Luau autocomplete panel. It is intended to be a full development workspace with five connected parts:

1. **A model-agnostic agent workspace** — chat, project context, task planning, code edits, tool use, session history, review, and recovery.
2. **A Roblox Studio control plane** — inspect the DataModel, understand scripts and instances, edit Luau, run Studio operations, capture screenshots, simulate input, and diagnose test failures.
3. **A closed-loop development workflow** — plan, make a scoped change, review the diff, run or playtest, inspect evidence, repair, and repeat until the acceptance criteria pass.
4. **A creative asset pipeline** — optional Blender modeling/animation, image and thumbnail generation, asset review, and import into the game.
5. **An installable capability ecosystem** — Rojo, Git, MCP servers, local tools, and third-party providers are selectable add-ons, not mandatory infrastructure.

## The one-required-setup rule

A user should be able to install Open-Ro, run a free local model, choose a project, and start working. No Open-Ro account, hosted service, subscription, or credit wallet is required. Paid model APIs are optional. Open-Ro does not send telemetry.

### Model connections

Users can connect one or more model routes and choose which one is active. Local routes come first and need no paid account. Every later row is optional. Provider terms and available models remain controlled by their respective vendors.

| Route | How the user connects | Cost / account boundary | Current foundation |
|---|---|---|---|
| Ollama | Local endpoint + model | Free local compute; no cloud key | Default local route |
| llama.cpp server | Local endpoint + model | Free local compute; default port 9931; no API key | Local OpenAI-compatible route |
| OpenAI API | API key | Optional. Direct OpenAI Platform billing | API route |
| ChatGPT subscription | Official Codex CLI sign-in | Optional. Subscription and limits handled by Codex | CLI handoff |
| Anthropic API | API key | Optional. Direct Anthropic Console billing | API route |
| Claude subscription | Official Claude Code sign-in | Optional. Sign-in and subscription handled by Claude Code | CLI handoff |
| OpenRouter | API key + model ID | Optional. OpenRouter billing and model routing | API-compatible route |
| OpenCode / OpenCode Go | Official OpenCode CLI sign-in | Optional. Provider account and OpenCode plan | CLI handoff |
| Other OpenAI-compatible services | Base URL, model ID, optional key | Local or paid, depending on the service you select | Custom endpoint |

**Subscription credentials are not scraped or copied.** Where subscription access is exposed only through an official CLI, Open-Ro launches that unmodified CLI and lets it authenticate itself. Direct API-key use is available for users who prefer a conventional provider API. See [`docs/AUTH.md`](docs/AUTH.md).

### Optional capabilities

Everything outside the model connection is selected from a capability catalog. Each capability should explain what it unlocks, what it installs, whether an external account/key is needed, whether it is free or paid, the supported platforms, the permissions it receives, and how to test or remove it.

Examples: Roblox Studio MCP, Blender, Rojo, Git, image generation, 3D generation, local scripts, documentation MCPs, and community extensions. Users can skip all optional capabilities and still use the core chat and local project workspace.

## The target workflow

```mermaid
flowchart TD
    A[Describe the game or change] --> B[Open-Ro understands the project]
    B --> C[Plan tasks and identify required capabilities]
    C --> D[User reviews the plan and permissions]
    D --> E[Edit Luau / project files]
    E --> F[Review diff and checkpoint]
    F --> G[Run tests or launch Studio playtest]
    G --> H[Capture output, screenshots, and behavior]
    H --> I{Acceptance checks pass?}
    I -- No --> J[Diagnose and propose a focused repair]
    J --> E
    I -- Yes --> K[Summarize evidence and remaining risks]
    K --> L[User chooses save, sync, or publish]
```

The model proposes work; Open-Ro enforces the user's selected permissions and gates. A successful tool call, model response, or green status badge is not proof that a Roblox experience works. The final result must be tied to observable checks.

## Product principles

- **One required dependency:** one working model connection. No mandatory second account.
- **Bring your own access:** users connect directly to providers and third-party services. Open-Ro does not sell inference credits or proxy vendor subscriptions.
- **Local-first by default:** projects, history, settings, and credentials stay on the user's machine unless a user-selected provider or integration receives data as part of a task.
- **Capability-driven:** integrations are independently selectable, installable, testable, and removable.
- **Permission before action:** file writes, shell commands, Studio mutations, uploads, and publishing require explicit scoped permissions and meaningful review.
- **Evidence over claims:** display what was inspected, changed, tested, skipped, or left uncertain.
- **Portable:** the same core product model across Windows and Linux, with OS-specific adapters and installers.
- **Independent implementation:** public behavior and documented interfaces may inform compatibility goals; no proprietary source, private APIs, credentials, or Ropilot branding are reused.

## Platform commitment

| Platform | Product target | Important qualification |
|---|---|---|
| Windows 10/11 x64 | Native desktop app and NSIS installer | Primary baseline for Roblox Studio integration and validation |
| Omarchy / Arch Linux x64 | Native desktop app, AppImage, and pacman package | The Open-Ro client must run natively; optional tools are discovered locally |
| Other Linux distributions | AppImage / Debian package where practical | Validate dependencies and desktop secret service per distribution |
| Roblox Studio on Linux | Best-effort bridge through Wine/Vinegar | Roblox does not officially support Studio on Linux; never promise parity without real-machine tests |

Windows and Linux are equal first-class targets for the Open-Ro desktop client. Roblox Studio's own platform limitations are separate from Open-Ro's platform support. See [`docs/PLATFORM-SUPPORT.md`](docs/PLATFORM-SUPPORT.md).

## Current feature status

| Area | 0.1.0 state | Target |
|---|---|---|
| Desktop shell | Chat, provider forms, project picker, and an approval tray | A beginner can prompt a playable game without writing Luau |
| Provider connections | Catalog with Ollama and llama.cpp first, then API keys and official CLI handoffs | A tested local model with the network off, plus a visible data-flow line |
| Local project access | List and read files. Writes stay staged until approved. Checkpoint restores are staged for review too, and a restore is itself checkpointed | The same gate for Studio edits, with a line diff and restore |
| Agent tools | Approval-gated local shell, file, and MCP tool calls; once every approval in an agent turn is decided, the results return to that chat and the agent continues. A playtest report is marked passed only when the report lists assertions, state checks, and a PNG path in the project with each expected value equal to the actual one — Open-Ro renders that report but does not yet verify it against a real run | Plans, evidence, and recovery tied to a real playtest |
| MCP | One stdio server. Starting it and every agent tool call need your approval. Tested against the MCP reference server, not yet against Studio | A tested Studio connection, not a typed command alone |
| Roblox Studio | Three agent tools: read the place tree, read a script, and write a Script, LocalScript or ModuleScript. Each one sends Luau through the Studio MCP server's `execute_luau` (or `run_code`) tool and needs your approval first. A write shows a line diff against the last read, refuses client-side currency changes, stops if the script changed since the read, and is read back. Tested against a mock DataModel in the Luau CLI, **not yet against a live Studio session** | Edit the right instance, playtest, and store the evidence |
| Rojo | `rojo --version` detection only | Sync direction and overwrite scope shown before files move |
| Blender | `blender --version` detection only | Reviewed models and animations, then a separate import approval |
| Skills and asset library | Not implemented | Local skills the model actually receives, and optional downloads with a checksum and license |
| Published-game upkeep | Not implemented | Recommendations that quote an official metric, and a separate approval before publishing |
| Images / thumbnails | Not implemented | An optional provider and local review before any upload |
| Windows packaging | NSIS installer builds locally and in CI (`Open-Ro Studio Setup 0.1.0.exe`); install/uninstall and code signing not yet recorded | Install and uninstall recorded on a real machine |
| Omarchy packaging | AppImage, deb, and pacman packages build in CI; install/uninstall not yet recorded | Install and uninstall recorded on a real machine |

Open-Ro has not yet been run against a live Studio session, and does not yet playtest a game, drive Blender, download an asset pack, or watch a published experience. Those are goals, not current behavior.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for milestones and acceptance criteria, and [`docs/BUILD-STATUS.md`](docs/BUILD-STATUS.md) for the exact verification boundary.

## Get started

The project is currently distributed as source; native installers have not yet passed release acceptance.

**Requirements:** Node.js 22.12+, npm 10+, Git, and the platform-specific desktop build prerequisites.

```bash
git clone https://github.com/cayleb-james2008/open-ro.git
cd open-ro
npm install
npm run dev
```

Build and package:

```bash
npm run typecheck
npm run lint
npm test
npm run package:win       # Windows x64 NSIS
npm run package:linux     # AppImage + deb + pacman
```

For Omarchy-specific instructions and keyring requirements, see [`docs/INSTALL.md`](docs/INSTALL.md). For model setup and official sign-in flows, see [`docs/AUTH.md`](docs/AUTH.md).

## Repository guide

| Path | Purpose |
|---|---|
| [`PROJECT.md`](PROJECT.md) | Canonical mission, standing goals, principles, scope, and project decisions |
| [`docs/PRODUCT-SPEC.md`](docs/PRODUCT-SPEC.md) | User journeys, requirements, functional scope, and acceptance criteria |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Prioritized milestones, dependencies, and parity checklist |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Desktop architecture, agent runtime, provider adapters, and integration boundaries |
| [`docs/PLATFORM-SUPPORT.md`](docs/PLATFORM-SUPPORT.md) | Windows, Omarchy, and Roblox Studio support matrix |
| [`docs/EXTENSIONS.md`](docs/EXTENSIONS.md) | Optional capabilities and extension safety model |
| [`docs/AUTH.md`](docs/AUTH.md) | Provider connection and subscription-safe authentication |
| [`docs/INSTALL.md`](docs/INSTALL.md) | Source setup and packaging instructions |
| [`docs/BUILD-STATUS.md`](docs/BUILD-STATUS.md) | What was and was not built or verified |
| [`docs/RELEASE-GATES.md`](docs/RELEASE-GATES.md) | Evidence required before a release can be called ready |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Development, pull requests, and quality expectations |

## Contributing

Open-Ro is MIT-licensed and welcomes issue reports, implementation proposals, provider adapters, optional capability integrations, and platform testing. Start with [`CONTRIBUTING.md`](CONTRIBUTING.md) and the standing goals in [`PROJECT.md`](PROJECT.md). A proposed feature should solve a user workflow, preserve the one-required-setup rule, and include acceptance criteria.

## Legal and attribution

Open-Ro is an independent community project. It is not affiliated with Roblox Corporation, Ropilot, OpenAI, Anthropic, OpenRouter, or OpenCode. Product and company names identify compatibility targets only. Each optional tool, provider, model, and generated asset retains its own license and terms; see [`docs/EXTENSIONS.md`](docs/EXTENSIONS.md).

**License:** MIT. See [`LICENSE`](LICENSE).

# Open-Ro — standing project charter

> **Mission:** Build a free, open-source, local-first AI development studio that helps people create, edit, test, and ship Roblox experiences using the model and tools they choose.

**Product:** Open-Ro Studio  
**Repository:** `cayleb-james2008/open-ro`  
**License:** MIT  
**Current state:** 0.1.0 early functional foundation  
**Primary platforms:** Windows 10/11 x64 and native Linux (Omarchy/Arch first-class)

This document is the standing source of truth for why Open-Ro exists, what it must become, and the rules every implementation decision must preserve. When a feature request conflicts with this charter, revise the charter deliberately rather than silently changing the product direction.

## 1. The problem Open-Ro solves

Roblox development is spread across a coding environment, Studio, project files, model providers, asset tools, 3D software, and testing workflows. AI-assisted coding products can help write code, but a useful Roblox development assistant must also understand the actual game project, operate the right tools, observe playtest results, and repair problems without hiding what it changed.

Open-Ro aims to connect those steps into one desktop workflow while leaving the user in control of the model, accounts, tools, files, and publishing decisions.

## 2. Product promise

A new user should be able to:

1. Install Open-Ro on Windows or Linux.
2. Connect one model using a provider API key, an official provider CLI/subscription sign-in, OpenRouter, OpenCode, or a local model.
3. Open a local Roblox project or begin with a new project.
4. Ask for a feature in ordinary language.
5. See a plan and the optional capabilities required for that task.
6. Choose which tools to install or connect, with guided official setup instructions.
7. Review and approve changes.
8. Run tests or a Studio playtest, inspect real evidence, and iterate.
9. Keep the project local, sync it with tools such as Rojo/Git when selected, and decide personally whether to publish.

**Only the model connection is required.** Every other tool, integration, and account is optional until the user chooses a workflow that needs it.

## 3. Standing goals (do not drift)

### G1 — A complete AI development workspace

Provide a polished desktop workspace with project selection, model selection, conversation history, persistent project context, plans, task progress, tool activity, diffs, approvals, checkpoints, and recovery.

### G2 — Provider freedom

Support multiple model routes and keep provider-specific authentication correct. Users must be able to bring API keys, use supported official subscription CLIs, connect aggregators such as OpenRouter, use OpenCode, or run local models such as Ollama. Open-Ro must not lock users into a proprietary model, inference proxy, credit system, or hosted account.

### G3 — Optional capability routing

Detect the user's intent and explain which optional tools can complete the job. Guide the user to official installation/signup pages, show free/paid requirements, request only necessary permissions, test the connection, and let the user disable or uninstall it later.

### G4 — Roblox Studio integration

Make Roblox Studio a first-class optional integration. The goal is to inspect the live DataModel, traverse instances, search and edit scripts, run Luau/Studio operations, start and stop playtests, simulate inputs, read output, capture viewport screenshots, and pass evidence back to the agent.

### G5 — A real test-and-repair loop

A requested feature is not complete merely because code was generated. The workflow must be able to define acceptance checks, run an appropriate test/playtest, capture observable results, diagnose failures, propose a focused fix, and repeat. Users can stop, inspect, or reject each mutation.

### G6 — A creative asset pipeline

Offer optional image, icon, thumbnail, audio, and 3D asset providers. Use Blender as the local modeling/animation hub. A user should be able to review an asset, approve it, and move it into a Roblox project without relying on a proprietary Open-Ro asset backend.

### G7 — Native Windows and Linux

The desktop client must be a native-feeling app on Windows and Linux. Omarchy/Arch must receive a documented pacman/AppImage path. Platform-specific helpers are acceptable; feature gaps and support limits must be explicit. Roblox Studio's unsupported Linux/Wine path must not be confused with Open-Ro's Linux support.

### G8 — Open, portable project ownership

Use open file formats and ordinary project layouts. Integrate with Git and Rojo where useful. Users can take their files and leave; no cloud account or Open-Ro server is required to open a project.

### G9 — Safety and auditability

Never hide file changes, shell commands, Studio mutations, uploads, publishing actions, or network data flows. Ask for scope-appropriate approval; keep diffs/checkpoints; separate proposed actions from completed actions; and report evidence and uncertainty honestly.

### G10 — Open-source sustainability

Keep the core MIT-licensed, document third-party licenses and terms, make optional extensions independently inspectable, and welcome community contributions. Do not vendor unlicensed code or copy proprietary product assets.

## 4. Non-negotiable product rules

- **One required setup:** a working model connection. All other integrations are optional.
- **No mandatory hosted service:** chat, local project context, and local tooling should work without an Open-Ro account or Open-Ro-operated server.
- **No secret scraping:** do not scrape browser cookies, extract subscription tokens, or reverse-engineer private authentication endpoints. Use documented API keys and official CLIs.
- **No false parity claims:** distinguish implemented, partially implemented, planned, experimentally supported, and verified on hardware.
- **No silent destructive actions:** writes, commands, deleting files, Studio edits, asset uploads, and publishing have distinct permission gates.
- **No arbitrary extension auto-install:** show publisher, package, version, install command, permissions, network needs, and uninstall route before approval.
- **No mandatory Roblox-only experience:** Open-Ro's core agent/workspace should remain useful for ordinary code/project tasks too.
- **No requirement to use Blender, Rojo, Git, or any image provider:** each is selected only when useful.

## 5. Primary users and jobs to be done

| User | Job to be done | Open-Ro outcome |
|---|---|---|
| Roblox beginner | Turn a game idea into a working starting project | Guided setup, scaffolded tasks, safe explanations and playtest feedback |
| Roblox scripter | Add or repair a feature in an existing experience | Project-aware Luau changes, diff review, Studio validation |
| Builder / creator | Create assets and iterate visually | Optional image/3D tools, Blender review, import workflow |
| Technical developer | Keep scripts and source under version control | Rojo, Git, project files, checkpoints, reproducible tests |
| Model-flexible power user | Choose models and route work across providers | Multiple provider connections, model/task routing, local inference |

## 6. Product pillars and scope

| Pillar | Included scope | Completion condition |
|---|---|---|
| Workspace | Projects, chats, history, model controls, context and settings | A user can reopen work and continue with the right project context |
| Agent runtime | Plan, tools, subagents, progress, structured results | Tool actions are scoped, observable, resumable and testable |
| Provider system | API keys, official CLIs, OpenRouter, OpenCode, Ollama, custom endpoints | A user can connect/test/switch a provider and understand its billing boundary |
| Capability catalog | Detect, install, connect, authorize, test and remove add-ons | Every capability has accurate requirements and a tested setup path |
| Roblox bridge | DataModel, Luau, playtest, screenshots, input and output | Complete Studio task can be validated through evidence |
| Creative pipeline | Images, thumbnails, icons, Blender, 3D, import/export | An asset can be generated/created, reviewed, revised and added to project |
| Project tools | Files, Git, Rojo, diffs, checkpoints | Users can inspect and roll back agent changes |
| Packaging | Windows NSIS, Linux AppImage, Debian and Arch | Install/update/uninstall work on target machines |

## 7. What Open-Ro is not

- It is not a Ropilot fork, clone of proprietary source code, or a service that depends on Ropilot accounts.
- It is not a hosted model provider or a bundled paid inference service.
- It is not an automatic publisher that uploads changes to Roblox without human approval.
- It is not an installer that silently adds dependencies or grants broad OS permissions.
- It is not a promise that Roblox Studio works natively on Linux.

## 8. Priority order

1. Make the foundation reliable: provider selection, secure credential handling, projects, diff/approval UX, command containment, and Windows/Linux packaging.
2. Complete the optional integration system: multiple MCP servers, lifecycle, tool permissions, health checks, and guided setup.
3. Complete the Roblox Studio closed loop: inspect → edit → playtest → observe → repair.
4. Complete Rojo/Git project workflows and checkpoint/recovery.
5. Complete Blender and image/thumbnail/3D asset creation and import.
6. Polish onboarding, accessibility, docs, community extensions, and release distribution.

These are product priorities, not claims that the work is already complete. The milestone detail and gates live in [`docs/ROADMAP.md`](docs/ROADMAP.md).

## 9. Definition of done

A capability is not done until it has:

- A specific user journey and observable success state.
- A clear availability state (implemented / experimental / planned).
- Official setup and dependency instructions.
- Permission and data-flow documentation.
- Automated tests for its deterministic behavior.
- Platform testing on each OS it claims to support.
- A failure and recovery path.
- A user-facing explanation of cost, credentials, and external terms.

A release is not ready until all required checks in [`docs/RELEASE-GATES.md`](docs/RELEASE-GATES.md) pass on the exact source revision being released.

## 10. Decision record

| Decision | Why | Revisit when |
|---|---|---|
| Electron + React + TypeScript | Shared Windows/Linux UI and mature desktop packaging | Native shell limits or resource usage justify a measured alternative |
| Local-first, no Open-Ro backend required | No mandatory account or vendor lock-in | A truly optional collaboration/sync feature is proposed with clear opt-in data flow |
| BYO model and official CLI subscription routes | Provider freedom without credential scraping | A provider ships a documented third-party subscription API |
| MCP for extensible tool transport | Reuse documented tool integrations | A specific capability requires a better documented protocol |
| Roblox Studio MCP as first integration | Documented Studio-side tool interface | Roblox introduces a supported better interface |
| Windows + native Linux as first-class app targets | Explicit cross-platform requirement | New OS target is requested and resourced |

## 11. How to use this charter

- Use this document to evaluate scope and resolve product ambiguity.
- Update it when a standing goal or non-negotiable rule changes.
- Update `docs/ROADMAP.md` when priorities or milestone evidence changes.
- Update `docs/BUILD-STATUS.md` when implementation or verification state changes.
- Do not mark a requirement complete merely because it has a UI element, config entry, or code path; attach test evidence.

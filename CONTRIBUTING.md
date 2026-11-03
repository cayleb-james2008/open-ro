# Contributing to Open-Ro

Thank you for considering a contribution. Start with [`PROJECT.md`](PROJECT.md), which defines the standing goals and non-negotiable rules.

## Before proposing work

1. Identify the user workflow the change improves.
2. Check `docs/PRODUCT-SPEC.md` and `docs/ROADMAP.md` for existing requirements or scope.
3. Preserve the one-required-model-connection rule and optional integration model.
4. Confirm provider authentication, package source, license and data-flow implications.
5. Open an issue for broad changes before submitting a large implementation.

## Development setup

- Node.js 22.12+
- npm 10+
- Git
- Windows or Linux desktop build prerequisites for native packaging

```bash
npm install
npm run dev
```

## Quality checks

Run before opening a pull request:

```bash
npm run typecheck
npm run lint
npm test
npm run build
```

For packaging changes, run the package command on the target operating system and attach the exact artifact/build receipt. A Linux build does not validate Windows packaging, and an AppImage build does not validate Omarchy pacman installation.

## Pull request expectations

- Explain the user problem and expected behavior.
- Describe any permissions, subprocesses, network traffic, credentials, or filesystem effects added.
- Add/update tests and docs.
- State which OSes were actually tested.
- Include screenshots for meaningful UI changes where practical.
- Preserve all existing user data unless the PR explicitly migrates it.
- Do not claim complete support without the evidence required in `docs/RELEASE-GATES.md`.

## Security issues

Do not post credentials, private user projects, or exploitable details in public issues. Follow [`SECURITY.md`](SECURITY.md).

## Project scope

We welcome provider adapters, Roblox Studio workflows, Rojo, Blender, local-first tooling, accessibility, documentation, and platform compatibility work. Integrations must remain optional, use official/documented authentication, and disclose external service costs and data flow.

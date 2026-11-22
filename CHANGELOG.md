# Changelog

Each release is tagged `vMAJOR.MINOR.PATCH` and published on GitHub Releases with
binaries and a `SHA256SUMS` file. The running app shows its version and can update
itself from the newest release (see README, "Versions and updates").

## 0.3.0 - 2026-11-21

- **Game Builder Studio Workbench (House Frontend):** Replaced the legacy chat/quest layout with a three-region Studio Workbench built on the House Frontend component stack (Motion Primitives, COSS UI, Cult UI, Kibo UI, Tremor, Vaul, Sonner, Motion) with self-hosted Geist and Geist Mono `.woff2` fonts in Light (default) and Dark themes, embedded into the single `openro-rs` binary.
  - **Left Studio Session Rail:** `256px` desktop rail (Vaul drawer below `768px`) with live Studio MCP status pill and `Check again` action, `/` keyboard-accessible chat search filter, gliding `AnimatedBackground` chat switcher, `New chat`, Light/Dark `SegmentedControl`, and release version link.
  - **Center Studio Stage & Build Trace:** Equal-height Game Blueprint Starter Cards (`Lava Obby`, `Coin Tycoon`, `Round Arena`, plus contextual feature blueprints) with category badges, part/script count chips, capability pills, `"Already in your game"` tracking (`built_titles` on `POST /api/ideas`), and one-click Studio builds; active Studio Build Trace with Tremor `ProgressCircle`, single-line `SlidingNumber` change counts, Playtest Verdict banner (`Passed` / `Failed` / `Not proven yet`), and expandable Luau console logs.
  - **Hairline Composer Dock & Update Strip:** Responsive `InputGroup` composer (`Enter` / `Shift+Enter`), `Undo` `MorphingDialog` confirmation with keyboard focus trap and return, and hardened self-update banner (loopback-only `Update now`, reachable `Show update` / `Check updates` at `390px`, focus preservation across apply/restart/dismiss, and plain-English recovery copy).
  - **Right "In Your Game" Studio Explorer Shelf:** `300px` desktop DataModel Explorer (Vaul bottom sheet below `1100px`) grouping `Workspace` parts and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, expandable hierarchy & Luau previews, and pinned `Copy game summary`.
- **Verification & E2E Suite:** Added Playwright E2E gate (`78` checks across `1440px`, `768px`, `390px`, and `320px` in Light and Dark), worst-case API replay (`replay.mjs`), `JOB.md`, `ROLE-COVERAGE.md`, and `ALIVE.md`.

## 0.2.0 - 2026-11-19

- New front end: chat shell with the build timeline, ideas bar, one-tap builds, and an Undo control that is enabled only when there is something to undo.
- Honest results: an ask that changed nothing because a step failed is refused in plain words; a playtest in an ask with no changes is reported as not run; a shop build refuses a missing prerequisite by its display name.
- Proof: playtest verdicts are kept per chat in `proof.json`, so a failed run still blocks "done" after a restart.
- Safety: same-site guard (Host, Origin, `Sec-Fetch-Site`, JSON-only POST), `OPENRO_ALLOWED_HOSTS`, 10 s connection timeouts, a 64-connection cap, and 413 JSON for bodies over 1 MiB.
- Recovery: corrupt saved files are backed up and named on the page and in `/api/status`.
- `--version`, `GET /api/version`, and self-update from GitHub releases (`OPENRO_UPDATE_MODE`).

## 0.1.0 - 2026-11-17

- First Rust + Rune build: one binary serving the web shell, the agent loop, the Studio bridge, Rune code generation for ten skills, the creativity engine, and revert with change sets.

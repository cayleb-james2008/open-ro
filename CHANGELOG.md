# Changelog

Each release is tagged `vMAJOR.MINOR.PATCH` and published on GitHub Releases with
binaries and a `SHA256SUMS` file. The running app shows its version and can update
itself from the newest release (see README, "Versions and updates").

## 0.2.0 - 2026-11-19

- New front end: chat shell with the build timeline, ideas bar, one-tap builds, and an Undo control that is enabled only when there is something to undo.
- Honest results: an ask that changed nothing because a step failed is refused in plain words; a playtest in an ask with no changes is reported as not run; a shop build refuses a missing prerequisite by its display name.
- Proof: playtest verdicts are kept per chat in `proof.json`, so a failed run still blocks "done" after a restart.
- Safety: same-site guard (Host, Origin, `Sec-Fetch-Site`, JSON-only POST), `OPENRO_ALLOWED_HOSTS`, 10 s connection timeouts, a 64-connection cap, and 413 JSON for bodies over 1 MiB.
- Recovery: corrupt saved files are backed up and named on the page and in `/api/status`.
- `--version`, `GET /api/version`, and self-update from GitHub releases (`OPENRO_UPDATE_MODE`).

## 0.1.0 - 2026-11-17

- First Rust + Rune build: one binary serving the web shell, the agent loop, the Studio bridge, Rune code generation for ten skills, the creativity engine, and revert with change sets.

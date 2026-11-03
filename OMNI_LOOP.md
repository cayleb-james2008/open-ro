# OMNI_LOOP.md — open-ro operating state

**System:** OMNI-LOOP persistent project-evolution loop (skill: `omni-loop`).
**Project:** Open-Ro — local-first AI development studio for Roblox (Electron 45 + React 19 + TypeScript, per recovered docs).
**Repo:** `cayleb-james2008/open-ro`, branch `freebuff/8651cb99a120ef99f53ffd58` (worktree of `main` @ `137fc16`).

## Current session state (2026-11-03)

1. Repo is in a **bootstrap state**: the real Open-Ro 0.1.0 source tree is staged as
   base64 fragments in `.bootstrap/parts/*.b64`; `.github/workflows/source-import.yml`
   is designed to reconstruct, validate, and import it on push to `main`.
2. **BLOCKER found in iteration 001:** the reconstructed `open-ro.tar.zst` is a
   **truncated/incomplete zstd frame**. Diagnosis evidence in
   `.omni-loop/iteration-log.md`. The tail of the archive (all of `src/` scripts,
   `electron/`, `package.json`, ~8+ of the 37 documented source files) was never
   successfully staged.
3. A best-effort recovery recovered **29 of the archive's files** (docs, configs,
   tests, workflows) into `.omni-loop/recovered-2026-11-03/`. `src/styles.css` is
   partial. These are recovery evidence only — they do NOT restore the project.
4. No alternate copy of the full source was found: `git ls-remote` shows only
   `main`; GitHub account inventory contains no second Open-Ro/source-snapshot repo;
   `talos-workspace` (recovery-hands hub) predates open-ro and holds no pin.

## You cannot proceed to product iterations until

The full `open-ro.tar.zst` is re-staged (all parts, fresh sha256) — or the project
source is provided by another route the owner controls (local machine / zip via
chat / another repo). After it lands, run the import locally in this workspace
first (`zstd --test`, extract, verify `PROJECT.md`, `README.md`, `docs/ROADMAP.md`,
`electron/main.ts`, `LICENSE`), then commit.

## Operating rules

- One atomic improvement per iteration; verify before accepting; record outcomes in
  `.omni-loop/iteration-log.md`.
- Baseline and backlog live in `.omni-loop/baseline.md` and
  `.omni-loop/opportunity-backlog.md`.
- Do not push, deploy, or mutate remotes without explicit user authorization.
- Once source exists: run `npm ci` (lockfile from import), `npm run typecheck`,
  `npm test`, `npm run build` to establish the verified baseline; record results.
- Product gates: `docs/RELEASE-GATES.md` (recovered copy) governs "done".

## Repo hygiene note

The GitHub Actions import timeline shows corrupted-stage-fix cycles (parts 01/02
were corrected in follow-up commits `f5027e4`, `55932a3`). When the archive is
re-staged, validate the **whole** reconstruction (frame size, checksum, tar
round-trip) *before* pushing, so the import workflow succeeds on first try.

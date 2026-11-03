# Baseline — open-ro (2026-11-03)

## Repository state

- Local worktree branch: `freebuff/8651cb99a120ef99f53ffd58` (clean, `main` @ `137fc16`).
- Working tree contains only: `README.md`, `LICENSE`, `.gitignore`, `.bootstrap/parts/`
  (16 base64 fragments), `.github/workflows/source-import.yml`. No package.json,
  no source, no tests.
- Remote has exactly one branch (`main`). No tags.

## Reconstructed-archive evidence

| Fact | Value |
|---|---|
| Concatenated base64 (`cat .bootstrap/parts/*.b64`) | 74,444 chars, decodes cleanly |
| Reconstructed archive size | 55,832 bytes |
| Reconstructed archive sha256 | `98e29e8fc1d1e2ba9dd0a4f2caaeb8725ddbab95355da4a44b690e9c74763093` |
| zstd decode result | 126,976 bytes OK, then `Data corruption detected` at input byte 55,827 (frame end) |
| Interpretation | frame is truncated/incomplete — the tail chunks were never staged; all present bytes decode |
| Tar recoverable | entries starting at `open-ro/` up to `open-ro/src/styles.css` (cut inside that member, ~24.8 KB of 28,133 recovered) |

Per-part byte ranges map the failure to the final staged fragment `09.0.b64`
(bytes 72,000–74,443) region; nothing after it exists.

## Recovered inventory (29 complete files → `.omni-loop/recovered-2026-11-03/`)

PROJECT.md (charter), README.md, docs/{ROADMAP,ARCHITECTURE,PRODUCT-SPEC,BUILD-STATUS,
RELEASE-GATES,INSTALL,AUTH,EXTENSIONS,PLATFORM-SUPPORT,BRAG}.md, CHANGELOG.md,
CODE_OF_CONDUCT.md, SECURITY.md, CONTRIBUTING.md, LICENSE, index.html,
vite.config.ts, tsconfig.{renderer,electron}.json, tests/{README,providerCatalog.test.ts},
.github (CI, templates), docs/assets/open-ro-wordmark.svg, src/styles.css (partial).

Missing (unrecoverable from this repo): entire `electron/` tree, remaining `src/`
renderers, `package.json`, `package-lock.json`/lockfile, remaining configs —
roughly the other 8 of the 37 documented source files.

## Baseline checks run

- `git status` / `git log --oneline` (13 commits, archive-stage history) — OK.
- `git ls-remote origin` — only `main`; no other branch/tag anywhere.
- base64 reconstruction + zstd decode — **FAILED** (truncated frame). This failure
  reproduces exactly what the `source-import.yml` workflow would hit; the latest
  commit added diagnostics around it.
- Full GitHub account inventory scan — no alternate source copy.

No project build/test checks can run yet: no package.json or dependencies exist.

## Baseline scorecard (0–5; unknown ≠ complete)

| Dimension | Score | Evidence / confidence |
|---|---|---|
| Product completeness | unknown | Source missing; docs define 0.1.0 as early foundation (med-high confidence in doc), runtime unverified |
| Functionality | unknown | Only docs and one recovered test file; BUILD-STATUS declares runtime unverified |
| UX | unknown | no UI source |
| Visual design | unknown | styles.css partial; 28 KB stylesheet suggests real effort |
| Interaction craft | unknown | no source |
| Accessibility | unknown | Roadmap lists it as backlog item |
| Performance | unknown | — |
| Reliability | 1 | Repo cannot reproduce its own source; import pipeline has failed repeatedly (med) |
| Security/privacy | unknown | Docs set strong principles; `source-import.yml` uses persist-credentials: true (minor flag, med confidence) |
| Architecture | 4 (documentation) | ARCHITECTURE.md recovered and coherent (med-high) |
| Maintainability | 2 | Excellent docs + CI config, but zero runnable baseline (med) |
| Operations | 1 | Import gate broken; CI exists but is idle until source lands (high) |

Release blockers currently: truncated source archive (blocks everything);
unverifiable build/test baseline.

## UPDATE 2026-11-03 (after iteration 002, user-authorized rebuild)

The truncated archive was bypassed: the owner authorized a spec-based rebuild of
the missing 0.1.0 modules; all 29 recovered originals were restored to project
paths and the rebuilt tree passes `npm run typecheck`, `npm test` (3/3 on the
recovered, unchanged test file) and `npm run build` (dist-electron + dist).
Rebuilt code is a faithful reconstruction, NOT provenance-verified original
source; runtime, packaging, live inference, secureStorage and Studio/Blender
workflows remain unverified exactly as documented in the recovered BUILD-STATUS.
Scorecard deltas: Reliability 1→3, Maintainability 2→4, Operations 1→2.
Next gate: real-machine runtime + packaging receipts (backlog B2).


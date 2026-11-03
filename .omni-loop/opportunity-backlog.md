# Opportunity backlog — open-ro (2026-11-03)

Ranked by impact × confidence ÷ risk. Re-rank after every accepted iteration.

| # | Opportunity | Impact | Confidence | Risk | Status |
|---|---|---|---|---|---|
| B1 | ~~Restore the full source tree~~ — **superseded 2026-11-03** by iteration 002: user authorized spec-based rebuild; import pipeline remains as a historical artifact (source-import.yml should be removed before any push to main to avoid a conflicting re-import) | — | — | — | ✅ resolved-by-decision |
| B2 | Runtime + packaging proof: launch Electron on a real desktop, run `npm run package:win` / `package:linux`, attach receipts | Critical | high | med | ⬜ OPEN — next wave. Headless Chrome covered the renderer only. |
| Authorized | User authorized "commit everything to main from your Omni loops" on 2026-11-03; commit + push executed as the final step of iteration 002. The superseded one-time source importer (`.github/workflows/source-import.yml`) was removed in the same commit — its `zstd --test` gate can never pass against the truncated archive and would fail CI on every push. | — | — | — | ✅ done |
| B3 | Harden import pipeline: add pre-push validation script + sha256 pin of the archive so a partial stage can never re-occur (validate whole frame, tar listing count vs manifest, then push) | high | high | low | blocked by B1 |
| B4 | Fix `source-import.yml` hygiene: `persist-credentials: true` is unnecessary for the commit/push step's blast radius — use the default scoped token and un-set after push; add `test ! -e .bootstrap` post-condition | med | high | low | blocked by B1 |
| B5 | Durable build: once installable, attempt dependency-resolved `npm run build` and record any Windows/Linux packaging gaps per BUILD-STATUS "not verified" list | high | med | low | blocked by B1 |
| B6 | Provider-catalogue and safety tests | med | high | low | ✅ iteration 003 added path, approval, provider-state, MCP framing, command env, diff, and session tests. 13 passing. More IPC-level tests still useful. |
| B7 | Onboarding journey: fresh install → connect model → open project → approve a change → restart and recover | high | med | med | ⬜ OPEN — chat history now persists; project root does not |
| B8 | Accessibility of the renderer | med | high | low | partial — nav, dialogs, 12px floor, and contrast pairs were checked. Full keyboard audit and a screen reader pass remain. |
| B9 | Feed an approved tool result back into the model as a tool message | high | high | med | ⬜ OPEN |
| B10 | Classify Studio MCP tools before calling them | high | med | med | ⬜ OPEN |
| B11 | Align the docs wordmark (purple/teal SVG) with the lime in-app mark | low | high | low | ⬜ OPEN |

Notes: B1 is the only actionable-adjacent item and it is owner-gated. Everything else derives from the recovered ROADMAP/ARCHITECTURE priorities (M0 first: reliable foundation before integrations).

# Red-team checklist — iteration 4

64 v4 checks. Evidence is from this workspace on 2026-11-03. `N/A` means the check does not apply to this repository.

| # | Result | Evidence |
|---|---|---|
| 01 | PASS | No new feature flags or unused helpers in the CSS/verifier diff. |
| 02 | PASS | Transitions edit the existing stylesheet. No second motion helper. |
| 03 | PASS | Verifier uses Node `fetch` and `WebSocket` plus system Chrome. |
| 04 | PASS | `transition` shorthand was the shared rule, replaced at each rule rather than one caller. |
| 05 | PASS | No new `any` in the CSS wave. Typecheck still passes. |
| 06 | PASS | Product files for iteration 4 are `src/styles.css`, `scripts/verify-ui-craft.mjs`, and `package.json`. |
| 07 | PASS | CSS edits are the transition, reduced-motion, and narrow-shell rules. |
| 08 | PASS | No package was added. |
| 09 | PASS | Direction is in `.omni-loop/genesis-dag.json`. Banned adjectives were not used. |
| 10 | FAIL | Historical CSS still uses odd paddings such as 23px, 13px, and 9px. Not part of this wave. |
| 11 | FAIL | `.connection-card` is a two-stop gradient and `.status-dot.on` glows. |
| 12 | PASS | UI icons are inline SVG strokes. No emoji icons in the shell. |
| 13 | FAIL | Sidebar group labels are still uppercase. No exclamation marks in the UI copy that was reviewed. |
| 14 | FAIL | Brand and some buttons use weight 700 or 600. Uppercase labels remain. `tabular-nums` is on counters. |
| 15 | PASS | Sidebar is 252px. Below 700px the shell stacks and did not overflow at 375. |
| 16 | PASS | No decorative 3D art was added. Screenshots show the real shell. |
| 17 | PASS | Remaining motion is hover color and the waiting dots. |
| 18 | PASS | Screen changes are not animated. |
| 19 | PASS | Named transitions are 160ms, or 120ms under reduced motion. |
| 20 | PASS | Easing is `cubic-bezier(0.22, 1, 0.36, 1)`. No `ease-in`. |
| 21 | N/A | No popover scale animation was added. |
| 22 | FAIL | Waiting dots still use a non-interruptible `@keyframes` pulse. |
| 23 | PASS | `verify-ui-craft` reported `transitionAll=0` at all three viewports. |
| 24 | PARTIAL | Reduced motion keeps color transitions. Hover is not gated on `(hover: hover)`. |
| 25 | PARTIAL | Earlier contrast pairs for the muted ramp were calculated above 4.5:1. A full light theme does not exist. |
| 26 | FAIL | Disabled, loading, and error states are not implemented on every control. |
| 27 | PARTIAL | Buttons and dialogs are keyboard reachable. A full Tab walk was not recorded. |
| 28 | PASS | `src/Dialog.tsx` traps Tab, closes on Escape, and restores focus. |
| 29 | PASS | The shell has `nav`, `main`, a skip link, and `aria-current`. |
| 30 | PASS | At 375x812 the craft gate found no overflow and no text under 11px. Touch targets are still under 44px. |
| 31 | PASS | 768x1024 and 1440x900 rendered without horizontal overflow. |
| 32 | FAIL | CLS was not measured. |
| 33 | PASS | Empty commands and empty provider fields throw in unit tests. |
| 34 | N/A | No numeric formula surface. |
| 35 | PASS | External links are https-only. Project paths reject `..` and symlink escape. Shell still runs an approved string, which is the intended gate. |
| 36 | PASS | `ApprovalQueue.take` deletes before the write, so a second resolve throws. |
| 37 | PARTIAL | Provider and command calls have timeouts. There is no retry with jitter. |
| 38 | PASS | Tools use the main-process project root, not a renderer path. There is no multi-user role model. |
| 39 | PASS | The command-env test asserts a sentinel key is absent from stdout. No secret was committed. |
| 40 | FAIL | The renderer has no error boundary. A failed refresh sets a banner and keeps the draft. |
| 41 | N/A | No ledger. |
| 42 | N/A | No spreadsheet. |
| 43 | N/A | No formula cells. |
| 44 | N/A | No financial metric. |
| 45 | PARTIAL | The line diff bails out above 250,000 cells. No large-input benchmark was run. |
| 46 | FAIL | No 1,000-vector reference oracle. |
| 47 | N/A | No currency table. |
| 48 | N/A | No stochastic simulation in the product. The craft gate is deterministic. |
| 49 | PARTIAL | Verification claims in BUILD-STATUS map to commands from this session. Not every historical doc URL was re-read. |
| 50 | PASS | README badge and repository links point at this repository's workflow or tree, not a bare github.com root. |
| 51 | PARTIAL | The Studio MCP URL was opened from search results earlier in the session. The full docs page was not re-read for iteration 4. |
| 52 | N/A | No conflicting sources in this wave. |
| 53 | PASS | Dates in the loop records are 2026-11-03. Versions match `package.json` and the Node version that ran the commands. |
| 54 | PASS | No new paper, DOI, or package name was introduced by iteration 4. |
| 55 | N/A | No slide deck. |
| 56 | PASS | The composite in `state.json` matches the dimension table in that file. |
| 57 | PASS | `providerCatalog.test.ts` and `safety.test.ts` hashes match the iteration 3 commit. |
| 58 | PASS | `npm run lint` rejects suppressions and skipped tests. |
| 59 | PASS | The suppression scan reported no placeholders. |
| 60 | PASS | `last-verdict.json` records `clean_room_cot_stripped: true` and cites stdout, not a self-score narrative. |
| 61 | PASS | `.omni-loop/screenshots/route_desktop.png`, `route_tablet.png`, and `route_mobile.png` were inspected. |
| 62 | PARTIAL | Desktop composition is balanced. The 375px sidebar clips lower rows until that region scrolls. |
| 63 | PASS | The viewport failure mode is now `scripts/verify-ui-craft.mjs`. |
| 64 | PASS | `omni-loop/iter-3-pre` points at the pre-approval commit. `iter-4-pre` points at `ecf91db`. `state.json` is the v4 handoff. |

Applicable failures that remain open are the next contract (uppercase labels and 44px targets) plus Electron runtime proof, which this environment cannot produce.

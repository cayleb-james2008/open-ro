# Completion — OpenRO to its FULL canonical objective (18 Nov 2026)

**Objective (the spec, Cayleb-approved, un-drifted):** you describe the
game, it builds it — a PROACTIVE creative agent in a plain chat window:
it proposes ideas, features and improvements UNPROMPTED (its staple is
creativity, not gates), full access by default, and it can ONE-SHOT a
complete game.

Base at completion start: v1 + scale-up — real MCP client, 5 Rune
generation skills, checkpoint/repair/revert loop proven against the
mock Studio, 46/46 tests, sealed exam 12/12 (`.loop/REPORT.md`).

## Gap list (against the objective)

- **G1 — No proactivity exists.** The agent is purely reactive: it
  never proposes an idea, a feature, or an improvement on its own
  initiative. Nothing in the loop, the server, or the UI pitches
  anything. The objective's headline trait ("proposes ideas, features
  and improvements UNPROMPTED") is entirely absent.
  → Build a creativity engine (`src/ideas.rs`): a deterministic,
  registry-driven idea catalogue. On an empty chat the agent pitches
  complete-game ideas in the thread, each with a one-tap **Build this**
  that runs a built-in plan through the same agent loop machinery
  (checkpointed, playtested, revertable — no model call needed, stated
  plainly in the thread). After a build lands, the agent pitches
  follow-up features based on what the game now has. Pitches are part
  of the persisted chat record, never fabricated progress: a pitch is
  an offer, a build is proven by its playtest like any ask.
- **G2 — The generation kit is too thin to one-shot a COMPLETE game.**
  5 skills cover rounds/leaderboard/day-night/kill-brick/spawn-link.
  A complete small game also needs: persistent currency, something to
  spend it on, a player-facing GUI, and progression. Missing:
  currency-with-persistence, a shop, menus/GUI, checkpoints.
  → Add 5 Rune skills (10 total): `currency` (leaderstats +
  DataStore-shaped persistence with BindToClose saving), `shop`
  (RemoteEvent purchase flow), `game_gui` (StarterGui LocalScript:
  title, live currency label, shop button that fires the shop's
  RemoteEvent), `checkpoints` (stage progression with respawn),
  `soundtrack` (SoundService music). Same validation standard as the
  existing five (types, ranges, control-char refusal, Luau escaping).
- **G3 — No proof a WHOLE game can be one-shot.** The exam's one-shot
  check builds one script. Nothing proves: many skills + instances
  composed into one complete game, generated, playtested, repaired
  (a real cross-skill dependency failing first), and passing in ONE
  ask — nor that a built-in idea plan does the same with no model.
  → Extend the mock fixture with deterministic failure rules for the
  new systems (DataStore without BindToClose fails; a GUI waiting on
  a shop RemoteEvent that was never created fails; a checkpoints
  script referencing a folder nothing created fails), add integration
  tests for the full one-shot (scripted model, GUI-first so the
  playtest genuinely fails and is repaired by adding the shop), and
  extend the sealed exam: O13 one-shot complete game with repair,
  O14 built-in plan builds a complete game over HTTP.
  Also: MAX_STEPS 16 → 40 — a complete-game ask legitimately needs
  more steps than a one-script ask; the honest step-cap failure is
  preserved (the garbage-model test feeds 40 replies instead of 16).
- **G4 — Front ends graded "very underwhelming" (Cayleb, 18 Nov).**
  After functionality: a front-end elevation pass over the whole UI
  using the Hallmark-Rune skill (~/workspace/custom-skills/hallmark-rune/),
  leaning into OpenRO's vernacular — the playful creative studio —
  within the skill's gates. Checker grades BOTH directions: the new
  look at 8 widths in both themes, AND every flow from the
  functionality phase still passing. Public surfaces update only
  after the elevation. The site walkthrough video + captures of the
  current UI go stale at that point — flagged, NOT re-recorded
  (re-shoot is a separate later pass).

## Fences (unchanged, binding)

- Real Roblox Studio is desktop-only, unreachable from this VM: the
  mock bridge stays the proof surface; public copy says exactly that.
  Never fake a Studio session.
- Model route: DeepSeek V4.1 Flash via OpenCode Go behind one config
  point; the vault key is NOT exportable — never attempt extraction;
  live-model behaviour stays stub-proven and copy says so.
- Light + dark, toggle in Settings, dark default; icons are symbols,
  never letters.
- Luau comes only from Rune skills — the new skills follow the same
  law; built-in idea plans compose skill/tool steps, never raw Luau.

## Done-when

1. `bash .loop/verify completion` green (build + full suite).
2. Fresh-context checker PASS on the functionality diff (both
   directions, whitelist scope, fresh artifacts).
3. Thermonuclear review pass with findings dispositioned.
4. Sealed exam extended (O13, O14) and fully green, independently run.
5. Elevation pass + its own both-directions checker PASS.
6. Public repo README + site page updated to checker-proven facts
   only; staging CLAIMS.md refreshed.
7. Weakest remaining point named below.

## Build log

- Functionality round (maker): creativity engine (`src/ideas.rs`,
  /api/ideas + /api/ideas/build, per-chat built/pitched state, pitch
  lines in the persisted thread, idea buttons in the shell); 5 new
  Rune skills (currency, shop, game_gui, checkpoints, soundtrack —
  10 total); 3 new mock playtest rules (DataStore/BindToClose,
  shop RemoteEvent, checkpoints folder); MAX_STEPS 16 → 40 (garbage
  test now feeds 40 replies); 6 new integration tests. 52/52.
  Commit 209bfa0.
- Fresh-context checker: **PASS-WITH-NOTES** (report:
  `.loop/runs/completion-checker-functionality.md`). Scope exact;
  all five claims verified live incl. adversarial probes (injection
  through every new string arg; source-arg refusal on the plan path).
  One defect reproduced (probe C): creativity state was memory-only —
  a restart re-pitched the opening games into a chat that had built
  one, duplicating pitch lines in the persisted thread.
- Fix: creativity state persists to `ideas.json` (strict load +
  whole-file backup on corruption, same discipline as chats/changes);
  regression test `ideas_state_survives_a_restart`. 53/53.
- Sealed exam extended maker-side: O13 (one-shot complete game with
  repair), O14 (proactivity + built-in plan, zero model calls).
  14 checks total; run recorded below.

## Weakest remaining point

(to be named at close)

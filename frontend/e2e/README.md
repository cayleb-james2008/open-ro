# Open-Ro UI checks

`npm run check:ui`, run from `frontend/`, builds the frontend and the binaries, including `mock_studio`, which stands in for Roblox Studio. It then starts Open-Ro on free ports with a fresh game and a stub model (`fixtures/stub-model.mjs` replays `fixtures/replies.json`), and runs two scripts, each on fresh data:

- `gate.mjs`: the Quest gate, 53 checks. It measures a fresh game and a built one at 1440, 768, 390 and 320 px in light and dark. It then walks the quest (pick, build, playtest, share, conversation, undo, new chat), the failed build, offline Studio and thread error states, the phone drawers, console errors and reduced motion.
- `replay.mjs`: worst-case data fed in through `page.route` mocks, 10 checks.

The check fails unless every check passes and each script ran its full count. It stops everything it started, also when interrupted.

Setup, once:

- `npm ci` (or `npm install`). Playwright is the frontend's own `playwright-core` devDependency.
- A Rust toolchain with this crate's dependencies already fetched (`cargo build --offline` must work).
- Chrome for Testing 151 at `~/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome` (what `npx playwright@1.62.0 install chromium` puts there). To use another Chromium build, set `E2E_CHROME` to its path.

Flags, after `--` (`npm run check:ui -- --skip-build`):

- `--skip-build` reuses the last build.
- `--shots` adds screenshots at 1440 and 390, light and dark, on fresh data.
- `--lints` adds the taste, impeccable and Hallmark lints. They only report and never fail the check. The tools live outside this repo; `lints.sh` lists their default paths and the env vars that override them.

Screenshots, `result.json` files, logs and the run's data go to `e2e/.out/`, which git ignores. To bring the fixture up by hand, run `node e2e/fixture.mjs start` (it prints the URL); stop it with `node e2e/fixture.mjs stop`.

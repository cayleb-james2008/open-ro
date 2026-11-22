# OpenRO UI E2E Checks (`v0.3.0`)

`npm run check:ui` (run from `frontend/`) verifies `role-coverage.mjs` and `motion-source.mjs`, compiles the single-file frontend (`assets/index.html`), touches `src/*.rs` and builds `openro-rs` + `mock_studio`, then launches an isolated fixture instance (`18070-18079` or free loopback port) against `fixtures/stub-model.mjs` and runs:

- `gate.mjs` (`78` checks): pixel/contrast/target/overflow sweeps across `1440x900`, `768x1024`, `390x844`, and `320x640` in Light and Dark on both fresh and built games; `JOB.md` first-fold answers; composer `InputGroup` box & responsive placeholder fit down to `320px`; starter blueprint build, `ProgressCircle` & `Passed` playtest verdict with expandable Luau console log; Studio Explorer quick filter, expandable DataModel hierarchy paths, and pinned `Copy game summary`; custom prompt dispatch; `Undo` `MorphingDialog` `Tab` trap, `Escape` / `Keep it` cancel, and `Yes, undo it` revert; `/` chat search; mobile `390px` Vaul drawers; Studio disconnect/reconnect, refused step, and thread `500` recovery; full `BANNER-FIX-BRIEF` update banner matrix (loopback vs non-loopback, focus transitions, developer copy, plain error mapping); skip link; and `prefers-reduced-motion`.
- `replay.mjs` (`8` checks): worst-case browser `page.route` mocks (`null` fields, `260`-char unbroken paths/errors, singular `1` and four-digit `1234` inline `SlidingNumber` counts at `1440px` and `390px`).

Flags (`npm run check:ui -- --skip-build --shots --lints`):
- `--skip-build`: reuse the existing build.
- `--shots`: capture `1440x900` and `390x844` Light/Dark screenshots into `e2e/.out/shots/`.
- `--lints`: run report-only external linters (`taste`, `impeccable`, `Hallmark sloplint`).

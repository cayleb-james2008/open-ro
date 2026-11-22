# OpenRO v0.3.0 — Game Builder Studio Workbench (`DESIGN.md`)

## 1. The Job (`JOB.md`)

You describe a Roblox game in plain words or pick a Studio blueprint; OpenRO generates Luau from verified Rune skills, writes instances and scripts into Roblox Studio, proves every build with a live playtest before calling it done, and reverts the last build in one click.

First-fold questions answered at `1440x900` and `390x844`:
1. **What complete games or features can I build in Roblox Studio right now?** (`data-answers="1"` — Game Blueprint Starter Cards on empty chat, or pitched follow-up blueprints after a build)
2. **Is Roblox Studio connected and ready to build?** (`data-answers="2"` — live Studio MCP status `Pill` + `Check again` button in Left Rail on desktop `>=768px` and Top Studio Bar on `<768px`)
3. **What Workspace parts and Luau scripts are in my game, and did the playtest pass?** (`data-answers="3"` on Right "In Your Game" Studio Explorer Shelf at `>=1100px`, and `data-answers-via="3"` on the Top Bar `In your game` Vaul drawer button with live `SlidingNumber` piece count below `1100px`)
4. **Where do I describe a custom mechanic or undo the last Studio build?** (`data-answers="4"` — pinned Hairline Bottom Composer Dock with `InputGroupTextarea`, `Undo` `MorphingDialog`, `Send`, and `Enter` / `Shift+Enter` `Kbd` hints)

## 2. Region Map & Uniqueness Comparison Gate

**Region map (`1440x900` desktop / `390x844` mobile):**
`Left Studio Session Rail (256px >=768px, Vaul left drawer <768px: brand + game place name, live Studio status Pill + Check again, primary New chat button, / chat search InputGroup with Kbd, AnimatedBackground chat session list, Light/Dark SegmentedControl, release version link) | Center Studio Stage & Build Thread (flex-1: Studio Header with active chat title + mobile Studio status + mobile Explorer drawer trigger, server/update banner slot, scrollable Studio Stage rendering equal-height Game Blueprint Starter Cards when fresh OR Conversation + Studio Build Trace with Tremor ProgressCircle, inline SlidingNumber change count & Playtest Verdict banner with Cult UI Expandable Luau console log when active, pinned Hairline Bottom Composer Dock) | Right "In Your Game" Studio Explorer Shelf (300px >=1100px, Vaul bottom drawer <1100px: Roblox DataModel hierarchy grouping Workspace Parts & Luau Scripts with SlidingNumber totals, quick filter InputGroup, kind Pills, Expandable path/Luau previews, and pinned Copy game summary footer).`

**Comparison gate (`references/gates.md` section 9):**
Matches none of:
- Square UI board preview (narrow icon rail + project sidebar with user card + tab bar + multi-column kanban board)
- Standard admin dashboard (alert banner over a table/list with a right rail of metric cards)
- House Frontend starter bench (`starter/src/bench/` grid of 53 library specimens)
- Previous OpenRO round-2 Quest (5-circle quest stepper + stage illustration box + folded "The whole conversation" disclosure + torn-paper polygon dock)

## 3. Typography, Palette & Surface Discipline

- **Typography:** Self-hosted `Geist` (`400`, `500` woff2) for UI and prose; `Geist Mono` (`400` woff2, `tabular-nums`) for Studio counts, DataModel paths, Luau source, version tags, and keyboard shortcuts. Body text >= `14px`; metadata/chips/kbd >= `12px`; weights `400` and `500` only.
- **Surfaces & Palette:** Shared House/owner palette (`theme.css` + `tokens.css`), Light default (`data-theme="light"`) + crisp Dark mode (`data-theme="dark"`). Calm nested Square UI hairline surfaces (`--radius-shell: 24px`, `--radius-panel: 16px`, `--radius-dialog: 16px`, `--radius-card: 10px`, `--radius: 9px`, `--radius-segment: 7px`, `--radius-chip: 6px`). Zero gradients, zero fractal grain, zero torn-paper polygons, zero backdrop blur, and zero resting box-shadows (`--elev-lift` used strictly on floating dialogs and toasts).
- **Update Strip (`BANNER-FIX-BRIEF.md`):**
  1. `Update now` shown strictly when `can_apply` is true AND `location.hostname` is loopback (`localhost`, `127.0.0.1`, `[::1]`); non-loopback hosts hide `Update now` and `Check updates` and display `"To update, open OpenRO on the computer it runs on."` alongside `What's new`.
  2. Per-version `Dismiss` surfaces an always-reachable `Show update` button in the Composer status bar (visible at both `1440px` and `390px`); manual `Check updates` clears dismissal.
  3. All six `plainUpdateError(raw)` substrings map to plain-English recovery sentences; raw errors sit inside Cult UI `Expandable` `Details`.
  4. Focus moves to polite status while applying, `Restart now` when staged, and `Show update` when dismissed.

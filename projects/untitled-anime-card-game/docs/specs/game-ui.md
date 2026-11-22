# Client UI spec (game-ui) — prompt-ready

Audience: OpenRO's agent. Build the whole client UI of Untitled Anime Card Game (UACG) in Luau
under `src/client/` (VFX modules in `src/client/VFX/` belong to the world area; call them, never edit them).
Quality target: Anime Nexus RNG X — chunky, glossy, saturated, bouncy. Mobile first.
Server contract: `docs/specs/systems.md` (Net remotes: Request / Sync / Push, State, BattleLog). The client
sends intent only and never computes prices, odds, damage or rewards. Wording: plain words, no jargon.

## 1. Architecture

```
src/client/
  init.client.luau        bootstrap: mount stage, start Store, build HUD, wait for first snapshot
  Kit/                    Theme, Build, Anim, Sfx, Assets, Responsive, UI (components)
  Data/                   Signal, Store (state mirror + Net.call), Catalog (Config reader)
  Router.luau             lazy screen registry; exclusive screens close each other
  HUD.luau                always-on layer
  Screens/<Id>.luau       each returns function(ctx) -> Screen {open,close,isOpen}
  VFX/                    (world) required through pcall; absent VFX must never break a screen
```

Rules: `--!strict`; build every Instance in code (StarterGui `ui/` stays empty); set `Parent` last; no
`wait()`; disconnect connections when a screen closes; no remote call from a loop faster than the Net rate;
every network call shows a busy state and a plain-words error toast (map ErrCodes to words in one place).
All asset ids come from `ReplicatedStorage.Shared.AssetIds` through `Kit/Assets`; a missing id must draw a
procedural fallback (gradient + glyph), never an empty box. Selene and stylua must pass.

## 2. Look (tokens)

- Palette: night violet backgrounds (#0E0A20 → #261652), panels #221848 → lighter #3A2870, text white
  with a 2 px #0A0618 outline (3 px on titles), accents: green #56E06C, blue #4AA0FF, red #FF5464, gold
  #FFCD40, purple #AA68FF, orange #FF9632, gem cyan #58D6FF.
- Rarity ladder (8 tiers, colours + glow): grey, green, blue, purple, gold, red-pink, aqua, white-gold.
  Map the Config rarity list onto the ladder proportionally.
- Fonts: Fredoka One for display/buttons/numbers, Gotham SSm Bold/Medium for body.
- Buttons: face with vertical gradient + 2.5 px dark outline, a 5 px darker "lip" below, a top gloss
  streak. Press: face drops 3 px and scale spring to 0.94; release: overshoot back; hover 1.05 (mouse only);
  disabled: grey gradient + dim text + small shake. Sounds: click, error.
- Panels: rounded 22, vertical gradient, 3 px dark stroke, 1.5 px white inner stroke at 82% transparency.
  If an image id for `ui_panel` (9-slice, slice 48/48/80/80 at 128 px) exists, use it instead.
- Windows: dimmed backdrop, ribbon title overlapping the top edge, red round close button on the top-right
  corner, pop-in spring from 0.82, fade-out on close. Fit any viewport.
- Motion: springs for pops (stiffness ~320, damping ~18), Quad/Back tweens 0.12–0.32 s for state changes,
  count-up for currency gains, gentle looping bob on call-to-action elements.
- Respect `Settings` (reduce shake, mute, music/sfx volume).

## 3. Responsive

One full-screen `Stage` frame with a `UIScale` chosen so the logical short side is clamped to 430–720 px:
phone portrait 390x844, phone landscape 844x390, tablet 768x1024, desktop 1920x1080 must all look intended.
`ScreenGui.IgnoreGuiInset = true`, `ScreenInsets = DeviceSafeInsets`; keep the top-left 120x60 logical px
free for Roblox's own menu buttons. Expose mode ("portrait" | "landscape" | "wide") and a changed event.
Grids derive cell size from container width and a column count (2 cols phone portrait, 3–4 tablet, 5–6 wide).
Minimum tap target 44 px. Text never overflows (wrap, size constraints).

## 4. HUD (always visible)

- Left column under the Roblox buttons: currency pills (every currency in State.currencies the design marks
  as primary: gems, coins, roll tickets) with icon, value, count-up on change, "+" shortcut to the shop.
- Top right: rebirth count chip and active-event chip (weather/luck event with a countdown).
- Right side dock of round icon buttons with red dots when something is claimable: Quests, Skills, Codes,
  Items (potions), Settings.
- Bottom dock (5 big tabs): Cards, Deck, Battle, Rebirth, Shop.
- Roll cluster above the dock: large green ROLL button (shows cost), x10 button, Auto and Fast toggles
  (locked with a crown badge until the pass is owned, tap opens the shop on it), Luck chip ("Luck x2.4",
  tap opens the luck breakdown list from `GetOdds`), Odds button (opens the odds table), pity bar under it.
- Roll flow: `Roll{banner,count}` -> play Reveal for the results -> re-enable. Auto repeats until a stop
  condition (not enough currency, tap again, battle starts). Fast shortens the reveal. Roll button is
  disabled while a request is in flight or a reveal plays. Tutorial highlights the ROLL button first.

## 5. Screens

For each: open/close animation, loading + empty + error states, and the data source.

1. **Reveal** (roll cinematic). Rarity-escalating: common = quick card flip (0.6 s); uncommon/rare = glow burst
   + light rays; epic = darkened screen, shake 4 px, particle burst; legendary and above = slow-motion hold,
   screen shake that decays, rarity-coloured full-screen flash, name banner slam-in, server-wide "Rare"
   announcement banner for others. Shows card art, name, rarity, "NEW!" tag when `isNew`, trait/mutation tags,
   "1 in N" odds line, pity-hit tag. x10 results show a 2x5 grid reveal sequentially, best card last. Tap
   skips the current card; a Skip All button appears after the first reveal. Calls `VFX` modules for the burst.
   Sound: reveal_common/rare/epic/legendary/mythic. Never blocks for more than 2.5 s in Fast mode.
2. **Collection / Index**: grid of owned cards grouped and sortable (rarity, level, element, newest), filter chips,
   search; locked silhouettes for undiscovered index entries with "1 in N" hints; index milestone rewards
   (`ClaimIndex`); duplicates count; lock toggle; bulk dismantle with confirmation. Source: State.cards, State.index.
3. **Card inspect**: large art with rarity frame, stats, ability and ultimate text, trait, mutation, charm slot,
   level up (`CardLevelUp`), ascend with dupes (`CardAscend`), reroll trait/mutation with cost shown
   (`CardRerollTrait`, `CardRerollMutation`), lock, equip charm (`CharmEquip`, `CharmCraft`), "add to deck".
4. **Deck builder**: active and defense decks, slot rail (unlockable slots via `UnlockDeckSlot`), tap a slot
   then a card, drag where supported, power total, element/role synergy hints, validation message, `SetDeck`.
5. **Battle HUD**: replays `BattleLog` (never simulates). Player team bottom, enemy top, HP bars, turn order
   rail, damage numbers (crit bigger/yellow, heal green), status icons, ultimate cut-in (full-width rarity-coloured
   banner with character art slide, speed lines, freeze 0.5 s, calls VFX), speed 1x/2x/skip, result panel with
   rewards, stars and "Retry / Next / Back". Boss and raid variants show a big boss bar.
6. **World & act select** (Battle tab, tab 1): worlds list with lock states, acts 1..N with stars, boss node,
   recommended power, first-clear rewards. `BattleStart{mode:"act"|"boss"}`.
7. **Tower** (tab 2): next floor, best floor, floor rewards preview, start (`BattleStart{mode:"tower"}`).
8. **Raid** (tab 3): live boss HP bar, timer to end/next, my damage, top damage list, fight button
   (`RaidInfo`, `RaidFight`), Push "Raid"/"RaidEnd" updates; announces when a raid spawns.
9. **Ranked** (tab 4): elo, season countdown, attempts left, opponent cards (3 shown with power and team
   preview), fight (`RankedList`, `RankedFight`), claim season (`RankedClaimSeason`), leaderboard (`GetLeaderboard`).
10. **Rebirth**: requirements, what resets, what you keep, rebirth points and luck bonus preview, confirm dialog
    with typed-free two-step confirm, celebratory animation (`Rebirth`).
11. **Skill tree**: node graph (scrollable, pinch/zoom not required), costs, prerequisites, respec
    (`SkillUpgrade`, `SkillRespec`), points counter.
12. **Quests**: daily and weekly tabs with progress bars and reset timers, claim all, login streak calendar
    (`ClaimLogin`), playtime rewards (`ClaimPlaytime`), index milestones shortcut.
13. **Shop**: tabs Passes, Products (gem packs, luck potions, boosts), Season Pass (tier ladder with free and
    premium lanes, claim buttons, XP bar), Private-server / Premium bonus. Buying calls
    `PromptPurchase{kind,id}`; show "owned" state from State.passes; handle `Purchase` push (success banner).
    Prices shown only if Config provides them; Robux price comes from the product definition.
14. **Codes**: text box, redeem (`RedeemCode`), reward list, history of redeemed codes.
15. **Settings**: music/sound sliders, shake/reduce motion, auto-roll skip rarities, notifications, show/hide
    other players' pull announcements, Light/Dark is NOT here unless the owner asks (this game is dark-only),
    credits/version, "Report a problem" text. Persist through `SetSetting` (booleans 0/1).
16. **Onboarding**: first-time guided flow driven by `State.tutorial` and `TutorialStep`: welcome card, forced first
    roll, explain rarity, add the card to the deck, first battle, claim reward, unlock the rest of the dock
    step by step. Dimmed spotlight with pulsing ring on the target button, skippable after step 3.
17. **Items**: potions/boosts list with active timers, `UseItem`.
18. **Toasts and loot panel**: Push `Notice` (toast), `Reward` (loot panel with count-ups, tap to dismiss),
    `QuestDone` (small banner), `Rare` (top banner "Name pulled Card!").

## 6. Art requests (to @art; sizes are minimums, PNG with alpha, power-of-two friendly)

`ui_panel` 128x128 9-slice (slice 48/48/80/80) · `ui_button_face` and `ui_button_lip` 128x64 (slice 32) ·
`ui_pill` 128x48 · `ui_ribbon_title` 256x96 (slice 64) · `ui_close_x` 96x96 · `ui_gloss` 128x32 ·
rarity frames `frame_r1..frame_r8` 256x356 card frame with transparent window · rarity glow `glow_r1..r8`
256x256 · icons 128x128: gems, coins, roll_ticket, luck, potion, pass, star, lock, crown, sword, deck,
cards, world, tower, raid, ranked, rebirth, skills, quests, shop, codes, settings · reveal rays 512x512 ·
card back 256x356 · banner plate 512x128 · sounds (via OpenRO audio tools): click, error, open, close, roll,
reveal_common..reveal_mythic, win, lose, hit, ultimate, coin.

## 7. Acceptance (playtester checks)

0 console errors; HUD renders on 390x844, 844x390, 768x1024, 1920x1080 with no overlap with the Roblox menu
buttons; roll 100 times without error; every rarity tier reveal plays, is skippable and never blocks input
afterwards; collection, inspect, deck builder, battle replay, worlds, tower, raid, ranked, rebirth, skills,
quests, shop (every product button prompts), codes, settings and onboarding each open, close and recover from
a server error; no screen shows raw ids or "nil"; all text is plain words.

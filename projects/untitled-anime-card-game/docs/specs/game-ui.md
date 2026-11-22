# Client UI spec (game-ui) — prompt-ready

Audience: OpenRO's agent. Build the whole client UI of Untitled Anime Card Game (UACG) in Luau under
`src/client/` (VFX modules in `src/client/VFX/` belong to the world area: call them through `pcall`,
never edit them). Source of truth for content, numbers and names: `docs/GDD.md` (§3 systems, §5 screens),
`docs/ROSTER.md`, `docs/data/*.json` (via generated `Config`), `docs/QUALITY-BAR.md`. Server contract:
`docs/specs/systems.md` (Net: Request / Sync / Push, State, BattleLog). If an action or field this spec
names does not exist there, say so in the build log and ask; never invent server behaviour.
The client sends intent only; it never computes prices, odds, damage, rewards or Elo. Quality target:
Anime Nexus RNG X — chunky, glossy, saturated, bouncy. Mobile first. Plain wording, no internal jargon.

## 1. Architecture

```
src/client/
  init.client.luau      bootstrap: mount Stage, start Store, build HUD, wait for first snapshot (loading card)
  Kit/                  Theme, Build, Anim, Sfx, Assets, Responsive, UI (components), Format (numbers K M B T Qa Qi Sx)
  Data/                 Signal, Store (state mirror + Net.call + error words), Catalog (Config reader)
  Router.luau           lazy screen registry; exclusive screens close each other
  HUD.luau              always-on layer
  Screens/<Id>.luau     each returns function(ctx) -> Screen { open, close, isOpen, exclusive? }
  Debug.luau            Studio-only test hooks (see §8)
tests/ui/*.luau         Lune tests for the pure parts (Responsive.simulate, Format, Catalog rarity mapping)
```

Rules: `--!strict`; build every Instance in code (StarterGui `ui/` stays empty); set `Parent` last; no
`wait()`; disconnect connections on close; never call a remote faster than its Net rate (self-throttle from
`Net.Actions`); every request shows a busy state and a plain-words error toast, with ErrCode -> words in one
place; all asset ids come from `ReplicatedStorage.Shared.AssetIds` through `Kit/Assets` and a missing id draws
a procedural fallback (gradient + glyph), never an empty box. Selene and stylua must pass.

## 2. Look (tokens)

- Two token sets in `Theme`: **Dark** (default; night violet #0E0A20 → #261652, panels #221848 → #3A2870) and
  **Light** (lavender-white panels, same accents, dark text). The Light/Dark switch exists ONLY in Settings
  (owner rule #67), never in the HUD or any other screen; choice persists via `SetSetting`.
- Text: white with a 2 px #0A0618 outline (3 px titles) in Dark; dark text with a white 1.5 px outline in Light.
- Accents: green #56E06C, blue #4AA0FF, red #FF5464, gold #FFCD40, purple #AA68FF, orange #FF9632, gem #58D6FF.
- **Rarity ladder (8 tiers, exactly the GDD names):** Common, Uncommon, Rare, Epic, Legendary, Mythic, Secret,
  Transcendent. Each tier has a colour, a glow colour, a **shape glyph** and a **text label**; the glyph and
  label always appear together with the colour (never colour alone): Common ●, Uncommon ■, Rare ◆, Epic ★,
  Legendary ✦, Mythic ⬟, Secret ⬢, Transcendent ♛ (draw the glyph as a ShapeFrame/Text; image if provided).
  Colours: grey, green, blue, purple, gold, red-pink, aqua, white-gold with rainbow shimmer.
- Fonts: Fredoka One display/buttons/numbers; Gotham SSm Bold/Medium body.
- Buttons: face with vertical gradient + 2.5 px dark outline, a 5 px darker lip below, top gloss streak.
  Press: face drops 3 px, scale spring to 0.94; release overshoot; hover 1.05 (mouse only); disabled: grey +
  dim text + small shake. Unaffordable actions state the requirement and route to it (e.g. "Need 50 more Gems
  — Get Gems" opens the shop on gems). Sounds: click, error.
- Panels: radius 22, vertical gradient, 3 px dark stroke, 1.5 px white inner stroke at 82 % transparency; use the
  9-slice image `ui_panel` when it exists.
- Windows: dimmed backdrop, ribbon title overlapping the top edge, red round close button on the corner, pop-in
  spring from 0.82, fade-out; fit any viewport; one primary action per screen.
- Motion: springs for pops (stiffness ~320, damping ~18), Quad/Back tweens 0.12–0.32 s, count-up for gains,
  looping bob on call-to-action elements. Honour Settings: Reduced motion (no shake/parallax, shorter tweens),
  **Flash off** (replace every full-screen flash with a soft colour tint ≤ 25 % opacity), mute/volumes.

## 3. Responsive

One full-screen `Stage` with a `UIScale` so the logical short side is clamped to 430–720 px. Supported:
phone portrait 390x844, phone landscape 844x390, tablet 768x1024, desktop 1920x1080.
`ScreenGui.IgnoreGuiInset = true`, `ScreenInsets = DeviceSafeInsets`; the top-left 120x60 logical px corner is
reserved for Roblox's menu buttons: no GuiObject of ours may overlap it. Minimum tap target 44 px. Grids derive
cell size from container width and a column count (2 phone portrait, 3–4 tablet, 5–6 wide). Text wraps and
never overflows.
`Responsive.layoutFor(width, height)` is a PURE function returning `{ mode, scale, logicalSize, gridCols }`
(no Instances) so it runs under Lune; `Responsive.simulate(w, h)` (Studio only) applies it to the live Stage.

## 4. HUD (always visible)

- Left, under the Roblox corner: currency pills for Coins, Gems, Roll Tickets (icon, formatted value, count-up
  on change, "+" opens the shop); more currencies live in a "Wallet" popover.
- **Next Goal strip** (top centre): one line naming the nearest milestone (first Legendary, clear Act 3,
  rebirth ready…), progress bar, tap routes to the screen that advances it. Hidden during cinematics.
- Top right: Luck panel button (shows total "Luck x2.4"; opens itemised sources from `GetOdds`: base, rebirth,
  skills, potions, weather, group/Premium/passes) and active-event chip (Lucky Hour, weather) with countdown.
- Left rail (6 tall icon buttons, red dot when something is claimable): Cards, Deck, Worlds, Spire, Raid, Ranked,
  Shop, Skill Tree, Quests, Pass — collapsing into two columns on short landscape screens, always scrollable.
- Right rail: Codes, Settings, Leaderboards, Daily/Playtime gifts, Potions drawer, Profile.
- Roll cluster bottom centre: big green ROLL (shows roll count per press and cooldown ring), x10-style Ticket
  button (uses 100 roll tickets, opens the summary screen), AUTO and FAST toggles (crown badge + route to the
  shop when the pass is not owned; Auto is free at Rebirth 1), Odds button, and the **two guarantee counters**
  ("Legendary+ in 142 rolls", "Mythic+ in 4,211 rolls") always visible above the button; Luck chip.
- World boss HUD (when one is live): top banner with boss name, shared HP bar, timer, my damage, "Fight" button
  (`GetServerInfo`, Push `Raid`/world-boss events).
- Roll flow: `Roll{banner,count}` -> Reveal -> re-enable. Auto repeats until a stop condition (stop-on-rarity
  from Settings, not enough tickets/cooldown, tapped off, tutorial, battle). **Auto Sell** threshold applies to
  spare duplicates and is shown in the result. Fast shortens reveals; "Skip reveal below <rarity>" (Settings,
  default Epic) turns low reveals into a ticker line.

## 5. Screens

For each: open/close animation, loading + empty + error states, data source named. "Spire" is the GDD name for
the tower; use it everywhere in the UI.

1. **Reveal** (roll cinematic), rarity-escalating per GDD §3.1: Common–Rare quick flip 0.35 s, skippable;
   Epic glow + card slam; Legendary rarity-coloured burst + camera push (call `VFX`); Mythic screen shake +
   title ribbon; Secret/Transcendent full-screen cinematic (slow-mo hold, decaying shake, name banner slam,
   rarity flash, "1 in N" line) and the server-wide announcement from Push `Rare` shown to everyone as a top
   banner. Shows art, name ("Name (Title)"), rarity glyph + label, NEW tag when `isNew`, trait and mutation
   tags, odds line, guarantee tag when `pityHit`. Multi-roll shows a result grid, best last. Tap skips the
   card; Skip All after the first. Fast mode never blocks > 2.5 s. Result summary screen for Roll Tickets (counts
   by rarity, best pulls, new index entries, auto-sold coins).
2. **Collection + Index**: one entry per card per mutation; owned grid with sort (rarity, level, show, newest,
   power), filter chips (rarity, role, show, mutation), search, copies/Spare count, lock toggle; locked
   silhouettes for undiscovered entries with "1 in N" hints; Index completion milestones (`ClaimIndex`) and the
   Mutation Index; Auto Sell settings entry.
3. **Card detail**: art in rarity frame, stats (ATK, HP, SPD, Crit Rate, Crit Dmg, Resist) with suffix numbers,
   role, show, ability and ultimate text, bond hints, level up with coin cost (`CardLevelUp`), ascension stars
   with required copies (`CardAscend`), trait with grade I–V and Reroll Trait (+ previous trait swap-back if
   the server offers it), mutation, 2 charm slots (slot 2 locked until 3★) with equip/craft/fuse
   (`CharmEquip`, `CharmCraft`), lock, dismantle spares, "Add to deck".
4. **Deck builder**: Active/Defense tabs, 5 slots (6th shown locked until Rebirth 3), slot 1 = front line, slot 5 =
   back line labelled, presets (3 + more with pass), tap-slot-then-card (and drag where supported), auto-fill,
   total power, **synergy breakdown** (show bonus, role bonus, bonds: "why am I stronger"), validation message
   (`SetDeck`, `UnlockDeckSlot`).
5. **Battle HUD**: replays the server `BattleLog`, never simulates. 5v5 card portraits (whole portrait slams into
   the target, no humanoid rigs), HP bars, energy bars, turn/action meter, status icons (Stun, Slow, Burn,
   Shield, Regen, ATK Up/Down), damage numbers (crit bigger/yellow, heal green), ultimate cut-in (full-width
   rarity-coloured banner, character art slide, speed lines, 0.5 s freeze, calls `VFX`), speed 1x/2x/4x (4x locked
   behind its pass), Skip (after first view), Auto-Battle toggle, result panel with rewards, 3-star objectives
   (win / no card down / under 45 s), Retry / Next / Back. Boss variant shows a Break bar and big boss bar.
6. **World map**: 5 worlds with locks and boss state, acts 1..5 + boss, Normal/Nightmare toggle (unlocked after
   the boss), stars, recommended power vs yours, first-clear rewards, Story Skip option when eligible
   (`BattleStart{mode:"act"|"boss"}`).
7. **Spire**: current best floor, next floor, power gap, boss every 10th floor, milestone rewards (5, 10, 25, 50,
   100), weekly/all-time leaderboard tab, Auto-climb toggle (`BattleStart{mode:"tower"}`).
8. **Raid** (Colosseum of Legends): acts 1–3, Normal/Nightmare, Raid Tickets (regen, cap) with timer, rewards
   including the 2 exclusive cards and pity ("guaranteed on the 5th clear"), (`RaidInfo`, `RaidFight`).
9. **World boss HUD/screen**: live shared HP, countdown to next spawn, my damage and contribution tier
   (top 3 / top 25 % / participant), exclusive card drop info, kill-chest claim, extra attempt via Boss Potion.
10. **Ranked**: Elo + rank name (Bronze … Grandmaster), season countdown, Ranked Tickets with regen timer, 5
    opponents (name, Elo, power, team preview), fight, replay of attacks on your defense deck ("You were attacked:
    lost 12 Elo"), season rewards claim, top-100 board (`RankedList`, `RankedFight`, `RankedClaimSeason`,
    `GetLeaderboard`). Requires World 1 boss: otherwise state the requirement and route to the world map.
11. **Banners**: Standard World Pack for each unlocked world + rotating Featured Pack, odds table per rarity
    visible, two pity counters (Mythic+ at 150, Secret+ at 1,000, per banner), 1-pull and 10-pull (shown as 9x
    cost) paid with World Tokens or Gems, reveal through the same Reveal screen.
12. **Rebirth**: exact before/after (luck, coin %, stat %, kept vs reset lists), requirement and cost, what unlocks
    at R1/R3/R5/R10, two-step confirm, celebration animation (`Rebirth`).
13. **Skill tree**: pan/zoom graph with four branches (Rolling, Combat, Economy, Collection), node cost,
    prerequisites, level pips, Keep nodes, respec (`SkillUpgrade`, `SkillRespec`).
14. **Quests**: daily (5) and weekly (5) with progress and reset timers, clear-all chest, login streak calendar
    (7-day cycle, bonus days), play-time gifts (10 timed gifts), away earnings popup on join
    (`ClaimQuest`, `ClaimLogin`, `ClaimPlaytime`).
15. **Shop**: tabs Passes, Potions and Boosts, Gem Packs, Bundles, Private Server, Premium chest. Prices and
    Robux costs come only from Config/product definitions (nothing is live until the owner approves). Odds
    shown wherever the product is random. Owned state from State.passes. Buy -> `PromptPurchase`, never a
    client prompt; `Purchase` push -> success banner.
16. **Season Pass**: 60 tiers free + premium lanes, XP bar, claim buttons, buy +10 levels, exclusive card
    previews at tier 30 and 60 (`ClaimSeasonTier`).
17. **Skills / Luck panel and Potions drawer**: itemised Luck and Power panels (every source with its multiplier),
    potions list with active timers, strongest-Luck-wins note, banked time, pause/resume, Use (`UseItem`).
18. **Leaderboards**: tabs Luck (luckiest roll with 1-in-N), Rebirths, Ranked Elo, Spire floor, Total power;
    my rank pinned (`GetLeaderboard`). **Profile**: name, rank, favourite card, stats, titles.
19. **Codes**: textbox, redeem (`RedeemCode`), reward list, redeemed history, plain error words.
20. **Settings**: music and sound sliders, Reduced motion, **Flash** toggle, **Skip reveal below** rarity
    threshold, stop-auto-roll-on rarity, Auto Sell threshold, notifications, hide server-wide announcements,
    **Light/Dark** switch (the only one in the game), credits and version, "Report a problem". Persist with
    `SetSetting` (booleans 0/1).
21. **Onboarding** per GDD §2 (9 steps, `State.tutorial` + `TutorialStep`): guaranteed Rare first roll, place the
    card in the Deck, auto-battle World 1 Act 1, claim and level, Roll x9 (10th guaranteed Epic), fill deck to
    3, clear Act 2, daily gift + codes box, set "Next Goal: first Legendary". Spotlight with pulsing ring on the
    target, dock unlocks step by step, skippable after step 3 for the gem bonus.
22. **Toasts, loot panel, banners**: Push `Notice` (toast), `Reward` (loot panel, count-ups, tap to dismiss),
    `QuestDone` (small banner), `Rare` (top banner "Name rolled Card — 1 in N").

## 6. Art and sound requests (to @art; PNG with alpha, power-of-two friendly)

`ui_panel` 128x128 9-slice (48/48/80/80) · `ui_button_face` + `ui_button_lip` 128x64 (slice 32) · `ui_pill`
128x48 · `ui_ribbon_title` 256x96 (slice 64) · `ui_close_x` 96 · `ui_gloss` 128x32 · rarity frames
`frame_r1..frame_r8` 256x356 (transparent window) · rarity glows `glow_r1..r8` 256 · rarity glyph icons
`glyph_r1..r8` 64 · currency/item icons 128: coins, gems, roll_ticket, trait_reroll, world_token, charm_dust,
spire_token, raid_token, ranked_medal, pass_xp, potion_*, luck, star, lock, crown · nav icons 128: cards, deck,
worlds, spire, raid, ranked, shop, skills, quests, pass, codes, settings, leaderboard, daily, potion, profile ·
reveal rays 512 · card back 256x356 · banner plate 512x128. Sounds (via OpenRO audio tools): click, error, open,
close, roll, reveal_common..reveal_transcendent, win, lose, hit, ultimate, coin, level_up.

## 7. Acceptance (checked by playtester; section 8 gives the hooks)

0 console errors. HUD lays out at 390x844, 844x390, 768x1024, 1920x1080 with no GuiObject inside the 120x60
corner and no overlapping buttons. Roll 100 times without error; every rarity tier reveal plays, is skippable
and releases input afterwards; Flash-off and Reduced-motion change it. Every screen in §5 opens, closes and
recovers from a forced server error; no screen shows raw ids, "nil" or jargon; every rarity shows glyph +
label; Light/Dark exists only in Settings; unaffordable buttons route to what is needed; Lune tests pass.

## 8. Test hooks (Studio only, gated on `RunService:IsStudio()`, absent otherwise)

`Debug.openScreen(id, params?)` · `Debug.forceRarity(tierOrRarityId)` (next Reveal plays that tier with a fake
local result, no server write) · `Debug.forceError(code?)` (next Net.call returns that ErrCode) ·
`Debug.setMode(w, h)` (calls `Responsive.simulate`) · `Debug.cornerViolations()` returns the list of
GuiObjects whose AbsolutePosition rectangle intersects the 120x60 corner after scaling · `Debug.screens()`
lists registered screen ids. Hooks are reachable through `execute_luau` via a `BindableFunction` named
`UACGDebug` in `ReplicatedStorage` (created only in Studio).

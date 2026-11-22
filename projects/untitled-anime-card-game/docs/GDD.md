# Game Design Document — Untitled Anime Card Game (UACG)

> **Document Status:** Design Milestone 1 (Canonical Reference)
> **Target Genre:** Hybrid Anime RNG Card Collector & Auto-Battler (`anime-rng-cards`)
> **Benchmark References:** [docs/RESEARCH.md](./RESEARCH.md) (*Anime Nexus RNG X* `85829558919037` + *Anime Card Battles: Rebirth* `130253630950453`)
> **Companion Docs:** [docs/ROSTER.md](./ROSTER.md) · [docs/ECONOMY.md](./ECONOMY.md) · [docs/MONETIZATION.md](./MONETIZATION.md) · [docs/QUALITY-BAR.md](./QUALITY-BAR.md)

---

## 1. High-Concept Vision & Product Pillars

**Untitled Anime Card Game (UACG)** fuses the visceral, high-dopamine **1-in-N RNG card rolling, mutations, weather events, skill trees, and rebirth loop** of *Anime Nexus RNG X* with the **themed world banners, Active/Defense deck auto-battling, 100-floor tower, anti-stat world bosses, charms crafting, and async Elo PvP** of *Anime Card Battles: Rebirth*.

### Core Pillars
1. **Kinetic Roll Dopamine:** Every pull feels tactile—fast roll cadence, escalating rarity shake/shatter VFX, 5 visual mutations, and transparent odds + soft/hard pity.
2. **Meaningful Deck Strategy:** Rare cards matter beyond flex—roles, show/duo bonds, traits, charms, and counter-abilities decide PvE bosses, Tower floors, and Async Ranked PvP.
3. **Friction-Free Prestige:** Guided Rebirths preview kept vs. reset items and prompt players to spend leftover `Yen` before resetting—eliminating the #1 player complaint observed in *Anime Nexus RNG X*.
4. **Fair, Focused Launch:** **No player trading at launch** (protects economy integrity and eliminates scam vectors), **Light/Dark theme toggle in Settings only**, and **plain user-facing wording** across every screen.

---

## 2. Core Gameplay Loop

```text
  +-----------------------------------------------------------------------+
  |                         1. ROLL WORLD BANNERS                         |
  |  Spend Yen / Roll Tickets on World Packs (1-in-2 up to 1-in-25M)     |
  |  Weather Events + Stacked Luck + Charms + Pity shape odds & mutations |
  +-----------------------------------+-----------------------------------+
                                      |
                                      v
  +-----------------------------------------------------------------------+
  |                    2. COLLECT, ASCEND & OPTIMIZE                      |
  |  Level Cards (1-100) · Dupe Ascend (*0-*5) · Reroll Traits (Crystals) |
  |  Craft & Equip 2 Charms · Fill Index for Permanent Luck/Stat Bonuses  |
  +-----------------------------------+-----------------------------------+
                                      |
                                      v
  +-----------------------------------------------------------------------+
  |               3. BUILD ACTIVE (4) & DEFENSE (4) DECKS                 |
  |  Position 2 Frontline / 2 Backline · Trigger Show & Iconic Duo Bonds  |
  +-----------------------------------+-----------------------------------+
                                      |
          +---------------------------+---------------------------+
          |                           |                           |
          v                           v                           v
  +---------------+         +-------------------+       +-----------------+
  | 5 WORLDS x    |         | 100-FLOOR TOWER   |       | ASYNC RANKED    |
  | 5 ACTS + BOSS |         | & 2 CO-OP RAIDS   |       | ARENA (ELO PvP) |
  | + INFINITE    |         |                   |       | vs Defense Deck |
  +-------+-------+         +---------+---------+       +--------+--------+
          |                           |                          |
          +---------------------------+--------------------------+
                                      |
                                      v
  +-----------------------------------------------------------------------+
  |                    4. EARN CURRENCIES & PRESTIGE                      |
  |  Yen · Gems · Roll Tickets · Trait Crystals · Boss Tokens · Medals    |
  |  Unlock Skill Tree Nodes -> Guided Rebirth (Keep Cards, +Permanent    |
  |  Luck & All Stats) -> Push Higher Worlds, Mutations & Leaderboards    |
  +-----------------------------------------------------------------------+
```

---

## 3. Canonical Currencies, Rarities & Mutations

### 3.1 Currencies (6)
| Currency | Primary Sources | Primary Sinks |
| :--- | :--- | :--- |
| **Yen** | Story Acts, Infinite Mode, Quests, Playtime Rewards, Dupe Quick-Sell | Rolling standard World Banners, Card Leveling (`1–100`), Charm Crafting fees |
| **Gems** | Quests, Index Milestones, Tower Clears, Ranked Rewards, Codes, Packs | **Rebirths**, Skill Tree nodes, Shop Potions, Premium Banner Tickets |
| **Roll Tickets** | Daily/Weekly Quests, Login Streak, Tower Floors, Codes, Packs | Free/Discounted Banner Rolls (including +Luck Ticket Rolls) |
| **Trait Crystals** | Raids, Infinite Wave Milestones, Weekly Quests, Bosses, Bundle | Rerolling Card Traits (12 traits) |
| **Boss Tokens** | World Bosses (5 Worlds) & Co-op Raids | Boss Shop Unlocks, Strange Trader Charms, Boss Potions |
| **Ranked Medals** | Async Ranked Arena wins & Weekly/Seasonal Elo payouts | Ranked Shop (Exclusive cosmetics, Trait Crystals, Banner Tickets) |

### 3.2 Rarity Tiers (9) & Base Odds
| Tier | Base 1-in-N Odds | Soft / Hard Pity | Visual Signature |
| :--- | :--- | :--- | :--- |
| **Common** | `1-in-2` (`50.0%`) | — | Matte slate frame, quick flip |
| **Uncommon** | `1-in-5` (`20.0%`) | — | Emerald rim glow |
| **Rare** | `1-in-20` (`5.0%`) | — | Cobalt foil flash |
| **Epic** | `1-in-100` (`1.0%`) | Hard pity at `100` rolls | Amethyst crest burst |
| **Legendary** | `1-in-1,000` (`0.1%`) | Soft pity `450`, Hard `750` | Solar gold beam + screen shake |
| **Mythic** | `1-in-25,000` (`0.004%`) | Soft pity `8,000`, Hard `15,000` | Crimson flame pillar + cutscene |
| **Secret** | `1-in-250,000` (`0.0004%`) | — (luck-boosted) | Obsidian rift shatter + server banner |
| **Universal** | `1-in-2,500,000` (`0.00004%`) | — (luck-boosted) | Astral prism explosion + global toast |
| **Nexus** | `1-in-25,000,000` (`0.000004%`) | — (luck-boosted) | Celestial domain takeover + server chime |

### 3.3 Mutations (5)
Every rolled card independently rolls a Mutation tier (boosted by the **Mutation & Shiny Hunter** pass, Weather, and Skill Tree):
- **Normal (`1.0x` stats)** — ~92.0% base
- **Shiny (`1.5x` stats)** — ~5.5% base
- **Golden (`2.25x` stats)** — ~1.8% base
- **Rainbow (`4.0x` stats)** — ~0.6% base
- **Void (`7.5x` stats)** — ~0.1% base

---

## 4. Rolling, Stacked Luck & Weather Systems

### 4.1 Roll Modes & Banner Controls
- **Manual Roll:** Instant single pull (`1.6s` base cooldown, reduced by Charms).
- **Fast Roll (Gamepass):** Cuts animation/cooldown to `0.6s`.
- **Auto Roll (Gamepass):** Continuous rolling with configurable **Auto-Sell / Auto-Ascend filters** by Rarity and Mutation (never deletes locked, favorited, or `Secret+` cards).
- **Multi-Roll (Gamepass):** Rolls `5` cards simultaneously per pull cycle.

### 4.2 Stacked Luck Formula
Effective Luck Multiplier combines **Base (`1.0x`) + Additive Flat Luck + Multiplicative Sources**:
- **Additive Luck:** Group Join (`+1.0x`), Index Milestones (up to `+3.0x`), Charms (`+1.0x` to `+15.0x` each), Skill Tree (`+0.5x` to `+5.0x`), Guild/Party Bonus (`+0.2x` to `+0.5x`).
- **Multiplicative Luck:**
  - Gamepasses: **Lucky** (`1.5x`), **2x Luck / Super Lucky** (`2.0x`), **Mega Lucky** (`3.0x`) — stackable.
  - Platform Perks: **Roblox Premium** (`1.15x` Luck, `+10%` Potion duration) + **VIP Gamepass** (`1.20x` Luck, `+15%` Potion duration).
  - **Potions:** Minor (`1.5x`, 10m), Major (`2.5x`, 15m), Super (`4.0x`, 20m), Boss Luck Potion (`6.0x`, 10m).
  - **Rebirths:** `+15%` multiplicative Luck per Rebirth (compounding up to cap `250`).
  - **Weather & Events:** Active World Weather (`1.5x–2.5x`) and Global Events (`2.0x`).
  - **Milestone Roll Bonus:** Unlocked via World Bosses (`5x` every 25th roll, `20x` every 100th roll, `100x` every 500th roll).

### 4.3 Dynamic Weather & Server Events
Every **10 minutes**, a server-wide Weather Event activates for **4 minutes** with distinct skybox/VFX and banner buffs:
1. **Chakra Storm** — `2.0x` Luck in *Hidden Leaf Village*, `+25%` Yen drops.
2. **Supernova Flare** — `2.0x` Luck in *West City & Namek*, `+50%` Shiny/Golden odds.
3. **Grand Line Tempest** — `2.0x` Luck in *Grand Line Seas*, `-15%` Roll Cooldown.
4. **Shibuya Eclipse** — `2.5x` Rainbow/Void Mutation odds across all banners.
5. **Soul Surge** — `2.0x` Luck in *Soul Society* + `2x` Trait Crystal drop chance.

---

## 5. Card Depth, Traits, Charms & Progression

### 5.1 Card Levels & Dupe Ascension
- **Card Leveling (`Lv. 1–100`):** Spend `Yen` to scale `HP`, `ATK`, and `SPD` smoothly (`+4.5%` per level). Formatted stats display cleanly (`1.25K`, `84.2M`, `3.90B`, `1.42T`, `2.50Qa`).
- **Dupe Ascension (`★0–★5`):** Feed duplicate copies of the same Card ID to unlock Star tiers (`+20%` multiplicative stats per star and `+1` Ability Rank at `★3` and `★5`).
- **Index Completion Bonuses:** Discovering unique Cards and Mutation variants fills the **Card Index**, granting permanent account-wide `+Luck`, `+HP%`, and `+ATK%` per 10% World completion milestone.

### 5.2 Rerollable Traits (12)
Spend **Trait Crystals** to roll 1 passive Trait per card (with `Epic+` guarantee protection on every 25th crystal):
- *Common–Rare Traits:* **Swift** (`+12% SPD`), **Brawler** (`+15% ATK`), **Stalwart** (`+18% HP`), **Piercer** (`+15% DEF Ignore`), **Scholar** (`+15% Battle Yen`), **Focused** (`+12% Crit Chance`).
- *Epic–Mythic Traits:* **Vampiric** (`12% Lifesteal`), **Tactician** (`-1 turn ability cooldown / +20% Skill DMG`), **Ironclad** (`20% Damage Reduction`), **Executioner** (`+35% DMG vs <40% HP`), **Transcendent** (`+30% ATK, +20% SPD`), **Almighty** (`+40% All Stats + 15% Dodge`).

### 5.3 Charms Crafting (2 Slots)
Crafted from duplicate rolled cards + `Boss Tokens` + `Yen` at **The Merchant** (Hub) and **Strange Trader** (unlocked at Tower Floor 100):
- **Shuriken** (`-10%` Roll Cooldown) — Common/Uncommon dupes.
- **Cursed Finger** (`+2` Luck) — Rare dupes + World 4 mats.
- **Samehada** (`+3` Luck, `-15%` Roll Cooldown) — Epic dupes + Boss Tokens.
- **Stand Arrow** (`+5` Luck, `-15%` Roll Cooldown) — Legendary dupes + Boss Tokens.
- **Holy Grail** (`+7` Luck, `+15%` Potion Duration) — Strange Trader.
- **Champion's Crown** (`+10` Luck, `+10%` Ranked Elo Gain) — Strange Trader.
- **Author's Note** (`+15` Luck, `-20%` Roll Cooldown) — Endgame Strange Trader craft.

### 5.4 Three-Branch Skill Tree & Guided Rebirth
- **Skill Tree (`Gems`):**
  1. *Fortune Branch:* Flat Luck, Mutation Chance, Soft Pity threshold reduction.
  2. *Mastery Branch:* Deck ATK/HP/SPD %, Bond bonus amplification, Boss Damage.
  3. *Efficiency Branch:* Yen/Gem gain %, Roll speed, Charm & Potion duration.
- **Guided Rebirth (`Gems`, Cap: `250`):**
  - Base cost: `25,000 Gems` (scales per Rebirth; see `docs/ECONOMY.md`).
  - **Permanent Gain:** `+15%` multiplicative Luck + `+10%` All Card Stats per Rebirth.
  - **Kept:** All Cards, Levels, Stars, Traits, Mutations, Skill Tree, Index, Charms, Gamepasses, `Gems`, `Roll Tickets`, `Trait Crystals`, `Boss Tokens`, `Ranked Medals`.
  - **Reset:** `Yen` and current Story Act progression (fast-clear auto-skip available for previously conquered worlds).
  - **Pre-Rebirth Safety Modal:** Clearly lists Kept vs. Reset and offers a 1-click **Auto-Spend Leftover Yen on Levels** button before confirming.

---

## 6. Combat & Game Modes

### 6.1 Auto-Battle Engine (Active Deck & Defense Deck)
- **Decks:** Players configure a 4-card **Active Deck** (2 Frontline, 2 Backline) for all attacks and a 4-card **Defense Deck** saved to their profile for async PvP. Extra Deck Slots (`+2`, total 4 saved presets) unlock via the **+Deck Slots** gamepass.
- **Resolution:** Deterministic server-authoritative turn simulation (at `1x` or `2x` playback speed on client). Turns proceed by `SPD`, evaluating **Show/Duo Bonds**, **Role targeting**, and **Counter-Abilities** (`Dodge`, `Freeze`, `Stun`, `Reflect`, `Pierce`, `Resurrect`, `Burn`, `Bleed`, `Cleanse`, `Silence`).

### 6.2 Five Worlds (5 Acts + World Boss + Infinite Mode)
| World | Anime Franchise | Banner Name | Act 1–5 Escalation | World Boss & Anti-Stat Mechanic | Permanent Unlock |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **1. Hidden Leaf Village** | *Naruto* | `Shinobi Scroll Pack` | Genin Exam -> Chunin Forest -> Valley Clash | **Ten-Tails Jinchuriki** (Nullifies first 2 skills; punishes unshielded DPS) | Unlocks **Charm Slot 1** & **5x Roll Milestone** |
| **2. West City & Namek** | *Dragon Ball* | `Saiyan Capsule Pack` | Capsule Yard -> Namek Plains -> Frieza Ship | **Galactic Tyrant** (30% Damage Reflect; requires Pierce/Cleanse) | Unlocks **Auto-Ascend** & **World Potion Shop** |
| **3. Grand Line Seas** | *One Piece* | `Grand Line Bounty Pack` | East Pier -> Enies Bridge -> Onigashima Roof | **Azure Dragon Emperor** (Massive Shield + Stun-on-hit; punishes low-SPD decks) | Unlocks **Charm Slot 2** & **20x Roll Milestone** |
| **4. Tokyo Jujutsu High** | *Jujutsu Kaisen* | `Cursed Seal Pack` | School Courtyard -> Detention Center -> Shibuya | **Eight-Handled Divergent General** (Adapts +15% DR per repeated ability type) | Unlocks **Rainbow/Void Border Forge** & **Support Resonance** |
| **5. Soul Society** | *Bleach* | `Soul Reaper Zanpakuto Pack` | Rukon Gate -> Seireitei -> Royal Palace | **Father of the Quincy** (1x Lethal Negate + Counter; requires Execute/Multi-hit) | Unlocks **100x Roll Milestone** & **World 5 Infinite Boost** |

- **Infinite Mode (per World):** Unlocked after Act 5. Endless scaling waves granting accelerating `Yen`, `Gems`, and `Trait Crystals` every 10 waves.

### 6.3 100-Floor Tower (`Heavens Arena`)
- Single-player deterministic ladder from **Floor 1 to Floor 100**.
- Every 10th floor features a themed mini-boss awarding `Gems`, `Roll Tickets`, and `Trait Crystals`.
- **Floor 100 Clear:** Unlocks the **Strange Trader** NPC and awards the `Temporal Attendant` (`Whis`, `Universal`) card.

### 6.4 Co-op Raids (2 Launch Raids)
- Up to **4 players** queue into instanced boss arenas on a 30-minute timer:
  1. **Skull Island Raid** (*One Piece* alliance) — Drops `Boss Tokens`, `Trait Crystals`, and `Pirate King` (`Gol D. Roger`, `Universal`).
  2. **Shibuya Breach Raid** (*Jujutsu Kaisen* disaster curses) — Drops `Boss Tokens`, `Gems`, and `Disaster Flame` (`Jogo`, `Mythic`).

### 6.5 Async Ranked PvP (`Ranked Arena`)
- Challenges asynchronous snapshots of other players' **Defense Decks** matched by **Elo Rating** (starting at `1,000 Elo`).
- Tiers: `Bronze` (<1,100) -> `Silver` (1,100) -> `Gold` (1,300) -> `Platinum` (1,500) -> `Diamond` (1,750) -> `Master` (2,000) -> `Grandmaster` (2,300) -> `Nexus` (2,600+).
- Awards `Ranked Medals` per win/defense hold and seasonal payouts (including `Hokage Flash (KCM)` for Grandmaster+).
- **Rank Decay:** Players above `Master` lose `25 Elo/week` if fewer than 3 Ranked attacks are completed that week.

---

## 7. Retention, Social, Analytics & Guardrails

- **Daily & Weekly Quests:** Rolling, Battle clears, Tower pushes, Boss hits, Ranked matches.
- **7-Day Login Streak & Playtime Rewards:** Escalating `Yen`, `Roll Tickets`, `Luck Potions`, and `Gems` (`5m` to `120m` session ladder).
- **Leaderboards:** Global & Weekly boards for `Total Deck Power`, `Rarest Roll`, `Tower Floor`, `Ranked Elo`, and `Rebirths`.
- **Codes System:** Server-validated promo codes (e.g., `LAUNCH100`, `LEAFSHINOBI`) for update/like milestones.
- **Analytics (`AnalyticsService`):**
  - `LogEconomyEvent`: Tracks every `Yen`, `Gems`, `Roll Tickets`, `Trait Crystals`, `Boss Tokens`, and `Ranked Medals` source/sink.
  - `LogFunnelStepEvent`: Tracks Onboarding Funnel (`1. Join -> 2. First Roll -> 3. Equip Deck -> 4. Clear Act 1 -> 5. First Legendary -> 6. Enter Tower -> 7. First Rebirth`).
- **Platform & Safety Rules:**
  - **No player trading at launch.**
  - **Light/Dark theme toggle in Settings only** (no theme clutter on main HUD).
  - **Plain user-facing wording** everywhere (clear English labels like `Roll`, `Deck`, `Ascend`, `Trait`, `Rebirth`; zero internal/dev jargon).
  - **Private Servers (Free / Low-Cost):** Supports private lobbies for uninterrupted AFK/Weather rolling and Boss/Raid party coordination.

---

## 8. Technical Architecture (`default.project.json` / Rojo)

The project uses the canonical Rojo layout mapped in `default.project.json`:

```text
src/
+-- shared/                      -> ReplicatedStorage.Shared
|   +-- Config/                  (Worlds, Banners, Cards, RarityOdds, Mutations, Traits, Charms, Products)
|   +-- Combat/                  (Deterministic BattleEngine, AbilityResolver, BondCalculator, NumberFormat)
|   +-- Economy/                 (LuckCalculator, PityRules, RebirthCosts)
|   +-- Net/                     (Typed RemoteEvent / RemoteFunction contracts)
+-- server/                      -> ServerScriptService.Server
|   +-- Data/                    (ProfileStore persistence, DefenseDeck snapshots, Leaderboards)
|   +-- Services/                (RollService, BattleService, TowerService, RaidService, BossService,
|   |                             RankedService, RebirthService, WeatherService, AnalyticsService,
|   |                             MonetizationService, CodeService, QuestService)
|   +-- Security/                (Rate limits, server-authoritative RNG, MarketplaceService receipt validation)
+-- client/                      -> StarterPlayerScripts.Client
    +-- Controllers/             (RollController, BattleVFXController, WeatherVFXController, AudioController,
    |                             ThemeController [Settings Light/Dark], InputController)
    +-- UI/                      (HUD, BannerModal, DeckBuilder, CardInspector, Tower/Raid/Boss/Ranked Screens,
                                  Index, CharmCrafter, SkillTree, RebirthModal, Shop, Settings)
```

### Server Authority Principles
1. **All RNG rolls, pity counters, trait rolls, and battle simulations run strictly on the server.** The client receives the deterministic battle replay log / roll outcome and renders animations.
2. **Marketplace receipts & Gamepass ownership** are verified server-side via `MarketplaceService` (`ProcessReceipt` + `UserOwnsGamePassAsync` cached per session).
3. **Async Defense Decks** are stored as compact serialized snapshots in `DataStoreService` / `MemoryStoreService` for fast Elo matchmaking.

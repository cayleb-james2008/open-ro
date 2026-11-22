# Economy Specification — Untitled Anime Card Game (UACG)

> **Document Status:** Design Milestone 1 (Canonical Reference)
> **Companion Docs:** [docs/GDD.md](./GDD.md) · [docs/MONETIZATION.md](./MONETIZATION.md) · [docs/RESEARCH.md](./RESEARCH.md)
> **Headless Verification:** [tools/econ-sim/sim.luau](../tools/econ-sim/sim.luau) (`project_run_lune`)

---

## 1. Design Targets & Economic Guardrails

UACG balances high-frequency RNG excitement (*Anime Nexus RNG X*) with deterministic deck progression (*Anime Card Battles: Rebirth*).

| Milestone / Metric | Target Window | Verified Result (`sim.luau`) | Notes |
| :--- | :--- | :--- | :--- |
| **First Legendary** | `< 15 min` | `1.15m` P50 / `3.38m` mean / `8.27m` P90 (`100.0% <= 15m` F2P); `1.07m` P50 Paid | Driven by Tutorial Roll Tickets, World 1 Acts, 5m/10m Playtime Potions, and Soft Pity `450` |
| **First Rebirth (`25,000 Gems`)** | `2–4 hours` | `170 min` (`2.83 h`, `25,010` Gems F2P) / `145 min` (`2.42 h`, `28,972` Gems Paid) | Combines Story Acts, Tower Floors 1–30, Infinite waves, Daily Quests, Index, and Playtime |
| **First Mythic** | `12–20 hours` | `~14–18 hours` | Soft Pity `8,000` / Hard `15,000`, boosted by Rebirth 1–3 Luck & Weather |
| **First Secret+** | `35–80 hours` | Luck-gated | Requires stacked Charms, Rebirths, Weather, and Milestone Roll bonuses |
| **Player Trading** | **Disabled at launch** | N/A | Prevents RMT, duping, and scamming; all power is earned or pulled |

---

## 2. Canonical Currencies (6)

| Currency | Primary Sources | Primary Sinks | Hourly F2P Benchmark (Early -> Mid -> Late) |
| :--- | :--- | :--- | :--- |
| **Yen** | Story Acts, Infinite Mode, Dupe Quick-Sell, Quests, Playtime | Standard World Banner Rolls, Card Leveling (`1–100`), Charm Crafting fees | `45K/hr` -> `450K/hr` -> `6.5M/hr` |
| **Gems** | Quests, Story Clears, Tower Floors, Infinite Milestones, Index, Ranked, Codes | **Rebirths**, Skill Tree nodes, Shop Potions, Premium Ticket Packs | `8.5K/hr` -> `18K/hr` -> `42K/hr` |
| **Roll Tickets** | Daily/Weekly Quests, Login Streak, Tower Bosses, Codes, Packs | Ticket Banner Rolls (5-pull batching + `2.0x` Ticket Luck) | `45/hr` (Hour 1) -> `18/hr` steady |
| **Trait Crystals** | Co-op Raids, Infinite Wave 10s, Weekly Quests, World Bosses | Rerolling Card Traits (`12` traits, `25`-roll `Epic+` guarantee) | `0–4/hr` -> `12/hr` -> `28/hr` |
| **Boss Tokens** | 5 World Bosses & 2 Co-op Raids | Boss Shop Permanent Unlocks, Strange Trader Charms, Boss Potions | `0/hr` -> `15/hr` -> `45/hr` |
| **Ranked Medals** | Async Ranked Arena wins, Defense holds, Weekly/Seasonal Elo payouts | Ranked Shop (Cosmetics, Trait Crystals, Roll Tickets) | `0/hr` -> `60/hr` -> `140/hr` |

---

## 3. World Banners, Rarity Odds & Pity Math

### 3.1 World Banner Costs

| World | Banner Name | Yen Cost / Roll | Ticket Cost | Unlock Gate |
| :--- | :--- | :--- | :--- | :--- |
| **1. Hidden Leaf Village** | `Shinobi Scroll Pack` | `100 Yen` | `1 Roll Ticket` | Default (Join) |
| **2. West City & Namek** | `Saiyan Capsule Pack` | `350 Yen` | `1 Roll Ticket` | World 1 Boss Clear |
| **3. Grand Line Seas** | `Grand Line Bounty Pack` | `1,200 Yen` | `1 Roll Ticket` | World 2 Boss Clear |
| **4. Tokyo Jujutsu High** | `Cursed Seal Pack` | `4,500 Yen` | `1 Roll Ticket` | World 3 Boss Clear |
| **5. Soul Society** | `Soul Reaper Zanpakuto Pack` | `18,000 Yen` | `1 Roll Ticket` | World 4 Boss Clear |

> Higher World Banners introduce stronger base-stat cards while sharing the same canonical 9-tier rarity ladder.

### 3.2 Rarity Tiers (9) & Top-Down Resolution

`RollService` evaluates each roll server-side in strict order: **Hard Pity -> Soft Pity -> Top-Down Rarity Cascade (`Nexus` down to `Common`)**.

| Rarity Tier | Base `1-in-N` | Base Prob (`1x` Luck) | Soft Pity Start | Hard Pity | Quick-Sell Yen Multiplier (`x WorldBase`) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Common** | `1-in-2` (floor) | Remainder (`~73.8956%` at `1x`) | — | — | `0.25x` (`25 Yen` in W1) |
| **Uncommon** | `1-in-5` | `20.0000%` | — | — | `0.75x` (`75 Yen` in W1) |
| **Rare** | `1-in-20` | `5.0000%` | — | — | `3.00x` (`300 Yen` in W1) |
| **Epic** | `1-in-100` | `1.0000%` | — | `100` | `15.0x` (`1,500 Yen` in W1) |
| **Legendary** | `1-in-1,000` | `0.1000%` | `450` (`+0.30%/roll`) | `750` | `100x` (`10,000 Yen` in W1) |
| **Mythic** | `1-in-25,000` | `0.0040%` | `8,000` (`+0.014%/roll`) | `15,000` | `500x` (`50,000 Yen` in W1) |
| **Secret** | `1-in-250,000` | `0.0004%` | — (Luck-scaled) | — | Protected from Auto-Sell |
| **Universal** | `1-in-2,500,000` | `0.00004%` | — (Luck-scaled) | — | Protected from Auto-Sell |
| **Nexus** | `1-in-25,000,000` | `0.000004%` | — (Luck-scaled) | — | Protected from Auto-Sell |

- **Top-Down Probability Cap:** Each non-Common tier `i` uses `p_i = min(cap_i, base_i * EffectiveLuck)` (`Nexus` capped at `0.05`, `Universal` at `0.10`, `Secret` at `0.15`, `Mythic` at `0.25`, `Legendary` at `0.40`, `Epic` at `0.50`, `Rare` at `0.60`, `Uncommon` at `0.70`), consuming from remaining probability budget `1.0` top-down so probabilities always sum to exactly `1.0` without distorting high-tier scaling.
- **Pity Reset:** Pulling `Epic` resets only the `Epic` pity counter; pulling `Legendary` resets `Legendary` + `Epic` counters; pulling `Mythic+` resets `Mythic`, `Legendary`, and `Epic` counters.

### 3.3 Mutations (5)

| Mutation | Stat Multiplier | Base Weight | With `Mutation & Shiny Hunter` (`2.0x`) | During `Shibuya Eclipse` (`2.5x` R/V) |
| :--- | :--- | :--- | :--- | :--- |
| **Normal** | `1.00x` | `92.00%` | `84.00%` | `90.95%` |
| **Shiny** | `1.50x` | `5.50%` | `11.00%` | `5.50%` |
| **Golden** | `2.25x` | `1.80%` | `3.60%` | `1.80%` |
| **Rainbow** | `4.00x` | `0.60%` | `1.20%` | `1.50%` |
| **Void** | `7.50x` | `0.10%` | `0.20%` | `0.25%` |

---

## 4. Stacked Luck & Roll Cadence Formulas

### 4.1 Effective Luck Formula

```text
EffectiveLuck = (1.0 + FlatLuck) * M_Passes * M_Platform * M_Potion * M_Weather * M_Rebirth * M_Milestone * M_Ticket
```

- **`FlatLuck` (Additive):** Group Join (`+1.0`), Card Index Milestones (`+0.3` per 10% World tier, max `+3.0`), Equipped Charms (2 slots, `+1.0` to `+15.0` each), Skill Tree Fortune (`+0.5` to `+5.0`), Guild/Party (`+0.2` to `+0.5`).
- **`M_Passes`:** Product of owned passes: `Lucky` (`1.5x`) × `2x Luck` (`2.0x`) × `Mega Lucky` (`3.0x`) -> up to `9.0x`.
- **`M_Platform`:** `Roblox Premium` (`1.15x`) × `VIP` (`1.20x`) -> up to `1.38x`.
- **`M_Potion`:** `Minor` (`1.5x`, 10m), `Major` (`2.5x`, 15m), `Super` (`4.0x`, 20m), `Boss Luck` (`6.0x`, 10m) — duration extended `+10%` by Premium and `+15%` by VIP (additive `+25%`).
- **`M_Weather`:** `1.0x` baseline, `2.0x` when rolling in the active World Weather (10m cycle, 4m active).
- **`M_Rebirth`:** `1.0 + 0.15 * Rebirths` (cap `250` Rebirths -> `38.5x` max multiplier; scales cleanly without collapsing `Nexus` into common drops).
- **`M_Milestone`:** `5.0x` every 25th roll (W1 Boss), `20.0x` every 100th roll (W3 Boss), `100.0x` every 500th roll (W5 Boss).
- **`M_Ticket`:** `2.0x` when spending `Roll Tickets` (plus `1.5x` Novice Blessing active for the first 20 minutes of a new account).

### 4.2 Roll Speed & Multi-Pull

| Mode | Base Cooldown | With `Author's Note` + Skill Tree (`-35%`, min `0.30s`) | Cards / Cycle |
| :--- | :--- | :--- | :--- |
| **Manual / Auto (F2P)** | `1.60s` | `1.04s` | `1` (`5` when spending `Roll Tickets`) |
| **Fast Roll Pass** | `0.60s` | `0.39s` | `1` |
| **Fast Roll + Multi-Roll (5x)** | `0.60s` | `0.39s` | `5` (`~12.8 cards/sec` max) |

---

## 5. Card Power Scaling & Progression Sinks

### 5.1 Card Stat Formula (Big Number Formatting)

```text
CardPower = BasePower(Rarity, World) * 1.045^(Level - 1) * 1.20^(Stars) * MutationMult * TraitMult * (1 + 0.10 * Rebirths) * SkillTreeMastery * BondMult
```

| Rarity | W1 Base Power | W5 Base Power | Max Lv.100 ★5 Void (R=250) Sample |
| :--- | :--- | :--- | :--- |
| **Common** | `100` | `1,600` | `~18.1M` |
| **Rare** | `650` | `10,400` | `~118M` |
| **Epic** | `2,000` | `32,000` | `~362M` |
| **Legendary** | `7,500` | `120,000` | `~1.36B` |
| **Mythic** | `30,000` | `480,000` | `~5.43B` |
| **Secret** | `140,000` | `2,240,000` | `~25.3B` |
| **Universal** | `750,000` | `12,000,000` | `~136B` |
| **Nexus** | `4,500,000` | `72,000,000` | `~815B` (4-card Deck + Bonds/Traits -> `4.5T–12Qa`) |

### 5.2 Card Leveling (`Lv. 1–100`) & Dupe Ascension (`★0–★5`)

- **Level Cost (`Yen`):** `Cost(Lv) = floor(80 * 1.09^(Lv - 1) * WorldTier)` (`Lv.1->2` = `80 Yen`; `Lv.1->50` cumulative = `~65K Yen`; `Lv.1->100` cumulative = `~4.9M Yen` in W1).
- **Dupe Ascension:** `★1`: `1 dupe` · `★2`: `2 dupes` · `★3`: `4 dupes` (`+1` Ability Rank) · `★4`: `7 dupes` · `★5`: `12 dupes` (`+1` Ability Rank). Total `26` dupes to max a card.

### 5.3 Rebirth Cost & Skill Tree Curve

- **Rebirth Cost (`Gems`):**
  `RebirthCost(R) = floor(25,000 * 1.18^min(R, 30) + max(0, R - 30) * 150,000)`
  - `R=0 -> 1`: `25,000 Gems`
  - `R=1 -> 2`: `29,500 Gems`
  - `R=4 -> 5`: `48,470 Gems`
  - `R=30 -> 31`: `3.58M Gems`
  - `R=249 -> 250`: `36.58M Gems`
- **Early Skill Tree Budget (Before Rebirth 1):** Players typically spend `~1,800 Gems` across Tier 1–2 Fortune/Mastery/Efficiency nodes (`150–450 Gems/node`) to speed up World 2–3 clears and Tower Floor 30.

---

## 6. F2P Progression & Gem Pacing Breakdown (Hours 0–3)

| Source | Time Window | Gems Earned | Yen Earned | Roll Tickets |
| :--- | :--- | :--- | :--- | :--- |
| Tutorial + Launch Codes (`LAUNCH100`, `LEAFSHINOBI`) | `0–5m` | `1,500` | `3,000` | `25` |
| World 1 Acts 1–5 + Boss | `2–25m` | `2,800` | `65,000` | `35` |
| Playtime Rewards (`5m–120m`) + Login Day 1 | `5–120m` | `3,200` | `40,000` | `30` |
| World 2 Acts 1–5 + Boss | `25–75m` | `4,200` | `280,000` | `20` |
| Tower Floors 1–30 (incl. Boss Floors 10, 20, 30) | `40–130m` | `5,400` | `120,000` | `25` |
| World 1–2 Infinite Waves + Index Milestones (10–40%) | `60–155m` | `4,600` | `320,000` | `15` |
| Daily Quests (Day 1) + 5 Async Ranked Matches | `10–170m` | `5,110` | `90,000` | `20` |
| **Total Earned by `170 min` (`2.83 h`)** | **`0–170m`** | **`26,810` (`-1,800` Skill Tree = `25,010` net)** | **`918,000`** | **`170`** |

---

## 7. Robux Value-Per-Product Analysis

| Product | Type | Draft Price | 3-Hour Equivalent Output | Value Ratio vs Base Gem Pack (`12.12 Gems/R$`) |
| :--- | :--- | :--- | :--- | :--- |
| **Shinobi Starter Bundle** | Dev Product (1x) | `149 R$` | `2,500 Gems` + `25 Tickets` (`2,500 Gem eq.`) + `2x Major Potions` (`900 eq.`) + `Epic+` (`5,900 Gem-eq` total) | **`3.27x` (`39.60 Gem-eq/R$` — Best Onboarding Anchor)** |
| **Pouch / Chest / Vault / Treasury of Gems** | Dev Product | `99 / 249 / 499 / 999 R$` | `1,200 / 3,500 / 8,000 / 20,000 Gems` (`12.12 / 14.06 / 16.03 / 20.02 Gems/R$`) | `1.00x -> 1.65x` bulk scaling |
| **Lucky (`1.5x`)** | Gamepass | `99 R$` | `+50%` effective rarity yield forever (`~105 extra rolls eq./hr`) | Long-term compounder (breaks even < 2.5h) |
| **2x Luck (`2.0x`)** | Gamepass | `149 R$` | `+100%` effective rarity yield forever | Strongest mid-tier luck/R$ (`1.34x` luck/100 R$) |
| **Mega Lucky (`3.0x`)** | Gamepass | `199 R$` | `+200%` effective rarity yield forever | Highest single-pass luck jump (`1.51x` luck/100 R$) |
| **Fast Roll (`0.6s`)** | Gamepass | `129 R$` | `2.67x` roll throughput (`2.67x` Soft/Hard Pity speed) | Best QoL time-saver (`~2.07x` speed/100 R$) |
| **Auto Roll** | Gamepass | `149 R$` | Unlocks hands-free rolling + Auto-Sell/Ascend filters | Essential AFK QoL |
| **Multi-Roll (5x)** | Gamepass | `299 R$` | `5.0x` cards/pull (`13.3x` combined with Fast Roll) | Endgame banner-clearing powerhouse |
| **VIP** | Gamepass | `349 R$` | `1.20x` Luck, `+15%` Potion duration, `+25%` Yen/Gems, `100 Gems/day` | Accelerates First Rebirth by `~25%` |
| **Season 1 Pass (Premium Track)** | Dev Product | `399 R$` | `6,000 Gems` + `40 Tickets` + `35 Trait Crystals` + Guaranteed `Mythic` | `~2.4x` total value across 4-week season |

---

## 8. AnalyticsService Instrumentation Contract

All currency grants and sinks emit `AnalyticsService:LogEconomyEvent` with standardized item categories:
- `FlowType`: `Source` | `Sink`
- `CurrencyType`: `"Yen"` | `"Gems"` | `"RollTickets"` | `"TraitCrystals"` | `"BossTokens"` | `"RankedMedals"`
- `TransactionType`: `"BannerRoll"` | `"CardLevel"` | `"CharmCraft"` | `"Rebirth"` | `"SkillTree"` | `"StoryClear"` | `"TowerClear"` | `"RaidClear"` | `"QuestReward"` | `"IAP"` | `"CodeRedeem"`
- Onboarding Funnel (`LogFunnelStepEvent`): `1. Join` -> `2. First Roll` -> `3. Equip Active Deck` -> `4. Clear Act 1` -> `5. First Legendary` -> `6. Enter Tower` -> `7. First Rebirth`.

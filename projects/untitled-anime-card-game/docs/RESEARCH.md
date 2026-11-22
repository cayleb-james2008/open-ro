# Reference Game Research — Untitled Anime Card Game (UACG)

## 1. Research Summary & Methodology

Live Roblox API metadata, store descriptions, thumbnails, and community wiki documentation were gathered for both target benchmark titles (`refs/10774333764/` and `refs/10092619788/`). This document captures observed mechanics, monetization benchmarks, retention hooks, and design lessons to synthesize into **Untitled Anime Card Game (UACG)**.

---

## 2. Benchmark 1: Anime Nexus RNG X : GEAR 5

- **Place ID:** `85829558919037` | **Universe ID:** `10774333764`
- **Creator:** An Sample anime Game | **Genre:** Simulation (`anime-rng-cards`)
- **Live Metrics (snapshot):** ~8,000 concurrent players · 2.53M visits · 30k favorites · 93.7% like ratio (4,767 up / 319 down)
- **Local Evidence:** [refs/10774333764/README.md](../refs/10774333764/README.md) and `refs/10774333764/thumb_1.png`–`thumb_6.png`

### Core Systems Observed
1. **1-in-N RNG Rolling & Rarity Ladder:**
   - Players roll anime units across an escalating rarity ladder from `Common` up to ultra-rare `Nexus` tier (e.g., Kurumy / Gear 5 forms).
   - Unit evolution paths allow iconic transformations (e.g., Timeskip -> Sun God).
2. **Traits & Mutations:**
   - Units roll rare visual/stat variants (`Shiny` and `Mutated` variants; ~57% of badge-tracked players obtain a Shiny and ~42% obtain a Mutated unit organically without paid hunter passes).
   - Rerollable Traits add endgame optimization sinks beyond raw rarity.
3. **Stacked Luck Ecosystem:**
   - Luck is the primary chase stat: combines **Potions**, **Rebirths**, **Guild Luck** (`x1.5` 1st, `x1.3` 2nd, `x1.2` 3rd), and **3 tiered Luck Gamepasses** (`Lucky` 99 R$ for `x1.5`, `Super Lucky` 149 R$ for `x2`, `MEGA LUCKY` 199 R$ for `x3`).
4. **Rebirth Prestige Loop:**
   - Rebirthing costs scaling **Gems**, permanently boosts **Luck**, preserves **Units, Items, and Stat Upgrades**, and resets **Money, Gems, and Story stage position** (cap raised to 250 Rebirths).
   - *Friction note:* Players must remember to spend Money on stat upgrades and disable auto-upgrade before Rebirthing to avoid wasted currency.
5. **World Structure & Endgame Modes:**
   - Worlds feature **6 Story Acts + Infinite Mode** (e.g., *Blossom Capital*), **Raids** (*Skull Island*), **Trials** (*Sun God's Trial*), **Endless Wave / Boss Rush**, and **8-player free custom servers**.
   - Frequent code drops (`ONEPIECEUPDATE`, `GEARFIVE`, etc.) tied to like milestones drive social conversion.
6. **Monetization Profile (16 Gamepasses):**
   - Ranges from **40 Robux** (*Extra Unit Storage*) to **499 Robux** (*Shiny Hunter*), with `Mutation Hunter` (399 R$), `MEGA LUCKY` (199 R$), `Super Lucky` (149 R$), `Lucky` (99 R$), plus QoL speed/multi-roll/remote-access passes.

---

## 3. Benchmark 2: Anime Card Battles: Rebirth

- **Place ID:** `130253630950453` | **Universe ID:** `10092619788`
- **Creator:** ACBX Team | **Genre:** Simulation (`anime-rng-cards`)
- **Live Metrics (snapshot):** ~480 concurrent players · 15.13M visits · 10k favorites · 96.9% like ratio (24,324 up / 785 down)
- **Local Evidence:** [refs/10092619788/README.md](../refs/10092619788/README.md) and `refs/10092619788/thumb_1.png`–`thumb_6.png`

### Core Systems Observed
1. **Themed Card Packs / Banners:**
   - Eight themed Card Packs (`Normal`, `Spooky`, `Manhwa`, `Fantasy`, `Dark`, `Legends`, `Heroine`, `Sensei`) gate character pools by theme, with an Extra Card Pack gamepass to mix/access additional packs.
2. **Active Deck vs. Defense Deck Auto-Battler:**
   - Players assemble an **Active Deck** for offense/PvE/PvP attacks and a separate **Defense Deck** that represents them asynchronously when challenged.
   - Auto-battles evaluate huge formatted stats (`HP`, `ATK`), card borders (`Rainbow`, `Universal`), and **counter-abilities** (dodge, freeze, stun, reflect, pierce, resurrect).
3. **100-Floor Tower (`Heavens Arena`):**
   - Linear floor climb (`Floor 1`–`Floor 100`) acting as a deterministic gear-check ladder. Clearing Floor 100 unlocks the **Strange Trader**, gating endgame items and Boss Potions.
4. **World Bosses with Anti-Stat Mechanics & Permanent Unlocks:**
   - Bosses (*Galactic Tyrant*, *Shinobi God*, *King of Curses*, *Cifer*, *Wicked Weaver*, *Cosmic Menace*) punish pure brute-force decks using stat shred, ability mirroring, stun-on-hit, and multi-phase revives.
   - Defeating bosses unlocks systemic upgrades: Rainbow/Universal borders, Support Cards, Potion Shop, Card Fusions, and milestone luck (`5x`/`20x`/`100x` Luck every `N` rolls).
5. **Charms Crafting:**
   - Crafted from duplicate rolled cards + potions at **The Merchant** (town) and **Strange Trader** (Floor 100). Examples: *Author's Note* (`+15 Luck`), *Champion's Crown* (`+10 Luck`), *Holy Grail* (`+7 Luck`), *Stand Arrow* (`+5 Luck`, `-15% roll cooldown`), *Samehada* (`+3 Luck`, `-15% roll cooldown`), *Cursed Finger* (`+2 Luck`), *Legendary Stick* (`+1 Luck`), *Shuriken* (`-10% roll cooldown`).
6. **Async Ranked PvP & Platform Perks:**
   - **Ranked Arena** pits players against opponent Defense Decks using **Elo rating**, seasonal rewards, and rank decay above Masters (requiring weekly activity).
   - Cross-platform support (Desktop, Mobile, Tablet, Console), **Roblox Premium** perk (bonus luck + `10%` potion duration stacking with VIP), and **Group Join** (`+1 Luck`).

---

## 4. Competitive Synthesis & Design Opportunities for UACG

| Dimension | Anime Nexus RNG X | Anime Card Battles: Rebirth | UACG Hybrid Target |
| :--- | :--- | :--- | :--- |
| **Rolling Loop** | 1-in-N up to Nexus; Fast/Auto/Multi-Roll | Card Packs + Roll Cooldown Charms + Weather | 1-in-N World Banners with Soft/Hard Pity, Fast/Auto/Multi-Roll, and Charm cooldown reduction |
| **Card Depth** | Evolutions, Traits, Shiny/Mutations | Borders, Support abilities, Charms | Card Levels (1–100), Dupe Ascension (★0–★5), 5 Mutations, 12 Traits, 2 Charm Slots |
| **Combat** | Wave/Tower-Defense placement | Deterministic Auto-Battle (Active + Defense Decks) | Animated 3D/2.5D Card Auto-Battles with Role/Show Bonds, formatted big numbers, clear combat log |
| **Modes** | 6 Acts + Infinite, Raids, Trials, Boss Rush | World Portals, 100-Floor Tower, Bosses, Ranked Arena | 5 Worlds (5 Acts + Boss + Infinite), 100-Floor Tower, 2 Co-op Raids, Rotating World Bosses, Async Ranked PvP |
| **Prestige** | Rebirth up to 250 (risks accidental currency loss) | World Boss permanent unlocks | Guided Rebirth (clear preview of kept vs. reset + auto-spend prompt) + 3-branch Skill Tree |
| **UX & Safety** | High VFX density, dense HUD | Card-centric HUD, Premium + VIP stacking | Crisp anime card showcase, Settings-only Light/Dark toggle, plain labels, **no trading at launch** |

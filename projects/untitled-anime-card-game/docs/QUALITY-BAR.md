# Launch Quality Bar — Untitled Anime Card Game (UACG)

> **Document Status:** Design Milestone 1 (Canonical Reference)
> **Target Standard:** *Anime Nexus RNG X* (`85829558919037`) kinetic polish + *Anime Card Battles: Rebirth* (`130253630950453`) combat clarity
> **Release Gate:** All **8 dimensions** must score **`>= 4 / 5`** during final pre-launch verification.

---

## 1. Scoring Rubric (1–5)
| Score | Definition | Release Status |
| :---: | :--- | :--- |
| **1** | Broken, placeholder visuals, severe lag, confusing labels, or soft-locks | **Blocked** |
| **2** | Functional prototype, weak feedback, cramped mobile layout, unclear math | **Blocked** |
| **3** | Playable baseline, occasional jank, inconsistent contrast or pacing | **Hold — Polish Required** |
| **4** | Crisp, responsive, cohesive anime presentation, measurable targets met | **Launch Ready** |
| **5** | Best-in-class Roblox anime RNG battler feel; smooth 60 FPS & zero friction | **Flagship** |

---

## 2. Eight Launch Quality Dimensions & Measurable `>= 4` Criteria

### 1. UI & Visual Polish (Target: `>= 4 / 5`)
- **Criteria:**
  - Unified typography hierarchy (`1–2` clean sans/display weights), crisp rarity borders, and legible formatted numbers (`1.25K`, `42.8M`, `1.90B`, `3.15T`, `8.40Qa`).
  - **Light/Dark theme toggle lives exclusively in Settings** (`ThemeController`), with `>= 4.5:1` text contrast in both themes and zero theme buttons cluttering the main HUD.
  - **Plain user-facing wording** on every button, tooltip, and modal (`Roll`, `Deck`, `Ascend`, `Trait`, `Rebirth`)—zero internal/dev jargon.
  - **Pre-Rebirth Confirmation Modal** clearly contrasts **Kept** vs. **Reset** items and includes a 1-click **Auto-Spend Leftover Yen** button.

### 2. World, Hub & Models (Target: `>= 4 / 5`)
- **Criteria:**
  - Cohesive central **Anime Hub** with 5 distinct World Portals (*Hidden Leaf Village*, *West City & Namek*, *Grand Line Seas*, *Tokyo Jujutsu High*, *Soul Society*), Banner Pedestals, Charm Crafter (*The Merchant* / *Strange Trader*), Tower Gate, Raid Portal, Ranked Statue, and Live Leaderboards.
  - Clear spatial sightlines from spawn: first-time players see the **World 1 Banner** and **Act 1 Gate** within `2` seconds of joining without needing map arrows.
  - Stylized character standees/boss models with distinct silhouettes and idle animations.

### 3. VFX & Animation (Target: `>= 4 / 5`)
- **Criteria:**
  - Every rarity tier (`Common` through `Nexus`) has a distinct pull reveal sequence; `Legendary+` pulls trigger camera zoom/shake, pillar beams, and particle bursts.
  - All **5 Mutations** (`Normal`, `Shiny`, `Golden`, `Rainbow`, `Void`) are visually unmistakable in both card frames and inventory thumbnails.
  - **Weather Events** (5 distinct server skyboxes + ambient particles) transition smoothly over `1.5s` without popping or blinding combat readability.

### 4. Roll Feel & Dopamine (Target: `>= 4 / 5`)
- **Criteria:**
  - Manual Roll (`1.6s`), Fast Roll (`0.6s`), and Auto Roll loop seamlessly with zero dropped inputs or UI lockups.
  - Input-to-server-response latency `< 150ms` under normal load; server-authoritative result renders immediately with synchronized audio stingers.
  - **Auto-Sell / Auto-Ascend filters** never discard locked, favorited, or `Secret+` cards and display a clean toast/log summary.
  - Soft/Hard Pity counters and stacked **Effective Luck** breakdown are visible directly on the Banner screen.

### 5. Battle Feel & Readability (Target: `>= 4 / 5`)
- **Criteria:**
  - **Active Deck (4)** vs. **Defense Deck (4)** configuration clearly shows Frontline (`2`) / Backline (`2`) slots and live **Show/Duo Bond** activation badges.
  - Deterministic battle replay supports **`1x` and `2x` speed** plus **Skip** (for cleared Acts), with floating damage numbers, status icons (`Stun`, `Freeze`, `Silence`, `Burn`, `Bleed`, `Shield`, `Reflect`), and turn-order pips.
  - Boss anti-stat mechanics (e.g., *Reflect*, *Skill Nullify*, *Adaptation*) display clear warning telegraphs before combat so players understand why a counter-card or trait helps.

### 6. Progression Clarity (Target: `>= 4 / 5`)
- **Criteria:**
  - New player reaches **First Roll within `30s`**, clears **Act 1 within `2 min`**, pulls **First Legendary under `15 min`**, and reaches **First Rebirth in `2–4 hours`** (verified by `tools/econ-sim/sim.luau`).
  - **Card Index** shows exact completion `%` per world and exact permanent `+Luck / +ATK% / +HP%` rewards unlocked.
  - Active Quests, Login Streak, Playtime Timer, and Next Milestone Tracker remain glanceable in `<= 2` clicks.

### 7. Mobile & Responsive Layout (Target: `>= 4 / 5`)
- **Criteria:**
  - Full playability on phone (`375x667` / `844x390` landscape), tablet, and desktop (`1920x1080`) with zero clipped text or off-screen close buttons.
  - All primary touch targets (Roll button, Deck cards, Modal close `X`, Auto-Roll toggle) measure **`>= 44x44 px`** equivalent scaled UI.
  - Safe-area insets respected on notched devices; HUD occupies `<= 22%` of viewport during hub exploration and rolling.

### 8. Performance & Stability (Target: `>= 4 / 5`)
- **Criteria:**
  - Client maintains **`>= 55–60 FPS`** on desktop and **`>= 30–45 FPS`** on mid-tier mobile during `5x` Multi-Roll + active Weather VFX + Auto-Battle.
  - Server memory stays flat across `1,000+` rolls/player/hour; zero `ScriptTimeout` or `DataStore` throttling errors (ProfileStore session-locking + queued saves).
  - `luau-analyze` / `project_check` returns **0 errors**; `AnalyticsService` economy & onboarding funnel events fire cleanly without leaking secrets or PII.

---

## 3. Pre-Launch Signoff Checklist
- [ ] `docs/GDD.md`, `docs/ROSTER.md`, `docs/ECONOMY.md`, `docs/MONETIZATION.md`, and `docs/QUALITY-BAR.md` share identical world, banner, rarity, mutation, currency, and product names.
- [ ] `tools/econ-sim/sim.luau` exits `0` and verifies `< 15m` First Legendary and `2–4h` First Rebirth.
- [ ] No player trading UI or server endpoint exists at launch.
- [ ] Light/Dark toggle exists in Settings only; zero internal jargon in player-facing text.
- [ ] Every Robux price in `openro.project.json` keeps `priceApproved: false` until Owner approval.

# Production Taste Dossier — Untitled Anime Card Game

> Top-Trending Roblox Craft & Anti-Slop Quality Bar (`roblox-market-taste`) anchored by live anime card-battler market leaders (*Anime Card Battles*, *Anime RNG / Card Clash* niche) and visual reference inspection (`ref-1..8.png`).

## 1. Market Benchmark & Positioning
- **Niche:** `anime-rng-cards` (Anime Card Collection RNG + Tactical 4v4 Auto-Battler)
- **Visual Tone:** Sunlit, high-energy shonen arena spectacle — golden sunlight, cerulean sky, saturated elemental glow, comic-style bold frames, and instantly recognizable anime title epithets.
- **Core Rule ("Similar But Different"):** Preserve the addictive Roll -> Keep/Equip -> Build 4-Slot Team -> Clear Tower/Bosses/Raid loop while elevating 3D world lighting, HUD hierarchy, and active combat timing.

## 2. The 8 Production Dimensions

1. **Gameplay Loop:**
   - Instant Roll (`1x`, `Fast`, `Auto-Roll`, `50-Roll Mythic+ Pity`) across 6 themed Packs -> auto-shard/delete dupes -> upgrade Luck/Roll Speed/Inventory Cap -> equip 4-slot Elemental Synergy Deck -> clear 5 Story Bosses, Infinite Tower floors (`50+` scaling floors), Timed World Raid Boss, and PvP Duels.
2. **UI/UX Style:**
   - Compact edge-anchored HUD bars (`TopStatusBar`: Coins, Shards, Luck, Level/Power/Floor, Weather badge, Raid Countdown; `BottomDock`: Pity gauge, Pack cycle, Roll/Fast/Auto, 5 Drawers).
   - **Strict Modal Layering:** Dim background backdrop cleanly and hide `BottomDock` whenever the Summon Cutscene or Drawers open (`ZIndex` separation so action buttons never bleed behind reveal cards).
3. **Animations & Juice:**
   - Radial rarity shockwave + `Back.Out` card flip reveal tween on Roll; sliding card lunges, floating `CRIT!` / `OVERDRIVE +35%!` popups, skill cut-in banners, and variable `1x/2x/3x` replay speed in 4v4 clashes.
4. **3D World Style (Anti-Slop Sunlit Standard):**
   - **Sunlit Vibrant Anime Floating Archipelago** (`ClockTime = 13.5`, clear cerulean `Atmosphere` `Color/Decay`, lush emerald `Grass` terraces, sculpted terracotta/rock cliff undersides + floating satellite sky islets, warm cream `Marble` courtyard plaza, bright turquoise `Glass` koi moat, vermilion Japanese Torii Summon Shrine with translucent `0.76` light beam).
   - Reserve dark crimson/indigo sky lighting **strictly** for dark weather events (`Blood Moon`, `Eclipse 2x Luck`, `Crimson Eclipse`, `Celestial Tempest`).
   - Custom Blender 5.2.2 LTS `.obj` hero props (`<6k` tris, UV-unwrapped, `1m = 3.57 studs`): `summon_torii_gate`, `card_pedestal`, `boss_colosseum_arch`, `alchemist_cauldron`.
5. **Character Naming & Portrait Fidelity (`#352`):**
   - Portraits must clearly depict recognizable anime hero silhouettes without trademarked proper names on UI labels — replaced with iconic character title epithets:
     - `Golden Ascended Warrior` (`Secret · 1 in 50,000`)
     - `Six-Eyes Sorcerer` (`Mythic · 1 in 3,500`)
     - `Shadow Monarch` (`Mythic · 1 in 3,500`)
     - `Joyful Pirate` (`Mythic · 1 in 3,500`)
     - `Nine-Tail Shinobi` (`Mythic · 1 in 3,500`)
     - `Thunder Flash Blade` (`Legendary · 1 in 400`)
     - `Gravity Ronin` (`Epic · 1 in 60`)
     - `Steel Alchemist` (`Rare · 1 in 12`)
6. **Audio Cadence:**
   - Dedicated PCM16 44.1 kHz SFX cues routed through `ClientAudio`: tactile `ui_click`, crisp `summon_roll` chime, brass/synth `mythic_fanfare` sting, punchy `battle_crit` impact, and sparkling `potion_drink` brew.
7. **Monetization & Retention Loops:**
   - Pack tier progression (`Starter Free` -> `Eclipse Legends`), Pity safety net (`50` rolls), timed Potions (`Luck I/II/III`, `Speed I/II`, `Weather Reroll`), Trait rerolling, Shard-crafted Charms, Milestone Rebirths (`+20%` Power & Luck), and World Raid hourly urgency.
8. **3 Automatic Taste-Skill Twists ("Similar But Different"):**
   - **Twist A — `CHAIN OVERDRIVE` Timing Burst:** Before Round 2 of a 4v4 clash, the server opens a `1.6s` `OverdriveWindow`; pressing `[SPACE]` or clicking the pulsing `CHAIN OVERDRIVE` banner triggers a **+35% Dual-Card Finisher** across all enemies.
   - **Twist B — `FUSION WEATHER STORMS`:** Every 4th cycle or `35%` Weather Reroll erupts into **Crimson Eclipse (`5.0x` Luck)** or **Celestial Tempest (`4.5x` Luck)** with synchronized deep atmospheric sky shifts and Mythic drop surges.
   - **Twist C — Living 3D Lead-Card Guardian Aura:** The equipped slot-1 hero hovers smoothly behind the player's shoulder (`EquippedGuardianAura`) with double-sided Nano Banana art, glowing elemental halo ring, `PointLight`, `ParticleEmitter`, `Highlight`, and `Heartbeat` bobbing motion.

## 3. Anti-Slop Craft Checklist
- [x] Sunlit vibrant sky (`ClockTime = 13.5`) & zero muddy olive horizon fog in Clear mode
- [x] Sculpted multi-tier floating island cliffs + emerald grass + cream marble (zero flat black slabs)
- [x] Upright `PartType.Cylinder` geometry (`Size=(H, D, D)`, Z+90 rotation) across every pillar & spire
- [x] Translucent Torii summon beam (`Transparency = 0.76`) keeping crimson Torii silhouette crisp
- [x] Non-overlapping HUD modals (`BottomDock` cleanly hidden while reveal cutscene or drawers open)
- [x] Real headless Roblox Studio verification (`0` script runtime errors + inspected PNG captures)

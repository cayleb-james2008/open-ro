# Untitled Anime Card Game

Sunlit anime floating-archipelago card RNG and 4v4 auto-battler built end-to-end in `openro-rs` (`v0.9.0`), adhering to the 8-dimension Production Taste Dossier ([`TASTE-DOSSIER.md`](./TASTE-DOSSIER.md)) and verified live in real headless Roblox Studio.

## Sunlit 3D Archipelago & Assets ([`UntitledAnimeCardGame.rbxlx`](./UntitledAnimeCardGame.rbxlx))

- **215-Part Sunlit Floating Archipelago (`Workspace.ApexLobby` & `Workspace.CardGallery`):**
  - **Floating Terraces & Cliffs:** Emerald grass archipelago (`IslandBaseTier1`), sculpted terracotta rock cliff undersides (`CliffTierUpper`, `CliffTierMid`, `CliffTierSpire`), 4 floating sky islets (`SkyIsletTurf1..4`), warm cream-marble plaza (`CourtyardMain`), and sunlit golden `SpawnPad`.
  - **Japanese Torii Summon Shrine (`North`):** Cream-marble dais, vermilion Torii gate, dark timber `Kasagi` wings, spinning rarity halos, and **translucent (`0.76`) `SummonLightBeam`** leaving the crimson gate clean and legible.
  - **Boss Raid Colosseum (`East`), Infinite Tower Spire (`West`), Alchemist Cauldron & Trait Forge (`Southwest`), 4v4 PvP Clash Stage (`Southeast`), Sakura Grove & Hall of Legends (`South`):** All 8 Google Nano Banana fan-art portraits illuminated on light-marble pedestals (`Decal` + `SurfaceGui` + `PointLight` + `ParticleEmitter` + `Highlight` + `BillboardGui`) with iconic anime title epithets (`Golden Ascended Warrior`, `Six-Eyes Sorcerer`, `Shadow Monarch`, `Joyful Pirate`, `Nine-Tail Shinobi`, `Thunder Flash Blade`, `Gravity Ronin`, `Steel Alchemist`).
  - **Sunlit Lighting Rig (`ClockTime = 13.5`):** Cerulean `ApexAtmosphere` (`Color` & `Decay`), `ApexSky`, `ApexBloom`, `ApexSunRays`, and `ApexColorCorrection` — reserving dark crimson/indigo sky states strictly for dark/fusion weather events.

- **Blender 5.2.2 LTS Hero Meshes ([`assets/meshes/`](./assets/meshes/)):** `summon_torii_gate`, `card_pedestal`, `boss_colosseum_arch`, `alchemist_cauldron` (`.obj`, `.mtl`, `.png` preview renders; `<6k` tris, UV-unwrapped, `1m = 3.57 studs`).
- **PCM16 Mono 44.1 kHz SFX ([`assets/audio/`](./assets/audio/)):** `summon_roll.wav`, `mythic_fanfare.wav`, `battle_crit.wav`, `potion_drink.wav`, `ui_click.wav`.

## Gameplay Mechanics & 3 Taste-Skill Twists (`3,900+` lines across 8 Luau files under [`src/`](./src/))

- `CardDatabase.luau` & `CombatMath.luau`: 24 cards across 6 Packs, 6 Rarity Tiers (`Common 1/2` -> `Secret 1/50,000`), 5 Traits, 4 Craftable Charms, 9 Weather States (`Clear`..`Starfall` + **2 Fusion Weather Storms**), 5 Story Bosses, Infinite Tower, and World Raid Boss.
- **`CHAIN OVERDRIVE` (`BattleEngine.server.luau` + `ApexClientHUD.client.luau`):** Server opens a `1.6s` `overdrive_window` before Round 2 of 4v4 clashes; pressing `[SPACE]` or clicking the pulsing Overdrive button unleashes a **+35% Dual-Card Finisher**.
- **`FUSION WEATHER STORMS` (`WeatherRaidService.server.luau`):** Cycles/rerolls into **Crimson Eclipse (`5.0x` Luck)** and **Celestial Tempest (`4.5x` Luck)** with synced dynamic sky atmosphere transitions.
- **Living 3D Lead-Card Guardian Aura (`GameCore.server.luau`):** Equips `EquippedGuardianAura` hovering smoothly behind the player's shoulder with double-sided Nano Banana art, elemental halo ring, `PointLight`, rising particles, `Highlight`, and `Heartbeat` bobbing.
- **Modal Layering:** `BottomDock` cleanly hides while the Summon Cutscene or Drawers are visible, preventing overlap behind modal buttons.

## Real Headless Roblox Studio Verification (`studio-captures/`)

Tested on `Xvfb :99` via Vinegar Flatpak + `~/.local/bin/roblox-studio-mcp` (`240` runtime workspace parts including avatar/guardian, `24` cards, `9` weathers, `0` Luau runtime errors):
- [`hub_overview.png`](./studio-captures/hub_overview.png) — Sunlit floating archipelago overview
- [`torii_shrine.png`](./studio-captures/torii_shrine.png) — Crimson Japanese Torii Shrine w/ translucent summon beam
- [`hall_of_legends.png`](./studio-captures/hall_of_legends.png) — Hall of Legends 8 Nano Banana epithet showcases
- [`boss_colosseum_pvp.png`](./studio-captures/boss_colosseum_pvp.png) — Boss Colosseum & 4v4 PvP Clash Arena
- [`tower_cauldron.png`](./studio-captures/tower_cauldron.png) — Infinite Anime Tower & Alchemist Pavilion
- [`playtest_reveal.png`](./studio-captures/playtest_reveal.png) — Summon Cutscene with `BottomDock` hidden (`dockHidden=true`)
- [`playtest_battle.png`](./studio-captures/playtest_battle.png) — Live 4v4 Battle Clash Arena & Skill Cut-In banner
- [`playtest_view.png`](./studio-captures/playtest_view.png) — Live Playtest Plaza w/ Living 3D Guardian Aura & HUD

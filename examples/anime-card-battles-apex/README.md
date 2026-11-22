# Anime Card Battles: Rebirth — Apex Edition

Complete, Roblox Studio-verified anime card RNG and 4v4 battler built in `openro-rs` (`v0.9.0` rebuild).

## Hub World & Assets (`AnimeCardBattlesApex.rbxlx`)

- **204-Part 3D Celestial Arena Island (`Workspace.ApexLobby` & `Workspace.CardGallery`):**
  - **Celestial Courtyard & Koi Moat:** Multi-tier courtyard floor (`IslandBaseTier1`, `CourtyardMain`, glowing `CenterEmblemFloor` neon emblem, corner orb pillars, koi moat channels, and 4 glowing radial stone bridges).
  - **Japanese Torii Summon Shrine (`North`):** Raised double dais, neon `RollPedestal` with `ProximityPrompt`, crimson Torii pillars & crossbeams, flared dark-slate `Kasagi` roof wings, spinning cyan/gold rarity halos, particle sparks, and vertical summon beam.
  - **Boss Raid Colosseum (`East`):** Tiered colosseum ring, 6 perimeter pillars, 4 glowing particle braziers, Champion Statue with golden halo & dual blades, `BossRaidShrine` prompt, and highlighted `DamagePillar` live raid `SurfaceGui`.
  - **Infinite Anime Tower Spire (`West`):** 38-stud luminous cyan spiral spire, 12 glowing spiral steps, and 5 color-coded World Portal gates (`Ember Harbor F1+` through `Celestial Apex F41+`).
  - **Alchemist Potion & Trait Forge Pavilion (`Southwest`):** Timber & lantern pavilion housing the bubbling `AlchemistCauldron` and crystalline `TraitForgeAnvil`.
  - **4v4 PvP & Clash Duel Arena (`Southeast`):** Dual opposing 4-card stage pads, neon `ClashCenterPlatform`, and tiered spectator bleachers.
  - **Hall of Legends (`South`) & Sakura Grove:** 8 illuminated pedestals displaying all 8 Google Nano Banana fan-art portraits (`Decal` + `SurfaceGui` `ImageLabel` + `PointLight` + `ParticleEmitter` + `Highlight` + `BillboardGui`), backed by the `LeaderboardWall` and 8 cherry-blossom lantern trees.
  - **Lighting Rig:** `Sky`, `Atmosphere`, `BloomEffect`, `SunRaysEffect`, and `ColorCorrectionEffect` dynamically synced by `WeatherRaidService`.

- **Blender 5.2.2 LTS Custom 3D Props ([`assets/meshes/`](./assets/meshes/)):**
  - Roblox-scaled (`1m = 3.57 studs`, `<6k` tris) UV-unwrapped Wavefront `.obj` + `.mtl` hero props and Blender Eevee preview renders:
    - `summon_torii_gate.{obj,mtl,png}`
    - `card_pedestal.{obj,mtl,png}`
    - `boss_colosseum_arch.{obj,mtl,png}`
    - `alchemist_cauldron.{obj,mtl,png}`

- **Synthesized PCM16 Mono 44.1 kHz SFX ([`assets/audio/`](./assets/audio/)):**
  - `summon_roll.wav`, `mythic_fanfare.wav`, `battle_crit.wav`, `potion_drink.wav`, `ui_click.wav` loaded via `StarterPlayerScripts.ClientAudio`.

## Luau Architecture (`3,833` lines across 8 modules/scripts under [`src/`](./src/))

- `ReplicatedStorage/Modules/CardDatabase.luau`: 24 original anime-homage cards across 6 Packs (`Starter`, `Shibuya Spirit`, `Crimson Pirate`, `Shadow Gate`, `Celestial Alchemy`, `Eclipse Legends`), 6 Rarity Tiers (`Common 1/2` -> `Rare 1/12` -> `Epic 1/60` -> `Legendary 1/400` -> `Mythic 1/3,500` -> `Secret 1/50,000`), all 8 Nano Banana portraits mapped, unique Active/Passive skills (`Counter`, `Lifesteal`, `Mirror Shield`, `Burn AoE`, `Guard Break`, `Stun`, `Time Stop`, `Domain Buff`), Traits (`Golden`, `Voidtouched`, `Overcharged`, `Swift`, `Immortal`), craftable Charms (`Samehada`, `Spirit Arrow`, `Gravity Core`, `Eclipse Crown`), 7 Weather biomes (`Clear`, `Rain`, `Snow`, `Sandstorm`, `Blood Moon`, `Eclipse`, `Starfall`) with weather-locked cards, and 5 Story Bosses + World Raid Boss (`Abyssal Eclipse Leviathan`).
- `ReplicatedStorage/Modules/CombatMath.luau`: Deterministic card stat scaling, element wheel (`Solar > Shadow > Storm > Arcane > Crimson > Solar`), and damage formulas shared cleanly between server and client.
- `ServerScriptService/DataPersistence.server.luau`: Retry-hardened `DataStoreService` (`UpdateAsync` + `BindToClose`) with clean Studio-offline in-memory fallback.
- `ServerScriptService/WeatherRaidService.server.luau`: Cyclic 90s dynamic weather transitions, Studio lighting synchronization, weather-locked drop injection, and World Raid countdown/boss lifecycle.
- `ServerScriptService/GameCore.server.luau`: Rate-limited `RemoteFunction`/`RemoteEvent` handler supporting pack rolls (`Roll 1`, `Fast`, `Auto`), 50-roll Mythic+ Pity tracker, XP/upgrades (`+Luck`, `+Roll Speed`, `+Inventory Cap`), potions, codes (`REBIRTH`, `APEX2026`, `LUCKBOOST`), trait rerolls, charm crafting, card locking/auto-delete, 3D overhead equipped showcase cards, and live Hall pedestals.
- `ServerScriptService/BattleEngine.server.luau`: Server-authoritative 4v4 turn/tick auto-battle engine covering Story Bosses, Infinite Tower floors, World Raid Boss damage + Raid Shards, and PvP Duels, streaming `Lunge`, `SkillTrigger`, `Damage`, `Heal`, `KO`, and `Result` events.
- `StarterPlayer/StarterPlayerScripts/ClientAudio.client.luau` & `ApexClientHUD.client.luau`: Top currency/luck/weather/raid countdown HUD, bottom summon dock with Pity gauge, `TweenService` gacha summon cutscene, 4-slot Deck Builder & Card Index, Trait & Charm Forge modal, Potion & Code brewer, Upgrades shop, and animated 4v4 Battle Clash board (`1x`/`2x`/`3x` speed).

- Synced `.rn` generator scripts (`src/*.rn` & `src/ideas.rs`) remain passing against Open-Ro's static Luau prover (`0` errors, `0` warnings) and `tests/acb_v060.rs` + `tests/acb_rebuild_v090.rs`.

## Real Roblox Studio Verification (`studio-captures/`)

Tested headlessly on `Xvfb :99` via Flatpak `org.vinegarhq.Vinegar` + `~/.local/bin/roblox-studio-mcp` (`list_roblox_studios`, `get_studio_state`, `search_game_tree`, `start_stop_play`, `execute_luau`, `get_console_output`, and `screen_capture`):
- Place loads cleanly in Edit and Playtest mode with `0` Luau script errors (`DataPersistence` logs its expected local-file Studio fallback notice cleanly).
- Real Roblox Studio viewport PNG captures under [`studio-captures/`](./studio-captures/):
  - `hub_overview.png` — Full 204-part Celestial Arena hub island
  - `torii_shrine.png` — Japanese Torii Summon Shrine & summon beam
  - `hall_of_legends.png` — 8 illuminated Nano Banana character pedestals & leaderboard wall
  - `boss_colosseum_pvp.png` — Boss Raid Colosseum & 4v4 PvP Clash Arena
  - `tower_cauldron.png` — Infinite Anime Tower spire & Alchemist Pavilion
  - `playtest_view.png` — Live Playtest HUD + triggered `RollRequest` summon cutscene

## Opening Locally in Roblox Studio

1. Open [`AnimeCardBattlesApex.rbxlx`](./AnimeCardBattlesApex.rbxlx) in Roblox Studio (or import into Open-Ro).
2. Copy [`assets/`](./assets/) (`*.png` and `audio/*.wav`) into Roblox Studio's `content/openro-assets/` folder for instant offline `rbxasset://openro-assets/` preview, or publish images/sounds via Asset Manager to replace with `rbxassetid://<id>`.
3. Reference details for all cards, meshes, and audio live in [`asset-manifest.json`](./asset-manifest.json).

# Anime Card Battles: Rebirth — Apex Edition (Built End-to-End in OpenRO v0.6.0)

This directory contains a complete, Roblox Studio-ready `.rbxlx` place and high-resolution card artwork bundle built end-to-end exclusively through `openro-rs 0.6.0`.

## Contents

- [`AnimeCardBattlesApex.rbxlx`](./AnimeCardBattlesApex.rbxlx): Genuine `<roblox version="4">` XML place file containing:
  - **12 server-authoritative & client Luau scripts** (verified by OpenRO's Static Luau Prover with `0` errors / `0` warnings):
    - `ServerScriptService.CardGachaRoller` (7 rarity tiers, 5 foil borders, Luck × Weather × Potion × Rebirth multipliers, pity counter, fast/auto roll)
    - `ServerScriptService.DeckBuilder` (6-slot deck, collection bonuses, 1★–5★ Star Fusion, Rebirth Ascension, `pcall`-guarded DataStore + `BindToClose`)
    - `ServerScriptService.CardBattleArena` (5-element synergy wheel, turn order, Ultimate Burst Gauge, Active Counter/Interrupt, 3-phase Impossible Boss Raid)
    - `ServerScriptService.TowerProgression` (50-floor Endless Tower across 5 World Portals + `OrderedDataStore` global leaderboard)
    - `ServerScriptService.PotionsWeather` (dynamic server weather lighting cycles, craftable Luck/Speed potions, redeemable codes `REBIRTH`/`APEX2026`/`LUCKBOOST`)
    - `ServerScriptService.CardGalleryWorld` (3D Studio Lobby courtyard, roll pedestal, showcase gallery, tower gate, boss shrine, alchemist stand, leaderboard wall)
    - `ServerScriptService.Leaderboard`, `ServerScriptService.DataStoreSave`, `ServerScriptService.Shop`, `StarterGui.GameGui`, `ServerScriptService.Soundtrack`, plus live LLM-added `ServerScriptService.JumpPadInCardLobby`
  - **3D Studio Lobby & Card Gallery Instances** (`Workspace.SpawnPad`, `RollPedestal`, `TowerPortal`, `BossRaidShrine`, `PotionStand`, and `Workspace.CardGallery.Showcase_*` stands with bound `Decal` `<Content name="Texture">` and `SurfaceGui` -> `ImageLabel` `<Content name="Image">` references)
- [`asset-manifest.json`](./asset-manifest.json): Portable `openro-asset-manifest-v1` manifest mapping every card illustration, SHA-256 digest, stats, passive/ultimate kit, and exact Roblox Studio `Decal`/`ImageLabel` binding paths.
- [`assets/`](./assets/): 8 high-resolution (`768x1024`) stylized anime card illustrations (original DMCA-safe creative homage archetypes):
  1. `supernova-ascended-striker.png` (Secret · Flame · 1 in 8,192)
  2. `infinity-void-sorcerer.png` (Celestial · Void · 1 in 2,048)
  3. `shadow-sovereign.png` (Celestial · Shadow · 1 in 2,048)
  4. `crimson-sea-captain.png` (Mythic · Flame · 1 in 512)
  5. `nine-tail-ember-shinobi.png` (Mythic · Flame · 1 in 512)
  6. `thunder-flash-ronin.png` (Legendary · Storm · 1 in 128)
  7. `blood-moon-gravity-ronin.png` (Epic · Void · 1 in 32)
  8. `eclipse-arcane-alchemist.png` (Rare · Arcane · 1 in 8)

## Opening in Roblox Studio

1. Open `AnimeCardBattlesApex.rbxlx` directly in Roblox Studio (or import it back into OpenRO via **Import .rbxlx**).
2. Upload the 8 PNG files in `assets/` through Roblox Studio's **Asset Manager -> Images**, and replace each `rbxasset://openro-assets/<filename>` URI (listed in `asset-manifest.json`) with your published `rbxassetid://<id>` — or copy `assets/` into Roblox Studio's local `content/openro-assets/` folder for instant offline preview.

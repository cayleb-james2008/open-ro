# World, 3D and VFX spec (prompt-ready, for OpenRO's agent)

Owner: world director. Source: `docs/GDD.md` section 6 (Nexus Plaza), section 3.3 to 3.9 (battle, worlds, spire, raid, ranked). The agent builds everything below; nothing here is hand-written in the game files. If the GDD changes, the GDD wins.

Quality bar: Anime Nexus RNG X. A first-time player should see a lit, layered, stylised hub with real modelled props, glowing portals, a roll stage that reacts, and fights with hit-stop, camera shake, element effects and a death shatter. Flat grey or untextured primitive parts anywhere in the hub is a fail.

## 0. Rules the agent must follow

1. Files: Luau under `src/server/WorldBuild/**` and `src/client/VFX/**`; meshes, textures and audio under `assets/{meshes,audio}/**`; Blender scripts under `tools/blender/`.
2. Units are 3D card standees plus full-screen 2D anime cut-ins. No rigged 3D characters. NPCs are static stylised meshes with a procedural idle (bob and head turn).
3. Roblox cylinder axis: `PartType.Cylinder` runs along local X. Upright column: `Size = (height, diameter, diameter)` and `CFrame * CFrame.Angles(0, 0, math.rad(90))`. Z-facing disc: `X = thickness`, rotate Y by 90 degrees. Always check a live `screen_capture` of the result; never trust the maths alone.
4. Mobile budget (StreamingEnabled is on): hub at most 1,800 instances and 250k triangles in the whole place; any single mesh at most 20,000 triangles (Roblox limit); at most 40 `ParticleEmitter`s alive at once; at most 12 point or spot lights, all `Shadows = false` except the sun; no `Future` lighting on mobile quality levels; decals and surface GUIs at most 1,024 px.
5. Everything built at runtime is parented under `Workspace.Hub` (static) or `Workspace.Fx` (temporary, cleaned by `Debris`). Anything created in a loop must be pooled or destroyed.
6. Respect reduced motion and flash settings from `Config.Settings` (GDD section 5): shake and flashes scale to 0 when off; particles drop to 30 percent.
7. Plain words on every sign and label. No internal jargon.
8. Mesh upload goes only through OpenRO's asset pipeline into `assets.lock.json` and `src/shared/AssetIds.luau`. Until the user's Open Cloud key exists, the build runs from local previews: the hub uses a mesh catalog (`WorldBuild/MeshCatalog.luau`) that maps a key to an asset id, and a part named after the key sized by the manifest bounding box when the id is still missing. The proxy must be visibly tagged (`Attribute Proxy = true`) so tests can count how many real meshes are in use.

## 1. Blender mesh pipeline (what the agent's mesh tool must produce)

OpenRO's current `blender_generate` has five fixed presets and writes a flat 180-triangle `.obj` with no UVs. That is not enough. The agent needs a tool that takes a structured description (or a Blender Python script it wrote) and returns, per mesh:

- Geometry in studs (1 unit = 1 stud), Y-up OBJ or FBX; bevelled, smooth-shaded where round, hard edges where architectural.
- A real UV unwrap (smart project, equal texel density, packed 0 to 1, margin at least 4 px).
- One painted texture per mesh (1,024 or 512 px PNG, under 1.5 MB), baked in Cycles from procedural materials: colour times ambient occlusion with a warm-violet shadow tint. Stone blocks, lacquered wood, gold trim, roof tiles, cloth, grass and cherry leaves must read as different materials at game distance.
- Glow parts (rune lines, crystals, lanterns, portal rims) exported as a separate mesh named `<key>_glow` with no texture; Luau sets `Material = Neon` and the colour.
- `manifest.json`: per mesh `size` (studs, X Y Z), `tris`, texture file, pivot (base centre), named anchors (for example `screen` rectangle for a banner portal, `disc` radius for a world gate).
- Preview renders (two angles) saved next to the mesh so a human can judge without Studio.
- A hard failure when a mesh is over 20,000 triangles or a texture is over 1.5 MB.

A reference recipe that already produces this exists at `~/Work/scratch/swarm-v011/world-prototype/` (read-only reference; do not copy files from it into the game; the tool must generate its own).

## 2. Mesh list (all textured, UV-unwrapped, baked)

| Key | What it is | Size in studs (X Y Z) | Triangle cap |
|---|---|---|---|
| `shrine_altar` | Three-tier round roll stage, gold ring channels, eight pillars with orbs, red pedestal in the middle where the roll orb floats | 30 x 7 x 30 | 6,000 |
| `shrine_torii` | Grand red torii gate behind the altar, gold plaque | 24 x 20 x 3 | 4,000 |
| `banner_portal` | Tall red frame on a stone plinth around a flat dark screen plate; the screen is a SurfaceGui for the banner art | 12 x 17 x 3 | 3,000 |
| `world_gate` | Standing stone ring on a plinth with gold inner rims and twelve rune blocks; the energy disc is added in Luau | 22 x 17 x 22 | 8,000 |
| `battle_arena` | Round ranked arena: sunken floor, tiered seats on one side, four banner poles, entrance arch | 70 x 16 x 70 | 15,000 |
| `tower` | Five-tier pagoda on a stone drum, gold finial (the Infinite Spire) | 26 x 52 x 26 | 8,000 |
| `raid_arena` | Colosseum arch plus broken ring wall with a boss throne and chains | 60 x 30 x 60 | 18,000 |
| `leaderboard_wall` | Wide wooden board wall with three framed screen slots and a lantern roof | 40 x 18 x 4 | 4,000 |
| `shop_stall` | Market stall with striped cloth awning, counter, crates, hanging lanterns | 14 x 11 x 10 | 5,000 |
| `npc_shopkeeper`, `npc_guide` | Chibi stylised static NPC (big head, hair mass, outfit), idle animated in Luau | 4 x 6 x 3 | 3,000 each |
| `cherry_tree_a/b` | Sakura tree with a layered pink canopy | 14 x 18 x 14 | 3,500 |
| `lantern_stone` | Stone toro lantern; glow part for the flame | 3 x 8 x 3 | 1,200 |
| `bridge_arch` | Red arched bridge between plaza islands | 8 x 5 x 28 | 2,500 |
| `rock_cluster_a/b/c` | Mossy rocks | 10 x 6 x 10 | 1,500 |
| `island_base_a/b` | Floating island underside with hanging roots and a grass top, plaza and satellite islands | 280 x 60 x 280 and 80 x 40 x 80 | 12,000 / 6,000 |
| `fountain` | Wishing Fountain: stone basin, koi statue, water ring (water is a Luau part) | 16 x 9 x 16 | 4,000 |
| `skill_monolith`, `rebirth_obelisk` | Two tall glowing monuments with carved faces and floating rings | 8 x 24 x 8 | 4,000 each |
| `card_standee` | The 3D card: thin slab with bevel, stone base, art face UV exactly 0 to 1 over the card area so the card art maps 1:1 | 5.6 x 8 x 0.6 | 800 |
| `card_standee_foil` | Same silhouette, slightly larger, for the foil layer | 5.8 x 8.2 x 0.7 | 800 |
| `quest_board` | Notice board with five parchment slots under a small roof (Quest board, GDD 6) | 12 x 10 x 3 | 2,500 |
| `stage_floor` | Battle stage floor: long stone platform with a painted ring, edge trim | 90 x 2 x 30 | 3,000 |
| `stage_backdrop` | Curved backdrop wall with six banner poles; neutral stone and cloth so Luau tints it per world and a SurfaceGui carries the world's key art | 100 x 30 x 8 | 5,000 |
| `stage_banner` | Hanging cloth banner, neutral, tinted per world | 6 x 14 x 0.5 | 600 |
| `guardian_fox` | Lobby companion: floating spirit fox with nine small tails, no legs, glow tail tips | 4 x 3 x 6 | 3,000 |

If an item above is not possible, the agent says so in its reply and the director files a gap. The agent must not substitute a block of primitive parts and call it done.

## 3. Nexus Plaza layout

Origin (0, 0, 0) is the plaza ground at the centre of the Roll Altar; every mesh pivot is its base centre, so a landmark at y = 0 sits on the ground. The altar's top step is at y = 3.3, its pedestal top at y = 6.1, and the roll orb centre floats at y = 11. Angles are measured from +X toward +Z (90 degrees is +Z, the side the leaderboard wall is on). Yaw 0 faces -Z; "toward X" below means the landmark's front faces that point. All coordinates in studs. The agent stores the layout as data in `WorldBuild/Layout.luau` (key, position, yaw, scale) so tests can read it.

| Landmark | Position | Faces |
|---|---|---|
| Roll Altar (`shrine_altar`) | (0, 0, 0) | south to north |
| Torii (`shrine_torii`) | (0, 0, -34) | toward altar |
| Banner portals x3 (current banners) | (-34, 0, -30), (0, 0, -52), (34, 0, -30) | toward altar |
| World gate 1 (Naruto) | (-86.6, 0, -50.0) | toward origin |
| World gate 2 (Dragon Ball) | (-25.9, 0, -96.6) | toward origin |
| World gate 3 (One Piece) | (25.9, 0, -96.6) | toward origin |
| World gate 4 (Jujutsu Kaisen) | (86.6, 0, -50.0) | toward origin |
| World gate 5 (Bleach) | (61.6, 0, 78.8) | toward origin |
| World gate 6 (locked, "Coming soon", dimmed and chained) | (-61.6, 0, 78.8) | toward origin |
| Quest board (`quest_board`) | (-62, 0, 52) | toward origin |
| Wishing Fountain | (-30, 0, 36) | open |
| Shop stalls x2 with NPCs (potions, gems) | (30, 0, 38), (48, 0, 22) | toward altar |
| Leaderboard wall | (0, 0, 70) | toward altar |
| Ranked arena (`battle_arena`) | satellite island centred (-120, 0, 40), bridge from (-70, 0, 20) | toward plaza |
| Infinite Spire (`tower`) | satellite island centred (120, 0, 40), bridge from (70, 0, 20) | toward plaza |
| Raid arena (`raid_arena`) | satellite island centred (0, 0, 150), bridge from (0, 0, 84) | toward plaza |
| Skill monolith, Rebirth obelisk | (-52, 0, 8), (52, 0, 8) | toward altar |
| Spawn | (0, 3, 26), looking toward -Z (the altar) | |
| Trees, lanterns, rocks | scattered by a seeded rule, not on walkways, at most 14 trees | |

Walkways are flat parts with a stone texture, 12 studs wide, from spawn to every landmark. Players must reach every landmark on foot from spawn in under 20 seconds at the default WalkSpeed of 16 studs per second (so every landmark path is at most 320 studs long; the farthest, the ranked arena, is about 150). Satellite islands are connected by `bridge_arch`.

## 4. Lighting and atmosphere

Build in code (`WorldBuild/Lighting.luau`), one function per time of day driven by the weather table (clear day, sunset, night, rain, Lucky Hour):

- `Lighting`: `Technology = ShadowMap`, `ClockTime` 16.5 default, `Brightness` 2.6, warm `Ambient` (150, 140, 170), `OutdoorAmbient` (170, 165, 200), `ColorShift_Top` warm gold, `EnvironmentDiffuseScale` 0.6, `EnvironmentSpecularScale` 0.4, `GeographicLatitude` 25, `ExposureCompensation` 0.2.
- `Atmosphere`: `Density` 0.28, `Offset` 0.25, `Color` pink-lavender (220, 200, 235), `Decay` (255, 190, 200), `Glare` 0.4, `Haze` 1.6.
- `Sky` with stars and a large moon in the night preset; no default Roblox sky.
- Post effects: `ColorCorrectionEffect` (Saturation +0.18, Contrast +0.08, TintColor slightly warm), `BloomEffect` (Intensity 0.55, Size 28, Threshold 0.9), `SunRaysEffect` (Intensity 0.12), `DepthOfFieldEffect` active only during the roll reveal (far intensity eased in and out).
- Presets crossfade over 4 seconds through TweenService, never snap.
- Ambient life: drifting sakura petals (one emitter on a ring above the plaza), fireflies at night, mist at island edges. Respect the emitter budget.

## 5. Interactive hub pieces (Luau, built on the meshes)

- Roll stage: orb floating above the pedestal (Neon sphere with inner glow), idle bob and spin. On roll start the orb charges (particles inward, orb shrinks and shakes), then the reveal fires the per-rarity effect (section 7). The client calls `RollVfx.play(rarityId, variant)` and gets a promise-like handle with `:onPeak(fn)` so the UI shows the card at the peak.
- World gates: energy disc (Neon, rotating via a UV-scrolling `Texture` or a spinning ring), name sign above (BillboardGui with the world name and the clear status), locked gate dimmed with chains. Proximity prompt opens the world map for that world.
- Banner portals: SurfaceGui on the screen anchor showing the live banner art, countdown and pity count.
- Leaderboard wall: three SurfaceGuis (Luck, Rebirths, Ranked), refreshed from the server snapshot, not per-frame.
- Wishing Fountain: water ring (Neon, transparency 0.4, slow material scroll), coin toss particles on use.
- NPCs: idle bob, head turn toward the nearest player in 12 studs, speech bubble billboard.
- Guardian fox: follows the local player at 6 studs behind and 4 above on a spring (no physics), reacts to rolls (spins and sparkles on Legendary and above). Cosmetic only; client side.

## 6. Card standee and foil

- `card_standee` shows the card art on the front, per-rarity frame behind it, base plate with a glow ring in the rarity colour. UV maps the face 1:1 so the same image id used on the 2D card works.
- Foil: a second mesh slightly in front using a scrolling gradient `Texture` (or `SurfaceAppearance` plus a tweened offset) for the shine sweep, only on Epic and above; Legendary and above add a particle ring at the base. Tilt follows the camera by at most 8 degrees so it looks holographic.
- API: `CardStandee.create(cardId, variant, parent) -> Model` and `CardStandee.setState(model, "idle" | "hit" | "dead" | "ready")`.

## 7. VFX library (`src/client/VFX/`)

One module per concern, all pure client, each with a single `play(...)` that returns a handle with `:Destroy()` and respects the budgets.

- `Shake`: camera shake by intensity and duration (damped noise on the camera offset, not on the CFrame itself).
- `HitStop`: freezes the two units involved for 60 to 90 ms on a heavy hit while the rest continues.
- `Motion`: procedural lunge toward the target, flip on a crit, hit shake on the victim, knock-back, idle bob, all through `TweenService` on the standee model pivot. No animations uploaded.
- `Hits`: slash, pierce, blunt, magic burst; spark, shard and ring emitters; floating damage numbers with crit and heal styles (BillboardGui, pooled).
- `Elements`: fire, water, lightning, wind, earth, ice, light, dark, spirit (nine). Each has a projectile or cast, an impact and a lingering status mark.
- `Ultimates`: a full cut-in sequence per card: dim the screen, 2D anime cut-in slides across (the image comes from the card's art ids), name banner, then the element finisher. Skippable, and skipped automatically at 4x speed.
- `Death`: standee shatters into 12 to 20 shards with an element-coloured dissolve, shards pooled.
- `Heal`, `Buff`, `Debuff`: simple rising motes and rings with the status icon.
- `RollVfx`: eight rarity presets. Common and Uncommon: a short burst. Rare: a coloured beam. Epic: beam plus a shockwave ring. Legendary: tall beam, two shockwaves, camera push and shake, bloom pulse. Mythic: pillar of light, sky flash, petals reverse. Secret: screen inversion for one frame, black hole implosion, then a huge burst. Transcendent: slow-motion sequence, full-hub lighting change to dark, a prism beam, and a world-wide ring. Each preset has a duration, a peak time (the `:onPeak` hook) and a rarity-coloured palette. Add a Shiny and Mutation overlay (extra particles in the variant colour).
- `Ambient`: sakura petals, fireflies, mist, rain, and the Lucky Hour sparkles; one `toggle(name, bool)` entry.

Each VFX module has a Lune or in-Studio test showing it constructs and cleans up (instance count returns to the starting count after `Destroy`).

## 8. Battle diorama

One reusable stage per world (GDD 6), built by `WorldBuild/BattleStage.luau` from `stage_floor`, `stage_backdrop` and `stage_banner` (neutral textures; the world's palette is a `Color` tint and the key art is a SurfaceGui on the backdrop). Ten standee slots in two rows of five: slots are 7 studs apart along X within a team, and the gap between the two teams' nearest slots is 14 studs. Camera three-quarter (about 35 degrees above the floor, 20 degrees yaw). Spire, Raid and Ranked reuse the stage with a different tint and backdrop art.

## 9. Acceptance (the playtester checks these; each row is measurable)

Interim means before the Open Cloud key exists and meshes are still proxies; final means after upload.

1. Capture: a Studio `screen_capture` from spawn shows the altar, torii and two world gates, lit, with bloom and atmosphere visible. Untextured grey: every `MeshPart` under `Workspace.Hub` has a non-empty `TextureID` (or a `SurfaceAppearance`), or is `Neon`; count of violations is 0. Interim: proxy parts (`Attribute Proxy = true`) are exempt but their count is reported; final: proxy count is 0.
2. Budget: `Workspace.Hub` descendants at most 1,800. Triangle estimate = sum over MeshParts of manifest `tris` for that mesh key; at most 250,000 (the cap sum of one of each mesh is about 220,000, so the layout must use at most 14 trees and 20 rocks and lanterns together, as listed). No single mesh over 20,000.
3. Layout: every landmark in section 3 exists at its listed position within 3 studs (checked against `Layout.luau` and the live instance positions); six gate coordinates match exactly.
4. Reach: scripted walk from spawn to each landmark at WalkSpeed 16 completes in under 20 seconds.
5. Lighting: `Lighting`, `Atmosphere` and the four post effects exist with the section 4 values (within 5 percent); a time-of-day change tweens over 4 seconds, with no value jumping more than one frame step.
6. Roll: 100 rolls play each rarity's effect, it ends, and the `Workspace.Fx` descendant count returns to the pre-roll baseline within 6 seconds of the last roll; `:onPeak` fires exactly once per roll.
7. Battle: a 5 v 5 scripted battle shows lunge, hit shake, damage numbers, one ultimate cut-in and a death shatter; zero console errors; `Fx` count returns to baseline.
8. Limits: never more than 40 live `ParticleEmitter`s with `Enabled = true` and at most 12 lights in the whole place, every light except the sun with `Shadows = false`, sampled once a second during rolls and battles.
9. Reduced motion on: camera shake offset is 0, no full-screen flash frames, emitter rates at 30 percent.
10. API: `CardStandee.create` and `CardStandee.setState` accept the states in section 6 and leave no stray instances after `Destroy`; `Ambient.toggle(name, bool)` turns each ambient effect on and off with the emitter count changing accordingly.
11. Code: `selene` and `stylua --check` clean; `rojo build` succeeds; VFX modules have passing tests that show instance counts return to start.

# Art spec — Untitled Anime Card Game (prompt-ready)

Director: art. Builder: OpenRO's agent only (nobody hand-writes files under `art/`).
Rarity names, roster and card titles come from `docs/GDD.md` / `docs/ROSTER.md` (designer). Where this
spec says "the rarity list", use the one in the GDD. Where it names a rarity, that is a placeholder
until the GDD is final.

## 0. Rules for the builder

- Every file lands under `art/` in the project, as PNG, no file over 5 MB (target under 1.5 MB).
- Character art is made from the owner's reference images (`art/references/`, never committed, never
  uploaded, never shown in a public place). Keep the exact character: face, hair, outfit, colours,
  line weight, shading style. Change only the pose, camera angle and action.
- Neutral camera for portraits: eye-level or chest-height, three-quarter or straight-on. No low
  worm's-eye or extreme foreshortening shots.
- Never use Higgsfield Soul Cinema. Plain image generation only.
- If a generation is refused or moderated, re-draw with a different pose or wording and add one line to
  `art/ART-LOG.md` (date, card id, what was refused, what changed). Do not retry the same prompt more
  than twice.
- Keep `art/ART-LOG.md` with one row per final file: path, source reference file name, prompt, sha256.
- Plain wording in anything players see. No internal jargon.
- Uploads go through OpenRO's asset pipeline only, and only after the owner has entered the Open Cloud
  key in Settings. Upload the authorised set (`art/cards/**`, `art/frames/*/*.png`, `art/ui/*.png`,
  `art/store/*.png`). Record ids in `assets.lock.json`, then regenerate `src/shared/AssetIds.luau`
  from it (a table `AssetIds["art/ui/icon_coin.png"] = "rbxassetid://123"`; never hand-edit it).

## 1. Card art

Per card in ROSTER.md:

| File | Size | Content |
|---|---|---|
| `art/cards/<card-id>/art.png` | 640x960 (2:3) | Character full or three-quarter body, new action pose, background simple and on-theme (gradient plus light effects) so the frame can sit on top. Keep the lower 25% quiet: the name ribbon and stats cover it. |
| `art/cards/<card-id>/icon.png` | 256x256 | Head-and-shoulders crop of the same character for lists and team slots. |
| `art/cards/<card-id>/full.png` | 640x960 | Art plus its rarity frame, flattened. Used on 3D gallery stands and store images. |

Process: reference image(s) -> image generation with the reference attached and a prompt of this form
(fill the brackets): "Keep this exact character, same face, hair, outfit, colours and art style. Redraw
[her/him] [pose, e.g. mid-leap sword slash] in [angle, e.g. eye-level three-quarter view]. Vertical
2:3 card composition, character centred, lower quarter calm. No text, no frame, no logo, no watermark."
Then crop or resize to the exact sizes above. Check each result by eye against its reference (same
face, same colours) and re-draw if the character drifted.

One card per rarity-tier looks (background mood): common calm and flat, rare adds light rays, epic adds
particles and a coloured aura, legendary and above add a strong rim light and a radial burst.

Until the owner's references arrive, make clearly-labelled placeholder silhouettes with
`art/placeholders/<card-id>.png` (same sizes, flat gradient, anonymous silhouette, no character
likeness) so UI can lay out. Replace them when references arrive.

## 2. Rarity frames (animation-ready layers)

For each rarity, folder `art/frames/<rarity-id>/`, five PNG layers on the same 640x960 transparent
canvas, so game-ui can stack them as ImageLabels and animate each separately:

| Layer | Content | Animation use |
|---|---|---|
| `glow.png` | soft inner-edge halo in the rarity colour | pulse transparency |
| `base.png` | bevelled border ring, 14 px (common) growing to 26 px (top rarity), rounded corners radius 44, transparent window | static |
| `ornament.png` | corner studs, brackets, diamonds, top crest and side spikes; more ornament with higher rarity | bob or scale |
| `plate.png` | name ribbon (about 70 px tall) above a stat bar (about 110 px tall), dark translucent, rarity-coloured top line | static |
| `shine.png` | soft diagonal white highlight band, clipped to the card shape | slide across the card |

Rarity colour language (main / dark / light), unless the GDD names others: common grey `#9aa3b2`, uncommon
green `#4fd37a`, rare blue `#3f8cff`, epic purple `#a95cff`, legendary gold `#ffb627`, mythic crimson
`#ff3d5a`, top secret tier prismatic (rainbow ring on near-black). Each step up must be visibly
richer at a glance at 120 px wide: more ornament, thicker ring, brighter glow. Do not rely on colour alone.

Also `art/frames/preview/<rarity-id>.png`: a flattened preview over a placeholder, for reviews (not
uploaded, not in the authorised set).

## 3. UI skin (for game-ui)

All in `art/ui/`, PNG, RGBA. Style: glossy, chunky, outlined, dark-violet panels with bright accent
buttons, readable at phone size. Dark outline `#120e1e` around every icon, soft drop shadow underneath.

9-slice images (record the slice margin for each in `art/ui/slices.json` as
`{"name": {"slice": px, "size": [w, h]}}` so game-ui can set `SliceCenter`):

- Panels: `panel_dark` (128x128, slice 32), `panel_light` (128x128, 32), `panel_inset` (96x96, 24),
  `panel_glass` (128x128, 32, translucent), `pill` (96x40, 20).
- Buttons, each with a `_pressed` twin where the face sits 8 px lower and the bottom lip is gone, all
  192x72, slice 28: `button_primary` (green), `button_secondary` (blue), `button_danger` (red),
  `button_gold`, plus `button_disabled` (grey, no twin).
- Bars: `bar_track` and `bar_fill` (256x40, slice 18; the fill is white so the game can tint it).

Icons, 256x256 each, centred, about 12 px margin: `icon_coin`, `icon_gem`, `icon_ticket` (roll
ticket), `icon_luck` (four-leaf clover), `icon_potion_luck` (green), `icon_potion_speed` (yellow),
`icon_potion_power` (red), `icon_pass_basic` (blue shield), `icon_pass_vip` (gold shield with crown),
`icon_key`, `icon_crown`, `icon_atk`, `icon_hp`, `icon_speed`, `icon_star`, `icon_lock`,
`icon_rebirth`, `icon_trophy`, `icon_shard`, `icon_chest`, `icon_deck`.
Add more on request from game-ui (post the exact name and meaning); never rename existing ones.

## 4. Game icon, thumbnails and store imagery

Folder `art/store/`:

- `game_icon.png` 512x512: the strongest roster character's face and a burst of light, with the game
  name legible at 64 px. No text smaller than 14% of the height.
- `thumb_1.png` .. `thumb_3.png` 1920x1080: (1) hero shot of three to five best characters, (2) roll
  reveal moment (a rare card bursting out), (3) battle / team screen. Brightest subject at the third
  lines, clear space for nothing else; no fake UI.
- Developer product / game pass images, 512x512, one per product in `docs/MONETIZATION.md`, named
  `product_<product-id>.png`, built from the matching UI icon on a coloured burst.
- Open Cloud has no endpoint for the game icon and thumbnails. They are an owner step in the Creator
  Dashboard; leave the finished files in `art/store/` and list them in `docs/BUILD-LOG.md`.

## 5. Done means

- Every file above exists at the exact size, is under 5 MB, and loads (decode check).
- Each rarity at 120 px wide reads as a clear step up from the one before it.
- Every roster card has art, icon and flattened full image (placeholder or real) and a frame layer set.
- `art/ART-LOG.md` has a row per file; `assets.lock.json` and `src/shared/AssetIds.luau` agree
  (every uploaded path has an id).
- Screenshots of the finished card, frame and UI sheets are saved for review.

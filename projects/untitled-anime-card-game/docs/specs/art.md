# Art spec — Untitled Anime Card Game (prompt-ready)

Director: art. Builder: OpenRO's agent only (nobody hand-writes files under `art/`).
Rarities, roster and card names come from `docs/GDD.md` / `docs/ROSTER.md` (designer): 8 rarities and
72 cards. If they change, the designer's files win.

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
  key in Settings. Upload the authorised set (`art/cards/**`, `art/frames/r*/*.png`, `art/ui/*.png`,
  `art/store/*.png`). Record ids in `assets.lock.json`, then regenerate `src/shared/AssetIds.luau`
  from it (a table `AssetIds["art/ui/icon_coins.png"] = "rbxassetid://123"`; never hand-edit it).

## 1. Card art

72 cards (ROSTER.md; `docs/data/cards.json` has the ids). The owner sends one reference image per card,
filename = card id (e.g. `w1_rock_lee.png`), into `art/references/`. Build P1 (32 cards) first.

| File | Size | Content |
|---|---|---|
| `art/cards/<card-id>/art.png` | 512x712 | The character in a new action pose and angle, simple on-theme background (gradient plus light effects) so the frame can sit on top. Keep the lower 25% calm: the name ribbon and stats cover it. |
| `art/cards/<card-id>/icon.png` | 128x128 | Head-and-shoulders crop of the same character for lists and team slots. |
| `art/cards/<card-id>/full.png` | 512x712 | Art plus its rarity frame, flattened. For 3D gallery stands and store images. |

Process: reference image -> image generation with the reference attached and a prompt like: "Keep this
exact character, same face, hair, outfit, colours and art style. Redraw them [pose, e.g. mid-leap sword
slash] in [angle, e.g. eye-level three-quarter view]. Vertical card composition, character centred,
lower quarter calm. No text, no frame, no logo, no watermark." Then crop/resize to the exact size. Check
each result by eye against its reference (same face, same colours, same line style); re-draw if the
character drifted or a hand/face broke. Pose should suit the card's role and signature move in
ROSTER.md (Striker = attack, Support = casting or guarding, Vanguard = braced stance, Ranger = ranged
shot, Disruptor = control gesture). Background mood by rarity: Common/Uncommon calm and flat; Rare adds
light rays; Epic adds particles and a coloured aura; Legendary and above add a strong rim light and a
radial burst; Secret and Transcendent are the most dramatic.

Until references arrive, `art/placeholders/<card-id>.png` (512x712): flat gradient with an anonymous
silhouette, no likeness, so UI can lay out. Replace when references arrive.

## 2. Rarity frames (animation-ready layers)

Eight rarities, ids r1..r8 = Common, Uncommon, Rare, Epic, Legendary, Mythic, Secret, Transcendent.

Per rarity, folder `art/frames/r<N>/`, five PNG layers on the same 512x712 transparent canvas so
game-ui can stack and animate them separately:

| Layer | Content | Animation use |
|---|---|---|
| `glow.png` | soft inner-edge halo in the rarity colour | pulse transparency |
| `base.png` | bevelled border ring, 12 px (r1) growing to 24 px (r8), rounded corners radius 36, transparent window | static |
| `ornament.png` | corner studs, brackets, diamonds, top crest, side spikes; more ornament with higher rarity | bob or scale |
| `plate.png` | name ribbon (about 56 px tall) above a stat bar (about 88 px tall), dark translucent, rarity-coloured top line | static |
| `shine.png` | soft diagonal white highlight band, clipped to the card shape | slide across the card |

Also, as game-ui asked (docs/specs/game-ui.md section 6), in `art/ui/`: `frame_r1..frame_r8.png`
(256x356 minimum; deliver 512x712) = base + ornament + plate flattened, transparent window; and
`glow_r1..glow_r8.png` (256x256 minimum; deliver 512x512) = square soft halo for behind a card.

Colour language (main), unless the GDD says otherwise: r1 grey `#9aa3b2`, r2 green `#4fd37a`, r3 blue
`#3f8cff`, r4 purple `#a95cff`, r5 gold `#ffb627`, r6 crimson `#ff3d5a`, r7 cyan-to-magenta prismatic,
r8 white-gold with a rainbow edge. Each step up must read as clearly richer at 120 px wide: more
ornament, thicker ring, brighter glow. Never rely on colour alone.

Also `art/frames/preview/r<N>.png`: flattened preview over a placeholder for reviews (not uploaded).

## 3. UI skin (for game-ui)

`docs/specs/game-ui.md` section 6 is the authoritative list of names and sizes (panels, buttons, pill,
ribbon, close button, gloss, rarity frames and glows, icons, reveal rays, card back, banner plate). Build
exactly that list, one PNG per name in `art/ui/`, RGBA, deliver at 2x its stated minimum size when it is
not a 9-slice. If the list changes, game-ui's file wins; if a name is added later by game-ui, add it.

Style: glossy, chunky, outlined, dark-violet panels with bright accent buttons, readable at phone size.
Dark outline `#120e1e` around every icon, soft drop shadow underneath. Button face and lip are separate
images (`ui_button_face`, `ui_button_lip`) so the game can press the face down onto the lip.

Write `art/ui/slices.json`: `{"<name>": {"slice": [left, top, right, bottom], "size": [w, h]}}` for
every 9-slice image so game-ui sets `SliceCenter` from it.

Icons: 128x128 (deliver 256), centred, about 8 px margin; names `icon_<name>` for gems, coins,
roll_ticket, luck, potion, pass, star, lock, crown, sword, deck, cards, world, tower, raid, ranked,
rebirth, skills, quests, shop, codes, settings. Also `icon_hp`, `icon_speed`, `icon_key`, `icon_shard`
if game-ui adds them. Never rename an existing file.

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

# OpenRO v0.11 API: Shared Secrets, Open Cloud, Reference Search, Art & Audio/Mesh Assets

Contract for `@openro-ui`, `@openro-agent`, `@openro-team`, and the game drivers. Served by
`openro-rs` on loopback only (`403` for non-loopback `Host`). Request bodies are JSON
(`Content-Type: application/json`). Replies carry `{ "ok": bool, ... }` with plain-word
`"error"` messages on failure.

Paths inside a project are always **project-relative** (`..`, absolute paths, backslashes,
and escaping symlinks are rejected with `400`).

---

## 1. Shared Secrets Store & Settings (`/api/config`, `/api/opencloud*`)

- **Directory:** `$OPENRO_SECRETS_DIR` when set (shared across OpenRO instances), otherwise
  `<data_dir>/secrets`. Created with mode `0700`; each secret file is written atomically with
  mode `0600`.
- **Legacy migration:** Any existing `gemini_api_key` or `openai_image_api_key` in
  `data_dir/config.json` is migrated automatically on startup into the secrets store (existing
  stored secret wins) and blanked from `config.json`.
- **Masking & scrubbing:** Keys are write-only via Settings; API replies expose only
  `has_*` booleans and `••••abcd` masks. Raw keys are scrubbed from upstream errors and never
  logged.

### `GET /api/config` & `POST /api/config` (additions)

`GET /api/config` returns (alongside existing model/studio fields):
- `image_provider`: `"chatgpt"` (default — **ChatGPT (shared browser)**) | `"gemini"` | `"openai"` | `"auto"`
- `has_gemini_api_key`: bool, `gemini_api_key_masked`: string | null
- `has_openai_image_api_key`: bool, `openai_image_api_key_masked`: string | null
- `has_opencloud_key`: bool, `opencloud_key_masked`: string | null
- `opencloud_creator`: `{ userId?: string, groupId?: string }`
- `secrets_dir_shared`: bool (true when `OPENRO_SECRETS_DIR` is active)

`POST /api/config` accepts (write-only):
- `image_provider`: `"chatgpt"` | `"gemini"` | `"openai"` | `"auto"`
- `gemini_api_key` / `clear_gemini_api_key: true`
- `openai_image_api_key` / `clear_openai_image_api_key: true`
- `opencloud_api_key` / `clear_opencloud_api_key: true`

### Open Cloud Credentials & Creator Owner

| Method + Path | Body | Reply |
|---|---|---|
| `GET /api/opencloud` | | `{ ok, has_key, key_masked, creator: { userId?\|groupId? }, base, scopes_needed: [...] }` |
| `POST /api/opencloud/key` | `{ api_key }` or `{ clear: true }` | same as `GET /api/opencloud` |
| `POST /api/opencloud/settings` | `{ creator: { userId } \| { groupId } }` | same as `GET /api/opencloud` |

Base URL defaults to `https://apis.roblox.com` (`OPENRO_OPENCLOUD_BASE` overrides for tests;
`OPENRO_OPENCLOUD_POLL_MS` shortens operation polling in tests). Without a key, Open Cloud
calls answer `503 { ok: false, error: "Add your Open Cloud key in Settings first." }`.

Required Roblox Open Cloud scopes: `asset:read`, `asset:write`, `universe-places:write`,
`game-pass:read`, `game-pass:write`, `developer-product:read`, `developer-product:write`,
`universe:read`, `universe:write`.

---

## 2. Open Cloud Assets, Codegen, Publish, Products & Universe

### 2.1 Assets & `assets.lock.json`

| Method + Path | Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/assets` | | `{ ok, files: [{ path, kind, bytes, sha256, authorised, locked: { assetId, state, sha256 }\|null, stale }], default_authorised, locked_states: ["approved","pending","rejected","unknown"] }` |
| `POST /api/projects/{id}/assets/authorise` | `{ paths: [...], revoke?: false }` | updates `assets.authorised` in `openro.project.json` (supports `*`, `**`, `!` negation); `{ ok, authorised: [...] }` |
| `POST /api/projects/{id}/assets/upload` | see below | `{ ok, results: [...], summary }` |
| `GET /api/projects/{id}/assets/lock` | | raw `assets.lock.json` `{ version: 1, assets: { ... } }` |
| `POST /api/projects/{id}/assets/codegen` | `{ out?: "src/shared/AssetIds.luau" }` | generates `AssetIds.luau` from `assets.lock.json` + `assets/meshes/manifest.json`; `{ ok, path, count, mesh_count, skipped_rejected, keys: [...] }` |

**Default authorised globs (`DEFAULT_AUTHORISED_GLOBS`):**
`["art/cards/**", "art/ui/*.png", "art/frames/*/*.png", "!art/frames/preview/**", "art/store/*.png", "art/worlds/**", "art/audio/**", "assets/audio/**", "assets/meshes/**", "!assets/meshes/*preview*.png"]`.
`art/references/`, `art/previews/`, and `art/frames/preview/` are **never** walked or uploaded.

**Upload selection (`POST /api/projects/{id}/assets/upload`):**
- `{}` or `{ "authorised": true }`: uploads only files matching `assets.authorised` (default). Empty allowlist uploads nothing and returns guidance in `summary.note`.
- `{ "paths": ["art/cards/naruto/art.png"] }`: explicit file list.
- `{ "everything": true }`: explicit override uploading every uploadable file in the project (cannot combine with `paths` -> `400`). Prints and returns `summary.overrun: [...]` listing every file beyond `assets.authorised`.
- Optional flags: `dry_run: true`, `force: true`, `auto_codegen: true` (default `true` — automatically regenerates `src/shared/AssetIds.luau` whenever `assets.lock.json` changes), `types: { "<glob>": "Decal"|"Animation"|... }`.

**Asset types & moderation:**
- Extensions: `png/jpg/jpeg/bmp/tga` -> `Image` (automatic single retry as `Decal` on HTTP 400), `mp3/ogg/wav/flac` -> `Audio`, `fbx/gltf/glb/rbxm/rbxmx` -> `Model` (`Animation` via `types` override for `.rbxm`/`.rbxmx`), `mp4/mov` -> `Video`. Files > 20 MB are `skipped`.
- `locked.state` values: `"approved"`, `"pending"`, `"rejected"`, `"unknown"`.
- **Rejected moderation:** If Roblox returns moderation `rejected`, the result has `state: "rejected"`, `assetId: null`, and **no id is written to `assets.lock.json`** (any previous entry for that path is removed).

### 2.2 Place Publish

`POST /api/projects/{id}/publish` `{ versionType?: "Saved"|"Published" (default "Saved"), confirm?: bool, build_id?: string }`
- Publishes the latest build (or `build_id` from `build/builds/*.json`) via `POST /universes/v1/{universeId}/places/{placeId}/versions?versionType=`.
- `versionType: "Published"` makes the game live and **requires `confirm: true`** (otherwise `400` prompting owner confirmation).
- Files over 100 MB (configurable via `OPENRO_MAX_PUBLISH_BYTES`) are refused with `400`.
- Reply: `{ ok, versionNumber, versionType, build_id, bytes }`.

### 2.3 Game Passes & Developer Products (with `priceApproved` Gate)

| Method + Path | Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/products` | | `{ ok, manifest: [...], remote: { developer_products: [...], game_passes: [...] }\|null, remote_error }` |
| `POST /api/projects/{id}/products/sync` | `{ keys?: [...], dry_run?: false }` | `{ ok, results: [{ key, kind, action, id, error? }], dry_run }` |

- **Owner price approval gate:** Any manifest product without `priceApproved: true` is **skipped** with `action: "skipped", error: "price not approved"`.
- Matching: existing `id` -> `PATCH` if changed; null `id` -> adopts remote item with exact matching `name` (`adopted`), otherwise creates via `POST` (`created`, multipart with optional `icon` -> `imageFile`) and writes `id` back to `openro.project.json` while preserving all unknown fields.
- Upstream endpoints: `developer-products/v2/universes/{u}/developer-products` and `game-passes/v1/universes/{u}/game-passes` (paginated via `nextPageToken`).

### 2.4 Universe Settings

- `GET /api/projects/{id}/universe` -> `{ ok, universe, manual_steps: [...] }` (`GET /cloud/v2/universes/{u}`)
- `POST /api/projects/{id}/universe` `{ settings: { displayName?, description?, visibility?, voiceChatEnabled?, desktopEnabled?, mobileEnabled?, tabletEnabled?, consoleEnabled?, vrEnabled? } }` -> `PATCH /cloud/v2/universes/{u}?updateMask=...` -> `{ ok, universe, updated: [...], manual_steps: [...] }`
- Unsupported keys (`icon`, `thumbnails`, `ageRating`, etc.) return `400` pointing to Roblox Creator Dashboard. Both endpoints return `manual_steps` documenting the Creator Dashboard-only items (icon/thumbnails, monetisation policy, age questionnaire).

---

## 3. Reference Image Search (Shared Camoufox Browser `:9230`)

Uses the owner's single shared Camoufox browser (`sharedbrowser` API on `:9230`, lease owner
`"openro"`, brief boot-wait retry, never force-releases foreign leases, never restarts
`shared-browser.service`, stops immediately on any CAPTCHA/anti-bot challenge). When `:9230`
is unavailable, automatically falls back to a read-only direct HTTP image search
(`search_backend: "http_fallback"`, overridable via `OPENRO_REFERENCE_SEARCH_HTTP_BASE`;
pass `use_http_fallback: false` to disable). All outputs stay in the project's gitignored
`art/references/` folder and are never committed or uploaded.

| Method + Path | Body / Query | Reply |
|---|---|---|
| `POST /api/art/references/search` | `{ project?, kind?: "character"\|"weapon"\|"prop", character?, subject?, show?, card_id?, slug?, qualifiers?, max_candidates?: 6, auto_pick?: true }` | `{ ok, slug, kind, subject, character, show, query, chosen_index, reference_path, meta_path, url, sha256, bytes, candidates: [{ index, path, url, image_url, source_url, title, width, height, bytes, sha256, score, reasons, rejected_reason }] }` |
| `GET /api/art/references` | `?project=<id>` | `{ ok, count, references: [...] }` |
| `POST /api/art/references/select` | `{ project?, slug (or card_id), candidate_index }` | swaps active reference image; `{ ok, slug, chosen_index, reference_path, url, sha256, bytes, width, height }` |

Ranking behavior:
- **`kind: "character"` (default):** filters/penalizes static standing poses, official character/model sheets, turnarounds, transparent cutouts, merchandise/cosplay, and tiny images, and rewards dynamic anime action/battle scenes.
- **`kind: "weapon"` / `"prop"` (#76):** accepts `subject` (or `weapon`/`prop`/`character`) with optional `show`, builds isolated-prop queries (`"<subject> weapon anime wiki prop reference"`), and flips ranking to prefer **isolated weapon/prop artwork, wiki weapon renders, and prop/cosplay reference sheets** over cluttered battle/action scenes while still rejecting watermarks, logos, low-res thumbnails, and honoring all SSRF/CAPTCHA rules (Bing Images primary with clean CAPTCHA-free Google Images fallback). Tests override via `OPENRO_REFERENCE_SEARCH_BIN`.

---

## 4. Reference-Image Art Generation & Pixel-Review Approval

**Default provider:** `"chatgpt"` (`ChatGPT (shared browser)` wrapping
`~/.agents/skills/chatgpt-image-gen/chatgpt-camoufox-imagegen`, serialized via
in-process lock + `--lease openro`, one-line prompt collapse, fresh unused `-o` staging path).
Optional API providers: `"gemini"` (Nano Banana via `GEMINI_KEY` / `OPENRO_GEMINI_IMAGE_BASE`)
and `"openai"` (via `OPENAI_IMAGE_KEY` / `OPENRO_OPENAI_IMAGE_BASE`).
**Higgsfield / Soul Cinema is explicitly prohibited (`400`).**

| Method + Path | Body / Query | Reply |
|---|---|---|
| `POST /api/art/generate` | `{ project?, provider?: "chatgpt"\|"gemini"\|"openai", kind?: "card"\|"icon"\|"frame"\|"store"\|"world"\|"weapon_sheet"\|"weapon", card_id?, subject?, show?, auto_reference?: bool, prompt, reference_images?: [...], out_path?, character_art?: bool, size?, aspect_ratio?, model?, timeout_s? }` | see states below |
| `POST /api/art/approve` | `{ project?, candidate_path, target_path?, expected_sha256?, reviewer, verdict?: "pass"\|"redo", notes? }` | promotes reviewed candidate to final `target_path` (or records `redo`) and logs to `art/ART-LOG.jsonl`; `{ ok, status, path, url, meta_path, sha256, width, height, bytes, reviewer }` |
| `GET /api/art` | `?project=<id>` | `{ ok, count, items: [...], pending_review: [...], log: [...] }` |
| `GET /api/art/image` | `?project=<id>&path=art/...` | raw image bytes (jailed to `<project>/art/`) |

**Weapon/prop sheet auto-sourcing (#76):** When `kind` is `"weapon_sheet"` or `"weapon"` and no reference image is passed, passing `auto_reference: true` or a `subject` automatically runs `reference_search::search` (`kind: "prop"`) into gitignored `art/references/`, attaches the top isolated prop reference to generation, and records `"prop_reference_auto_sourced": true` in `.meta.json` and the response.

### `POST /api/art/generate` Response States

1. **`status: "generated"` (`ok: true`):** Exit 0 (or Gemini/OpenAI success). PNG written to
   `out_path` under `art/`, metadata written to `<stem>.meta.json`, logged to `art/ART-LOG.jsonl`.
   Reply: `{ ok: true, status: "generated", pending_review: false, refused: false, path, url, meta_path, provider, model, sha256, width, height, bytes, reference_images, metadata }`.
2. **`status: "pending_review"` (`ok: true`, `pending_review: true`):** ChatGPT `--character-art`
   exit 9 (`ANGLE_REVIEW_REQUIRED`). Real PNG staged at `<stem>.pending-angle.<sha12>.png`
   (final `out_path` is NOT written). Reply includes `candidate_path`, `candidate_url`,
   `candidate_sha256`, `requested_path`, `reference_images`, `reference_urls`, `angle_review`.
   UI shows side-by-side reference vs candidate and calls `POST /api/art/approve` after review.
3. **`status: "refused"` (`ok: false`, `refused: true`, HTTP 200):** Model/moderation refusal
   (ChatGPT exit 7, Gemini safety block, OpenAI content policy). Logged to `art/ART-LOG.jsonl`
   with `moderation_reason` so the agent can adjust the prompt and retry.
4. **ChatGPT exit code mapping:** `2` -> `503` (login required in shared browser), `3` -> `400`
   (usage / camera angle preflight), `4` -> `502` (`UI_DRIFT` / model tier), `5` -> `504`
   (timeout), `6` -> `429` (rate limited), `8` -> `503` (browser busy / unreachable).
   Character-art prompts pre-reject low-angle/worm's-eye words (`400`) before acquiring the
   lease, collapse newlines to one line, and append neutral eye-level framing when missing.

---

## 5. Image Post-Processing, Alpha & Card Packing (`Pillow`)

| Method + Path | Body | Reply |
|---|---|---|
| `POST /api/art/process` (alias `POST /api/tools/image/process`) | `{ project?, input?, output, ops: [...], slice?: [l,t,r,b], slices_json?, max_bytes?: 5242880 }` **or** `{ project?, script, output?, timeout_s?: 30, max_bytes? }` | `{ ok, path, url, width, height, bytes, sha256, has_alpha, opaque_ratio, slices_updated }` |
| `POST /api/art/pack-card` | `{ project?, card_id, source_image, reference_image?, frame_image?, out_dir?, art_size?: [512,712], icon_size?: [256,256], max_bytes? }` | `{ ok, card_id, art, icon, full, contact_sheet }` (writes `art.png`, `icon.png`, `full.png` + uncommitted `art/previews/<card_id>_contact.png`) |

Supported `ops`: `canvas` (`width`+`height` or `size: [w, h]`, `color?`), `draw` (`shapes: [{ kind|shape, ... }]` or flat single-shape `{ shape|kind, box|points|cx, fill|stroke|color, ... }` — shapes: `rect`, `rounded_rect`, `ellipse`, `ring`, `polygon`, `star`, `gradient`; returns `400` if 0 shapes drawn or unknown shape/op), `cover_resize` (`width`+`height` or `size`, `anchor`: `center`/`top`/`top_third`/`bottom`), `resize`, `crop`,
`remove_bg` (`corner_flood` or `chroma` with `tolerance` + `feather`), `rounded_mask`,
`composite`, `quantize`, `compress`, `contact_sheet`. Every output PNG is full-decoded,
verified `<= max_bytes`, hashed, and logged to `art/ART-LOG.jsonl`.

---

## 6. Audio Synth & Blender 3D Mesh Tools

### Audio (`POST /api/tools/audio/generate`)
- Body: `{ project?, name?, out_path?, preset?, duration_ms? (50..15000), layers?: [{ wave: "sine"|"triangle"|"square"|"saw"|"noise", start_ms?, duration_ms?, freq_start?, freq_end?, gain?, attack_ms?, decay_ms?, sustain?, release_ms? }] }`
- Presets: `summon_roll`, `mythic_fanfare`, `battle_crit`, `potion_drink`, `ui_click`,
  `level_up`, `death_shatter`, `card_flip`, `pack_open`, `rare_chime`, `legendary_drop`,
  `secret_alarm`, `transcendent_choir`, `battle_hit`, `button_hover`, `error_buzz`,
  `ambient_plaza`.
- When `project` is supplied, saves directly to `<project>/assets/audio/<name>.wav` (or
  `out_path`) in addition to `<data_dir>/game-assets/audio/` and returns `project_path`.

### Blender 3D Mesh Pipeline (`POST /api/tools/blender/generate`, `GET /api/tools/blender/status`)
- See `docs/API-V011-mesh.md` (structured `parts` or sandboxed `bpy` `script` -> smart UV
  unwrap -> Cycles colour x AO PNG bake -> OBJ + `_glow` OBJ + `manifest.json` + 2-angle
  previews, hard fail > 20,000 tris).

# OpenRO v0.12 — Creator Hub CDP Automation & Open Cloud Pipeline Upgrades

Documents the HTTP routes, Rust helpers, security guarantees, and Open Cloud pipeline upgrades added for `#68` and `#69`. All routes require a loopback `Host` header (`127.0.0.1` / `localhost`) and never leak raw Open Cloud API keys.

---

## 1. Creator Hub Browser Automation (`#68`, `src/creator_hub.rs` & `src/creator_hub_cdp.py`)

Connects **only** to a local loopback Chrome/Chromium DevTools endpoint (`http://127.0.0.1:<port>`, `http://localhost:<port>`, or `http://[::1]:<port>`). Default lookup order: `OPENRO_BROWSER_CDP_URL` -> local Chrome/Chromium `DevToolsActivePort` -> `http://127.0.0.1:9222`. Non-loopback CDP URLs are rejected with HTTP `400`.

### Security & Anti-Bypass Guarantees
- **Never reads `.ROBLOSECURITY` cookies** or inspects browser cookie stores.
- **Dedicated tab only:** opens a fresh Creator Hub tab (`context.new_page()`), never navigating or hijacking existing user tabs; closes the temporary tab automatically when complete, or leaves only the newly opened Creator Hub tab open when user action is required.
- **Zero bypass:** detects login walls, 2FA/Email OTP/Passkey prompts, and CAPTCHA/Arkose challenges, immediately stopping and returning `status: "action_required"` with `blocked_reason` (`"login_required" | "2fa_required" | "captcha_required"`), `url`, and `message`.
- **Atomic `0600` secret handoff:** Rust creates a unique `0600` temp file in `SecretStore::dir()`, passes `--key-out <path>` to `creator_hub_cdp.py`, reads & validates the key, persists it via `SecretStore::set(OPENCLOUD_KEY, &key)` (`0600`), wipes & unlinks the temp file on drop, scrubs the raw secret from all responses/errors, and returns only `key_masked`.

### `GET /api/creator-hub/status`
Query params:
- `cdp_url` (optional, loopback `http://` only)
- `check_login=1` (optional: probes Creator Hub login & experiences in a dedicated tab)
- `save_creator=1` (optional: persists `opencloud_creator = user:<id>` into `SecretStore` when signed in)

Response (`200`):
```json
{
  "ok": true,
  "cdp_url": "http://127.0.0.1:9222",
  "cdp_reachable": true,
  "browser_version": "Chrome/...",
  "logged_in": true,
  "user": { "id": "1170918805", "name": "Chrono_H3X", "displayName": "Chrono" },
  "experiences": [
    { "universeId": "10771116814", "rootPlaceId": "112916648108736", "name": "Untitled Anime Card Game" }
  ],
  "creator": { "userId": "1170918805" }
}
```
If not signed in or challenged: `logged_in: false`, `blocked_reason`, `url`, `message`.

### `POST /api/creator-hub/setup` & `POST /api/projects/:id/creator-hub/setup`
Request body:
```json
{
  "cdp_url": "http://127.0.0.1:9222",
  "key_name": "OpenRO Auto Key",
  "description": "Provisioned by OpenRO for local project publishing",
  "cidr": "0.0.0.0/0",
  "ensure_experience": true,
  "reuse_universe_id": "10771116814",
  "experience_name": "Untitled Anime Card Game",
  "scopes": ["asset:read", "asset:write", "universe-places:write", "game-pass:read", "game-pass:write", "developer-product:read", "developer-product:write", "universe:read", "universe:write"],
  "force_new_key": false
}
```
Behavior:
- Resolves & links `universeId` and `placeId` into `openro.project.json` (preserving unknown fields) when a project is targeted.
- Provisions an Open Cloud API key directly into the `0600` `SecretStore` if missing or `force_new_key: true`.
- Returns `{ ok: true, status: "ready", has_key, key_masked, key_provisioned, creator, user, universeId, placeId, experience_name, experiences }` or `{ ok: false, status: "action_required", blocked_reason, url, message, has_key, key_masked, creator, universeId, placeId }`.

### `POST /api/projects/:id/creator-hub/configure`
Request body:
```json
{
  "cdp_url": "http://127.0.0.1:9222",
  "private_servers": { "enabled": true, "price_robux": 0, "max_players": 50 },
  "icon_path": "art/store/icon.png",
  "thumbnail_paths": ["art/store/thumb_1.png"],
  "visibility": "PUBLIC",
  "name": "Untitled Anime Card Game",
  "description": "...",
  "check_questionnaire": true
}
```
Behavior:
- Validates `icon_path` and every `thumbnail_paths` entry strictly inside the project root via `opencloud::safe_rel` (HTTP `400` on traversal or missing file).
- Automates Private Servers on `/advanced-settings`, Icon upload on `/places/<placeId>/icon`, Thumbnails upload on `/places/<placeId>/thumbnails`, optional basic fields, and checks `/content-rating` (never auto-answers legal questionnaires).
- Returns `{ ok, status: "ready" | "action_required", universeId, placeId, configured: [...], manual_steps_remaining: [{ step, url }], blocked_reason?, url?, message? }`.

### Rust Public Helpers (`openro_rs::creator_hub`)
- `pub fn default_cdp_url() -> String`
- `pub fn validate_loopback_cdp_url(raw: &str) -> Result<String, String>`
- `pub fn status(ctx: &opencloud::Ctx, cdp_url: Option<&str>, check_login: bool, save_creator: bool) -> Result<Value, opencloud::Oc>`
- `pub fn setup(ctx: &opencloud::Ctx, project_root: Option<&Path>, body: &Value) -> Result<Value, opencloud::Oc>`
- `pub fn configure(ctx: &opencloud::Ctx, project_root: &Path, body: &Value) -> Result<Value, opencloud::Oc>`
- `pub fn route(ctx: &opencloud::Ctx, method: &str, path: &str, query: &str, body: &Value) -> Option<opencloud::Reply>`

---

## 2. Open Cloud Pipeline Upgrades (`#69`, `src/opencloud.rs`)

### Transparent Upload Conversions
Lock keys and SHA-256 hashes in `assets.lock.json` always track the **original project-relative file**, while upload payloads are transparently converted into Roblox-accepted wire formats:
- **`.wav` -> `.ogg`** (`audio/ogg`): converted via `ffmpeg -nostdin -y -v error -i <in> -c:a libvorbis -q:a 5 <out.ogg>` (`OPENRO_FFMPEG_BIN` override).
- **`.obj` -> `.fbx`** (`model/fbx`, `assetType: "Model"`): `.obj` added to `kind_by_ext` (`Model`) and `mime_for` (`model/obj`); converted via headless `blender --background --python-expr ...` (`OPENRO_BLENDER_BIN` override), attaching sibling `<stem>.png` diffuse texture when present and embedding textures into the temp `.fbx`.

### Decal -> Image Resolution on HTTP 400 Fallback
When an `Image` upload falls back to `Decal` on HTTP `400`, OpenRO resolves the underlying Image asset ID (from the response or an unauthenticated `GET https://assetdelivery.roblox.com/v1/asset/?id=<decalId>` XML lookup — never forwarding `x-api-key`), stores `"imageAssetId"` in `assets.lock.json`, and emits the usable Image `rbxassetid://<imageAssetId>` in `AssetIds.luau`.

### Incremental Lock Persistence & Expanded `429` Retries
- `assets.lock.json` is written atomically after **every** uploaded, reused, or rejected asset so partial batches survive interruptions or upstream errors.
- HTTP `429` retries expanded to **6 retries** (7 total attempts), honoring `Retry-After` and keeping `OPENRO_OPENCLOUD_POLL_MS` fast for tests.

### `POST /api/projects/:id/assets/codegen` (`openro_rs::opencloud::assets_codegen`)
- Prefers `imageAssetId` over `assetId` when emitting `rbxassetid://`.
- For packed card portraits matching `art/cards/<cardId>/art.png`, populates `AssetIds.byKey["<cardId>_art"]`, `AssetIds.byKey["cards/<cardId>"]`, and `AssetIds.byKey["<cardId>"]`.
- For locked meshes under `assets/meshes/<stem>.{obj,fbx,glb,gltf,rbxm,rbxmx}`, enriches `AssetIds.meshes["<stem>"]` with `assetId = "rbxassetid://<id>"`, wrapping lines exceeding 120 columns to stay 100% `stylua --check` clean.

### `POST /api/projects/:id/products/sync` & `POST /api/projects/:id/products/codegen`
- `products/sync` reads either `id` or `robloxId` from `openro.project.json`, writes **both** `id` and `robloxId` back on sync, accepts `default_for_sale` (`false` default), and supports `auto_codegen` (auto-generating `src/shared/ProductIds.luau` when IDs are synced or `auto_codegen: true`).
- `POST /api/projects/:id/products/codegen` (`pub fn products_codegen(root: &Path, body: &Value) -> Result<Value, Oc>`) emits a `stylua --check`-clean `--!strict` `src/shared/ProductIds.luau` with `ProductIds.byKey`, `ProductIds.gamePasses`, `ProductIds.developerProducts`, and `ProductIds.get(key)`.

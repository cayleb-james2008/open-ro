# OpenRO v0.11 API: Universal Anything-to-API / Anything-to-CLI Tool Adapters (#52)

Contract for `@openro-ui`, `@openro-agent`, and `@openro-team`. All routes are served by `openro-rs` on loopback only (`403` for non-loopback `Host`). Request/response bodies are JSON (`Content-Type: application/json`). Replies carry `{ "ok": bool, ... }` with plain-word `"error"` messages on failure.

Adapters are persisted at `<data_dir>/adapters.json`. Secret values are NEVER written to `adapters.json`, NEVER returned by any API, and ALWAYS scrubbed from logs and errors — they live exclusively in `SecretStore` (`$OPENRO_SECRETS_DIR` or `<data_dir>/secrets`, `0700`/`0600`).

---

## 1. Capabilities & Built-in Presets

Every adapter serves one capability:
- `image_gen` — image/card/UI art generation
- `reference_search` — reference image discovery
- `mesh_gen` — 3D mesh generation
- `audio_gen` — sound effect / music generation
- `custom` — any custom CLI binary/script or HTTP API callable by the agent and Teamwork

### Built-in Presets (dynamically resolved via `$HOME` + `PATH`, zero personal paths)

| Preset ID | Capability | Transport | Default Installed | Notes |
| --- | --- | --- | --- | --- |
| `chatgpt-browser-cli` | `image_gen` | `builtin` | Yes (prio 10) | Shared browser `chatgpt-camoufox-imagegen` (auto-detected) |
| `gemini-nano-banana-http` | `image_gen` | `builtin` | Yes (prio 20) | Requires `gemini_api_key` |
| `openai-images-http` | `image_gen` | `builtin` | Yes (prio 30) | Requires `openai_image_api_key` |
| `comfyui-local-http` | `image_gen` | `http` | Optional template (prio 40) | Local ComfyUI `http://127.0.0.1:8188/prompt` (`allow_loopback: true`) + async poll |
| `fal-flux-http` | `image_gen` | `http` | Optional template (prio 50) | `https://queue.fal.run/fal-ai/flux/schnell` (`fal_api_key`) + async poll |
| `camoufox-browser-cli` | `reference_search` | `builtin` | Yes (prio 10) | Shared Camoufox `:9230` search |
| `playwright-cdp-cli` | `reference_search` / `custom` | `cli` | Optional preset (prio 5) | Headless Chromium/Playwright loopback CDP passive inspection & search (`OPENRO_PLAYWRIGHT_CDP_BIN` / `$HOME` / `PATH`); replaces Camoufox when installed |
| `builtin-http-search` | `reference_search` | `builtin` | Yes (prio 20) | Direct read-only HTTP image search fallback |
| `blender-cycles-cli` | `mesh_gen` | `builtin` | Yes (prio 10) | Local Blender UV + Cycles AO bake |
| `meshy-http` | `mesh_gen` | `http` | Optional template (prio 20) | `https://api.meshy.ai/v2/text-to-3d` (`meshy_api_key`) + async poll |
| `rust-synth-builtin` | `audio_gen` | `builtin` | Yes (prio 10) | Built-in Rust 44.1 kHz WAV synthesizer |
| `elevenlabs-sfx-http` | `audio_gen` | `http` | Optional template (prio 20) | `https://api.elevenlabs.io/v1/sound-generation` (`elevenlabs_api_key`) |
| `craft-anything-cli` | `custom` | `cli` | Optional preset (prio 20) | Deterministic Craft Anything study ledger, unknowns tracker, mechanics/screenshot verifier & `KeptChecks` fingerprinter (`OPENRO_CRAFT_ANYTHING_BIN` / `craft-anything` / `$HOME/.agents/skills/*/scripts/craft_ledger.py`) |
| `rea-cli` | `custom` | `cli` | Optional preset (prio 30) | Reverse Engineer Anything (REA) shipped-artifact & passive runtime CLI (`OPENRO_REA_BIN` / `rea` / `rea-agents` / `$HOME` / `PATH`) |

Blank templates returned in `GET /api/adapters` -> `templates`: `custom-cli` and `custom-http`. All 3 Craft Anything / browser presets resolve binaries dynamically via env overrides, `$HOME`, and `PATH` (zero personal paths) and execute via strict `argv` arrays (zero shell interpolation). `POST /api/adapters` with `preset_id` also accepts an optional `capability` override (e.g. installing `playwright-cdp-cli` under `"custom"` instead of `"reference_search"`).

---

## 2. Adapter Schema

```json
{
  "id": "my-flux-cli",
  "name": "Local Flux CLI",
  "capability": "image_gen",
  "description": "Generates stylized PNGs via local CLI",
  "transport": "cli",
  "enabled": true,
  "priority": 15,
  "preset": false,
  "required_secrets": ["my_tool_key"],
  "cli": {
    "command": "~/bin/flux-gen",
    "argv": ["--prompt", "{{prompt}}", "--ref", "{{?ref_image}}", "--out", "{{out_path}}"],
    "env_secrets": { "FLUX_API_KEY": "my_tool_key" },
    "cwd": "project",
    "timeout_secs": 120,
    "output_mode": "file",
    "result_json_path": null,
    "exit_code_map": { "2": "login_required", "6": "rate_limited", "9": "pending_review" },
    "health_argv": ["--version"]
  }
}
```

### `cli` Transport Rules (Zero Shell Injection)
- `command` is executed directly via `std::process::Command::new` (NEVER through `/bin/sh -c`). Leading `~/` expands via `$HOME`; bare names resolve on `PATH`.
- `argv` is a strict string array. Template placeholders: `{{prompt}}`, `{{ref_image}}`, `{{out_path}}`, `{{width}}`, `{{height}}`, `{{query}}`, `{{project_dir}}`, `{{name}}`, `{{card_id}}`, `{{character}}`, `{{show}}`, `{{secret:NAME}}`, and `{{arg:key}}`.
- Optional placeholder `{{?var}}`: when `var` is empty, omits that arg (and its immediately preceding `-flag` token if present).
- `stdin` is always `Stdio::null()`; bounded by `timeout_secs` (1..=600).
- `output_mode`:
  - `"file"` — adapter writes to `out_path` (validated project-relative); returns `{ path, bytes, sha256 }`.
  - `"stdout_json"` — parses stdout JSON (optionally extracting `result_json_path`).
  - `"stdout_path"` — reads trimmed output path from stdout.
- `exit_code_map`: maps non-zero exit codes to typed statuses (`login_required`, `rate_limited`, `refused`, `pending_review`, etc.).

### `http` Transport Rules (SSRF-Guarded + Async Polling)

```json
{
  "id": "fal-flux-http",
  "name": "fal.ai Flux Schnell",
  "capability": "image_gen",
  "transport": "http",
  "enabled": true,
  "priority": 50,
  "required_secrets": ["fal_api_key"],
  "http": {
    "method": "POST",
    "url_template": "https://queue.fal.run/fal-ai/flux/schnell",
    "allow_loopback": false,
    "headers_template": { "Authorization": "Key {{secret:fal_api_key}}" },
    "body_mode": "json",
    "body_template": { "prompt": "{{prompt}}", "image_size": { "width": 768, "height": 1024 } },
    "timeout_secs": 120,
    "poll_spec": {
      "url_template": "{{resp:status_url}}",
      "status_path": "status",
      "done_values": ["COMPLETED", "SUCCEEDED"],
      "fail_values": ["FAILED", "ERROR"],
      "result_url_path": "response_url",
      "interval_ms": 1500,
      "max_attempts": 60
    },
    "response_extract": { "mode": "image_url_path", "path": "images.0.url" },
    "health_url": null
  }
}
```

- Every URL (initial, redirect, poll, and downloaded `image_url_path`) passes SSRF validation (`http`/`https` only; blocks private/loopback/link-local IPs unless `allow_loopback: true` is explicitly set on the adapter).
- `body_mode`: `"json"` | `"multipart"` (`{ "file_field": "image", "file_path": "{{ref_image}}", "fields": { ... } }`) | `"none"`.
- `response_extract.mode`: `"json"` | `"image_b64_path"` | `"image_url_path"` | `"file_bytes"` | `"items_path"`.
- Higgsfield / Soul Cinema is rejected (`400`) on registration and invocation.

---

## 3. HTTP Endpoints

| Method + Path | Body / Query | Reply |
| --- | --- | --- |
| `GET /api/adapters` | `?capability=<cap>` (optional) | `{ ok, capabilities, adapters: [AdapterView], presets: [PresetView], templates: [Template] }` |
| `POST /api/adapters` | Full adapter JSON **or** `{ preset_id, enabled?, priority?, secret_values? }` | `200`/`201` `{ ok, adapter: AdapterView }` (`secret_values` saved `0600` to `SecretStore`, stripped from response) |
| `DELETE /api/adapters/:id` | | `{ ok, id, deleted: true }` (`404` if unknown) |
| `POST /api/adapters/:id/test` | `{}` | `{ ok, id, available, status, details, latency_ms, missing_secrets }` |
| `POST /api/adapters/invoke` | `{ capability?, adapter_id?, project?, params: { ... } }` | `{ ok, capability, adapter_id, attempts: [{ adapter_id, ok, status?, error? }], result }` |

`AdapterView` includes every adapter field plus computed status fields:
- `available: bool` — binary/endpoint detected and all `required_secrets` present
- `missing_secrets: string[]` — names of missing keys (never values)
- `secrets_status: { "<name>": { has_key: bool, masked: string|null } }`

---

## 4. Agent & Teamwork Tools

- `adapter_list` `{ capability?: "image_gen"|"reference_search"|"mesh_gen"|"audio_gen"|"custom" }` — lists installed enabled adapters with availability & priority.
- `adapter_invoke` `{ capability?: string, adapter_id?: string, params: object }` — invokes a specific adapter or the capability's priority fallback chain (records written `out_path` into Teamwork `written_files`).
- Standard capability calls (`art_generate`, `reference_search`, `mesh_generate`, `audio_generate`) automatically check installed enabled adapters in ascending `priority` order when `provider` is omitted/`"auto"` or matches an adapter `id`.

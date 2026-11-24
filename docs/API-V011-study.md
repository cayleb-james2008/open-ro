# OpenRO v0.11 — Study Ledger & Contract-Driven Checks (`#62`)

Loopback-only HTTP API (`403 Forbidden` for non-loopback `Host`) and Rust library contract in `crate::study_check` for `#63 Part B` (`openro-agent`, `openro-team`) and `#64` (`openro-ui`).

All investigation artifacts live inside `<project>/.openro/investigation/` (already gitignored and blocked from direct file writes):
- `evidence.jsonl` — append-only canonical JSONL evidence ledger (`ev_<16hex>`)
- `unknowns.json` — optimistic-concurrency unknowns ledger (`unk_<16hex>`, `revision`)
- `kept_checks.json` — SHA-256 project-fingerprinted cache of **passing** `check` and `playtest` rounds (failures are never cached)

---

## 1. Clean Room Manifest Setting (`openro.project.json`)

- `"clean_room": true` (boolean, default `true` in `projects::create` and `projects::manifest`).
- Updated via `POST /api/projects/{id}/manifest` with `{ "clean_room": true | false }` (`400` if non-boolean).
- Exposed on `GET /api/projects` summaries, `GET /api/projects/{id}`, and `GET /api/projects/{id}/study/status`.
- Meaning: `true` = external-surface observation only (public APIs, visual references, rendered playtests); `false` = deep inspection opted in.

---

## 2. Evidence Ledger

| Method + path | Query / Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/investigation/evidence` | `?confidence=observed\|derived\|inferred&kind=...&source=...&limit=100` | `200`: `{ ok, total, matched, counts_by_confidence: { observed, derived, inferred }, evidence: [...] }` |
| `POST /api/projects/{id}/investigation/evidence` | `{ summary, confidence?: "observed"\|"derived"\|"inferred", kind?, source?, authority?, coverage?, limitations?: [string], clean_room?: bool, refs?: [string], data?: {} }` | `201` (or `200` if deduplicated): `{ ok, deduplicated, evidence: { id: "ev_<16hex>", recorded_at_ms, kind, source, summary, confidence, authority, coverage, limitations, clean_room, refs, data } }` |

- `id` is `ev_<16hex>` derived from SHA-256 of the recursively key-sorted canonical JSON core fields (excluding `recorded_at_ms`), so identical observations deduplicate cleanly.
- Invalid `confidence` returns `400 Bad Request`.
- **Auto-recorded `observed` evidence:**
  - `market_taste::research_roblox_game_to_project*` automatically records `kind: "roblox_game_research"`, `confidence: "observed"`, `authority: "external_surface"` and returns `evidence_id`.
  - `playtest::run` automatically records `kind: "playtest"`, `confidence: "observed"`, `authority: "self_test"` with untested core-loop limitations and returns `evidence_id`.

---

## 3. Unknowns Ledger (Optimistic Concurrency + Fail-Closed Resolution)

| Method + path | Query / Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/investigation/unknowns` | `?status=open\|investigating\|resolved\|wont_fix&severity=critical\|high\|medium\|low&domain=...` | `200`: `{ ok, revision, total, open_count, critical_open, high_open, counts_by_status, counts_by_severity, unknowns: [...] }` |
| `POST /api/projects/{id}/investigation/unknowns` | `{ title, question?, severity?, domain?, status?, probes?: [...], evidence_for?: [...], evidence_against?: [...], resolution?, expected_revision? }` | `201`: `{ ok, revision, unknown: { id: "unk_<16hex>", revision: 1, ... } }` (`409` on duplicate id or ledger `expected_revision` mismatch) |
| `PATCH /api/projects/{id}/investigation/unknowns` or `PATCH /api/projects/{id}/investigation/unknowns/{unk_id}` | `{ id?, expected_revision?, expected_ledger_revision?, severity?, domain?, status?, probes?, add_probes?, evidence_for?, add_evidence_for?, evidence_against?, add_evidence_against?, resolution? }` | `200`: `{ ok, revision, unknown: { id, revision: n+1, ... } }` (`404` unknown not found, `409` revision mismatch) |

**Fail-closed resolution rules:**
1. Setting `status: "resolved"` requires a non-empty `resolution` string **and** a non-empty `evidence_for` list (`400 Bad Request` otherwise).
2. Any `ev_*` ID listed in `evidence_for` or `evidence_against` must exist in `evidence.jsonl` (`400 Bad Request` if missing/fabricated).

---

## 4. `mechanics.json` Provenance Checker

| Method + path | Body | Reply |
|---|---|---|
| `POST /api/projects/{id}/study/check-mechanics` | `{ path?: "docs/mechanics.json", strict?: false }` | `200`: `{ ok, found, passed, strict, path, counts: { total, sourced, estimated, unsourced, runtime_verified, config_only_warnings, config_mismatches }, warnings: [...], items: [...] }` |

- Defaults to checking `docs/mechanics.json`, then `mechanics.json` (custom `path` is jailed via `projects::safe_join`; escapes return `400`).
- Counts `sourced`, `estimated`, and `unsourced` entries; downgrades any `"sourced"` entry lacking a `source`/`evidence_id` or citing a missing `ev_*` ID to `unsourced` with a warning.
- Emits a warning whenever a mechanic cites literal config (`config_path`/`config_value`/`config`) without `verified_runtime: true` (`"literal config != verified runtime behavior"`), or fails when `config_value != runtime_value`.

---

## 5. Study Status (`Study -> Build -> Check -> Ready`)

| Method + path | Reply |
|---|---|
| `GET /api/projects/{id}/study/status` | `{ ok, stage: "study"\|"build"\|"check"\|"ready", stages: [{ id, label, done }, ...], clean_room, fingerprint, evidence: { total, observed, derived, inferred }, unknowns: { revision, total, open, investigating, resolved, wont_fix, open_count, critical_open, high_open }, mechanics: { found, passed, path, counts, warnings }, checks: { static_check_kept, playtest_kept, last_playtest } }` |

---

## 6. Contract-Driven Checks on `/check` & `/playtest`

- **Playtest core-loop claim evaluator (`report.core_loop`):**
  - Evaluates `move`, `act`, `hud` (via explicit `check.claim`/`check.category`/`check.step` tags or conservative name classification) plus `suite_steps`.
  - Uncovered claims report `"status": "not tested"` (`"tested": false`).
  - Optional `"require_core_loop": true` on `POST /api/projects/{id}/playtest` (or playtest suite JSON) fails `report.passed` unless all three (`move`, `act`, `hud`) are tested and passing.
- **Blank/single-color PNG rejection & `640x360` visual likeness (`report.blank_screenshots`, `report.visual_likeness`):**
  - Every captured PNG is decoded in pure Rust; corrupt, single-color, or near-blank captures are rejected (`shot.rejected = true`, `shot.passed = false`) and fail `report.passed`.
  - Non-blank screenshots are normalized to `640x360` and scored (`0.0..=1.0`, `best_pct: 0..=100`) against thumbnails in `refs/<universeId>/` (or first populated `refs/*/` folder) when present.
- **`KeptChecks` SHA-256 project fingerprinting (`kept`, `cached`, `fingerprint`, `force`):**
  - `POST /api/projects/{id}/check` and `POST /api/projects/{id}/playtest` compute a deterministic SHA-256 across project sources, manifests, configs, and mechanics files combined with the request mode/spec.
  - Unchanged inputs reuse the last **passing** result (`kept: true, cached: true`).
  - **Failures are NEVER cached** (and evict any stale entry for that key).
  - Pass `"force": true` to bypass `KeptChecks` and force a fresh run.

---

## 7. Rust Library Exports (`crate::study_check`) for `#63 Part B`

- `study_check::record_evidence(project_dir: &Path, input: &Value) -> Result<Value, StudyError>`
- `study_check::list_evidence(project_dir: &Path, filter: &EvidenceFilter) -> Result<Value, StudyError>`
- `study_check::list_unknowns(project_dir: &Path, status: Option<&str>, severity: Option<&str>, domain: Option<&str>) -> Result<Value, StudyError>`
- `study_check::add_unknown(project_dir: &Path, input: &Value) -> Result<Value, StudyError>`
- `study_check::update_unknown(project_dir: &Path, id: &str, patch: &Value) -> Result<Value, StudyError>`
- `study_check::check_mechanics(project_dir: &Path, rel_path: Option<&str>, strict: bool) -> Result<Value, StudyError>`
- `study_check::study_status(project_dir: &Path) -> Result<Value, StudyError>`
- `study_check::project_fingerprint(project_dir: &Path) -> Result<String, String>`
- `projects::check_project_with_full_options(dir: &Path, format: bool, format_paths: Option<&[String]>, force: bool) -> Result<Value, String>`

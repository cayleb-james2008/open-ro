# OpenRO v0.11 API — Stylized Blender Mesh Pipeline

Headless Blender mesh pipeline producing Roblox-ready, UV-unwrapped, Cycles-baked props (colour multiplied by ambient occlusion with a warm shadow tint), optional separate glow meshes, bounding-box/anchor manifests, and 2-angle preview PNGs.

## Endpoints

### `GET /api/tools/blender/status`

Checks whether headless Blender is installed and returns pipeline limits and supported shapes.

**Response (`200 OK`)**
```json
{
  "ok": true,
  "available": true,
  "blender_detected": true,
  "blender_path": "/usr/bin/blender",
  "blender_version": "Blender 5.2.2 LTS",
  "max_tris": 20000,
  "script_timeout_secs": 90,
  "supported_shapes": ["box", "cylinder", "cone", "sphere", "wedge", "torus"],
  "modes": ["parts", "script", "preset"],
  "units": "studs",
  "up_axis": "Y"
}
```

---

### `POST /api/tools/blender/generate` (loopback only)

Routes automatically based on the request body:
- **New mesh pipeline (`src/mesh.rs`)** when `parts` or `script` is present.
- **Legacy preset generator (`src/library.rs`)** when neither `parts` nor `script` is present (preserves existing `{ "preset": "torii_gate", ... }` callers).

#### Mode A: Structured `parts` description

Coordinates are **Y-up in Roblox studs** (`X` right, `Y` up, `Z` toward the viewer), with Euler rotations in degrees (`XYZ`).

```json
{
  "name": "shrine_lantern",
  "bevel": 0.03,
  "texture_size": 1024,
  "ao_strength": 0.75,
  "preview": true,
  "preview_size": 480,
  "out_dir": "hub/props",
  "anchors": [
    { "name": "orb", "pos": [0.0, 5.5, 0.0] }
  ],
  "parts": [
    {
      "shape": "box",
      "size": [6.0, 1.0, 4.0],
      "pos": [0.0, 0.5, 0.0],
      "rot": [0.0, 0.0, 0.0],
      "color": "#8b8378",
      "glow": false
    },
    {
      "shape": "sphere",
      "size": [1.0, 1.0, 1.0],
      "pos": [0.0, 5.5, 0.0],
      "color": "#55eeff",
      "glow": true,
      "segments": 14
    }
  ]
}
```

| Field | Type | Default | Notes |
| --- | --- | --- | --- |
| `name` | string | `"mesh"` | Slugified to `[a-z0-9_-]` (max 64 chars). |
| `parts` | array | — | Up to 500 parts. Each part requires `shape`. |
| `parts[].shape` | string | required | `box`, `cylinder`, `cone`, `sphere`, `wedge`, or `torus`. |
| `parts[].size` | `[x, y, z]` | `[1, 1, 1]` | Positive studs (`0 < v <= 10000`). For `torus`, `[outer_x, tube_y, outer_z]` (rotate `[90, 0, 0]` to stand upright). |
| `parts[].pos` | `[x, y, z]` | `[0, 0, 0]` | Part center in studs (Y-up). |
| `parts[].rot` | `[rx, ry, rz]` | `[0, 0, 0]` | Euler rotation in degrees. |
| `parts[].color` | hex string | `"#b8b8c0"` | `#RGB` or `#RRGGBB`. |
| `parts[].glow` | bool | `false` | Parts with `glow: true` are exported as a separate `<name>_glow.obj` + `<name>_glow.png` so Roblox can assign `Enum.Material.Neon`. |
| `parts[].segments` | integer | shape default | `3..=64` segments for curved shapes. |
| `parts[].tube` | number | `size[1]` | Optional explicit tube thickness in studs for `torus`. |
| `anchors` | array | `[]` | Optional `[{ "name": "...", "pos": [x, y, z] }]`. `base-centre`, `top-centre`, and `centre` are always added automatically. |
| `bevel` | number | `0.03` | Edge bevel width in studs (`0..=2`). Set `0` for sharp un-beveled edges. |
| `texture_size` | integer | `1024` | Square baked PNG size (`64..=2048`). |
| `ao_strength` | number | `0.75` | Ambient occlusion shading weight (`0..=1`). |
| `preview` | bool | `true` | Render 2-angle Cycles preview PNGs (`<name>_preview_0.png`, `<name>_preview_1.png`). Alias: `render_preview`. |
| `preview_size` | integer | `480` | Square preview resolution (`64..=1024`). |
| `out_dir` | string | omitted | Optional relative path under `<data_dir>/meshes-out` (no `..`, leading `/`, or drive letters). When omitted, writes to `<data_dir>/game-assets/meshes`. |
| `timeout_secs` | integer | `90` | Execution cap (`1..=90` seconds). |

#### Mode B: Agent-written `bpy` `script`

Provide `"script": "..."` (alone or alongside `parts`). Scripts run in headless Blender (`Z`-up Blender scene space; export automatically converts to Roblox `Y`-up studs) with `bpy`, `bmesh`, `math`, `np`, `Vector`, `Matrix`, `Euler`, `add_part(...)`, and `material_for("#rrggbb")` pre-imported in scope.

- Any mesh object whose name starts or ends with `glow` (case-insensitive) or has custom property `ob["glow"] = True` is placed in the separate `<name>_glow` mesh.
- Any `EMPTY` object whose name starts with `anchor` (e.g. `anchor_orb`) is recorded in `manifest.anchors` (converted to Y-up studs).

#### Pipeline & Limits

1. **Build** structured parts and/or execute the `bpy` script in an isolated scratch working directory.
2. **Triangle cap (hard fail, `400 Bad Request`)**: meshes exceeding **20,000 triangles** (`solid + glow`) are rejected in plain words before unwrapping or baking, and no output files are written.
3. **UV unwrap**: Smart UV Project + average island scale + island pack.
4. **Cycles texture bake**: `DIFFUSE (COLOR)` pass multiplied with `AO` pass using a warm shadow tint (`[1.0, 0.82, 0.70]`), saved as `<name>.png` (and `<name>_glow.png` without AO when glow parts exist).
5. **OBJ export**: triangulated, Y-up (`forward=-Z, up=Y`), 1 unit = 1 stud, with normals and UV `vt` coordinates.
6. **Manifest**: writes `<name>.manifest.json` and atomically merges `<name>` into `<out_dir>/manifest.json`.
7. **Previews**: renders 2 angles (`elev=28°, yaw=30°` and `elev=28°, yaw=150°`) with the baked texture applied.

#### Script Safety (best-effort)

Agent-supplied `script` bodies are protected by three best-effort controls:
1. **Static denylist** checked before Blender starts: scripts containing `os.system`, `subprocess`, `socket`, `urllib`, `shutil.rmtree`, `__import__`, `os.popen`, `os.exec`, `os.spawn`, `importlib`, `ctypes`, or `http.client` are refused with `400 Bad Request`. (This denylist is best-effort, not a hardened OS sandbox.)
2. **Timeout**: hard-stopped after `90` seconds (enforced both inside Python and by killing the Blender child process from Rust).
3. **Scratch working directory**: Blender runs with `cwd` inside a per-run scratch directory under `<data_dir>/meshes-scratch/`, which is deleted when the request finishes. Outputs are staged and verified before moving to the output directory.

#### Response (`200 OK`)

```json
{
  "ok": true,
  "mode": "structured",
  "name": "shrine_lantern",
  "blender_used": true,
  "blender_version": "Blender 5.2.2 LTS",
  "units": "studs",
  "up_axis": "Y",
  "out_dir": "/path/to/data/game-assets/meshes",
  "out_dir_rel": null,
  "tris": 1032,
  "solid_tris": 876,
  "glow_tris": 156,
  "max_tris": 20000,
  "size": [6.0, 6.0, 4.0],
  "bbox": { "min": [-3.0, 0.0, -2.0], "max": [3.0, 6.0, 2.0] },
  "anchors": [
    { "name": "orb", "pos": [0.0, 5.5, 0.0] },
    { "name": "base-centre", "pos": [0.0, 0.0, 0.0] },
    { "name": "top-centre", "pos": [0.0, 6.0, 0.0] },
    { "name": "centre", "pos": [0.0, 3.0, 0.0] }
  ],
  "obj_file": "shrine_lantern.obj",
  "obj_path": "/path/to/data/game-assets/meshes/shrine_lantern.obj",
  "obj_url": "/api/assets/meshes/shrine_lantern.obj",
  "texture_file": "shrine_lantern.png",
  "texture_path": "/path/to/data/game-assets/meshes/shrine_lantern.png",
  "texture_url": "/api/assets/meshes/shrine_lantern.png",
  "glow_obj_file": "shrine_lantern_glow.obj",
  "glow_obj_path": "/path/to/data/game-assets/meshes/shrine_lantern_glow.obj",
  "glow_obj_url": "/api/assets/meshes/shrine_lantern_glow.obj",
  "glow_texture_file": "shrine_lantern_glow.png",
  "glow_texture_path": "/path/to/data/game-assets/meshes/shrine_lantern_glow.png",
  "glow_texture_url": "/api/assets/meshes/shrine_lantern_glow.png",
  "manifest_file": "shrine_lantern.manifest.json",
  "manifest_path": "/path/to/data/game-assets/meshes/shrine_lantern.manifest.json",
  "manifest_url": "/api/assets/meshes/shrine_lantern.manifest.json",
  "dir_manifest_file": "manifest.json",
  "dir_manifest_path": "/path/to/data/game-assets/meshes/manifest.json",
  "dir_manifest_url": "/api/assets/meshes/manifest.json",
  "previews": [
    {
      "angle": 0,
      "file": "shrine_lantern_preview_0.png",
      "path": "/path/to/data/game-assets/meshes/shrine_lantern_preview_0.png",
      "url": "/api/assets/meshes/shrine_lantern_preview_0.png"
    },
    {
      "angle": 1,
      "file": "shrine_lantern_preview_1.png",
      "path": "/path/to/data/game-assets/meshes/shrine_lantern_preview_1.png",
      "url": "/api/assets/meshes/shrine_lantern_preview_1.png"
    }
  ],
  "preview_url": "/api/assets/meshes/shrine_lantern_preview_0.png",
  "manifest": { "...": "full manifest entry" },
  "elapsed_ms": 6200
}
```

---

### `GET /api/assets/meshes/:file`

Serves `.obj` (`text/plain; charset=utf-8`), `.png` (`image/png`), and `.json` (`application/json; charset=utf-8`) files:
- Flat filenames (`GET /api/assets/meshes/shrine_lantern.obj`, `manifest.json`, `shrine_lantern_preview_0.png`) are read from `<data_dir>/game-assets/meshes/`.
- Files generated with `out_dir` (`GET /api/assets/meshes/out/<out_dir>/<file>`) are read from `<data_dir>/meshes-out/<out_dir>/`.
- Path traversal (`..`), backslashes, NUL bytes, symlink escapes, and unsupported file extensions are refused (`400`/`403`).

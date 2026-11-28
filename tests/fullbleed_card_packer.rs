//! TDD verification for Gap #87A (true 100% edge-to-edge full-bleed card packer)
//! and Gap #87B (sleek edge-hugging `card_borders_sheet` + prompt guidance).

use openro_rs::image_ops::pack_card;
use openro_rs::secrets::SecretStore;
use openro_rs::ui_kit::{
    build_ui_sheet_prompt, generate_ui_kit, presets_json, prompt_guidance, sheet_recipes,
    visual_skins,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn scratch(label: &str) -> PathBuf {
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("fullbleed-card-packer-tests")
        .join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_py(script: &str, args: &[&Path]) -> Value {
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(script);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().expect("run python3 PIL helper");
    assert!(
        out.status.success(),
        "python3 helper failed:\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "parse python3 json failed: {e}\nraw: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

#[test]
fn fullbleed_pack_card_art_fills_edge_to_edge_upper_third_across_aspect_ratios() {
    let dir = scratch("art-edge");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("art/raw")).unwrap();

    // Generate 3 aspect ratios with distinct upper-third warm gold/coral band (face region)
    // and vivid cyan/indigo lower body — zero black borders anywhere.
    let gen_py = r#"
import json, sys
from PIL import Image, ImageDraw
for path, w, h in [
    (sys.argv[1], 1024, 1024),
    (sys.argv[2], 1600, 900),
    (sys.argv[3], 1024, 1792),
]:
    im = Image.new("RGBA", (w, h))
    px = im.load()
    for y in range(h):
        ny = y / max(1, h - 1)
        for x in range(w):
            nx = x / max(1, w - 1)
            if 0.14 <= ny <= 0.36:
                px[x, y] = (245, 190, 75, 255)
            else:
                r = int(45 + 150 * nx)
                g = int(85 + 120 * (1.0 - ny))
                b = int(175 + 70 * ny)
                px[x, y] = (r, g, b, 255)
    im.save(path)
print(json.dumps({"ok": True}))
"#;
    let sq = proj.join("art/raw/sq.png");
    let wide = proj.join("art/raw/wide.png");
    let tall = proj.join("art/raw/tall.png");
    run_py(gen_py, &[&sq, &wide, &tall]);

    let inspect_py = r#"
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
w, h = im.size
px = im.load()
min_a = 255
black_rows = 0
for y in range(h):
    row_black = True
    for x in range(w):
        r, g, b, a = px[x, y]
        if a < min_a:
            min_a = a
        if r > 15 or g > 15 or b > 15:
            row_black = False
    if row_black:
        black_rows += 1
black_cols = 0
for x in range(w):
    col_black = True
    for y in range(h):
        r, g, b, _ = px[x, y]
        if r > 15 or g > 15 or b > 15:
            col_black = False
            break
    if col_black:
        black_cols += 1
edge_samples = [px[0, 0], px[w - 1, 0], px[0, h - 1], px[w - 1, h - 1]]
# Measure fraction of gold/coral face-band pixels in the upper half
warm_upper = 0
for y in range(0, h // 2):
    r, g, b, _ = px[w // 2, y]
    if r >= 220 and g >= 165 and b <= 110:
        warm_upper += 1
print(json.dumps({
    "w": w,
    "h": h,
    "min_alpha": min_a,
    "black_rows": black_rows,
    "black_cols": black_cols,
    "edge_samples": edge_samples,
    "warm_upper": warm_upper,
}))
"#;

    for (card_id, rel) in [
        ("card_sq", "art/raw/sq.png"),
        ("card_wide", "art/raw/wide.png"),
        ("card_tall", "art/raw/tall.png"),
    ] {
        let res = pack_card(
            &proj,
            &json!({
                "card_id": card_id,
                "source_image": rel,
            }),
        )
        .unwrap();
        assert_eq!(res["ok"], true);
        let art_path = proj.join(format!("art/cards/{card_id}/art.png"));
        let icon_path = proj.join(format!("art/cards/{card_id}/icon.png"));
        let art_info = run_py(inspect_py, &[&art_path]);
        assert_eq!(art_info["w"], 512);
        assert_eq!(art_info["h"], 712);
        assert_eq!(art_info["min_alpha"], 255, "{card_id} art must be 100% opaque");
        assert_eq!(art_info["black_rows"], 0, "{card_id} art has black letterbox rows: {art_info}");
        assert_eq!(art_info["black_cols"], 0, "{card_id} art has black pillarbox cols: {art_info}");
        assert!(
            art_info["warm_upper"].as_u64().unwrap() >= 40,
            "{card_id} art upper-third anchor missed upper face band: {art_info}"
        );

        let icon_info = run_py(inspect_py, &[&icon_path]);
        assert_eq!(icon_info["w"], 256);
        assert_eq!(icon_info["h"], 256);
        assert_eq!(icon_info["min_alpha"], 255);
        assert_eq!(icon_info["black_rows"], 0);
        assert_eq!(icon_info["black_cols"], 0);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fullbleed_pack_card_octagonal_frame_has_transparent_exterior_and_fullbleed_interior() {
    let dir = scratch("oct-frame");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("art/raw")).unwrap();
    std::fs::create_dir_all(proj.join("art/ui")).unwrap();

    // Colorful full-bleed source art + ornamental octagonal frame (transparent outer corners + transparent center).
    let setup_py = r#"
import json
from PIL import Image, ImageDraw
import sys

src = Image.new("RGBA", (1024, 1024))
spx = src.load()
for y in range(1024):
    for x in range(1024):
        spx[x, y] = (
            60 + int(170 * x / 1023),
            90 + int(140 * y / 1023),
            210 - int(110 * x / 1023),
            255,
        )
src.save(sys.argv[1])

w, h, cut = 512, 712, 64
fr = Image.new("RGBA", (w, h), (0, 0, 0, 0))
d = ImageDraw.Draw(fr)
outer = [
    (cut, 8), (w - cut, 8), (w - 8, cut), (w - 8, h - cut),
    (w - cut, h - 8), (cut, h - 8), (8, h - cut), (8, cut),
]
inner = [
    (cut + 14, 24), (w - cut - 14, 24), (w - 24, cut + 14), (w - 24, h - cut - 14),
    (w - cut - 14, h - 24), (cut + 14, h - 24), (24, h - cut - 14), (24, cut + 14),
]
d.polygon(outer, fill=(245, 195, 55, 255))
d.polygon(inner, fill=(0, 0, 0, 0))
fr.save(sys.argv[2])
print(json.dumps({"ok": True}))
"#;
    let src_path = proj.join("art/raw/hero.png");
    let frame_path = proj.join("art/ui/oct_frame.png");
    run_py(setup_py, &[&src_path, &frame_path]);

    // Default transparent_exterior = true
    let res = pack_card(
        &proj,
        &json!({
            "card_id": "oct_hero",
            "source_image": "art/raw/hero.png",
            "frame_image": "art/ui/oct_frame.png",
        }),
    )
    .unwrap();
    assert_eq!(res["ok"], true);

    let verify_py = r#"
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
w, h = im.size
px = im.load()
corners = [px[0, 0], px[w - 1, 0], px[0, h - 1], px[w - 1, h - 1], px[14, 14]]
interior_pts = [px[w // 2, h // 2], px[36, h // 2], px[w - 36, h // 2], px[w // 2, 36], px[w // 2, h - 36]]
border_pt = px[w // 2, 16]
print(json.dumps({
    "w": w,
    "h": h,
    "corner_alphas": [c[3] for c in corners],
    "interior_alphas": [p[3] for p in interior_pts],
    "interior_luma": [p[0] + p[1] + p[2] for p in interior_pts],
    "border_pt": border_pt,
}))
"#;
    let full_path = proj.join("art/cards/oct_hero/full.png");
    let info = run_py(verify_py, &[&full_path]);
    assert_eq!(info["w"], 512);
    assert_eq!(info["h"], 712);
    for a in info["corner_alphas"].as_array().unwrap() {
        assert_eq!(a.as_u64().unwrap(), 0, "exterior corner outside octagonal frame must be alpha=0: {info}");
    }
    for a in info["interior_alphas"].as_array().unwrap() {
        assert_eq!(a.as_u64().unwrap(), 255, "interior inside frame must be 100% opaque art: {info}");
    }
    for lum in info["interior_luma"].as_array().unwrap() {
        assert!(lum.as_u64().unwrap() >= 250, "interior must contain colorful source art, not black void: {info}");
    }
    let bp = info["border_pt"].as_array().unwrap();
    assert!(bp[0].as_u64().unwrap() > 200 && bp[3].as_u64().unwrap() == 255, "frame border must composite cleanly: {info}");

    // Also verify explicit transparent_exterior=false preserves corner art
    let res_opaque = pack_card(
        &proj,
        &json!({
            "card_id": "oct_hero_opaque",
            "source_image": "art/raw/hero.png",
            "frame_image": "art/ui/oct_frame.png",
            "transparent_exterior": false,
        }),
    )
    .unwrap();
    assert_eq!(res_opaque["ok"], true);
    let opaque_full = proj.join("art/cards/oct_hero_opaque/full.png");
    let info_op = run_py(verify_py, &[&opaque_full]);
    for a in info_op["corner_alphas"].as_array().unwrap() {
        assert_eq!(a.as_u64().unwrap(), 255);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn card_borders_sheet_generates_7_sleek_edge_hugging_borders_and_packs_cleanly() {
    let dir = scratch("card-borders");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("art/raw")).unwrap();
    let store = SecretStore::for_data_dir(&dir);

    // 1. Recipe & prompt checks across all 5 visual skins
    let recipes = sheet_recipes();
    assert_eq!(recipes.len(), 5, "expected 5 recipes including card_borders_sheet + panels_sheet");
    let borders = recipes.iter().find(|r| r.id == "card_borders_sheet").expect("card_borders_sheet");
    assert_eq!(borders.items.len(), 7);
    let presets = presets_json();
    assert_eq!(presets["sheets"].as_array().unwrap().len(), 5);

    for skin in visual_skins() {
        let prompt = build_ui_sheet_prompt(skin.id, "card_borders_sheet").unwrap();
        assert!(!prompt.contains('\n'), "prompt must be single-line");
        assert!(prompt.contains("#00FF00"), "card_borders_sheet prompt must use chroma-green: {prompt}");
        let low = prompt.to_ascii_lowercase();
        assert!(low.contains("zero text") && !low.contains("anrx") && !low.contains("anime nexus") && !low.contains("rune"), "prompt leaked forbidden terms: {prompt}");
    }

    // 2. Procedural 2x Pillow generation + auto-slicing into art/ui/card-borders/ + art/ui/slices.json
    let gen = generate_ui_kit(
        &proj,
        &store,
        None,
        &json!({
            "skin": "cyber-forge",
            "sheet": "card_borders_sheet",
            "procedural_fallback": true,
        }),
    )
    .unwrap();
    assert_eq!(gen["ok"], true);
    assert_eq!(gen["sliced_count"], 7);
    assert_eq!(gen["nine_slice_count"], 7);
    assert_eq!(gen["slices_json"], "art/ui/slices.json");

    let slices_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("art/ui/slices.json")).unwrap())
            .unwrap();

    let inspect_border_py = r#"
import hashlib, json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
w, h = im.size
px = im.load()
alpha = im.getchannel("A")
hist = alpha.histogram()
opaque = sum(hist[17:])
total = w * h
transparent_ratio = (total - opaque) / total
# Measure mid-side border thickness along horizontal & vertical centerlines
left_t = sum(1 for x in range(w // 2) if px[x, h // 2][3] > 24)
right_t = sum(1 for x in range(w // 2, w) if px[x, h // 2][3] > 24)
top_t = sum(1 for y in range(h // 2) if px[w // 2, y][3] > 24)
bot_t = sum(1 for y in range(h // 2, h) if px[w // 2, y][3] > 24)
sha = hashlib.sha256(im.tobytes()).hexdigest()
print(json.dumps({
    "w": w,
    "h": h,
    "corner_alpha": px[0, 0][3],
    "center_alpha": px[w // 2, h // 2][3],
    "transparent_ratio": round(transparent_ratio, 4),
    "side_thickness": [left_t, right_t, top_t, bot_t],
    "sha": sha,
}))
"#;

    let tiers = [
        "border_common",
        "border_uncommon",
        "border_rare",
        "border_epic",
        "border_legendary",
        "border_mythic",
        "border_legacy",
    ];
    let mut seen_hashes = BTreeSet::new();
    for tier in tiers {
        let rel = format!("art/ui/card-borders/{tier}.png");
        let abs = proj.join(&rel);
        assert!(abs.is_file(), "missing sliced border {rel}");

        let slice_key = format!("card-borders/{tier}");
        let entry = &slices_doc[&slice_key];
        let sl = entry["slice"].as_array().unwrap_or_else(|| panic!("missing SliceCenter for {slice_key} in {slices_doc}"));
        let l = sl[0].as_i64().unwrap();
        let t = sl[1].as_i64().unwrap();
        let r = sl[2].as_i64().unwrap();
        let b = sl[3].as_i64().unwrap();
        assert!(0 < l && l < r && r < 512, "invalid X SliceCenter for {slice_key}: {sl:?}");
        assert!(0 < t && t < b && b < 712, "invalid Y SliceCenter for {slice_key}: {sl:?}");

        let info = run_py(inspect_border_py, &[&abs]);
        assert_eq!(info["w"], 512);
        assert_eq!(info["h"], 712);
        assert_eq!(info["corner_alpha"], 0, "{tier} outer corner must be transparent: {info}");
        assert_eq!(info["center_alpha"], 0, "{tier} center window must be transparent: {info}");
        let tr = info["transparent_ratio"].as_f64().unwrap();
        assert!(tr >= 0.88, "{tier} must leave >=88% transparent window, got {tr}: {info}");
        let st = info["side_thickness"].as_array().unwrap();
        assert!(st[0].as_u64().unwrap() <= 40 && st[1].as_u64().unwrap() <= 40, "{tier} side border > 8% width: {info}");
        assert!(st[2].as_u64().unwrap() <= 56 && st[3].as_u64().unwrap() <= 56, "{tier} top/bottom border > 8% height: {info}");
        seen_hashes.insert(info["sha"].as_str().unwrap().to_string());
    }
    assert_eq!(seen_hashes.len(), 7, "all 7 rarity borders must have distinct pixel geometry/colors");

    // 3. Pack a card with `border_mythic.png` -> transparent outer corners + full-bleed interior
    let src_gen_py = r#"
import json, sys
from PIL import Image
im = Image.new("RGBA", (1024, 1024), (240, 140, 60, 255))
im.save(sys.argv[1])
print(json.dumps({"ok": True}))
"#;
    let raw_src = proj.join("art/raw/card_art.png");
    run_py(src_gen_py, &[&raw_src]);

    let packed = pack_card(
        &proj,
        &json!({
            "card_id": "mythic_test",
            "source_image": "art/raw/card_art.png",
            "frame_image": "art/ui/card-borders/border_mythic.png",
        }),
    )
    .unwrap();
    assert_eq!(packed["ok"], true);
    let check_full_py = r#"
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
px = im.load()
print(json.dumps({
    "corner": px[0, 0],
    "center": px[256, 356],
}))
"#;
    let full_info = run_py(check_full_py, &[&proj.join("art/cards/mythic_test/full.png")]);
    assert_eq!(full_info["corner"][3], 0, "packed full.png corner outside border_mythic must be alpha=0: {full_info}");
    assert_eq!(full_info["center"][3], 255, "packed full.png center must be opaque art: {full_info}");
    assert!(full_info["center"][0].as_u64().unwrap() > 200, "packed full.png center must show source art: {full_info}");

    // 4. Prompt guidance checks
    let guidance = prompt_guidance();
    assert!(guidance.contains("UDim2.fromScale(1, 1)"));
    assert!(guidance.contains("ScaleType.Crop"));
    assert!(guidance.contains("card-borders/"));
    assert!(guidance.contains("Anime Nexus RNG X"));
    assert!(guidance.contains("hotbar"));
    let g_low = guidance.to_ascii_lowercase();
    assert!(!g_low.contains("rune") && !g_low.contains("unmistakably recognizable") && !g_low.contains("anime-homage"));

    let _ = std::fs::remove_dir_all(&dir);
}

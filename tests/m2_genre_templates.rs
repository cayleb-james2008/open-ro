//! M2 PR B integration & verification suite:
//! - `Idea` + `MarketTaste` extended with `FeelProfile` and `template_id`
//! - 3 complete 100% original Feel-Proven Genre Templates:
//!   1. `anime-card-battles-apex` (`anime-rng-cards`, "Anime RNG Card Battler")
//!   2. `viral-sim-tycoon-template` (`viral-sim-tycoon`, "Viral Sim & Tycoon")
//!   3. `anime-action-rpg-td-template` (`anime-action-rpg-td`, "Anime Action RPG / Tower Defense")
//! - Clean Luau generation (static prover 0 errors/warnings, BindToClose, FeelProfile timings)
//! - `gui_designer::template_screen_doc` passing safe-area + 44x44 + >=4.5:1 contrast across 3 viewports
//! - `POST /api/ideas/build` saving `ScreenDoc` into `GuiStore`, persisting Headless Lune Sandbox proof
//!   on disk, evaluating Economy Sim (`balanced: true`, `blocker_count: 0`), and returning `template_proof`.

use openro_rs::agent::Agent;
use openro_rs::audio_synth::AUDIO_PRESETS;
use openro_rs::gui_designer::{analyze_safe_areas, template_screen_doc, GuiNode};
use openro_rs::ideas;
use openro_rs::luau_check::analyze_place;
use openro_rs::market_taste;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::playtest::evaluate_economy_sim;
use openro_rs::sandbox::find_lune_binary;
use openro_rs::server::{serve, Shared};
use openro_rs::skills;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

const TEMPLATE_SPECS: [(&str, &str, &str); 3] = [
    ("anime-card-battles-apex", "anime-rng-cards", "Anime RNG Card Battler"),
    ("viral-sim-tycoon-template", "viral-sim-tycoon", "Viral Sim & Tycoon"),
    ("anime-action-rpg-td-template", "anime-action-rpg-td", "Anime Action RPG / Tower Defense"),
];

const FORBIDDEN_ORIGINALITY_TERMS: &[&str] = &[
    "shibuya",
    "shikigami",
    "domain curse",
    "samehada",
    "chakra",
    "jutsu",
    "bankai",
    "super saiyan",
    "pirate king",
];

fn temp_dir(label: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("m2-genre-templates-test");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn start_server(data_dir: PathBuf) -> (u16, std::sync::Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:9/v1".into(), "stub-model".into(), None),
        LocalPlace::in_memory("TemplateTest".into()),
    );
    let shared = Shared::with_data_dir(Some(agent), "TemplateTest".to_string(), data_dir);
    let s_clone = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, s_clone);
    });
    (port, shared)
}

fn api_json(port: u16, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let payload = body.map(|v| v.to_string()).unwrap_or_default();
    let ct = if payload.is_empty() {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{ct}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(req.as_bytes()).expect("write");
    stream.flush().ok();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http header separator");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let val = serde_json::from_slice(&raw[split + 4..]).unwrap_or_else(|_| {
        json!({ "raw": String::from_utf8_lossy(&raw[split + 4..]).to_string() })
    });
    (status, val)
}

fn hex_to_rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()? as f64 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).ok()? as f64 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).ok()? as f64 / 255.0;
    Some((r, g, b))
}

fn rel_luminance((r, g, b): (f64, f64, f64)) -> f64 {
    let lin = |c: f64| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast_ratio(fg_hex: &str, bg_hex: &str) -> f64 {
    let l1 = rel_luminance(hex_to_rgb(fg_hex).expect("valid fg hex"));
    let l2 = rel_luminance(hex_to_rgb(bg_hex).expect("valid bg hex"));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

fn assert_node_wcag_contrast(node: &GuiNode, inherited_bg: &str) {
    let bg_trans = node
        .props
        .get("BackgroundTransparency")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let effective_bg = if bg_trans < 0.85 {
        node.props
            .get("BackgroundColor3")
            .and_then(Value::as_str)
            .unwrap_or(inherited_bg)
    } else {
        inherited_bg
    };

    if matches!(node.class_name.as_str(), "TextLabel" | "TextButton") {
        if let Some(fg) = node.props.get("TextColor3").and_then(Value::as_str) {
            let ratio = contrast_ratio(fg, effective_bg);
            assert!(
                ratio >= 4.5,
                "node '{}' ({}) contrast {ratio:.2}:1 (<4.5:1) for {fg} on {effective_bg}",
                node.name,
                node.id
            );
        }
    }
    for child in &node.children {
        assert_node_wcag_contrast(child, effective_bg);
    }
}

#[test]
fn three_genre_templates_have_complete_feel_profiles_and_market_taste_links() {
    let opening = ideas::opening_ideas();
    for (id, niche_id, title) in TEMPLATE_SPECS {
        let idea = opening
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("missing opening idea {id}"));
        assert_eq!(idea.title, title);
        assert_eq!(idea.niche_id, Some(niche_id));
        let fp = idea
            .feel_profile
            .as_ref()
            .unwrap_or_else(|| panic!("idea {id} missing feel_profile"));

        assert!(!fp.reveal_tiers.is_empty(), "{id} missing reveal_tiers");
        assert!(!fp.gui_screen_slug.is_empty(), "{id} missing gui_screen_slug");
        assert!(!fp.audio_cues.is_empty(), "{id} missing audio_cues");
        for cue in fp.audio_cues {
            assert!(
                AUDIO_PRESETS.contains(cue),
                "{id} uses non-canonical audio cue {cue:?}"
            );
        }
        assert!(
            fp.hud_roles.min_tap_target_px >= 44,
            "{id} min_tap_target_px < 44"
        );
        assert!(!fp.playtest_assertions.is_empty(), "{id} missing playtest_assertions");
        assert!(!fp.world_direction.landmarks.is_empty(), "{id} missing world landmarks");

        let profile = market_taste::find_niche_profile(niche_id)
            .unwrap_or_else(|| panic!("missing niche profile {niche_id}"));
        assert_eq!(
            profile.template_id,
            Some(id),
            "niche {niche_id} must link template_id -> {id}"
        );
        let dossier = market_taste::build_niche_dossier_json(profile, &[]);
        assert_eq!(dossier["template_id"], json!(id));

        let pitch_low = format!("{} {}", idea.title, idea.pitch).to_ascii_lowercase();
        for bad in FORBIDDEN_ORIGINALITY_TERMS {
            assert!(
                !pitch_low.contains(bad),
                "{id} title/pitch leaked forbidden term {bad:?}"
            );
        }
    }

    // Specific FEEL-BIBLE invariants for Template 1
    let card_idea = ideas::find("anime-card-battles-apex").unwrap();
    let fp = card_idea.feel_profile.as_ref().unwrap();
    let durations: Vec<u32> = fp.reveal_tiers.iter().map(|t| t.duration_ms).collect();
    assert_eq!(durations, vec![450, 950, 1650, 2600]);
    assert_eq!(fp.juice.hit_stop_ms, 50);
    assert_eq!(fp.juice.lunge_ms, 150);
    assert_eq!(fp.juice.recoil_ms, 220);
    assert!((fp.juice.crit_camera_kick_deg - 2.0).abs() < 1e-6);
    assert!(fp.hud_roles.reserve_strip_max_pct <= 16);
    assert!((fp.economy_targets.gamepass_mult - 1.0).abs() < 1e-6);
    assert!(fp.economy_targets.zero_paid_luck);
    for expected_cue in [
        "summon_roll",
        "rare_chime",
        "legendary_drop",
        "mythic_fanfare",
        "battle_hit",
        "battle_crit",
        "death_shatter",
        "ui_click",
    ] {
        assert!(
            fp.audio_cues.contains(&expected_cue),
            "Template 1 missing audio cue {expected_cue}"
        );
    }
}

#[test]
fn template_screen_docs_pass_safe_areas_tap_targets_and_wcag_contrast_across_3_viewports() {
    for (id, _, _) in TEMPLATE_SPECS {
        let doc = template_screen_doc(id)
            .unwrap_or_else(|e| panic!("template_screen_doc({id}) failed: {e}"));
        let analysis = analyze_safe_areas(&doc);
        assert!(
            analysis.passed && analysis.warning_count == 0 && analysis.warnings.is_empty(),
            "template '{id}' screen had safe-area warnings: {:?}",
            analysis.warnings
        );
        for dev in ["desktop", "tablet", "mobile_landscape"] {
            assert!(
                analysis.devices.contains_key(dev),
                "template '{id}' analysis missing device {dev}"
            );
        }
        assert_node_wcag_contrast(&doc.root, &doc.theme.surface);
    }
}

#[test]
fn generated_luau_for_all_3_templates_is_original_balanced_and_clean() {
    for (id, _, title) in TEMPLATE_SPECS {
        let idea = ideas::find(id).expect("template exists");
        let mut place = LocalPlace::in_memory(title.to_string());
        let mut generated: BTreeMap<String, String> = BTreeMap::new();
        for step in &idea.plan {
            if let Some(sk) = step.get("skill").and_then(Value::as_str) {
                let args = step.get("args").cloned().unwrap_or_else(|| json!({}));
                let src = skills::generate(sk, &args)
                    .unwrap_or_else(|e| panic!("{id} skill {sk} failed: {e}"));
                generated.insert(sk.to_string(), src);
            } else if let Some(tool) = step.get("tool").and_then(Value::as_str) {
                let mut args = step.get("args").cloned().unwrap_or_else(|| json!({}));
                if tool == "write_script" {
                    let sk = args["skill"].as_str().unwrap();
                    args["source"] = Value::String(generated.get(sk).unwrap().clone());
                }
                let res = place.call_tool(tool, args).unwrap();
                if tool == "playtest" {
                    let verdict: Value = serde_json::from_str(&res).unwrap();
                    assert_eq!(verdict["status"], "passed", "{id} playtest failed: {verdict}");
                }
            }
        }

        let report = analyze_place(place.scripts(), place.instances());
        assert!(
            report.is_ok() && report.warning_count() == 0,
            "{id} static analysis had findings: {:?}",
            report.diagnostics
        );

        let all_luau: String = place
            .scripts()
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            all_luau.contains("BindToClose"),
            "{id} generated Luau missing game:BindToClose"
        );
        let low = all_luau.to_ascii_lowercase();
        for bad in FORBIDDEN_ORIGINALITY_TERMS {
            assert!(
                !low.contains(bad),
                "{id} generated Luau leaked forbidden term {bad:?}"
            );
        }

        let econ = evaluate_economy_sim(place.scripts(), &json!({}));
        assert_eq!(econ["balanced"], json!(true), "{id} economy unbalanced: {econ}");
        assert_eq!(econ["blocker_count"], json!(0), "{id} economy blockers: {econ}");
        if id == "anime-card-battles-apex" {
            let gp = econ["constants"]["gamepass_mult"].as_f64().unwrap_or(0.0);
            assert!(
                (gp - 1.0).abs() < 1e-6,
                "anime-card-battles-apex must have zero paid luck (gamepass_mult=1.0), got {gp}: {econ}"
            );
        }
    }
}

#[test]
fn api_ideas_build_generates_saved_sandbox_proof_economy_sim_and_gui_store_entry() {
    let (lune_bin, searched) = find_lune_binary();
    assert!(
        lune_bin.is_some(),
        "lune binary is required for m2_genre_templates tests (searched: {searched:?})"
    );

    for (id, niche_id, _title) in TEMPLATE_SPECS {
        let data_dir = temp_dir(id);
        let (port, _shared) = start_server(data_dir.clone());

        // 1. POST /api/ideas serializes niche_id + feel_profile
        let (st_ideas, ideas_resp) = api_json(port, "POST", "/api/ideas", Some(json!({})));
        assert_eq!(st_ideas, 200);
        let arr = ideas_resp["ideas"].as_array().expect("ideas array");
        let entry = arr
            .iter()
            .find(|x| x["id"] == id)
            .unwrap_or_else(|| panic!("POST /api/ideas missing {id}: {ideas_resp}"));
        assert_eq!(entry["niche_id"], json!(niche_id));
        assert!(entry["feel_profile"].is_object(), "{id} missing feel_profile JSON");

        // 2. POST /api/ideas/build executes template + returns template_proof
        let (st_build, build_resp) =
            api_json(port, "POST", "/api/ideas/build", Some(json!({ "id": id })));
        assert_eq!(st_build, 200, "build {id} failed: {build_resp}");
        assert_eq!(build_resp["ok"], json!(true));

        let proof = &build_resp["template_proof"];
        assert!(proof.is_object(), "{id} missing template_proof: {build_resp}");

        // 2a. Headless Lune Sandbox passed + proof JSON saved on disk
        let sb = &proof["sandbox"];
        assert_eq!(sb["passed"], json!(true), "{id} sandbox failed: {sb}");
        assert_eq!(sb["error_count"], json!(0), "{id} sandbox errors: {sb}");
        assert_eq!(
            sb["core_loop"]["all_passed"],
            json!(true),
            "{id} core_loop.all_passed must be true: {sb}"
        );
        let proof_path = sb["proof_path"]
            .as_str()
            .expect("sandbox proof_path must be returned");
        assert!(
            Path::new(proof_path).is_file(),
            "{id} sandbox proof file missing at {proof_path}"
        );
        let disk_proof: Value =
            serde_json::from_str(&std::fs::read_to_string(proof_path).unwrap()).unwrap();
        assert_eq!(disk_proof["passed"], json!(true));

        // 2b. Economy Sim balanced with 0 blockers
        let econ = &proof["economy_sim"];
        assert_eq!(econ["balanced"], json!(true), "{id} economy_sim failed: {econ}");
        assert_eq!(econ["blocker_count"], json!(0), "{id} economy_sim blockers: {econ}");

        // 2c. GUI safe-area check 0 warnings + ScreenDoc saved into GuiStore
        let gui_safe = &proof["gui_safe_area"];
        assert_eq!(gui_safe["passed"], json!(true), "{id} gui_safe_area failed: {gui_safe}");
        assert_eq!(gui_safe["warning_count"], json!(0));

        let slug = entry["feel_profile"]["gui_screen_slug"]
            .as_str()
            .expect("gui_screen_slug");
        let (st_gui, gui_resp) =
            api_json(port, "GET", &format!("/api/studio/gui?slug={slug}"), None);
        assert_eq!(st_gui, 200, "{id} ScreenDoc not saved in GuiStore: {gui_resp}");
        assert_eq!(gui_resp["ok"], json!(true));

        let _ = std::fs::remove_dir_all(&data_dir);
    }
}

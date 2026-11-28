//! Integration & security tests for OpenRO v1.34.0 (Gap #90):
//! - Copyright-free YouTube music sourcing: anti-dubstep filter, strict YouTube URL allowlist
//!   (SSRF / CLI flag-injection rejection), license classifier + credit line, mock `yt-dlp`
//!   search (duration window + dedupe), mock import with 44.1 kHz PCM16 mono WAV mastering,
//!   `.source.json` provenance receipt, and the `<data_dir>/bin/openro-yt-music` runner.
//! - `youtube-nocopyright-music-cli` adapter preset, 9-pack SFX catalog with Modern UI /
//!   Motion-Graphics packs first, loopback-guarded `/api/library/music*` routes,
//!   `youtube_music_adapter` capability flags, and Agent `music_search` / `music_import` tools.

use openro_rs::agent::Agent;
use openro_rs::audio_library::{
    analyze_pcm16_wav, cc0_pack_catalog, classify_music_license, filter_youtube_music_jsonl,
    find_catalog_pack, import_youtube_music, install_yt_music_runner, is_dubstep_like,
    music_genres, search_youtube_music, validate_youtube_url, yt_music_runner_path,
};
use openro_rs::audio_synth::encode_pcm16_wav;
use openro_rs::library::LibraryManager;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use openro_rs::tool_adapters::{optional_preset_adapters, resolve_adapter_cli_binary_in};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, val: Option<&str>) -> Self {
        let prev = std::env::var(key).ok();
        match val {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        EnvGuard { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "openro-v134-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn yt_line(id: &str, title: &str, channel: &str, duration: f64, description: &str, tags: &[&str]) -> String {
    json!({
        "id": id,
        "title": title,
        "channel": channel,
        "duration": duration,
        "description": description,
        "tags": tags,
        "view_count": 12345,
    })
    .to_string()
}

fn mock_jsonl() -> String {
    [
        yt_line("AAAAAAAAAA1", "Chill Lofi Study Beat", "StreamBeats by Harris Heller", 150.0, "Copyright free lofi", &["lofi"]),
        yt_line("AAAAAAAAAA2", "HEAVY DUBSTEP DROP 2026", "BassLord", 200.0, "no copyright", &[]),
        yt_line("AAAAAAAAAA3", "Chill beat", "Wobbler", 180.0, "free for profit", &["Wobble-Bass", "edm"]),
        yt_line("AAAAAAAAAA4", "Lofi 1 Hour Mix", "Mixes", 3600.0, "no copyright", &[]),
        yt_line("AAAAAAAAAA5", "Lofi short", "Shorts", 3.0, "no copyright", &[]),
        yt_line("bad id!", "Broken", "x", 120.0, "", &[]),
        yt_line("-AAAAAAAAA6", "[FREE FOR PROFIT] Carti Type Beat - Rage", "ProdNova", 140.0, "Free for profit, credit me", &[]),
        yt_line("AAAAAAAAAA7", "100% Royalty Free No Copyright Trap Beat", "TrapHouse", 160.0, "", &[]),
        yt_line("AAAAAAAAAA8", "Pluggnb beat", "SoftPlug", 130.0, "Free for non-profit use only", &[]),
        yt_line("AAAAAAAAAA1", "Chill Lofi Study Beat (dupe)", "StreamBeats by Harris Heller", 150.0, "", &[]),
        yt_line("AAAAAAAAAA9", "Riddim Session", "Bass", 200.0, "", &[]),
        "not json at all".to_string(),
    ]
    .join("\n")
}

/// 15 s 22.05 kHz mono PCM16 sine WAV (non-44.1 kHz so mastering must resample).
fn write_source_wav(dir: &Path) -> PathBuf {
    let sr = 22_050u32;
    let samples: Vec<i16> = (0..(sr as usize * 15))
        .map(|i| ((i as f64 * 220.0 * 2.0 * std::f64::consts::PI / sr as f64).sin() * 9_000.0) as i16)
        .collect();
    let path = dir.join("source_track.wav");
    std::fs::write(&path, encode_pcm16_wav(&samples, sr)).unwrap();
    path
}

fn start_openro_server(data_dir: &Path) -> u16 {
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "test-model".into(), None);
    let place = LocalPlace::open(data_dir.to_path_buf(), "MusicTestGame".to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::with_data_dir(Some(agent), "MusicTestGame".to_string(), data_dir.to_path_buf());
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind openro server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    std::thread::sleep(Duration::from_millis(60));
    port
}

fn http_raw(port: u16, host: &str, method: &str, path: &str, body: Option<&[u8]>) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect server");
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let payload = body.unwrap_or(b"");
    let ct = if payload.is_empty() {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{ct}Content-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    if !payload.is_empty() {
        s.write_all(payload).unwrap();
    }
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let split = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP header separator");
    let header_str = String::from_utf8_lossy(&resp[..split]).to_string();
    let status = header_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    (status, resp[split + 4..].to_vec())
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let host = format!("127.0.0.1:{port}");
    let raw_body = body.map(|b| b.to_string().into_bytes());
    let (st, bytes) = http_raw(port, &host, method, path, raw_body.as_deref());
    let val = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes).to_string() }));
    (st, val)
}

#[test]
fn sfx_catalog_has_nine_packs_with_motion_graphics_first() {
    let catalog = cc0_pack_catalog();
    assert_eq!(catalog.len(), 9);
    let ids: Vec<&str> = catalog.iter().map(|p| p.id).collect();
    assert_eq!(
        &ids[..3],
        &["motion-ui-swishes", "modern-interface-sfx", "modern-ui-pack-1"]
    );
    for keep in [
        "kenney-ui-sfx",
        "kenney-casino-cards",
        "kenney-digital-sfx",
        "rpg-combat-pack",
        "rpg-fantasy-80",
        "general-game-100",
    ] {
        assert!(ids.contains(&keep), "missing {keep}");
    }
    assert!(find_catalog_pack("arcade-fanfare-pack").is_none(), "broken pack must be removed");
    for p in &catalog[..3] {
        assert!(p.tags.contains(&"motion-graphics"), "{} must be tagged motion-graphics", p.id);
        assert!(p.url.starts_with("https://opengameart.org/sites/default/files/"));
    }
    let swish = find_catalog_pack("motion-ui-swishes").unwrap();
    assert_eq!(swish.author, "artisticdude");
    assert_eq!(swish.license, "CC0-1.0");
    assert_eq!(swish.url, "https://opengameart.org/sites/default/files/swishes.zip");
    let iface = find_catalog_pack("modern-interface-sfx").unwrap();
    assert_eq!(iface.author, "p0ss");
    assert_eq!(iface.url, "https://opengameart.org/sites/default/files/interface.zip");
    let ui1 = find_catalog_pack("modern-ui-pack-1").unwrap();
    assert_eq!(ui1.author, "junggle");
    assert_eq!(ui1.license, "CC-BY-3.0");
    assert_eq!(ui1.url, "https://opengameart.org/sites/default/files/UI%20pack%201.zip");
}

#[test]
fn anti_dubstep_filter_and_license_classifier() {
    for bad in [
        "DUBSTEP banger",
        "Melodic Brostep",
        "riddim drop",
        "Wobble Bass Madness",
        "wobble-bass",
        "#Dubstep",
        "WOBBLE   BASS",
    ] {
        assert!(is_dubstep_like(bad), "'{bad}' must be filtered");
    }
    for ok in ["Chill lofi", "Jazz rap boom bap", "Carti type beat rage", "bass boosted trap", "wobbly synth"] {
        assert!(!is_dubstep_like(ok), "'{ok}' must not be filtered");
    }

    let (st, credit) = classify_music_license("Chill Lofi", "", "StreamBeats by Harris Heller");
    assert_eq!(st, "streambeats_dmca_free");
    assert_eq!(credit, "StreamBeats by Harris Heller");
    let (st, credit) = classify_music_license("100% Royalty Free No Copyright Trap Beat", "", "TrapHouse");
    assert_eq!(st, "royalty_free_verified");
    assert_eq!(credit, "Prod. TrapHouse");
    let (st, _) = classify_music_license("[FREE FOR PROFIT] Carti Type Beat", "", "ProdNova");
    assert_eq!(st, "free_for_profit");
    let (st, _) = classify_music_license("Pluggnb beat", "Free for non-profit use only", "SoftPlug");
    assert_eq!(st, "non_profit_only");
    let (st, credit) = classify_music_license("Some beat", "", "");
    assert_eq!(st, "verify_description");
    assert_eq!(credit, "Prod. Unknown Producer");
    let (_, credit) = classify_music_license("x", "", "Evil\nChannel\u{7}");
    assert_eq!(credit, "Prod. Evil Channel", "control chars must be stripped from credit line");
}

#[test]
fn youtube_url_allowlist_rejects_ssrf_and_flag_injection() {
    let (id, norm) = validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
    assert_eq!(id, "dQw4w9WgXcQ");
    assert_eq!(norm, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    let (_, norm) = validate_youtube_url("  https://youtu.be/dQw4w9WgXcQ  ").unwrap();
    assert_eq!(norm, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    let (_, norm) = validate_youtube_url("https://youtu.be/dQw4w9WgXcQ?si=Ab_c-1").unwrap();
    assert_eq!(norm, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    let (_, norm) = validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLx&index=2").unwrap();
    assert_eq!(norm, "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "playlist params discarded");
    // ids may begin with '-', but the normalized argv token always starts with https:// (no flag injection).
    let (id, norm) = validate_youtube_url("https://youtu.be/-xec_AAAAAA").unwrap();
    assert_eq!(id, "-xec_AAAAAA");
    assert!(norm.starts_with("https://"));

    for bad in [
        "",
        "--exec=rm -rf /",
        "-o/etc/passwd",
        "http://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtube.com/watch?v=dQw4w9WgXcQ",
        "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://www.youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
        "https://www.youtube.com/watch?v=dQw4w9WgXc",
        "https://www.youtube.com/watch?v=dQw4w9WgX!Q",
        "https://youtu.be/dQw4w9WgXcQ/../../admin",
        "https://youtu.be/dQw4w9WgXcQ@127.0.0.1",
        "https://youtu.be/dQw4w9WgXcQ#frag",
        "https://youtu.be/dQw4w9WgXcQ?x=1 --exec",
        "https://youtu.be/dQw4w9WgXcQ\n--exec",
        "file:///etc/passwd",
        "http://127.0.0.1:8080/watch?v=dQw4w9WgXcQ",
        "ytsearch1:lofi",
        "https://www.youtube.com/playlist?list=PLdQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQé",
    ] {
        assert!(validate_youtube_url(bad).is_err(), "must reject {bad:?}");
    }
}

#[test]
fn mock_search_filters_duration_dubstep_and_classifies() {
    let _lock = lock_env();
    // Pure filter function.
    let (tracks, counts) = filter_youtube_music_jsonl(&mock_jsonl(), "lofi", 10);
    let ids: Vec<&str> = tracks.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["AAAAAAAAAA1", "-AAAAAAAAA6", "AAAAAAAAAA7", "AAAAAAAAAA8"]);
    assert_eq!(counts["dubstep"], 3, "{counts}");
    assert_eq!(counts["duration"], 2, "{counts}");
    assert_eq!(counts["duplicate"], 1, "{counts}");
    assert_eq!(counts["invalid"], 2, "{counts}");
    assert_eq!(tracks[0]["license_status"], "streambeats_dmca_free");
    assert_eq!(tracks[1]["license_status"], "free_for_profit");
    assert_eq!(tracks[1]["credit_line"], "Prod. ProdNova");
    assert_eq!(tracks[2]["license_status"], "royalty_free_verified");
    assert_eq!(tracks[3]["license_status"], "non_profit_only");
    assert_eq!(tracks[1]["youtube_url"], "https://www.youtube.com/watch?v=-AAAAAAAAA6");

    // Full search through OPENRO_YT_MUSIC_MOCK_JSON (inline JSONL).
    let _mock = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_JSON", Some(&mock_jsonl()));
    let res = search_youtube_music(Some("trap"), None, 2).unwrap();
    assert_eq!(res["ok"], true);
    assert_eq!(res["source"], "mock");
    assert_eq!(res["genre"], "trap");
    assert_eq!(res["anti_dubstep"], true);
    assert_eq!(res["count"], 2);
    assert!(res["search_query"].as_str().unwrap().contains("royalty free no copyright trap"));

    // Default genre is lofi; custom queries get a copyright-free suffix.
    let res = search_youtube_music(None, None, 10).unwrap();
    assert_eq!(res["genre"], "lofi");
    let res = search_youtube_music(None, Some("--exec rm boss fight beat"), 10).unwrap();
    assert_eq!(res["genre"], "custom");
    let sq = res["search_query"].as_str().unwrap();
    assert!(sq.starts_with("exec rm boss fight beat"), "leading dashes stripped: {sq}");
    assert!(sq.contains("no copyright"));

    // Mock from a JSONL file path.
    let dir = temp_dir("mock-file");
    let file = dir.join("mock.jsonl");
    std::fs::write(&file, mock_jsonl()).unwrap();
    let _mock2 = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_JSON", Some(file.to_str().unwrap()));
    let res = search_youtube_music(Some("pluggnb"), None, 10).unwrap();
    assert_eq!(res["count"], 4);

    // Unknown genre + dubstep queries are rejected.
    assert!(search_youtube_music(Some("dubstep"), None, 5).is_err());
    assert!(search_youtube_music(None, Some("best riddim 2026"), 5).is_err());
    assert_eq!(music_genres().len(), 7);
    let gids: Vec<&str> = music_genres().iter().map(|g| g.id).collect();
    assert_eq!(
        gids,
        vec!["jazz-rap", "lofi", "trap", "carti-type-beat", "pluggnb", "anime-hiphop", "synthwave"]
    );
    for g in music_genres() {
        assert!(!is_dubstep_like(g.query));
    }
}

#[test]
fn mock_import_masters_wav_and_writes_receipt() {
    let _lock = lock_env();
    let data_dir = temp_dir("import-data");
    let project = temp_dir("import-project");
    let src_dir = temp_dir("import-src");
    let src = write_source_wav(&src_dir);
    let _mock = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_AUDIO_FILE", Some(src.to_str().unwrap()));

    // URL validation happens before any I/O.
    let err = import_youtube_music(&data_dir, Some(&project), &json!({ "url": "https://evil.example/x.mp3" }))
        .unwrap_err();
    assert!(err.contains("only https://www.youtube.com"), "{err}");
    let err = import_youtube_music(&data_dir, Some(&project), &json!({ "genre": "lofi" })).unwrap_err();
    assert!(err.contains("url"), "{err}");
    let err = import_youtube_music(
        &data_dir,
        Some(&project),
        &json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "title": "Brutal Dubstep Drop" }),
    )
    .unwrap_err();
    assert!(err.contains("anti-dubstep"), "{err}");

    let res = import_youtube_music(
        &data_dir,
        Some(&project),
        &json!({
            "url": "https://youtu.be/dQw4w9WgXcQ",
            "genre": "lofi",
            "out_name": "Lobby Music!",
            "max_duration_s": 12,
            "title": "Chill Lofi Study Beat",
            "channel": "StreamBeats by Harris Heller"
        }),
    )
    .unwrap();
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["filename"], "lobby_music.wav");
    assert_eq!(res["path"], "assets/audio/lobby_music.wav");
    assert_eq!(res["source"], "youtube_nocopyright_music");
    assert_eq!(res["youtube_url"], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    assert_eq!(res["license_status"], "streambeats_dmca_free");
    assert_eq!(res["credit_line"], "StreamBeats by Harris Heller");
    assert_eq!(res["genre"], "lofi");
    assert_eq!(res["sample_rate"], 44_100);
    assert_eq!(res["channels"], 1);
    assert_eq!(res["bits_per_sample"], 16);
    for key in [
        "url", "wav_url", "bytes", "sha256", "duration_ms", "duration_s", "peak_dbfs", "rms_dbfs", "title", "channel",
    ] {
        assert!(res.get(key).is_some(), "missing {key}: {res}");
    }
    let dur_ms = res["duration_ms"].as_u64().unwrap();
    assert!((11_500..=12_100).contains(&dur_ms), "trimmed to 12 s, got {dur_ms} ms");
    assert!(res["peak_dbfs"].as_f64().unwrap() <= -0.5, "{res}");

    let wav_path = project.join("assets/audio/lobby_music.wav");
    let bytes = std::fs::read(&wav_path).expect("project wav written");
    let stats = analyze_pcm16_wav(&bytes).unwrap();
    assert_eq!((stats.sample_rate, stats.channels, stats.bits_per_sample), (44_100, 1, 16));
    assert_eq!(bytes.len() as u64, res["bytes"].as_u64().unwrap());
    // Fade-in: the first sample is (near) silent.
    let data_at = bytes.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
    let first = i16::from_le_bytes([bytes[data_at], bytes[data_at + 1]]);
    assert!(first.abs() < 600, "fade-in must start near silence, got {first}");
    assert!(data_dir.join("game-assets/audio/lobby_music.wav").is_file(), "preview copy written");

    let receipt: Value =
        serde_json::from_slice(&std::fs::read(project.join("assets/audio/lobby_music.source.json")).unwrap()).unwrap();
    assert_eq!(receipt["youtube_url"], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    assert_eq!(receipt["license_status"], "streambeats_dmca_free");
    assert_eq!(receipt["credit_line"], "StreamBeats by Harris Heller");
    assert_eq!(receipt["sha256"], res["sha256"]);
    let gi = std::fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(gi.lines().any(|l| l == "assets/audio/*.source.json"));
    // Scratch dir cleaned up.
    let tmp_left = std::fs::read_dir(data_dir.join("tmp")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(tmp_left, 0, "scratch dirs must be removed");

    // Clamp max_duration_s to 110 and no-project fallback into <data_dir>/game-assets/audio.
    let res2 = import_youtube_music(
        &data_dir,
        None,
        &json!({ "url": "https://www.youtube.com/watch?v=abcdefghijk", "max_duration_s": 500 }),
    )
    .unwrap();
    assert_eq!(res2["max_duration_s"], 110);
    assert_eq!(res2["project_path"], Value::Null);
    assert_eq!(res2["license_status"], "verify_description");
    assert!(res2["license_warning"].is_string());
    let fname = res2["filename"].as_str().unwrap();
    assert!(data_dir.join("game-assets/audio").join(fname).is_file());
    let stem = fname.trim_end_matches(".wav");
    assert!(data_dir.join("game-assets/audio").join(format!("{stem}.source.json")).is_file());
    let d2 = res2["duration_ms"].as_u64().unwrap();
    assert!((14_500..=15_100).contains(&d2), "15 s source kept whole under clamp, got {d2}");

    // Mock bypass only accepts real audio files.
    let _bad = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_AUDIO_FILE", Some("/etc/passwd"));
    assert!(import_youtube_music(&data_dir, None, &json!({ "url": "https://youtu.be/abcdefghijk" })).is_err());
}

#[test]
fn runner_installer_and_adapter_preset() {
    let preset = optional_preset_adapters()
        .into_iter()
        .find(|a| a.id == "youtube-nocopyright-music-cli")
        .expect("youtube preset present");
    assert_eq!(preset.capability, "audio_gen");
    assert_eq!(preset.priority, 5);
    assert_eq!(preset.hardware_tier.as_deref(), Some("youtube-nocopyright"));
    assert_eq!(preset.model_size.as_deref(), Some("YouTube Copyright-Free · yt-dlp"));
    assert_eq!(
        preset.install_hint.as_deref(),
        Some("Requires yt-dlp + ffmpeg; click Install Music Runner")
    );
    let cli = preset.cli.as_ref().unwrap();
    assert_eq!(cli.command, "openro-yt-music");
    assert_eq!(
        cli.argv,
        vec![
            "--download", "{{arg:youtube_url}}", "--query", "{{?prompt}}", "--genre", "{{?arg:genre}}",
            "--max-duration", "{{?duration_s}}", "--out", "{{out_path}}",
        ]
    );
    assert_eq!(cli.health_argv, vec!["--check"]);
    assert_eq!(cli.output_mode, "file");

    let data_dir = temp_dir("runner");
    let res = install_yt_music_runner(&data_dir).unwrap();
    assert_eq!(res["ok"], true);
    assert_eq!(res["command"], "openro-yt-music");
    let path = yt_music_runner_path(&data_dir);
    assert!(path.is_file());
    let script = std::fs::read_to_string(&path).unwrap();
    assert!(script.starts_with("#!/usr/bin/env python3"));
    assert!(script.contains(env!("CARGO_PKG_VERSION")));
    assert!(script.contains("carti-type-beat"));
    assert!(!script.contains("__OPENRO_"), "placeholders replaced");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let resolved = resolve_adapter_cli_binary_in(Some(&data_dir), "youtube-nocopyright-music-cli", "openro-yt-music")
        .expect("runner resolvable from <data_dir>/bin");
    assert_eq!(resolved, path);

    if res["python_available"] == true {
        assert_eq!(res["check"]["tool"], "openro-yt-music");
        assert_eq!(res["check"]["version"], env!("CARGO_PKG_VERSION"));
        let py = if cfg!(windows) { "python" } else { "python3" };
        let out = Command::new(py)
            .arg(&path)
            .args(["--download", "https://evil.example/--exec", "--out"])
            .arg(data_dir.join("x.wav"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "runner must reject non-YouTube URLs");
        assert!(!data_dir.join("x.wav").exists());
    }
}

#[test]
fn http_music_routes_capabilities_library_and_agent_tools() {
    let _lock = lock_env();
    let data_dir = temp_dir("http-v134");
    let src_dir = temp_dir("http-src");
    let src = write_source_wav(&src_dir);
    let _mock_json = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_JSON", Some(&mock_jsonl()));
    let _mock_audio = EnvGuard::set("OPENRO_YT_MUSIC_MOCK_AUDIO_FILE", Some(src.to_str().unwrap()));
    let port = start_openro_server(&data_dir);

    for ep in ["/api/capabilities", "/api/health", "/api/status"] {
        let (st, v) = http_json(port, "GET", ep, None);
        assert_eq!(st, 200, "GET {ep}: {v}");
        assert_eq!(v["youtube_music_adapter"], true, "{ep} missing youtube_music_adapter: {v}");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }
    assert_eq!(env!("CARGO_PKG_VERSION"), "1.34.0");

    for (method, path) in [
        ("GET", "/api/library/music/genres"),
        ("POST", "/api/library/music/search"),
        ("POST", "/api/library/music/import"),
        ("POST", "/api/library/music-runner/install"),
    ] {
        let (st, _) = http_raw(port, "evil.example:8077", method, path, Some(b"{}"));
        assert_eq!(st, 403, "expected 403 for non-loopback {method} {path}");
    }

    let (st, genres) = http_json(port, "GET", "/api/library/music/genres", None);
    assert_eq!(st, 200, "{genres}");
    assert_eq!(genres["genres"].as_array().unwrap().len(), 7);
    assert_eq!(genres["anti_dubstep_terms"].as_array().unwrap().len(), 4);

    let (st, found) = http_json(port, "POST", "/api/library/music/search", Some(&json!({ "genre": "jazz-rap", "limit": 3 })));
    assert_eq!(st, 200, "{found}");
    assert_eq!(found["count"], 3);
    assert!(found["tracks"].as_array().unwrap().iter().all(|t| !is_dubstep_like(t["title"].as_str().unwrap())));
    let (st, _) = http_json(port, "POST", "/api/library/music/search", Some(&json!({ "genre": "brostep" })));
    assert_eq!(st, 400);

    let (st, bad) = http_json(port, "POST", "/api/library/music/import", Some(&json!({ "url": "http://169.254.169.254/latest" })));
    assert_eq!(st, 400, "{bad}");
    let (st, imported) = http_json(
        port,
        "POST",
        "/api/library/music/import",
        Some(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "genre": "anime-hiphop", "out_name": "http_bgm", "max_duration_s": 10 })),
    );
    assert_eq!(st, 200, "{imported}");
    assert_eq!(imported["filename"], "http_bgm.wav");
    let wav_url = imported["wav_url"].as_str().unwrap().to_string();
    let host = format!("127.0.0.1:{port}");
    let (st, wav) = http_raw(port, &host, "GET", &wav_url, None);
    assert_eq!(st, 200);
    assert_eq!(&wav[0..4], b"RIFF");

    let (st, runner) = http_json(port, "POST", "/api/library/music-runner/install", Some(&json!({})));
    assert_eq!(st, 200, "{runner}");
    assert!(yt_music_runner_path(&data_dir).is_file());
    let (st, adapters) = http_json(port, "GET", "/api/adapters?capability=audio_gen", None);
    assert_eq!(st, 200, "{adapters}");
    let yt = adapters["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "youtube-nocopyright-music-cli")
        .expect("youtube preset listed");
    assert_eq!(yt["available"], true, "runner installed -> preset available: {yt}");

    // Library pack advertises music tools.
    let mut mgr = LibraryManager::open(&data_dir);
    mgr.install("cc0-sfx-packs").expect("install cc0-sfx-packs");
    let tools = mgr.active_tools();
    assert!(tools.contains(&"music_search".to_string()));
    assert!(tools.contains(&"music_import".to_string()));
    let tested = mgr.test_pack("cc0-sfx-packs").expect("test pack");
    assert_eq!(tested["passed"], true);

    // Agent music_search + music_import via mock LLM.
    let turn = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let turn_c = Arc::clone(&turn);
    let llm = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in llm.incoming().flatten() {
            let mut buf = [0u8; 16384];
            let _ = s.read(&mut buf);
            let n = turn_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let content = match n {
                0 => json!({
                    "steps": [
                        { "tool": "music_search", "args": { "genre": "lofi", "limit": 2 } },
                        { "tool": "music_import", "args": { "url": "https://youtu.be/AAAAAAAAAA1", "genre": "lofi", "out_name": "agent_bgm", "max_duration_s": 10, "channel": "StreamBeats by Harris Heller" } }
                    ],
                    "done": true,
                    "summary": "Imported lofi BGM."
                })
                .to_string(),
                _ => json!({ "done": true, "summary": "done" }).to_string(),
            };
            let body = json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    let (st, cfg) = http_json(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "provider": "openai", "base_url": format!("http://127.0.0.1:{llm_port}/v1"), "model_id": "mock-model" })),
    );
    assert_eq!(st, 200, "{cfg}");
    let (st, chat) = http_json(port, "POST", "/api/chat", Some(&json!({ "message": "Add chill lofi lobby music." })));
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    assert!(
        data_dir.join("game-assets/audio/agent_bgm.wav").is_file(),
        "agent music_import must write mastered WAV"
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(data_dir.join("game-assets/audio/agent_bgm.source.json")).unwrap()).unwrap();
    assert_eq!(receipt["credit_line"], "StreamBeats by Harris Heller");
}

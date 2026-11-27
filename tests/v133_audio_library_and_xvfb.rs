//! Integration & security tests for OpenRO v1.33.0 (Gaps #88 & #89):
//! - Free CC0-1.0 SFX Pack catalog, zero-dependency safe ZIP extractor (CWE-22/59/ZIP-slip/
//!   POSIX-symlink/size-limit/zip-bomb/SSRF), search, preview, local import, 44.1 kHz PCM16 WAV
//!   mastering & multi-clip layering, and `<data_dir>/bin/openro-audio-gen` installer.
//! - Hardware-tiered `audio_gen` presets (all 8 presets), `audio_b64_path` base64 audio decode +
//!   WAV normalization, `<data_dir>/bin` CLI lookup, and uniform `/api/tools/audio/generate`.
//! - `cc0-sfx-packs` library pack, `/api/capabilities`, `/api/health`, `/api/status`, and Agent
//!   `sfx_pack_*` tools.
//! - Gap #89 stale `Xvfb` lock/socket detection & owned teardown cleanup.

use flate2::write::DeflateEncoder;
use flate2::Compression;
use openro_rs::agent::Agent;
use openro_rs::assets::encode_base64;
use openro_rs::audio_library::{
    analyze_pcm16_wav, cc0_pack_catalog, extract_safe_zip, import_local_sfx_pack,
    install_local_audio_runner, install_sfx_pack, list_sfx_packs, local_audio_runner_path,
    remove_sfx_pack, resolve_preview_clip_path, search_sfx_packs, use_sfx_clip,
};
use openro_rs::audio_synth::{encode_pcm16_wav, synthesize_wav, SAMPLE_RATE};
use openro_rs::library::LibraryManager;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::secrets::SecretStore;
use openro_rs::server::{serve, Shared};
use openro_rs::tool_adapters::{
    invoke, optional_preset_adapters, resolve_adapter_cli_binary_in, AdapterRegistry, InvokeCtx,
    ToolAdapter,
};
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

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "openro-v133-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

struct ZipEntrySpec<'a> {
    name: &'a [u8],
    data: &'a [u8],
    method: u16,
    external_attr: u32,
    declared_uncomp_override: Option<u32>,
}

fn build_test_zip(entries: &[ZipEntrySpec<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for e in entries {
        let local_offset = out.len() as u32;
        let comp_data: Vec<u8> = match e.method {
            0 => e.data.to_vec(),
            8 => {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                enc.write_all(e.data).unwrap();
                enc.finish().unwrap()
            }
            _ => panic!("unsupported test zip method"),
        };
        let crc = crc32(e.data);
        let comp_len = comp_data.len() as u32;
        let uncomp_len = e.declared_uncomp_override.unwrap_or(e.data.len() as u32);
        let name_len = e.name.len() as u16;

        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // mtime
        out.extend_from_slice(&0u16.to_le_bytes()); // mdate
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&comp_len.to_le_bytes());
        out.extend_from_slice(&uncomp_len.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(e.name);
        out.extend_from_slice(&comp_data);

        // Central directory header
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&((3u16 << 8) | 20u16).to_le_bytes()); // version made by (Unix)
        cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
        cd.extend_from_slice(&0u16.to_le_bytes()); // flags
        cd.extend_from_slice(&e.method.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&comp_len.to_le_bytes());
        cd.extend_from_slice(&uncomp_len.to_le_bytes());
        cd.extend_from_slice(&name_len.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes()); // extra len
        cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
        cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
        cd.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        cd.extend_from_slice(&e.external_attr.to_le_bytes());
        cd.extend_from_slice(&local_offset.to_le_bytes());
        cd.extend_from_slice(e.name);
    }

    let cd_offset = out.len() as u32;
    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);

    // EOCD
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

fn spawn_file_server(payload: Vec<u8>, content_type: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock file server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut req_buf = [0u8; 4096];
            let _ = stream.read(&mut req_buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&payload);
        }
    });
    port
}

fn start_openro_server(data_dir: &Path) -> u16 {
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "test-model".into(), None);
    let place = LocalPlace::open(data_dir.to_path_buf(), "AudioTestGame".to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::with_data_dir(Some(agent), "AudioTestGame".to_string(), data_dir.to_path_buf());
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind openro server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    std::thread::sleep(Duration::from_millis(60));
    port
}

fn http_raw(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect server");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
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
    (status, header_str, resp[split + 4..].to_vec())
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let host = format!("127.0.0.1:{port}");
    let raw_body = body.map(|b| b.to_string().into_bytes());
    let (st, _, bytes) = http_raw(port, &host, method, path, raw_body.as_deref());
    let val = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        json!({ "raw": String::from_utf8_lossy(&bytes).to_string() })
    });
    (st, val)
}

#[test]
fn catalog_and_safe_zip_security_guards() {
    // 1. Catalog has all 7 verified CC0-1.0 packs.
    let catalog = cc0_pack_catalog();
    assert_eq!(catalog.len(), 7);
    let ids: Vec<&str> = catalog.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        vec![
            "kenney-ui-sfx",
            "kenney-casino-cards",
            "kenney-digital-sfx",
            "rpg-combat-pack",
            "rpg-fantasy-80",
            "general-game-100",
            "arcade-fanfare-pack",
        ]
    );
    for pack in catalog {
        assert_eq!(pack.license, "CC0-1.0");
        assert!(pack.url.starts_with("https://opengameart.org/"));
    }

    let wav_click = synthesize_wav("ui_click", Some(80)).unwrap().wav_bytes;
    let wav_hit = synthesize_wav("battle_hit", Some(120)).unwrap().wav_bytes;

    // 2. Valid archive with Stored (0) + Deflate (8), license .txt, and disallowed .exe + __MACOSX ignored.
    let good_zip = build_test_zip(&[
        ZipEntrySpec {
            name: b"Audio/click_01.wav",
            data: &wav_click,
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"Audio/combat hit.wav",
            data: &wav_hit,
            method: 8,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"License.txt",
            data: b"CC0 1.0 Universal",
            method: 8,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"__MACOSX/._click_01.wav",
            data: b"appledouble",
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"tools/ignore_me.exe",
            data: b"MZfake",
            method: 0,
            external_attr: 0o100755 << 16,
            declared_uncomp_override: None,
        },
    ]);
    let dst = temp_dir("zip-good");
    let extracted = extract_safe_zip(&good_zip, &dst).expect("valid zip extracts");
    assert_eq!(extracted.len(), 3, "only 2 wavs + 1 license txt extracted: {extracted:?}");
    assert!(dst.join("Audio/click_01.wav").is_file());
    assert!(dst.join("Audio/combat hit.wav").is_file());
    assert!(dst.join("License.txt").is_file());
    assert!(!dst.join("tools/ignore_me.exe").exists());
    assert!(!dst.join("__MACOSX").exists());

    // 3. Reject ZIP-slip, backslash, leading slash, Windows drive letter, NUL byte, empty filename.
    for bad_name in [
        b"../escape.wav".as_slice(),
        b"sub/../../escape.wav".as_slice(),
        b"sub\\evil.wav".as_slice(),
        b"/etc/evil.wav".as_slice(),
        b"C:evil.wav".as_slice(),
        b"C:/evil.wav".as_slice(),
        b"bad\0name.wav".as_slice(),
        b"".as_slice(),
    ] {
        let bad_zip = build_test_zip(&[ZipEntrySpec {
            name: bad_name,
            data: &wav_click,
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        }]);
        let d = temp_dir("zip-bad-path");
        let err = extract_safe_zip(&bad_zip, &d).expect_err("must reject unsafe path");
        assert!(!err.is_empty());
    }

    // 4. Reject POSIX symlink entry (`(external_attr >> 16) & 0o170000 == 0o120000`).
    let sym_zip = build_test_zip(&[ZipEntrySpec {
        name: b"Audio/link.wav",
        data: b"/etc/passwd",
        method: 0,
        external_attr: 0o120777 << 16,
        declared_uncomp_override: None,
    }]);
    let err = extract_safe_zip(&sym_zip, &temp_dir("zip-sym")).expect_err("must reject symlink");
    assert!(err.to_ascii_lowercase().contains("symlink"), "got: {err}");

    // 5. Reject declared single-entry > 32 MiB AND sneaky deflate zip-bomb > 32 MiB.
    let declared_huge_zip = build_test_zip(&[ZipEntrySpec {
        name: b"Audio/huge.wav",
        data: b"tiny",
        method: 0,
        external_attr: 0o100644 << 16,
        declared_uncomp_override: Some(33 * 1024 * 1024),
    }]);
    let err = extract_safe_zip(&declared_huge_zip, &temp_dir("zip-huge"))
        .expect_err("must reject >32 MiB declared entry");
    assert!(err.contains("32 MiB"), "got: {err}");

    let bomb_payload = vec![0u8; 33 * 1024 * 1024];
    let bomb_zip = build_test_zip(&[ZipEntrySpec {
        name: b"Audio/bomb.wav",
        data: &bomb_payload,
        method: 8,
        external_attr: 0o100644 << 16,
        declared_uncomp_override: Some(128), // Lies in central directory!
    }]);
    let err = extract_safe_zip(&bomb_zip, &temp_dir("zip-bomb"))
        .expect_err("must catch streaming deflate zip bomb");
    assert!(err.contains("32 MiB"), "got: {err}");

    // 6. Reject > 1000 entries.
    let names: Vec<String> = (0..1001).map(|i| format!("c{i}.wav")).collect();
    let many_specs: Vec<ZipEntrySpec<'_>> = names
        .iter()
        .map(|n| ZipEntrySpec {
            name: n.as_bytes(),
            data: b"x",
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        })
        .collect();
    let many_zip = build_test_zip(&many_specs);
    let err = extract_safe_zip(&many_zip, &temp_dir("zip-many"))
        .expect_err("must reject >1000 entries");
    assert!(err.contains("1000"), "got: {err}");
}

#[test]
fn ssrf_rejection_and_loopback_mock_zip_install_search_preview_use_remove() {
    let _lock = ENV_LOCK.lock().unwrap();
    let data_dir = temp_dir("sfx-e2e");
    let proj_dir = temp_dir("sfx-proj");

    // 1. SSRF rejection when OPENRO_SFX_PACK_ALLOW_LOOPBACK is unset.
    {
        let _g = EnvGuard::set("OPENRO_SFX_PACK_ALLOW_LOOPBACK", None);
        for blocked in [
            "http://127.0.0.1:9999/pack.zip",
            "http://localhost:9999/pack.zip",
            "http://169.254.169.254/latest/meta-data",
        ] {
            let err = install_sfx_pack(&data_dir, "kenney-ui-sfx", Some(blocked))
                .expect_err("must block SSRF URL");
            assert!(err.contains("blocked"), "expected SSRF error for {blocked}, got: {err}");
        }
    }

    // 2. Build mock ZIP with two real synthesized WAVs and serve over loopback.
    let click_wav = synthesize_wav("ui_click", Some(90)).unwrap().wav_bytes;
    let fanfare_wav = synthesize_wav("level_up", Some(320)).unwrap().wav_bytes;
    let pack_zip = build_test_zip(&[
        ZipEntrySpec {
            name: b"UI/button click 1.wav",
            data: &click_wav,
            method: 8,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"Fanfare/victory_chime.wav",
            data: &fanfare_wav,
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
        ZipEntrySpec {
            name: b"LICENSE.md",
            data: b"CC0-1.0 Public Domain",
            method: 0,
            external_attr: 0o100644 << 16,
            declared_uncomp_override: None,
        },
    ]);
    let mock_port = spawn_file_server(pack_zip, "application/zip");
    let mock_url = format!("http://127.0.0.1:{mock_port}/UI_SFX_Set.zip");

    let _allow = EnvGuard::set("OPENRO_SFX_PACK_ALLOW_LOOPBACK", Some("1"));
    let installed = install_sfx_pack(&data_dir, "kenney-ui-sfx", Some(&mock_url))
        .expect("loopback mock zip install");
    assert_eq!(installed["ok"], true);
    assert_eq!(installed["pack_id"], "kenney-ui-sfx");
    assert_eq!(installed["installed_clips"], 2);
    assert_eq!(installed["installed_clips_total"], 2);

    // 3. List + Search + Preview path resolution + containment guard.
    let list = list_sfx_packs(&data_dir);
    assert_eq!(list["installed_packs_count"], 1);
    assert_eq!(list["installed_clips_total"], 2);

    let search = search_sfx_packs(&data_dir, "click", Some("kenney-ui-sfx"), 10);
    assert_eq!(search["ok"], true);
    assert_eq!(search["total"], 1);
    let clip0 = &search["clips"][0];
    assert_eq!(clip0["rel_path"], "UI/button click 1.wav");
    assert_eq!(
        clip0["preview_url"],
        "/api/library/sfx-packs/preview/kenney-ui-sfx/UI/button%20click%201.wav"
    );

    let (preview_path, mime) =
        resolve_preview_clip_path(&data_dir, "kenney-ui-sfx", "UI/button%20click%201.wav")
            .expect("preview clip resolves");
    assert_eq!(mime, "audio/wav");
    assert!(preview_path.is_file());

    // Traversal & double-encoding rejected on preview.
    assert!(resolve_preview_clip_path(&data_dir, "kenney-ui-sfx", "../index.json").is_err());
    assert!(resolve_preview_clip_path(&data_dir, "kenney-ui-sfx", "%252e%252e/x.wav").is_err());

    // 4. Use clip + layer_clips + fx -> 44.1 kHz 16-bit PCM mono WAV in project/assets/audio/.
    let used = use_sfx_clip(
        &data_dir,
        Some(&proj_dir),
        &json!({
            "pack_id": "kenney-ui-sfx",
            "clip": "UI/button click 1.wav",
            "out_name": "hero_confirm",
            "layer_clips": [
                {
                    "pack_id": "kenney-ui-sfx",
                    "clip": "Fanfare/victory_chime.wav",
                    "delay_ms": 40,
                    "gain_db": -3.0,
                    "pitch_semitones": 2.0
                }
            ],
            "fx": {
                "gain_db": -1.0,
                "fade_in_ms": 5,
                "fade_out_ms": 25
            }
        }),
    )
    .expect("use_sfx_clip succeeds");
    assert_eq!(used["ok"], true);
    assert_eq!(used["path"], "assets/audio/hero_confirm.wav");
    assert_eq!(used["filename"], "hero_confirm.wav");
    assert_eq!(used["sample_rate"], SAMPLE_RATE);
    assert_eq!(used["channels"], 1);
    assert_eq!(used["bits_per_sample"], 16);
    assert_eq!(used["layers_used"], 2);
    assert_eq!(
        used["attribution"],
        "None required (CC0 1.0 Public Domain)"
    );
    let written = std::fs::read(proj_dir.join("assets/audio/hero_confirm.wav")).expect("wav exists");
    let stats = analyze_pcm16_wav(&written).expect("valid 44.1kHz PCM16 WAV");
    assert_eq!(stats.sample_rate, 44_100);
    assert_eq!(stats.channels, 1);
    assert_eq!(stats.bits_per_sample, 16);
    assert!(stats.peak_dbfs <= 0.0);

    // 5. Local directory import + symlink rejection.
    let local_src = temp_dir("local-sfx-src");
    std::fs::write(local_src.join("sword_slash.wav"), &click_wav).unwrap();
    let imported = import_local_sfx_pack(
        &data_dir,
        "my-local-foley",
        "My Local Foley",
        "CC0-1.0",
        &local_src,
    )
    .expect("import local dir");
    assert_eq!(imported["installed_clips"], 1);

    #[cfg(unix)]
    {
        let sym_src = temp_dir("local-sfx-sym");
        std::os::unix::fs::symlink(local_src.join("sword_slash.wav"), sym_src.join("bad.wav"))
            .unwrap();
        let err = import_local_sfx_pack(&data_dir, "bad-sym", "Bad", "CC0-1.0", &sym_src)
            .expect_err("must reject symlink in local import");
        assert!(err.to_ascii_lowercase().contains("symlink"), "got: {err}");
    }

    // 6. Remove installed pack cleanly.
    let removed = remove_sfx_pack(&data_dir, "kenney-ui-sfx").expect("remove pack");
    assert_eq!(removed["removed"], true);
    assert!(remove_sfx_pack(&data_dir, "kenney-ui-sfx").is_err());
}

#[test]
fn audio_presets_runner_installer_and_audio_b64_path_decode() {
    // 1. All 8 optional audio_gen presets present with hardware_tier & model_size.
    let optionals = optional_preset_adapters();
    let audio_presets: Vec<&ToolAdapter> = optionals
        .iter()
        .filter(|a| a.capability == "audio_gen")
        .collect();
    let audio_ids: Vec<&str> = audio_presets.iter().map(|a| a.id.as_str()).collect();
    for expected in [
        "elevenlabs-sfx-http",
        "audioldm-s-local-cli",
        "stable-audio-small-cli",
        "tangoflux-sfx-cli",
        "audioldm2-sfx-cli",
        "stable-audio-open-cli",
        "ace-step-local-http",
        "google-gemini-lyria-http",
    ] {
        let preset = audio_presets
            .iter()
            .find(|p| p.id == expected)
            .unwrap_or_else(|| panic!("missing audio_gen preset '{expected}': {audio_ids:?}"));
        assert!(
            preset.hardware_tier.is_some(),
            "preset '{expected}' missing hardware_tier"
        );
        assert!(
            preset.model_size.is_some(),
            "preset '{expected}' missing model_size"
        );
    }
    assert_eq!(audio_presets.len(), 8);

    // 2. Old adapters.json without new optional metadata fields loads cleanly.
    let data_dir = temp_dir("adapters-compat");
    let legacy_json = json!({
        "version": 1,
        "adapters": [
            {
                "id": "legacy-audio-http",
                "name": "Legacy Audio",
                "capability": "audio_gen",
                "description": "Old format",
                "transport": "http",
                "enabled": true,
                "priority": 50,
                "preset": false,
                "required_secrets": [],
                "http": {
                    "method": "POST",
                    "url_template": "http://127.0.0.1:19999/gen",
                    "allow_loopback": true,
                    "response_extract": { "mode": "file_bytes" }
                }
            }
        ],
        "removed_defaults": []
    });
    std::fs::write(
        data_dir.join("adapters.json"),
        serde_json::to_string_pretty(&legacy_json).unwrap(),
    )
    .unwrap();
    let reg = AdapterRegistry::load(&data_dir);
    let loaded = reg.get("legacy-audio-http").expect("legacy adapter loaded");
    assert!(loaded.hardware_tier.is_none());
    assert!(loaded.model_size.is_none());
    assert!(loaded.install_hint.is_none());

    // 3. Install local neural audio runner -> 0755 on Unix, --check & --version output JSON,
    //    and resolve_adapter_cli_binary_in locates `<data_dir>/bin/openro-audio-gen`.
    let runner_res = install_local_audio_runner(&data_dir).expect("install local audio runner");
    assert_eq!(runner_res["ok"], true);
    let runner_path = local_audio_runner_path(&data_dir);
    assert!(runner_path.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&runner_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "runner must be chmod 0755");
    }
    let resolved = resolve_adapter_cli_binary_in(
        Some(&data_dir),
        "audioldm-s-local-cli",
        "openro-audio-gen",
    )
    .expect("resolve_adapter_cli_binary_in finds installed runner");
    assert_eq!(resolved, runner_path);

    if runner_res["python_available"] == true {
        let check = &runner_res["check"];
        assert_eq!(check["ok"], true);
        assert_eq!(check["tool"], "openro-audio-gen");
        let py = if cfg!(windows) { "python" } else { "python3" };
        let ver_out = Command::new(py)
            .arg(&runner_path)
            .arg("--version")
            .output()
            .expect("run --version");
        assert!(ver_out.status.success());
        let ver_json: Value = serde_json::from_slice(&ver_out.stdout).unwrap();
        assert_eq!(ver_json["ok"], true);
        assert_eq!(ver_json["version"], "1.33.0");
    }

    // 4. `audio_b64_path` decode + 44.1 kHz 16-bit PCM mono WAV normalization.
    // Craft a 22,050 Hz PCM16 WAV (non-44.1 kHz) so we prove normalization resamples to 44,100 Hz!
    let tone_22k: Vec<i16> = (0..2205)
        .map(|i| ((i as f64 * 440.0 * 2.0 * std::f64::consts::PI / 22_050.0).sin() * 16_000.0) as i16)
        .collect();
    let wav_22k = encode_pcm16_wav(&tone_22k, 22_050);
    let b64_resp = json!({
        "outputs": [
            { "data": format!("data:audio/wav;base64,{}", encode_base64(&wav_22k)) }
        ]
    })
    .to_string()
    .into_bytes();
    let b64_port = spawn_file_server(b64_resp, "application/json");

    let store = SecretStore::for_data_dir(&data_dir);
    let mut reg = AdapterRegistry::load(&data_dir);
    reg.upsert(
        &store,
        &json!({
            "id": "mock-lyria-b64",
            "name": "Mock Lyria B64",
            "capability": "audio_gen",
            "transport": "http",
            "priority": 1,
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{b64_port}/interactions"),
                "allow_loopback": true,
                "body_mode": "json",
                "body_template": { "input": "{{prompt}}", "duration": "{{?duration_s}}" },
                "response_extract": { "mode": "audio_b64_path", "path": "outputs.0.data" }
            }
        }),
    )
    .expect("upsert audio_b64_path adapter");

    let proj_dir = temp_dir("b64-proj");
    let ctx = InvokeCtx {
        data_dir: &data_dir,
        assets_root: &data_dir,
        store: &store,
        project_dir: Some(&proj_dir),
    };
    let inv = invoke(
        &ctx,
        Some("audio_gen"),
        Some("mock-lyria-b64"),
        &json!({
            "prompt": "epic crystal chime",
            "duration_ms": 1500,
            "out_path": "assets/audio/lyria_chime.wav"
        }),
    )
    .expect("invoke audio_b64_path adapter");
    assert_eq!(inv["ok"], true);
    let out_bytes = std::fs::read(proj_dir.join("assets/audio/lyria_chime.wav")).unwrap();
    let stats = analyze_pcm16_wav(&out_bytes).expect("normalized valid WAV");
    assert_eq!(stats.sample_rate, 44_100);
    assert_eq!(stats.channels, 1);
    assert_eq!(stats.bits_per_sample, 16);
}

#[test]
fn http_routes_capabilities_library_pack_and_uniform_audio_generate() {
    let _lock = ENV_LOCK.lock().unwrap();
    let data_dir = temp_dir("http-v133");
    let port = start_openro_server(&data_dir);

    // 1. /api/capabilities, /api/health, /api/status advertise new v1.33 flags.
    for ep in ["/api/capabilities", "/api/health", "/api/status"] {
        let (st, v) = http_json(port, "GET", ep, None);
        assert_eq!(st, 200, "GET {ep} failed: {v}");
        assert_eq!(v["cc0_sfx_library"], true, "{ep} missing cc0_sfx_library: {v}");
        assert_eq!(v["audio_presets_tiered"], true, "{ep} missing audio_presets_tiered: {v}");
    }

    // 2. Non-loopback host returns 403 on SFX & runner endpoints.
    for (method, path) in [
        ("GET", "/api/library/sfx-packs"),
        ("POST", "/api/library/sfx-packs/install"),
        ("POST", "/api/library/audio-runner/install"),
    ] {
        let (st, _, _) = http_raw(port, "evil.example:8077", method, path, Some(b"{}"));
        assert_eq!(st, 403, "expected 403 for non-loopback {method} {path}");
    }

    // 3. cc0-sfx-packs in LibraryManager + POST /api/library/test.
    let mut mgr = LibraryManager::open(&data_dir);
    let inst = mgr.install("cc0-sfx-packs").expect("install cc0-sfx-packs");
    assert_eq!(inst["pack"]["id"], "cc0-sfx-packs");
    assert!(mgr.active_tools().contains(&"sfx_pack_use".to_string()));
    let tested = mgr.test_pack("cc0-sfx-packs").expect("test cc0-sfx-packs");
    assert_eq!(tested["passed"], true);

    // 4. POST /api/library/audio-runner/install makes local CLI audio presets available in /api/adapters.
    let (st, runner) = http_json(port, "POST", "/api/library/audio-runner/install", Some(&json!({})));
    assert_eq!(st, 200, "{runner}");
    assert_eq!(runner["ok"], true);
    let (st, adapters) = http_json(port, "GET", "/api/adapters?capability=audio_gen", None);
    assert_eq!(st, 200, "{adapters}");
    let audioldm = adapters["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "audioldm-s-local-cli")
        .expect("audioldm-s-local-cli preset present");
    assert_eq!(audioldm["available"], true, "runner installed -> preset available: {audioldm}");
    assert_eq!(audioldm["hardware_tier"], "tiny-cpu");

    // 5. Uniform POST /api/tools/audio/generate response (builtin fallback + custom adapter).
    let (st, gen_builtin) = http_json(
        port,
        "POST",
        "/api/tools/audio/generate",
        Some(&json!({ "name": "TestBell", "preset": "ui_click", "duration_ms": 100 })),
    );
    assert_eq!(st, 200, "{gen_builtin}");
    for key in ["ok", "path", "filename", "url", "bytes", "sha256", "adapter_id", "source"] {
        assert!(gen_builtin.get(key).is_some(), "missing '{key}' in {gen_builtin}");
    }
    assert_eq!(gen_builtin["adapter_id"], "rust-synth-builtin");
    assert_eq!(gen_builtin["source"], "builtin");

    // 6. Full HTTP loop for /api/library/sfx-packs/* via loopback mock ZIP.
    let wav = synthesize_wav("potion_drink", Some(150)).unwrap().wav_bytes;
    let zip = build_test_zip(&[ZipEntrySpec {
        name: b"Potions/bubble_drink.wav",
        data: &wav,
        method: 8,
        external_attr: 0o100644 << 16,
        declared_uncomp_override: None,
    }]);
    let mock_zip_port = spawn_file_server(zip, "application/zip");
    let _allow = EnvGuard::set("OPENRO_SFX_PACK_ALLOW_LOOPBACK", Some("1"));

    let (st, inst_res) = http_json(
        port,
        "POST",
        "/api/library/sfx-packs/install",
        Some(&json!({
            "pack_id": "rpg-combat-pack",
            "url": format!("http://127.0.0.1:{mock_zip_port}/rpg_sound_pack.zip")
        })),
    );
    assert_eq!(st, 200, "{inst_res}");
    assert_eq!(inst_res["installed_clips"], 1);

    let (st, srch) = http_json(port, "GET", "/api/library/sfx-packs/search?q=bubble", None);
    assert_eq!(st, 200, "{srch}");
    assert_eq!(srch["total"], 1);
    let prev_url = srch["clips"][0]["preview_url"].as_str().unwrap();

    let host = format!("127.0.0.1:{port}");
    let (st_prev, hdrs, prev_bytes) = http_raw(port, &host, "GET", prev_url, None);
    assert_eq!(st_prev, 200);
    assert!(hdrs.to_ascii_lowercase().contains("audio/wav"));
    assert_eq!(prev_bytes, wav);

    let (st_use, used) = http_json(
        port,
        "POST",
        "/api/library/sfx-packs/use",
        Some(&json!({
            "pack_id": "rpg-combat-pack",
            "clip": "Potions/bubble_drink.wav",
            "out_name": "drink_sfx"
        })),
    );
    assert_eq!(st_use, 200, "{used}");
    assert_eq!(used["filename"], "drink_sfx.wav");
    assert_eq!(used["sample_rate"], 44_100);
    assert_eq!(used["attribution"], "None required (CC0 1.0 Public Domain)");

    let (st_del, del) = http_json(port, "DELETE", "/api/library/sfx-packs/rpg-combat-pack", None);
    assert_eq!(st_del, 200, "{del}");
    assert_eq!(del["removed"], true);
}

#[test]
fn agent_sfx_pack_tools_end_to_end() {
    let data_dir = temp_dir("agent-sfx");
    let local_src = temp_dir("agent-sfx-src");
    let wav = synthesize_wav("ui_click", Some(85)).unwrap().wav_bytes;
    std::fs::write(local_src.join("menu_confirm.wav"), &wav).unwrap();
    import_local_sfx_pack(
        &data_dir,
        "agent-pack",
        "Agent Test Pack",
        "CC0-1.0",
        &local_src,
    )
    .expect("seed local pack");

    let turn = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let turn_c = Arc::clone(&turn);
    let llm = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in llm.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let n = turn_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let content = match n {
                0 => json!({
                    "steps": [
                        { "tool": "sfx_pack_list", "args": {} },
                        { "tool": "sfx_pack_search", "args": { "query": "confirm", "pack_id": "agent-pack", "limit": 5 } },
                        { "tool": "sfx_pack_use", "args": { "pack_id": "agent-pack", "clip": "menu_confirm.wav", "out_name": "agent_confirm" } }
                    ],
                    "done": true,
                    "summary": "Installed and mastered confirm SFX."
                })
                .to_string(),
                _ => json!({ "done": true, "summary": "done" }).to_string(),
            };
            let body = json!({
                "choices": [{ "message": { "role": "assistant", "content": content } }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    let port = start_openro_server(&data_dir);
    let (st, cfg) = http_json(
        port,
        "POST",
        "/api/config",
        Some(&json!({
            "provider": "openai",
            "base_url": format!("http://127.0.0.1:{llm_port}/v1"),
            "model_id": "mock-model"
        })),
    );
    assert_eq!(st, 200, "{cfg}");
    let (st, chat) = http_json(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "Find and master a menu confirm sound from our CC0 SFX library." })),
    );
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    assert!(
        data_dir.join("game-assets/audio/agent_confirm.wav").is_file(),
        "agent sfx_pack_use must write mastered WAV"
    );
}

#[cfg(unix)]
#[test]
fn gap89_stale_xvfb_lock_detection_and_cleanup() {
    use openro_rs::studio_embed::{
        cleanup_stale_x11_lock_in, parse_x11_display_num, x11_socket_exists_in,
    };

    assert_eq!(parse_x11_display_num(":99"), Some("99"));
    assert_eq!(parse_x11_display_num(":99.0"), Some("99"));
    assert_eq!(parse_x11_display_num("invalid"), None);

    let fake_tmp = temp_dir("x11-tmp");
    let fake_proc = temp_dir("x11-proc");
    std::fs::create_dir_all(fake_tmp.join(".X11-unix")).unwrap();

    let sock99 = fake_tmp.join(".X11-unix/X99");
    let lock99 = fake_tmp.join(".X99-lock");
    std::fs::write(&sock99, b"socket-placeholder").unwrap();

    // Case 1: Lock points to a DEAD PID (not in fake_proc) -> x11_socket_exists_in returns false,
    // and cleanup_stale_x11_lock_in removes both lock & socket.
    std::fs::write(&lock99, "     999991\n").unwrap();
    assert!(
        !x11_socket_exists_in(&fake_tmp, &fake_proc, ":99"),
        "dead Xvfb PID must report socket unavailable"
    );
    assert!(
        cleanup_stale_x11_lock_in(&fake_tmp, &fake_proc, ":99", None),
        "stale lock & socket should be unlinked"
    );
    assert!(!lock99.exists(), "stale .X99-lock must be removed");
    assert!(!sock99.exists(), "stale .X11-unix/X99 must be removed");

    // Case 2: Lock points to a LIVE PID (exists in fake_proc) -> x11_socket_exists_in returns true,
    // and cleanup_stale_x11_lock_in MUST NOT touch the files!
    let live_pid = std::process::id();
    std::fs::create_dir_all(fake_proc.join(live_pid.to_string())).unwrap();
    std::fs::write(&sock99, b"live-socket").unwrap();
    std::fs::write(&lock99, format!("{live_pid}\n")).unwrap();
    assert!(
        x11_socket_exists_in(&fake_tmp, &fake_proc, ":99"),
        "live Xvfb PID must report socket available"
    );
    assert!(
        !cleanup_stale_x11_lock_in(&fake_tmp, &fake_proc, ":99", Some(live_pid)),
        "cleanup must never remove files owned by a live PID"
    );
    assert!(lock99.exists(), "live .X99-lock must remain untouched");
    assert!(sock99.exists(), "live .X11-unix/X99 must remain untouched");

    // Case 3: Owned xvfb_child stopped (PID no longer in fake_proc) -> stop teardown cleans up.
    std::fs::remove_dir_all(fake_proc.join(live_pid.to_string())).unwrap();
    assert!(
        cleanup_stale_x11_lock_in(&fake_tmp, &fake_proc, ":99", Some(live_pid)),
        "owned dead PID lock should be cleaned on stop"
    );
    assert!(!lock99.exists());
    assert!(!sock99.exists());
}

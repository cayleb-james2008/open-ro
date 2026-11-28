//! Live Game Teardown Engine (Gap #93):
//! Inspects any live Roblox game via public Roblox APIs (game metadata, votes, thumbnails,
//! badges, game passes, media), optional YouTube gameplay/caption/frame research (`yt-dlp` + `ffmpeg`),
//! and optional public wiki/guide research (`web_search` + `web_fetch`), producing a structured
//! 7-pillar `e2e_spec`, curated UI reference screenshots, `refs/teardowns/<uid>/teardown.json`,
//! `refs/teardowns/<uid>/TEARDOWN.md`, `refs/teardowns/latest.json`, and `observed` evidence.

use crate::audio_library::{
    find_ffmpeg_binary, find_yt_dlp_binary, read_mock_jsonl, run_argv_with_timeout,
    validate_youtube_url,
};
use crate::market_taste::{
    detect_image_ext, download_image_bytes, fetch_bytes_checked,
    research_roblox_game_to_project_with_base, validate_public_url, web_fetch, web_search,
};
use crate::opencloud::{now_iso, safe_rel, sha256_hex, Oc};
use crate::projects::now_ms;
use crate::study_check::record_evidence;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const YTDLP_TIMEOUT: Duration = Duration::from_secs(45);
const HTTP_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_MEDIA_DOWNLOADS: usize = 6;
const MAX_SCREENSHOT_BYTES: usize = 20 * 1024 * 1024;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub const E2E_SPEC_PILLARS: [&str; 7] = [
    "core_loop",
    "first_60s_onboarding",
    "screens_and_copy_density",
    "economy_and_monetization",
    "world_3d_and_props",
    "audio_sfx_mood",
    "retention_hooks",
];

static TEARDOWN_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bad(msg: impl Into<String>) -> Oc {
    Oc {
        status: 400,
        msg: msg.into(),
    }
}

fn err(status: u16, msg: impl Into<String>) -> Oc {
    Oc {
        status,
        msg: msg.into(),
    }
}

fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), Oc> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(500, format!("create dir '{}': {e}", parent.display())))?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(bad(format!(
                "refusing to overwrite symlink '{}'",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| err(500, "missing parent path for atomic write"))?;
    let fname = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("teardown");
    let seq = TEARDOWN_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(
        ".{fname}.tmp-{}-{seq}",
        std::process::id()
    ));
    let write_res = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|_| f.flush()));
    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(500, format!("write '{}': {e}", path.display())));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(500, format!("commit '{}': {e}", path.display())));
    }
    Ok(())
}

fn write_atomic_json(path: &Path, value: &Value) -> Result<(), Oc> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| err(500, format!("serialize teardown JSON: {e}")))?;
    write_atomic_bytes(path, text.as_bytes())
}

fn resolve_api_base(body: &Value) -> Result<Option<String>, Oc> {
    let raw = body
        .get("api_base_override")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            std::env::var("OPENRO_ROBLOX_API_BASE")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    if let Some(ref base) = raw {
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(bad("api_base_override must use http:// or https://"));
        }
    }
    Ok(raw)
}

fn fetch_json_url(url: &str, allow_loopback: bool) -> Result<Value, String> {
    let (_, _, bytes) = fetch_bytes_checked(url, allow_loopback, 2 * 1024 * 1024, HTTP_TIMEOUT)?;
    serde_json::from_slice::<Value>(&bytes)
        .map_err(|e| format!("parse JSON from {url}: {e}"))
}

fn fetch_roblox_badges(
    universe_id: u64,
    api_base: Option<&str>,
) -> (Vec<Value>, Option<String>) {
    let allow_loopback = api_base.is_some();
    let base = api_base
        .map(|b| b.trim_end_matches('/'))
        .unwrap_or("https://badges.roblox.com");
    let url = format!("{base}/v1/universes/{universe_id}/badges?limit=50&sortOrder=Asc");
    match fetch_json_url(&url, allow_loopback) {
        Ok(val) => {
            let items = val
                .get("data")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .take(50)
                        .map(|b| {
                            json!({
                                "id": b.get("id").and_then(Value::as_u64),
                                "name": b.get("displayName").or_else(|| b.get("name")).and_then(Value::as_str).unwrap_or("").trim(),
                                "description": b.get("displayDescription").or_else(|| b.get("description")).and_then(Value::as_str).unwrap_or("").trim(),
                                "enabled": b.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                                "awarded_count": b.pointer("/statistics/awardedCount").and_then(Value::as_u64),
                                "win_rate_pct": b.pointer("/statistics/winRatePercentage").and_then(Value::as_f64),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            (items, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    }
}

fn fetch_roblox_game_passes(
    universe_id: u64,
    api_base: Option<&str>,
) -> (Vec<Value>, Option<String>) {
    let allow_loopback = api_base.is_some();
    let base = api_base
        .map(|b| b.trim_end_matches('/'))
        .unwrap_or("https://apis.roblox.com");
    let url =
        format!("{base}/game-passes/v1/universes/{universe_id}/game-passes?passView=Full&pageSize=50");
    match fetch_json_url(&url, allow_loopback) {
        Ok(val) => {
            let arr_opt = val
                .get("gamePasses")
                .or_else(|| val.get("data"))
                .and_then(Value::as_array);
            let items = arr_opt
                .map(|arr| {
                    arr.iter()
                        .take(50)
                        .map(|gp| {
                            json!({
                                "id": gp.get("id").and_then(Value::as_u64),
                                "name": gp.get("displayName").or_else(|| gp.get("name")).and_then(Value::as_str).unwrap_or("").trim(),
                                "description": gp.get("description").and_then(Value::as_str).unwrap_or("").trim(),
                                "price_robux": gp.get("price").or_else(|| gp.get("priceRobux")).and_then(Value::as_u64),
                                "is_for_sale": gp.get("isForSale").and_then(Value::as_bool).unwrap_or(true),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            (items, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    }
}

fn fetch_roblox_media_and_download(
    project_dir: &Path,
    universe_id: u64,
    api_base: Option<&str>,
) -> (Vec<Value>, Vec<Value>, Option<String>) {
    let allow_loopback = api_base.is_some();
    let games_base = api_base
        .map(|b| b.trim_end_matches('/'))
        .unwrap_or("https://games.roblox.com");
    let thumbs_base = api_base
        .map(|b| b.trim_end_matches('/'))
        .unwrap_or("https://thumbnails.roblox.com");
    let media_url = format!("{games_base}/v2/games/{universe_id}/media");

    let raw_media = match fetch_json_url(&media_url, allow_loopback) {
        Ok(v) => v,
        Err(e) => return (Vec::new(), Vec::new(), Some(e)),
    };

    let entries = raw_media
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut direct_urls: Vec<(Option<u64>, String)> = Vec::new();
    let mut asset_ids: Vec<u64> = Vec::new();
    let mut media_items: Vec<Value> = Vec::new();

    for entry in entries.iter().take(12) {
        let asset_type = entry
            .get("assetType")
            .and_then(Value::as_str)
            .unwrap_or("");
        let asset_type_id = entry.get("assetTypeId").and_then(Value::as_u64);
        let image_id = entry.get("imageId").and_then(Value::as_u64);
        let direct_url = entry
            .get("imageUrl")
            .or_else(|| entry.get("url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
            .map(ToString::to_string);
        let is_image = asset_type.eq_ignore_ascii_case("Image")
            || asset_type_id == Some(1)
            || image_id.is_some()
            || direct_url.is_some();

        media_items.push(json!({
            "asset_type": if asset_type.is_empty() && is_image { "Image" } else { asset_type },
            "image_id": image_id,
            "video_hash": entry.get("videoHash").and_then(Value::as_str),
            "video_title": entry.get("videoTitle").and_then(Value::as_str),
            "approved": entry.get("approved").and_then(Value::as_bool).unwrap_or(true),
            "url": direct_url,
        }));

        if direct_urls.len() + asset_ids.len() < MAX_MEDIA_DOWNLOADS && is_image {
            if let Some(u) = direct_url {
                direct_urls.push((image_id, u));
            } else if let Some(aid) = image_id.filter(|&id| id > 0) {
                asset_ids.push(aid);
            }
        }
    }

    if !asset_ids.is_empty() {
        let id_csv = asset_ids
            .iter()
            .take(MAX_MEDIA_DOWNLOADS)
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let thumb_api = format!(
            "{thumbs_base}/v1/assets?assetIds={id_csv}&returnPolicy=PlaceHolder&size=768x432&format=Png&isCircular=false"
        );
        if let Ok(tv) = fetch_json_url(&thumb_api, allow_loopback) {
            if let Some(arr) = tv.get("data").and_then(Value::as_array) {
                for row in arr {
                    if direct_urls.len() >= MAX_MEDIA_DOWNLOADS {
                        break;
                    }
                    let tid = row.get("targetId").and_then(Value::as_u64);
                    if let Some(u) = row
                        .get("imageUrl")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                    {
                        direct_urls.push((tid, u.to_string()));
                    }
                }
            }
        }
    }

    let mut screenshots: Vec<Value> = Vec::new();
    for (idx, (image_id, url)) in direct_urls.into_iter().take(MAX_MEDIA_DOWNLOADS).enumerate() {
        if !allow_loopback && validate_public_url(&url, false).is_err() {
            continue;
        }
        if let Ok(bytes) = download_image_bytes(&url, allow_loopback, HTTP_TIMEOUT) {
            let Some(ext) = detect_image_ext(&bytes) else {
                continue;
            };
            let rel = format!("refs/teardowns/{universe_id}/screenshots/media_{}.{ext}", idx + 1);
            if let Ok(abs) = safe_rel(project_dir, &rel) {
                if write_atomic_bytes(&abs, &bytes).is_ok() {
                    screenshots.push(json!({
                        "path": rel,
                        "role": "storefront_media",
                        "source": "roblox_media",
                        "timestamp_s": Value::Null,
                        "notes": image_id
                            .map(|id| format!("Roblox media asset {id}"))
                            .unwrap_or_else(|| format!("Roblox storefront media #{}", idx + 1)),
                        "copy_observed": [],
                    }));
                }
            }
        }
    }

    (media_items, screenshots, None)
}

fn sanitize_yt_query(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '\'' | ':') {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

fn parse_vtt_snippet(vtt: &str, max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut prev = String::new();
    for raw_line in vtt.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("WEBVTT")
            || trimmed.starts_with("Kind:")
            || trimmed.starts_with("Language:")
            || trimmed.starts_with("NOTE")
            || trimmed.contains("-->")
            || trimmed.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        // Strip inline VTT tags like `<00:00:01.000><c>`
        let mut clean = String::with_capacity(trimmed.len());
        let mut in_tag = false;
        for ch in trimmed.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => clean.push(ch),
                _ => {}
            }
        }
        let collapsed = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() && collapsed != prev {
            prev = collapsed.clone();
            lines.push(collapsed);
        }
    }
    let joined = lines.join(" ");
    joined.chars().take(max_chars).collect()
}

fn extract_youtube_captions_ytdlp(yt_dlp: &Path, watch_url: &str, tmp_dir: &Path, vid: &str) -> String {
    let out_tmpl = tmp_dir.join("%(id)s").to_string_lossy().into_owned();
    let argv = vec![
        "--write-auto-sub".to_string(),
        "--write-sub".to_string(),
        "--sub-lang".to_string(),
        "en".to_string(),
        "--sub-format".to_string(),
        "vtt".to_string(),
        "--skip-download".to_string(),
        "--no-warnings".to_string(),
        "--no-playlist".to_string(),
        "-o".to_string(),
        out_tmpl,
        "--".to_string(),
        watch_url.to_string(),
    ];
    if run_argv_with_timeout(yt_dlp, &argv, YTDLP_TIMEOUT).is_err() {
        return String::new();
    }
    if let Ok(rd) = std::fs::read_dir(tmp_dir) {
        for entry in rd.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(vid) && fname.ends_with(".vtt") {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    return parse_vtt_snippet(&text, 1200);
                }
            }
        }
    }
    String::new()
}

fn capture_youtube_frames_ffmpeg(
    yt_dlp: &Path,
    ffmpeg: &Path,
    watch_url: &str,
    vid: &str,
    tmp_dir: &Path,
    project_dir: &Path,
    universe_id: u64,
) -> Vec<Value> {
    let vid_tmpl = tmp_dir.join(format!("{vid}.%(ext)s")).to_string_lossy().into_owned();
    let dl_argv = vec![
        "--no-playlist".to_string(),
        "--no-warnings".to_string(),
        "--max-filesize".to_string(),
        "60M".to_string(),
        "-f".to_string(),
        "mp4/best[height<=480]".to_string(),
        "-o".to_string(),
        vid_tmpl,
        "--".to_string(),
        watch_url.to_string(),
    ];
    let Ok(dl_run) = run_argv_with_timeout(yt_dlp, &dl_argv, YTDLP_TIMEOUT) else {
        return Vec::new();
    };
    if dl_run.timed_out || dl_run.code != Some(0) {
        return Vec::new();
    }
    let video_path = std::fs::read_dir(tmp_dir)
        .ok()
        .and_then(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with(vid) && !n.ends_with(".vtt") && !n.ends_with(".png"))
                })
        });
    let Some(video_path) = video_path else {
        return Vec::new();
    };

    let frame_dir = tmp_dir.join(format!("frames_{vid}"));
    if std::fs::create_dir_all(&frame_dir).is_err() {
        return Vec::new();
    }
    let pattern = frame_dir.join(format!("yt_{vid}_%02d.png"));
    let ff_argv = vec![
        "-nostdin".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-i".to_string(),
        video_path.to_string_lossy().into_owned(),
        "-vf".to_string(),
        "fps=1/30,scale=1280:-1".to_string(),
        "-frames:v".to_string(),
        "4".to_string(),
        pattern.to_string_lossy().into_owned(),
    ];
    let Ok(ff_run) = run_argv_with_timeout(ffmpeg, &ff_argv, YTDLP_TIMEOUT) else {
        return Vec::new();
    };
    if ff_run.timed_out || ff_run.code != Some(0) {
        return Vec::new();
    }

    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&frame_dir) {
        let mut frame_files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        frame_files.sort();
        for (idx, src) in frame_files.into_iter().take(4).enumerate() {
            if let Ok(bytes) = std::fs::read(&src) {
                if bytes.starts_with(PNG_MAGIC) {
                    let fname = src
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("frame.png");
                    let rel = format!("refs/teardowns/{universe_id}/screenshots/{fname}");
                    if let Ok(dst) = safe_rel(project_dir, &rel) {
                        if write_atomic_bytes(&dst, &bytes).is_ok() {
                            out.push(json!({
                                "path": rel,
                                "role": "youtube_frame",
                                "source": "youtube",
                                "timestamp_s": (idx as f64) * 30.0,
                                "notes": format!("Sampled frame {} from {watch_url}", idx + 1),
                                "copy_observed": [],
                            }));
                        }
                    }
                }
            }
        }
    }
    out
}

fn normalize_youtube_video_entry(raw: &Value, default_captions: Option<&str>) -> Option<(String, String, Value)> {
    let candidate_url = raw
        .get("webpage_url")
        .or_else(|| raw.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            raw.get("id")
                .and_then(Value::as_str)
                .map(|id| format!("https://www.youtube.com/watch?v={}", id.trim()))
        })?;
    let (vid, watch_url) = validate_youtube_url(&candidate_url).ok()?;
    let title = raw
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    let captions = raw
        .get("captions")
        .or_else(|| raw.get("caption_snippet"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(default_captions)
        .unwrap_or("")
        .to_string();
    let desc: String = raw
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(500)
        .collect();
    let entry = json!({
        "id": vid,
        "url": watch_url,
        "title": title,
        "channel": raw.get("channel").or_else(|| raw.get("uploader")).and_then(Value::as_str).unwrap_or("").trim(),
        "duration_s": raw.get("duration").or_else(|| raw.get("duration_s")).and_then(Value::as_u64),
        "view_count": raw.get("view_count").and_then(Value::as_u64),
        "description": desc,
        "captions": captions,
    });
    Some((vid, watch_url, entry))
}

fn run_youtube_research(
    project_dir: &Path,
    universe_id: u64,
    game_name: &str,
    queries: &[String],
    max_videos: usize,
    capture_frames: bool,
) -> (Value, Vec<Value>) {
    let queries_used: Vec<String> = if queries.is_empty() {
        vec![
            sanitize_yt_query(&format!("{game_name} roblox gameplay")),
            sanitize_yt_query(&format!("{game_name} roblox guide tier list")),
        ]
    } else {
        queries
            .iter()
            .map(|q| sanitize_yt_query(q))
            .filter(|q| !q.is_empty())
            .take(4)
            .collect()
    };

    if let Ok(mock_raw) = std::env::var("OPENRO_TEARDOWN_YT_MOCK_JSON") {
        let trimmed = mock_raw.trim();
        if !trimmed.is_empty() {
            let text = read_mock_jsonl(trimmed);
            let mut videos = Vec::new();
            let mut seen = BTreeSet::new();
            let mut extra_screens = Vec::new();

            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                let captions_map = val.get("captions").and_then(Value::as_object);
                let arr_opt = val
                    .get("videos")
                    .and_then(Value::as_array)
                    .or_else(|| val.as_array());
                if let Some(arr) = arr_opt {
                    for item in arr {
                        if videos.len() >= max_videos {
                            break;
                        }
                        let vid_key = item.get("id").and_then(Value::as_str).unwrap_or("");
                        let cap_override = captions_map
                            .and_then(|m| m.get(vid_key))
                            .and_then(Value::as_str);
                        if let Some((vid, _url, entry)) =
                            normalize_youtube_video_entry(item, cap_override)
                        {
                            if seen.insert(vid) {
                                videos.push(entry);
                            }
                        }
                    }
                }
                if capture_frames {
                    if let Some(frames) = val.get("frames").and_then(Value::as_array) {
                        for f in frames {
                            if let Some(rel) = f.get("path").and_then(Value::as_str) {
                                extra_screens.push(json!({
                                    "path": rel,
                                    "role": f.get("role").and_then(Value::as_str).unwrap_or("youtube_frame"),
                                    "source": "youtube",
                                    "timestamp_s": f.get("timestamp_s").cloned().unwrap_or(Value::Null),
                                    "notes": f.get("notes").and_then(Value::as_str).unwrap_or("Mock captured frame"),
                                    "copy_observed": f.get("copy_observed").cloned().unwrap_or_else(|| json!([])),
                                }));
                            }
                        }
                    }
                }
            } else {
                for line in text.lines() {
                    if videos.len() >= max_videos {
                        break;
                    }
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(item) = serde_json::from_str::<Value>(line) {
                        if let Some((vid, _url, entry)) = normalize_youtube_video_entry(&item, None) {
                            if seen.insert(vid) {
                                videos.push(entry);
                            }
                        }
                    }
                }
            }

            return (
                json!({
                    "ok": true,
                    "source": "mock",
                    "queries": queries_used,
                    "capture_frames": capture_frames,
                    "video_count": videos.len(),
                    "videos": videos,
                }),
                extra_screens,
            );
        }
    }

    let Some(yt_dlp) = find_yt_dlp_binary() else {
        return (
            json!({
                "ok": false,
                "source": "unavailable",
                "queries": queries_used,
                "capture_frames": capture_frames,
                "video_count": 0,
                "videos": [],
                "warning": "yt-dlp not found on PATH; skipped YouTube deep research",
            }),
            Vec::new(),
        );
    };

    let tmp_dir = std::env::temp_dir().join(format!(
        "openro-teardown-yt-{}-{}",
        std::process::id(),
        TEARDOWN_TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let _tmp_guard = TempDirGuard(tmp_dir.clone());

    let mut seen = BTreeSet::new();
    let mut videos: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let per_query = ((max_videos + queries_used.len().saturating_sub(1)) / queries_used.len().max(1)).clamp(2, 6);

    for q in &queries_used {
        if videos.len() >= max_videos {
            break;
        }
        let argv = vec![
            "--dump-json".to_string(),
            "--no-warnings".to_string(),
            "--skip-download".to_string(),
            "--ignore-errors".to_string(),
            "--".to_string(),
            format!("ytsearch{per_query}:{q}"),
        ];
        match run_argv_with_timeout(&yt_dlp, &argv, YTDLP_TIMEOUT) {
            Ok(run) => {
                if run.timed_out {
                    warnings.push(format!("yt-dlp query '{q}' timed out after 45s"));
                }
                for line in run.stdout.lines() {
                    if videos.len() >= max_videos {
                        break;
                    }
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(raw) = serde_json::from_str::<Value>(line) {
                        if let Some((vid, watch_url, mut entry)) =
                            normalize_youtube_video_entry(&raw, None)
                        {
                            if seen.insert(vid.clone()) {
                                let snippet = extract_youtube_captions_ytdlp(
                                    &yt_dlp, &watch_url, &tmp_dir, &vid,
                                );
                                if !snippet.is_empty() {
                                    entry["captions"] = json!(snippet);
                                }
                                videos.push(entry);
                            }
                        }
                    }
                }
            }
            Err(e) => warnings.push(e),
        }
    }

    let mut frame_screens = Vec::new();
    if capture_frames {
        if let Some(ffmpeg) = find_ffmpeg_binary() {
            for v in videos.iter().take(2) {
                if let (Some(vid), Some(url)) =
                    (v.get("id").and_then(Value::as_str), v.get("url").and_then(Value::as_str))
                {
                    if let Ok((valid_id, valid_url)) = validate_youtube_url(url) {
                        if valid_id == vid {
                            frame_screens.extend(capture_youtube_frames_ffmpeg(
                                &yt_dlp,
                                &ffmpeg,
                                &valid_url,
                                &valid_id,
                                &tmp_dir,
                                project_dir,
                                universe_id,
                            ));
                        }
                    }
                }
            }
        } else {
            warnings.push("ffmpeg not found on PATH; frame capture skipped".to_string());
        }
    }

    (
        json!({
            "ok": true,
            "source": "yt-dlp",
            "queries": queries_used,
            "capture_frames": capture_frames,
            "video_count": videos.len(),
            "videos": videos,
            "warnings": warnings,
        }),
        frame_screens,
    )
}

fn run_wiki_research(game_name: &str, wiki_urls: &[String]) -> Value {
    if let Ok(mock_raw) = std::env::var("OPENRO_TEARDOWN_WEB_MOCK_JSON") {
        let trimmed = mock_raw.trim();
        if !trimmed.is_empty() {
            let text = read_mock_jsonl(trimmed);
            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                let search_results = val
                    .get("search_results")
                    .or_else(|| val.get("results"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let pages = val
                    .get("pages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                return json!({
                    "ok": true,
                    "source": "mock",
                    "search_results": search_results,
                    "pages": pages,
                    "page_count": pages.len(),
                });
            }
        }
    }

    let mut urls_to_fetch: Vec<String> = Vec::new();
    let mut seen_urls = BTreeSet::new();
    let mut warnings: Vec<String> = Vec::new();
    for raw in wiki_urls {
        let clean = raw.trim();
        if clean.is_empty() {
            continue;
        }
        match validate_public_url(clean, false) {
            Ok(valid) => {
                if seen_urls.insert(valid.clone()) {
                    urls_to_fetch.push(valid);
                }
            }
            Err(e) => warnings.push(format!("skipped wiki_url '{clean}': {e}")),
        }
    }

    let mut search_results: Vec<Value> = Vec::new();
    if urls_to_fetch.is_empty() {
        let query = format!("\"{game_name}\" roblox wiki guide");
        match web_search(&query, 5) {
            Ok(res) => {
                if let Some(arr) = res.get("results").and_then(Value::as_array) {
                    for item in arr {
                        search_results.push(item.clone());
                        if urls_to_fetch.len() < 4 {
                            if let Some(u) = item.get("url").and_then(Value::as_str) {
                                if let Ok(valid) = validate_public_url(u, false) {
                                    if seen_urls.insert(valid.clone()) {
                                        urls_to_fetch.push(valid);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => warnings.push(format!("web_search failed: {e}")),
        }
    }

    let mut pages: Vec<Value> = Vec::new();
    for url in urls_to_fetch.into_iter().take(4) {
        match web_fetch(&url, 6000) {
            Ok(fetched) => {
                let text = fetched
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(2000)
                    .collect::<String>();
                pages.push(json!({
                    "url": fetched.get("url").and_then(Value::as_str).unwrap_or(&url),
                    "bytes": fetched.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                    "excerpt": text,
                }));
            }
            Err(e) => warnings.push(format!("web_fetch '{url}' failed: {e}")),
        }
    }

    json!({
        "ok": true,
        "source": "web_search+web_fetch",
        "search_results": search_results,
        "pages": pages,
        "page_count": pages.len(),
        "warnings": warnings,
    })
}

fn screenshot_priority(role: &str) -> u8 {
    let r = role.trim().to_ascii_lowercase();
    if r == "hud" {
        0
    } else if r.starts_with("modal_") {
        1
    } else if r == "pack_open" {
        2
    } else if !matches!(
        r.as_str(),
        "youtube_frame" | "storefront_media" | "storefront_thumbnail"
    ) {
        3
    } else if r == "youtube_frame" {
        4
    } else if r == "storefront_media" {
        5
    } else {
        6
    }
}

fn recompute_recommended_ui_refs(project_dir: &Path, screenshots: &[Value]) -> Vec<String> {
    let mut ranked: Vec<(u8, usize, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for (idx, item) in screenshots.iter().enumerate() {
        let Some(rel) = item.get("path").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(rel.to_string()) {
            continue;
        }
        if !is_valid_project_png(project_dir, rel) {
            continue;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("storefront_thumbnail");
        ranked.push((screenshot_priority(role), idx, rel.to_string()));
    }
    ranked.sort_by_key(|(prio, idx, _)| (*prio, *idx));
    ranked.into_iter().take(6).map(|(_, _, rel)| rel).collect()
}

fn is_valid_project_png(project_dir: &Path, rel: &str) -> bool {
    let Ok(abs) = safe_rel(project_dir, rel) else {
        return false;
    };
    let Ok(meta) = std::fs::symlink_metadata(&abs) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() < 8 {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(&abs) else {
        return false;
    };
    let mut head = [0u8; 8];
    use std::io::Read;
    f.read_exact(&mut head).is_ok() && &head == PNG_MAGIC
}

fn build_e2e_spec(
    game: &Value,
    badges: &[Value],
    game_passes: &[Value],
    yt: &Value,
    wiki: &Value,
    new_niche: Option<&str>,
) -> Value {
    let name = game.get("name").and_then(Value::as_str).unwrap_or("Reference Game");
    let genre = game.get("genre").and_then(Value::as_str).unwrap_or("All");
    let niche = new_niche
        .or_else(|| game.get("niche").and_then(Value::as_str))
        .unwrap_or("simulator");
    let badge_names: Vec<String> = badges
        .iter()
        .filter_map(|b| b.get("name").and_then(Value::as_str).map(ToString::to_string))
        .filter(|s| !s.is_empty())
        .take(8)
        .collect();
    let pass_summaries: Vec<String> = game_passes
        .iter()
        .filter_map(|p| {
            let pname = p.get("name").and_then(Value::as_str)?.trim();
            if pname.is_empty() {
                return None;
            }
            let price = p
                .get("price_robux")
                .and_then(Value::as_u64)
                .map(|r| format!(" ({r} R$)"))
                .unwrap_or_default();
            Some(format!("{pname}{price}"))
        })
        .take(8)
        .collect();
    let yt_titles: Vec<String> = yt
        .get("videos")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("title").and_then(Value::as_str).map(ToString::to_string))
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    let wiki_urls: Vec<String> = wiki
        .get("pages")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("url").and_then(Value::as_str).map(ToString::to_string))
                .take(4)
                .collect()
        })
        .unwrap_or_default();

    json!({
        "core_loop": {
            "summary": format!("Observed '{name}' ({genre} -> target niche '{niche}') progression signals across {} badges and {} YouTube references.", badges.len(), yt_titles.len()),
            "observed_badges": badge_names,
            "youtube_references": yt_titles,
            "design_targets": [
                "Tight <=20s loop: earn -> upgrade/roll -> equip -> advance zone",
                "Clear visual feedback on every reward & tier breakthrough"
            ],
        },
        "first_60s_onboarding": {
            "summary": "Immediate spawn clarity, auto-directed first milestone within 15s, and zero multi-paragraph modal walls.",
            "observed_early_milestones": badges.iter().take(3).filter_map(|b| b.get("name").and_then(Value::as_str)).collect::<Vec<_>>(),
            "design_targets": [
                "Visible objective beacon + single primary CTA in first 10 seconds",
                "First reward grant within 30 seconds"
            ],
        },
        "screens_and_copy_density": {
            "summary": "Perimeter HUD with bold outlined numerals, dense multi-column inventory/shop grids, and <=1-line labels.",
            "reference_sources": ["storefront_thumbnails", "recorded_hud_screenshots"],
            "design_targets": [
                "HUD leaves >=65% center viewport open on 1920x1080 and 844x390",
                "Reference-guided UI skin variant (not a 1:1 clone)"
            ],
        },
        "economy_and_monetization": {
            "summary": format!("Observed {} public GamePasses on '{name}'.", game_passes.len()),
            "observed_game_passes": pass_summaries,
            "design_targets": [
                "F2P baseline remains rewarding; GamePasses accelerate convenience/slots/2x",
                "Verify earn-to-cost curve via economy-sim before publish"
            ],
        },
        "world_3d_and_props": {
            "summary": format!("Hub/plaza focal layout adapted for '{niche}' with sculpted <=20k-tri Blender FileMesh props."),
            "wiki_sources": wiki_urls,
            "design_targets": [
                "Distinct landmark zone portals, pedestals, and upgrade stations",
                "Mobile StreamingEnabled budgets: <=20k tris/mesh, <=12 non-shadow lights"
            ],
        },
        "audio_sfx_mood": {
            "summary": "Crisp tactile UI click/pop/fanfare cues paired with loop-ready copyright-free background music.",
            "design_targets": [
                "CC0 / motion-UI SFX mastered to 44.1kHz mono WAV",
                "Non-intrusive loop BGM with Settings mute toggle"
            ],
        },
        "retention_hooks": {
            "summary": format!("Badge milestones ({}) + escalating rarity/zone unlocks driving session goals.", badges.len()),
            "design_targets": [
                "Daily/milestone progression bar + badge unlocks wired to real actions",
                "DataStore persistence verified across rejoin in headless sandbox"
            ],
        },
    })
}

fn render_teardown_markdown(doc: &Value) -> String {
    let game = &doc["game"];
    let uid = doc["universe_id"].as_u64().unwrap_or(0);
    let name = game["name"].as_str().unwrap_or("Unknown Game");
    let genre = game["genre"].as_str().unwrap_or("All");
    let playing = game["playing"].as_u64().unwrap_or(0);
    let visits = game["visits"].as_u64().unwrap_or(0);
    let likes = game["like_ratio_pct"].as_f64().unwrap_or(0.0);
    let niche = doc["new_niche"]
        .as_str()
        .or_else(|| game["niche"].as_str())
        .unwrap_or("simulator");

    let mut md = String::new();
    md.push_str(&format!("# Live Game Teardown: {name}\n\n"));
    md.push_str(&format!("- **Universe ID:** `{uid}`\n"));
    md.push_str(&format!("- **Root Place ID:** `{}`\n", doc["root_place_id"].as_u64().unwrap_or(0)));
    md.push_str(&format!("- **Genre / Target Niche:** `{genre}` -> `{niche}`\n"));
    md.push_str(&format!("- **Live Metrics:** {playing} playing · {visits} visits · {likes:.1}% likes\n\n"));

    let badges = doc["badges"].as_array().cloned().unwrap_or_default();
    md.push_str(&format!("## Badges ({})\n\n", badges.len()));
    for b in badges.iter().take(20) {
        let bn = b["name"].as_str().unwrap_or("");
        let bd = b["description"].as_str().unwrap_or("");
        md.push_str(&format!("- **{bn}**: {bd}\n"));
    }
    md.push('\n');

    let passes = doc["game_passes"].as_array().cloned().unwrap_or_default();
    md.push_str(&format!("## GamePasses ({})\n\n", passes.len()));
    for gp in passes.iter().take(20) {
        let gn = gp["name"].as_str().unwrap_or("");
        let price = gp["price_robux"]
            .as_u64()
            .map(|p| format!("{p} Robux"))
            .unwrap_or_else(|| "off-sale / unknown".to_string());
        md.push_str(&format!("- **{gn}** ({price})\n"));
    }
    md.push('\n');

    let videos = doc
        .pointer("/youtube_research/videos")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    md.push_str(&format!("## YouTube Research ({})\n\n", videos.len()));
    for v in &videos {
        let title = v["title"].as_str().unwrap_or("");
        let url = v["url"].as_str().unwrap_or("");
        let cap = v["captions"].as_str().unwrap_or("");
        md.push_str(&format!("- [{title}]({url})\n"));
        if !cap.is_empty() {
            let short: String = cap.chars().take(240).collect();
            md.push_str(&format!("  - Captions: {short}\n"));
        }
    }
    md.push('\n');

    let pages = doc
        .pointer("/wiki_research/pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    md.push_str(&format!("## Wiki / Guide Research ({})\n\n", pages.len()));
    for p in &pages {
        let url = p["url"].as_str().unwrap_or("");
        let ex: String = p["excerpt"].as_str().unwrap_or("").chars().take(200).collect();
        md.push_str(&format!("- {url}\n  - {ex}\n"));
    }
    md.push('\n');

    let screens = doc["screenshots"].as_array().cloned().unwrap_or_default();
    md.push_str(&format!("## Screenshots ({})\n\n", screens.len()));
    for s in &screens {
        let p = s["path"].as_str().unwrap_or("");
        let role = s["role"].as_str().unwrap_or("");
        let src = s["source"].as_str().unwrap_or("");
        let notes = s["notes"].as_str().unwrap_or("");
        md.push_str(&format!("- `{p}` (`{role}` / `{src}`) — {notes}\n"));
    }
    md.push('\n');

    md.push_str("## 7-Pillar E2E Spec\n\n");
    for pillar in E2E_SPEC_PILLARS {
        let val = &doc["e2e_spec"][pillar];
        let summary = val
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("");
        md.push_str(&format!("### `{pillar}`\n{summary}\n\n"));
    }
    md
}

fn teardown_paths(universe_id: u64) -> (String, String) {
    (
        format!("refs/teardowns/{universe_id}/teardown.json"),
        format!("refs/teardowns/{universe_id}/TEARDOWN.md"),
    )
}

fn save_teardown_files(
    project_dir: &Path,
    doc: &mut Value,
    update_latest: bool,
) -> Result<(), Oc> {
    let uid = doc["universe_id"]
        .as_u64()
        .filter(|&u| u > 0)
        .ok_or_else(|| bad("teardown missing valid universe_id"))?;
    let (json_rel, md_rel) = teardown_paths(uid);
    doc["teardown_json_path"] = json!(json_rel);
    doc["teardown_md_path"] = json!(md_rel);

    let json_abs = safe_rel(project_dir, &json_rel)?;
    let md_abs = safe_rel(project_dir, &md_rel)?;
    let md = render_teardown_markdown(doc);

    write_atomic_json(&json_abs, doc)?;
    write_atomic_bytes(&md_abs, md.as_bytes())?;

    if update_latest {
        let latest_abs = safe_rel(project_dir, "refs/teardowns/latest.json")?;
        let latest = json!({
            "universe_id": uid,
            "name": doc.pointer("/game/name").and_then(Value::as_str).unwrap_or(""),
            "teardown_path": json_rel,
            "markdown_path": md_rel,
            "updated_at_ms": now_ms(),
        });
        write_atomic_json(&latest_abs, &latest)?;
    }
    Ok(())
}

/// Run a full live game teardown into `<project_dir>/refs/teardowns/<universe_id>/`.
pub fn teardown_roblox_game(project_dir: &Path, body: &Value) -> Result<Value, Oc> {
    let target = body
        .get("target")
        .or_else(|| body.get("url"))
        .or_else(|| body.get("place_id"))
        .or_else(|| body.get("universe_id"))
        .and_then(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .ok_or_else(|| bad("teardown requires body.target (Roblox URL, PlaceId, or UniverseId)"))?;

    let new_niche = body
        .get("new_niche")
        .or_else(|| body.get("niche_hint"))
        .or_else(|| body.get("niche"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let include_youtube = body
        .get("include_youtube")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_wiki = body
        .get("include_wiki")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let capture_frames = body
        .get("capture_frames")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_videos = body
        .get("max_videos")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 8) as usize;
    let youtube_queries: Vec<String> = body
        .get("youtube_queries")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let wiki_urls: Vec<String> = body
        .get("wiki_urls")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let api_base = resolve_api_base(body)?;
    let game = research_roblox_game_to_project_with_base(
        project_dir,
        &target,
        new_niche,
        api_base.as_deref(),
        Duration::from_secs(15),
    )
    .map_err(|msg| {
        if msg.contains("invalid") || msg.contains("requires") || msg.contains("parse") {
            bad(msg)
        } else {
            err(502, msg)
        }
    })?;

    let uid = game
        .get("universe_id")
        .and_then(Value::as_u64)
        .filter(|&u| u > 0)
        .ok_or_else(|| err(502, "Roblox game research did not return a universe_id"))?;
    let root_place_id = game
        .get("root_place_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let game_name = game
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Roblox Game");

    let shots_dir = safe_rel(project_dir, &format!("refs/teardowns/{uid}/screenshots"))?;
    std::fs::create_dir_all(&shots_dir)
        .map_err(|e| err(500, format!("create teardown screenshots dir: {e}")))?;

    let (badges, badges_warn) = fetch_roblox_badges(uid, api_base.as_deref());
    let (game_passes, passes_warn) = fetch_roblox_game_passes(uid, api_base.as_deref());
    let (media, media_screens, media_warn) =
        fetch_roblox_media_and_download(project_dir, uid, api_base.as_deref());

    let mut screenshots: Vec<Value> = Vec::new();
    if let Some(thumbs) = game.get("local_thumbnails").and_then(Value::as_array) {
        for (idx, t) in thumbs.iter().enumerate() {
            if let Some(rel) = t.as_str() {
                screenshots.push(json!({
                    "path": rel,
                    "role": "storefront_thumbnail",
                    "source": "roblox_thumbnails",
                    "timestamp_s": Value::Null,
                    "notes": format!("Storefront thumbnail #{}", idx + 1),
                    "copy_observed": [],
                }));
            }
        }
    }
    screenshots.extend(media_screens);

    let youtube_research = if include_youtube {
        let (yt, yt_screens) = run_youtube_research(
            project_dir,
            uid,
            game_name,
            &youtube_queries,
            max_videos,
            capture_frames,
        );
        screenshots.extend(yt_screens);
        yt
    } else {
        json!({ "ok": true, "skipped": true, "video_count": 0, "videos": [] })
    };

    let wiki_research = if include_wiki {
        run_wiki_research(game_name, &wiki_urls)
    } else {
        json!({ "ok": true, "skipped": true, "page_count": 0, "pages": [] })
    };

    let recommended_ui_reference_screenshots =
        recompute_recommended_ui_refs(project_dir, &screenshots);
    let e2e_spec = build_e2e_spec(
        &game,
        &badges,
        &game_passes,
        &youtube_research,
        &wiki_research,
        new_niche,
    );

    let mut warnings: Vec<String> = Vec::new();
    for w in [badges_warn, passes_warn, media_warn].into_iter().flatten() {
        warnings.push(w);
    }

    let mut doc = json!({
        "ok": true,
        "has_teardown": true,
        "schema": "openro.teardown.v1",
        "universe_id": uid,
        "root_place_id": root_place_id,
        "target": target,
        "new_niche": new_niche,
        "researched_at": now_iso(),
        "researched_at_ms": now_ms(),
        "game": game,
        "badges": badges,
        "game_passes": game_passes,
        "media": media,
        "youtube_research": youtube_research,
        "wiki_research": wiki_research,
        "screenshots": screenshots,
        "recommended_ui_reference_screenshots": recommended_ui_reference_screenshots,
        "e2e_spec": e2e_spec,
        "warnings": warnings,
    });

    save_teardown_files(project_dir, &mut doc, true)?;

    let (json_rel, md_rel) = teardown_paths(uid);
    if let Ok(ev) = record_evidence(
        project_dir,
        &json!({
            "kind": "live_game_teardown",
            "source": "teardown_roblox_game",
            "summary": format!(
                "Teardown of '{}' (universe {}): {} badges, {} game passes, {} media items, {} YouTube videos, {} wiki pages, {} screenshots",
                game_name,
                uid,
                doc["badges"].as_array().map(|a| a.len()).unwrap_or(0),
                doc["game_passes"].as_array().map(|a| a.len()).unwrap_or(0),
                doc["media"].as_array().map(|a| a.len()).unwrap_or(0),
                doc.pointer("/youtube_research/video_count").and_then(Value::as_u64).unwrap_or(0),
                doc.pointer("/wiki_research/page_count").and_then(Value::as_u64).unwrap_or(0),
                doc["screenshots"].as_array().map(|a| a.len()).unwrap_or(0),
            ),
            "confidence": "observed",
            "authority": "external_surface",
            "coverage": "roblox_api+badges+gamepasses+media+youtube+wiki",
            "refs": [json_rel, md_rel],
            "data": {
                "universe_id": uid,
                "root_place_id": root_place_id,
                "name": game_name,
                "badge_count": doc["badges"].as_array().map(|a| a.len()).unwrap_or(0),
                "game_pass_count": doc["game_passes"].as_array().map(|a| a.len()).unwrap_or(0),
                "recommended_ui_reference_screenshots": doc["recommended_ui_reference_screenshots"],
            }
        }),
    ) {
        if let Some(eid) = ev.pointer("/evidence/id").and_then(Value::as_str) {
            doc["evidence_id"] = json!(eid);
            let _ = save_teardown_files(project_dir, &mut doc, false);
        }
    }

    Ok(doc)
}

fn resolve_teardown_uid(project_dir: &Path, universe_id: Option<u64>) -> Result<u64, Oc> {
    if let Some(uid) = universe_id.filter(|&u| u > 0) {
        return Ok(uid);
    }
    let latest_abs = safe_rel(project_dir, "refs/teardowns/latest.json")?;
    let raw = std::fs::read_to_string(&latest_abs)
        .map_err(|_| err(404, "no teardown found for this project yet"))?;
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| err(500, format!("corrupt refs/teardowns/latest.json: {e}")))?;
    v.get("universe_id")
        .and_then(Value::as_u64)
        .filter(|&u| u > 0)
        .ok_or_else(|| err(404, "no valid universe_id in refs/teardowns/latest.json"))
}

/// Read an existing teardown (`refs/teardowns/<uid>/teardown.json` or `latest.json`).
pub fn get_teardown(project_dir: &Path, universe_id: Option<u64>) -> Result<Value, Oc> {
    let uid = resolve_teardown_uid(project_dir, universe_id)?;
    let (json_rel, _) = teardown_paths(uid);
    let abs = safe_rel(project_dir, &json_rel)?;
    let raw = std::fs::read_to_string(&abs)
        .map_err(|_| err(404, format!("teardown {uid} not found")))?;
    let mut val = serde_json::from_str::<Value>(&raw)
        .map_err(|e| err(500, format!("corrupt teardown {uid}: {e}")))?;
    if let Some(obj) = val.as_object_mut() {
        obj.entry("has_teardown".to_string()).or_insert_with(|| json!(true));
    }
    Ok(val)
}

/// Safely read a teardown screenshot/thumbnail image under `<project_dir>/refs/...`
/// for `GET /api/projects/:id/teardown/screenshot?path=<rel>`, enforcing CWE-22/CWE-59
/// and verifying real PNG/JPEG/WEBP magic bytes.
pub fn read_teardown_screenshot_image(
    project_dir: &Path,
    raw_rel: &str,
) -> Result<(Vec<u8>, &'static str), Oc> {
    let rel = raw_rel.trim();
    let lower = rel.to_ascii_lowercase();
    if rel.is_empty()
        || rel.contains('\0')
        || rel.contains('\\')
        || rel.contains("..")
        || lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
        || lower.contains("%00")
        || lower.contains("%25")
    {
        return Err(bad("invalid teardown screenshot path"));
    }
    if !rel.starts_with("refs/") {
        return Err(bad("teardown screenshot path must stay under refs/"));
    }
    let abs = safe_rel(project_dir, rel)?;
    let meta = std::fs::symlink_metadata(&abs)
        .map_err(|_| err(404, format!("teardown screenshot '{rel}' not found")))?;
    if meta.file_type().is_symlink() {
        return Err(bad(format!("teardown screenshot '{rel}' is a symlink (CWE-59)")));
    }
    if !meta.is_file() {
        return Err(bad(format!("teardown screenshot '{rel}' is not a regular file")));
    }
    if meta.len() > MAX_SCREENSHOT_BYTES as u64 {
        return Err(bad("teardown screenshot exceeds 20 MiB limit"));
    }
    let bytes = std::fs::read(&abs)
        .map_err(|e| err(500, format!("read teardown screenshot '{rel}': {e}")))?;
    let mime = match detect_image_ext(&bytes) {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err(bad(format!("teardown screenshot '{rel}' is not a valid PNG/JPEG/WEBP image"))),
    };
    Ok((bytes, mime))
}

fn sanitize_png_filename(raw: &str) -> String {
    let stem = raw
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .strip_suffix(".png")
        .or_else(|| raw.rsplit('/').next().unwrap_or(raw).strip_suffix(".PNG"))
        .unwrap_or_else(|| raw.rsplit('/').next().unwrap_or(raw));
    let mut clean = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            clean.push(ch.to_ascii_lowercase());
        } else if !clean.ends_with('_') && !clean.is_empty() {
            clean.push('_');
        }
        if clean.len() >= 48 {
            break;
        }
    }
    let trimmed = clean.trim_matches('_');
    if trimmed.is_empty() {
        "screenshot.png".to_string()
    } else {
        format!("{trimmed}.png")
    }
}

/// Record a user-supplied or CUA-captured PNG screenshot into `refs/teardowns/<uid>/screenshots/`,
/// tag its UI role / timestamp / notes / observed copy, update `recommended_ui_reference_screenshots`,
/// and regenerate `teardown.json` + `TEARDOWN.md`.
pub fn record_teardown_screenshot(project_dir: &Path, body: &Value) -> Result<Value, Oc> {
    let uid_opt = body
        .get("universe_id")
        .or_else(|| body.get("teardown_id"))
        .and_then(Value::as_u64);
    let mut doc = get_teardown(project_dir, uid_opt)?;
    let uid = doc["universe_id"]
        .as_u64()
        .ok_or_else(|| bad("teardown missing universe_id"))?;

    let src_rel = body
        .get("path")
        .or_else(|| body.get("source_path"))
        .or_else(|| body.get("image_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad("teardown screenshot requires body.path (project-relative PNG)"))?;

    let src_abs = safe_rel(project_dir, src_rel)?;
    let meta = std::fs::symlink_metadata(&src_abs)
        .map_err(|_| err(404, format!("screenshot '{src_rel}' not found in project")))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(bad(format!("screenshot '{src_rel}' must be a regular file")));
    }
    if meta.len() > MAX_SCREENSHOT_BYTES as u64 {
        return Err(bad("screenshot exceeds 20 MiB limit"));
    }
    let bytes = std::fs::read(&src_abs)
        .map_err(|e| err(500, format!("read screenshot '{src_rel}': {e}")))?;
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(bad(format!("screenshot '{src_rel}' is not a valid PNG file")));
    }

    let role = body
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("hud");
    let timestamp_s = body.get("timestamp_s").and_then(Value::as_f64);
    let notes = body
        .get("notes")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let copy_observed: Value = match body.get("copy_observed") {
        Some(Value::Array(arr)) => Value::Array(arr.clone()),
        Some(Value::String(s)) if !s.trim().is_empty() => json!([s.trim()]),
        _ => json!([]),
    };

    let target_prefix = format!("refs/teardowns/{uid}/screenshots/");
    let dst_rel = if src_rel.starts_with(&target_prefix) {
        src_rel.to_string()
    } else {
        let base_name = sanitize_png_filename(src_rel);
        let hash8 = &sha256_hex(&bytes)[..8];
        let stem = base_name.strip_suffix(".png").unwrap_or(&base_name);
        format!("{target_prefix}{stem}_{hash8}.png")
    };
    let dst_abs = safe_rel(project_dir, &dst_rel)?;
    if dst_abs != src_abs {
        write_atomic_bytes(&dst_abs, &bytes)?;
    }

    let entry = json!({
        "path": dst_rel,
        "role": role,
        "source": body.get("source").and_then(Value::as_str).unwrap_or("recorded"),
        "timestamp_s": timestamp_s,
        "notes": notes,
        "copy_observed": copy_observed,
    });

    let screens = doc
        .get_mut("screenshots")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| err(500, "teardown screenshots field is not an array"))?;
    if let Some(existing) = screens
        .iter_mut()
        .find(|s| s.get("path").and_then(Value::as_str) == Some(&dst_rel))
    {
        *existing = entry.clone();
    } else {
        screens.push(entry.clone());
    }

    let rec = recompute_recommended_ui_refs(project_dir, screens);
    doc["recommended_ui_reference_screenshots"] = json!(rec);
    save_teardown_files(project_dir, &mut doc, true)?;

    Ok(json!({
        "ok": true,
        "universe_id": uid,
        "screenshot": entry,
        "recommended_ui_reference_screenshots": doc["recommended_ui_reference_screenshots"],
        "teardown_json_path": doc["teardown_json_path"],
        "teardown_md_path": doc["teardown_md_path"],
    }))
}

fn merge_pillar_value(dst: &mut Value, patch: &Value) {
    match (dst, patch) {
        (Value::Object(d_map), Value::Object(p_map)) => {
            for (k, pv) in p_map {
                if let Some(dv) = d_map.get_mut(k) {
                    merge_pillar_value(dv, pv);
                } else {
                    d_map.insert(k.clone(), pv.clone());
                }
            }
        }
        (Value::Array(d_arr), Value::Array(p_arr)) => {
            for item in p_arr {
                if !d_arr.contains(item) {
                    d_arr.push(item.clone());
                }
            }
        }
        (d, p) => {
            *d = p.clone();
        }
    }
}

/// Merge new findings into the 7 `e2e_spec` pillars and regenerate `TEARDOWN.md`.
pub fn merge_teardown_spec(project_dir: &Path, body: &Value) -> Result<Value, Oc> {
    let uid_opt = body
        .get("universe_id")
        .or_else(|| body.get("teardown_id"))
        .and_then(Value::as_u64);
    let mut doc = get_teardown(project_dir, uid_opt)?;
    let uid = doc["universe_id"]
        .as_u64()
        .ok_or_else(|| bad("teardown missing universe_id"))?;

    let mut updates: Map<String, Value> = Map::new();
    if let Some(spec_obj) = body.get("e2e_spec").and_then(Value::as_object) {
        for (k, v) in spec_obj {
            updates.insert(k.clone(), v.clone());
        }
    }
    if let Some(pillar) = body.get("pillar").and_then(Value::as_str).map(str::trim) {
        if !pillar.is_empty() {
            let patch = body
                .get("patch")
                .or_else(|| body.get("data"))
                .cloned()
                .unwrap_or_else(|| {
                    let mut m = Map::new();
                    if let Some(s) = body.get("summary").and_then(Value::as_str) {
                        m.insert("summary".into(), json!(s));
                    }
                    if let Some(n) = body.get("notes") {
                        m.insert("notes".into(), n.clone());
                    }
                    Value::Object(m)
                });
            updates.insert(pillar.to_string(), patch);
        }
    }
    for pillar in E2E_SPEC_PILLARS {
        if let Some(v) = body.get(pillar) {
            updates.insert(pillar.to_string(), v.clone());
        }
    }

    if updates.is_empty() {
        return Err(bad(
            "teardown spec merge requires e2e_spec object, pillar + patch, or one of the 7 pillar keys",
        ));
    }

    for k in updates.keys() {
        if !E2E_SPEC_PILLARS.contains(&k.as_str()) {
            return Err(bad(format!(
                "unknown e2e_spec pillar '{k}' (expected one of: {})",
                E2E_SPEC_PILLARS.join(", ")
            )));
        }
    }

    let spec = doc
        .get_mut("e2e_spec")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| err(500, "teardown e2e_spec is not an object"))?;
    for (k, v) in updates {
        if let Some(existing) = spec.get_mut(&k) {
            merge_pillar_value(existing, &v);
        } else {
            spec.insert(k, v);
        }
    }

    save_teardown_files(project_dir, &mut doc, true)?;

    Ok(json!({
        "ok": true,
        "universe_id": uid,
        "e2e_spec": doc["e2e_spec"],
        "teardown_json_path": doc["teardown_json_path"],
        "teardown_md_path": doc["teardown_md_path"],
    }))
}

/// Return up to 3 existing valid PNG paths from `recommended_ui_reference_screenshots`.
pub fn default_teardown_ui_references(project_dir: &Path, teardown_id: Option<u64>) -> Vec<String> {
    let Ok(doc) = get_teardown(project_dir, teardown_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(arr) = doc
        .get("recommended_ui_reference_screenshots")
        .and_then(Value::as_array)
    {
        for item in arr {
            if out.len() >= 3 {
                break;
            }
            if let Some(rel) = item.as_str() {
                if seen.insert(rel.to_string()) && is_valid_project_png(project_dir, rel) {
                    out.push(rel.to_string());
                }
            }
        }
    }
    out
}

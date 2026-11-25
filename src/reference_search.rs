//! Reference-image search via the owner's shared Camoufox browser (`sharedbrowser` on :9230).
//!
//! Searches `<character> <show>` with dynamic/action qualifiers, collects candidates and
//! source URLs, filters and ranks out static standing poses / official character sheets /
//! transparent cutouts, saves candidates and the active choice under `<project>/art/references/`
//! (gitignored, never committed or uploaded), and exposes candidates to the UI for override.
//!
//! Lease owner is `"openro"`. Never force-releases another process's lease and never starts or
//! restarts `shared-browser.service`. Tests override via `OPENRO_REFERENCE_SEARCH_BIN`.

use crate::assets::{detect_image_mime, sanitize_art_slug};
use crate::opencloud::{now_iso, safe_rel, sha256_hex, Oc};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_QUALIFIERS: &str = "anime action battle scene dynamic pose";
const DEFAULT_PROP_QUALIFIERS: &str = "weapon anime wiki prop reference";
const MAX_CANDIDATES: usize = 12;
const MAX_CANDIDATE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    Character,
    Prop,
}

impl ReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Character => "character",
            ReferenceKind::Prop => "prop",
        }
    }
}

pub fn parse_reference_kind(raw: Option<&str>) -> ReferenceKind {
    match raw
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("weapon" | "prop" | "weapon_sheet" | "prop_sheet" | "item") => ReferenceKind::Prop,
        _ => ReferenceKind::Character,
    }
}

type R<T> = Result<T, Oc>;
fn bad<T>(msg: impl Into<String>) -> R<T> {
    Err(Oc { status: 400, msg: msg.into() })
}
fn err<T>(status: u16, msg: impl Into<String>) -> R<T> {
    Err(Oc { status, msg: msg.into() })
}

#[derive(Clone, Debug)]
pub struct RawCandidate {
    pub image_url: String,
    pub source_url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub local_file: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ScoredCandidate {
    pub raw: RawCandidate,
    pub score: i32,
    pub reasons: Vec<String>,
    pub rejected_reason: Option<String>,
}

const REJECT_TERMS: &[(&str, &str)] = &[
    ("character sheet", "official character sheet"),
    ("model sheet", "model sheet"),
    ("reference sheet", "reference sheet"),
    ("turnaround", "character turnaround"),
    ("t-pose", "static T-pose"),
    ("a-pose", "static A-pose"),
    ("transparent png", "cutout render"),
    ("png render", "cutout render"),
    ("full body render", "static full-body render"),
    ("standing pose", "static standing pose"),
    ("character render", "static character render"),
    ("funko", "merchandise/figure"),
    ("nendoroid", "merchandise/figure"),
    ("cosplay", "cosplay photo"),
    ("action figure", "merchandise/figure"),
    ("coloring page", "line-art coloring page"),
    ("chibi", "chibi style"),
];

const PENALIZE_TERMS: &[(&str, i32, &str)] = &[
    ("standing", -35, "standing pose keyword"),
    ("portrait", -15, "static portrait"),
    ("render", -25, "possible isolated render"),
    ("wikia", -20, "wiki infobox image"),
    ("fandom.com", -15, "wiki infobox image"),
    ("static.wikia", -20, "wiki infobox image"),
    ("pngitem", -35, "cutout site"),
    ("pngwing", -35, "cutout site"),
    ("kindpng", -35, "cutout site"),
    ("cleanpng", -35, "cutout site"),
    ("clipart", -40, "clipart"),
    ("vector", -20, "flat vector"),
    ("sticker", -30, "sticker"),
    ("plush", -40, "merchandise"),
];

const REWARD_TERMS: &[(&str, i32, &str)] = &[
    ("action", 22, "action cue"),
    ("battle", 24, "battle scene"),
    ("fight", 22, "fight scene"),
    ("attack", 20, "attack pose"),
    ("sakuga", 28, "sakuga frame"),
    ("scene", 16, "anime scene"),
    ("dynamic", 18, "dynamic composition"),
    ("clash", 18, "clash frame"),
    ("aura", 14, "energy aura"),
    ("epic", 10, "dramatic framing"),
    ("anime", 8, "anime source"),
    ("screenshot", 16, "episode screenshot"),
    ("frame", 12, "animation frame"),
];

const PROP_REJECT_TERMS: &[(&str, &str)] = &[
    ("shutterstock", "watermarked stock image"),
    ("gettyimages", "watermarked stock image"),
    ("istockphoto", "watermarked stock image"),
    ("dreamstime", "watermarked stock image"),
    ("alamy", "watermarked stock image"),
    ("123rf", "watermarked stock image"),
    ("depositphotos", "watermarked stock image"),
    ("watermark", "watermarked image"),
    ("coloring page", "line-art coloring page"),
    ("chibi", "chibi style"),
    ("funko", "merchandise/figure"),
    ("nendoroid", "merchandise/figure"),
    ("plush", "merchandise/figure"),
    ("t-shirt", "apparel merchandise"),
    ("hoodie", "apparel merchandise"),
    ("desktop wallpaper", "cluttered wallpaper"),
    ("logo", "logo/branding graphic"),
    ("meme", "meme image"),
];

const PROP_PENALIZE_TERMS: &[(&str, i32, &str)] = &[
    ("battle scene", -32, "cluttered battle scene"),
    ("fight scene", -32, "cluttered fight scene"),
    ("action scene", -28, "cluttered action scene"),
    ("episode screenshot", -26, "episode screenshot"),
    ("screenshot", -22, "cluttered screenshot"),
    ("sakuga", -24, "motion-blurred action frame"),
    ("clash", -22, "cluttered clash frame"),
    ("wallpaper", -24, "cluttered wallpaper"),
    ("character portrait", -24, "character portrait"),
    ("group", -20, "multi-character group image"),
    ("thumbnail", -18, "low-detail thumbnail"),
];

const PROP_REWARD_TERMS: &[(&str, i32, &str)] = &[
    ("reference sheet", 28, "prop reference sheet"),
    ("model sheet", 24, "prop model sheet"),
    ("turnaround", 22, "prop turnaround"),
    ("weapon", 24, "weapon subject"),
    ("prop", 24, "prop reference"),
    ("isolated", 22, "isolated prop framing"),
    ("wikia", 22, "wiki weapon render"),
    ("static.wikia", 22, "wiki weapon render"),
    ("fandom.com", 18, "anime wiki source"),
    ("wiki", 18, "wiki reference"),
    ("render", 20, "isolated render"),
    ("transparent png", 18, "isolated cutout"),
    ("png render", 18, "isolated cutout"),
    ("concept art", 20, "prop concept art"),
    ("replica", 20, "prop replica"),
    ("cosplay", 16, "cosplay prop reference"),
    ("3d model", 18, "3D prop model"),
    ("sketchfab", 16, "3D prop viewer"),
    ("katana", 12, "blade detail"),
    ("sword", 12, "blade detail"),
    ("blade", 12, "blade detail"),
    ("spear", 12, "polearm detail"),
    ("staff", 12, "staff detail"),
    ("saber", 12, "blade detail"),
    ("scythe", 12, "weapon detail"),
    ("hilt", 14, "hilt detail"),
    ("scabbard", 14, "scabbard detail"),
    ("sheath", 14, "sheath detail"),
];

/// Score a candidate reference image for the requested `ReferenceKind`:
/// - `Character`: penalizes static standing poses / official sheets and rewards dynamic action scenes.
/// - `Prop`: rewards isolated weapon/prop artwork, wiki weapon renders, and prop/cosplay sheets
///   over cluttered action scenes, while still rejecting watermarks, logos, and low-res thumbnails (#76).
pub fn score_candidate_for_kind(c: &RawCandidate, kind: ReferenceKind) -> ScoredCandidate {
    let hay = format!(
        "{} {} {}",
        c.title.to_ascii_lowercase(),
        c.image_url.to_ascii_lowercase(),
        c.source_url.to_ascii_lowercase()
    );
    let mut score: i32 = 50;
    let mut reasons = Vec::new();
    let mut rejected_reason = None;

    let (rejects, penalizes, rewards) = match kind {
        ReferenceKind::Character => (REJECT_TERMS, PENALIZE_TERMS, REWARD_TERMS),
        ReferenceKind::Prop => (PROP_REJECT_TERMS, PROP_PENALIZE_TERMS, PROP_REWARD_TERMS),
    };

    for (term, label) in rejects {
        if hay.contains(term) {
            score -= 80;
            rejected_reason = Some((*label).to_string());
            reasons.push(format!("filtered: {label}"));
            break;
        }
    }
    for (term, delta, label) in penalizes {
        if hay.contains(term) {
            score += delta;
            reasons.push(format!("{delta}: {label}"));
        }
    }
    for (term, delta, label) in rewards {
        if hay.contains(term) {
            score += delta;
            reasons.push(format!("+{delta}: {label}"));
        }
    }

    if c.width > 0 && c.height > 0 {
        let min_dim = c.width.min(c.height);
        let max_dim = c.width.max(c.height);
        let min_threshold = match kind {
            ReferenceKind::Character => 260,
            ReferenceKind::Prop => 220,
        };
        if min_dim < min_threshold {
            score -= 60;
            if rejected_reason.is_none() {
                rejected_reason = Some(format!("too small ({}x{})", c.width, c.height));
            }
            reasons.push("too small".into());
        } else if max_dim >= 800 {
            score += 18;
            reasons.push("high-res".into());
        } else if max_dim >= 480 {
            score += 8;
            reasons.push("usable resolution".into());
        }
        let aspect = c.width as f64 / c.height as f64;
        match kind {
            ReferenceKind::Character => {
                if aspect < 0.55 {
                    score -= 40;
                    reasons.push("narrow standing cutout aspect".into());
                } else if (0.65..=2.0).contains(&aspect) {
                    score += 12;
                    reasons.push("balanced action framing".into());
                }
            }
            ReferenceKind::Prop => {
                // Weapons & props naturally have elongated horizontal/vertical silhouettes or square sheets.
                if (0.25..=4.5).contains(&aspect) {
                    score += 10;
                    reasons.push("clean prop silhouette aspect".into());
                }
            }
        }
    }

    ScoredCandidate { raw: c.clone(), score, reasons, rejected_reason }
}

/// Score a character candidate reference image (default `ReferenceKind::Character`).
pub fn score_candidate(c: &RawCandidate) -> ScoredCandidate {
    score_candidate_for_kind(c, ReferenceKind::Character)
}

pub fn rank_candidates_for_kind(raw: Vec<RawCandidate>, kind: ReferenceKind) -> Vec<ScoredCandidate> {
    let mut scored: Vec<ScoredCandidate> = raw
        .iter()
        .map(|c| score_candidate_for_kind(c, kind))
        .collect();
    // Non-rejected candidates always sort ahead of rejected ones, then highest score first.
    scored.sort_by(|a, b| {
        a.rejected_reason
            .is_some()
            .cmp(&b.rejected_reason.is_some())
            .then_with(|| b.score.cmp(&a.score))
    });
    scored
}

pub fn rank_candidates(raw: Vec<RawCandidate>) -> Vec<ScoredCandidate> {
    rank_candidates_for_kind(raw, ReferenceKind::Character)
}

/// Embedded Python driver using `sharedbrowser` (:9230) with lease owner `"openro"`.
/// Never restarts the service, never force-releases foreign leases, stops on CAPTCHA.
const PY_REFERENCE_SEARCH: &str = r#"
import json, os, sys, time, urllib.parse, urllib.request
from pathlib import Path

def fail(code, kind, msg):
    print(json.dumps({"ok": False, "error_kind": kind, "error": msg}))
    sys.exit(code)

query = sys.argv[1]
max_n = int(sys.argv[2])
cand_dir = Path(sys.argv[3])
cand_dir.mkdir(parents=True, exist_ok=True)

try:
    import sharedbrowser as sb
except Exception as e:
    fail(8, "DRIVER", f"sharedbrowser client unavailable: {e}")

info = None
for _attempt in range(5):
    info = sb.health(timeout=1.5)
    if info and info.get("ok") and info.get("context_alive", True):
        break
    time.sleep(0.5)
if not info or not info.get("ok") or not info.get("context_alive", True):
    fail(8, "DRIVER", "Shared Camoufox browser is not running or still starting on port 9230.")

tab = None
raw_candidates = []
try:
    with sb.connect(owner="openro", ensure=False, timeout=30.0, ttl=120.0, retries=1) as b:
        search_urls = [
            "https://www.bing.com/images/search?q=" + urllib.parse.quote_plus(query) + "&form=HDRSC3&first=1",
            "https://www.google.com/search?tbm=isch&udm=2&q=" + urllib.parse.quote_plus(query),
        ]
        check_js = r"""(() => {
            const href = (location.href || "").toLowerCase();
            const txt = (document.body && document.body.innerText || "").toLowerCase();
            if (href.includes("/sorry/") || txt.includes("verify you are human") || txt.includes("captcha") || txt.includes("unusual traffic")) {
                return { blocked: true };
            }
            const out = [];
            const seen = new Set();
            for (const a of document.querySelectorAll("a.iusc[m]")) {
                try {
                    const m = JSON.parse(a.getAttribute("m") || "{}");
                    const murl = m.murl || "";
                    if (!murl || !murl.startsWith("http") || seen.has(murl)) continue;
                    seen.add(murl);
                    const img = a.querySelector("img");
                    out.push({
                        image_url: murl,
                        source_url: m.purl || "",
                        title: m.t || (img && img.alt) || "",
                        width: Number(m.w || (img && img.naturalWidth) || 0),
                        height: Number(m.h || (img && img.naturalHeight) || 0),
                    });
                } catch (_) {}
                if (out.length >= 24) break;
            }
            if (out.length === 0) {
                for (const img of document.querySelectorAll("img")) {
                    const src = img.src || img.getAttribute("data-src") || "";
                    if (!src.startsWith("http") || src.includes("gstatic.com/images") || seen.has(src)) continue;
                    const w = img.naturalWidth || 0, h = img.naturalHeight || 0;
                    if (w < 180 || h < 180) continue;
                    seen.add(src);
                    out.push({ image_url: src, source_url: location.href, title: img.alt || "", width: w, height: h });
                    if (out.length >= 24) break;
                }
            }
            return { blocked: false, candidates: out };
        })()"""
        for url in search_urls:
            opened = b.open(url, new_tab=True, wait_until="domcontentloaded", timeout_ms=30000)
            tab = opened.get("tab")
            time.sleep(2.5)
            res = b.eval(check_js, tab=tab, timeout_ms=20000)
            payload = res.get("result") if isinstance(res, dict) else {}
            if tab:
                try:
                    b.close_tab(tab)
                except Exception:
                    pass
                tab = None
            if isinstance(payload, dict) and payload.get("blocked"):
                fail(7, "CAPTCHA", "Image search showed an anti-bot challenge; stopping without bypassing it.")
            raw_candidates = (payload.get("candidates") or []) if isinstance(payload, dict) else []
            if raw_candidates:
                break
except Exception as e:
    name = type(e).__name__
    if "Lease" in name or "423" in str(e):
        fail(8, "LEASE_BUSY", f"Shared browser lease is currently held by another run: {e}")
    fail(8, "DRIVER", f"Shared browser search failed: {e}")

print(json.dumps({"ok": True, "candidates": raw_candidates[:max(max_n * 3, 18)]}))
"#;

fn run_command_with_timeout(mut cmd: Command, timeout: Duration) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("failed to launch reference search helper: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = child
                    .wait_with_output()
                    .map_err(|e| format!("read helper output: {e}"))?;
                return Ok((out.status.code().unwrap_or(-1), out.stdout, out.stderr));
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("reference search timed out waiting on the shared browser".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("wait helper: {e}"));
            }
        }
    }
}

fn url_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn unescape_html_attr(raw: &str) -> String {
    raw.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Parse Bing image card `m="{...}"` JSON payloads from HTML (or JSON `{"candidates":[...]}`).
pub fn parse_http_image_candidates(body: &str, max_items: usize) -> Result<Vec<RawCandidate>, String> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("verify you are human")
        || lower.contains("unusual traffic")
        || lower.contains("g-recaptcha")
        || lower.contains("cf-turnstile")
        || lower.contains("hcaptcha")
    {
        return Err("Image search returned an anti-bot challenge; stopping without bypassing it.".into());
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(arr) = v.get("candidates").and_then(Value::as_array) {
            for item in arr {
                let image_url = item.get("image_url").and_then(Value::as_str).unwrap_or("").trim().to_string();
                let local_file = item.get("local_file").and_then(Value::as_str).map(String::from);
                if (image_url.is_empty() && local_file.is_none()) || (!image_url.is_empty() && !seen.insert(image_url.clone())) {
                    continue;
                }
                out.push(RawCandidate {
                    image_url,
                    source_url: item.get("source_url").and_then(Value::as_str).unwrap_or("").to_string(),
                    title: item.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                    width: item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                    height: item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    local_file,
                });
                if out.len() >= max_items {
                    break;
                }
            }
            return Ok(out);
        }
    }

    // Scan Bing `m="{&quot;murl&quot;:...}"` attributes.
    let mut cursor = body;
    while let Some(pos) = cursor.find("m=\"{") {
        let rest = &cursor[pos + 3..];
        let Some(end) = rest.find('"') else { break };
        let attr = unescape_html_attr(&rest[..end]);
        cursor = &rest[end + 1..];
        let Ok(v) = serde_json::from_str::<Value>(&attr) else { continue };
        let murl = v.get("murl").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if !(murl.starts_with("http://") || murl.starts_with("https://")) || !seen.insert(murl.clone()) {
            continue;
        }
        out.push(RawCandidate {
            image_url: murl,
            source_url: v.get("purl").and_then(Value::as_str).unwrap_or("").to_string(),
            title: v
                .get("t")
                .or_else(|| v.get("desc"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            width: v.get("w").and_then(Value::as_u64).unwrap_or(0) as u32,
            height: v.get("h").and_then(Value::as_u64).unwrap_or(0) as u32,
            local_file: None,
        });
        if out.len() >= max_items {
            break;
        }
    }
    // Also scan Google Images inline `["https://...",h,w]` image tuples if Bing cards were absent.
    if out.is_empty() {
        let mut rest = body;
        while let Some(pos) = rest.find("[\"https://") {
            let after = &rest[pos + 2..];
            let Some(end_q) = after.find('"') else { break };
            let raw_u = &after[..end_q];
            rest = &after[end_q + 1..];
            let decoded = raw_u.replace("\\u003d", "=").replace("\\u0026", "&");
            let low = decoded.to_ascii_lowercase();
            if low.contains("gstatic.com")
                || low.contains("google.com")
                || low.contains("googleusercontent.com")
                || !(low.contains(".png")
                    || low.contains(".jpg")
                    || low.contains(".jpeg")
                    || low.contains(".webp"))
                || !seen.insert(decoded.clone())
            {
                continue;
            }
            let (mut h_val, mut w_val) = (0u32, 0u32);
            if let Some(nums) = rest.strip_prefix(',') {
                let mut parts = nums.split([',', ']']);
                if let (Some(a), Some(b)) = (parts.next(), parts.next()) {
                    if let (Ok(h), Ok(w)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                        h_val = h;
                        w_val = w;
                    }
                }
            }
            out.push(RawCandidate {
                image_url: decoded,
                source_url: String::new(),
                title: String::new(),
                width: w_val,
                height: h_val,
                local_file: None,
            });
            if out.len() >= max_items {
                break;
            }
        }
    }
    Ok(out)
}

fn fetch_one_http_search(url: &str, max_items: usize) -> R<Vec<RawCandidate>> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(12)).build();
    let resp = agent
        .get(url)
        .set("User-Agent", "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0")
        .set("Accept", "text/html,application/xhtml+xml,application/json")
        .set("Accept-Language", "en-US,en;q=0.9")
        .call()
        .map_err(|e| Oc { status: 502, msg: format!("Direct image search request failed: {e}") })?;
    let mut buf = Vec::new();
    let _ = std::io::Read::take(resp.into_reader(), 2 * 1024 * 1024).read_to_end(&mut buf);
    let body = String::from_utf8_lossy(&buf);
    parse_http_image_candidates(&body, max_items).map_err(|m| Oc { status: 502, msg: m })
}

/// Read-only direct HTTP image-search fallback when shared Camoufox `:9230` is unavailable.
/// Tries Bing Images first and falls back cleanly to Google Images only when Bing returns 0
/// items (never bypassing CAPTCHAs).
fn fetch_candidates_via_http(query: &str, max_n: usize) -> R<Vec<RawCandidate>> {
    let custom_base = std::env::var("OPENRO_REFERENCE_SEARCH_HTTP_BASE")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());
    let max_items = (max_n * 3).max(18);
    let enc = url_encode_query(query);
    let bing_base = custom_base.as_deref().unwrap_or("https://www.bing.com");
    let bing_url = format!("{bing_base}/images/search?q={enc}&form=HDRSC3&first=1");
    let bing_res = fetch_one_http_search(&bing_url, max_items);
    match bing_res {
        Ok(list) if !list.is_empty() || custom_base.is_some() => Ok(list),
        Err(e) if e.msg.to_ascii_lowercase().contains("anti-bot") || custom_base.is_some() => Err(e),
        _ => {
            let google_url = format!("https://www.google.com/search?tbm=isch&udm=2&q={enc}");
            fetch_one_http_search(&google_url, max_items)
        }
    }
}

fn fetch_candidates(query: &str, max_n: usize, cand_dir: &Path, allow_http_fallback: bool) -> R<(Vec<RawCandidate>, &'static str)> {
    let custom_bin = std::env::var("OPENRO_REFERENCE_SEARCH_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let has_custom_bin = custom_bin.is_some();
    match fetch_candidates_via_browser(query, max_n, cand_dir, custom_bin) {
        Ok(list) => Ok((list, "shared_browser")),
        Err(browser_err) => {
            // Never silently bypass CAPTCHA or an explicitly configured mock binary.
            if !allow_http_fallback || has_custom_bin || browser_err.status == 502 {
                return Err(browser_err);
            }
            let list = fetch_candidates_via_http(query, max_n)?;
            Ok((list, "http_fallback"))
        }
    }
}

fn fetch_candidates_via_browser(
    query: &str,
    max_n: usize,
    cand_dir: &Path,
    custom_bin: Option<String>,
) -> R<Vec<RawCandidate>> {
    let cmd = if let Some(bin) = custom_bin {
        let mut c = Command::new(bin);
        c.arg(query)
            .arg(max_n.to_string())
            .arg(cand_dir.to_string_lossy().as_ref());
        c
    } else {
        let py = crate::tool_adapters::resolve_shared_browser_python().ok_or_else(|| Oc {
            status: 503,
            msg: "Shared browser Python not found. Install shared-browser, configure a custom reference_search adapter, or pass reference images directly.".into(),
        })?;
        if !py.is_file() {
            return err(
                503,
                format!(
                    "Shared browser Python not found at {}. Install shared-browser or pass reference images directly.",
                    py.display()
                ),
            );
        }
        let mut c = Command::new(py);
        c.arg("-c")
            .arg(PY_REFERENCE_SEARCH)
            .arg(query)
            .arg(max_n.to_string())
            .arg(cand_dir.to_string_lossy().as_ref());
        c
    };

    let (code, stdout, stderr) = run_command_with_timeout(cmd, Duration::from_secs(90))
        .map_err(|m| Oc { status: 504, msg: m })?;
    let parsed: Value = serde_json::from_slice(&stdout).unwrap_or_else(|_| {
        let err_txt = String::from_utf8_lossy(&stderr);
        json!({ "ok": false, "error_kind": "DRIVER", "error": format!("reference search exited {code}: {}", err_txt.trim()) })
    });
    if parsed.get("ok").and_then(Value::as_bool) != Some(true) {
        let kind = parsed.get("error_kind").and_then(Value::as_str).unwrap_or("DRIVER");
        let msg = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Reference search failed.");
        let status = match kind {
            "CAPTCHA" => 502,
            "LEASE_BUSY" => 423,
            _ => 503,
        };
        return err(status, msg.to_string());
    }

    let mut out = Vec::new();
    for item in parsed.get("candidates").and_then(Value::as_array).into_iter().flatten() {
        let image_url = item.get("image_url").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let local_file = item.get("local_file").and_then(Value::as_str).map(String::from);
        if image_url.is_empty() && local_file.is_none() {
            continue;
        }
        out.push(RawCandidate {
            image_url,
            source_url: item.get("source_url").and_then(Value::as_str).unwrap_or("").to_string(),
            title: item.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            width: item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
            height: item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
            local_file,
        });
    }
    Ok(out)
}

fn is_safe_candidate_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
    } else {
        authority.split(':').next().unwrap_or("")
    }
    .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    let test_loopback_ok = std::env::var("OPENRO_REFERENCE_SEARCH_HTTP_BASE")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
        || std::env::var("OPENRO_REFERENCE_SEARCH_BIN")
            .ok()
            .is_some_and(|v| !v.trim().is_empty());
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return test_loopback_ok;
    }
    if host == "0.0.0.0"
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.starts_with("169.254.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
    {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                !(v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || v4.is_multicast())
            }
            std::net::IpAddr::V6(v6) => {
                !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast())
            }
        };
    }
    true
}

fn download_candidate(c: &RawCandidate, dest_stem: &Path) -> Option<(PathBuf, &'static str, Vec<u8>)> {
    let bytes = if let Some(local) = &c.local_file {
        std::fs::read(local).ok()?
    } else if is_safe_candidate_url(&c.image_url) {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
        let resp = agent
            .get(&c.image_url)
            .set("User-Agent", "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0")
            .call()
            .ok()?;
        let mut buf = Vec::new();
        std::io::Read::take(resp.into_reader(), MAX_CANDIDATE_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .ok()?;
        if buf.len() > MAX_CANDIDATE_BYTES {
            return None;
        }
        buf
    } else {
        return None;
    };
    let (_, ext) = detect_image_mime(&bytes).ok()?;
    let path = dest_stem.with_extension(ext);
    std::fs::write(&path, &bytes).ok()?;
    Some((path, ext, bytes))
}

fn write_json_atomic(path: &Path, v: &Value) -> R<()> {
    let text = serde_json::to_string_pretty(v).map_err(|e| Oc { status: 500, msg: format!("encode json: {e}") })?;
    let tmp = path.with_extension("json.tmp-openro");
    std::fs::write(&tmp, format!("{text}\n"))
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| Oc { status: 500, msg: format!("write {}: {e}", path.display()) })
}

/// `POST /api/art/references/search`
pub fn search(root: &Path, body: &Value) -> R<Value> {
    let ref_kind = parse_reference_kind(body.get("kind").and_then(Value::as_str));
    let character = body.get("character").and_then(Value::as_str).unwrap_or("").trim();
    let subject = body
        .get("subject")
        .or_else(|| body.get("weapon"))
        .or_else(|| body.get("prop"))
        .or_else(|| body.get("character"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let show = body.get("show").and_then(Value::as_str).unwrap_or("").trim();
    match ref_kind {
        ReferenceKind::Character => {
            if character.is_empty() || show.is_empty() {
                return bad("Send both 'character' and 'show' (e.g. {\"character\":\"Naruto Uzumaki\",\"show\":\"Naruto Shippuden\"}).");
            }
        }
        ReferenceKind::Prop => {
            if subject.is_empty() {
                return bad("Send 'subject' (or 'weapon'/'prop'/'character') for a weapon/prop reference search.");
            }
        }
    }
    let default_qualifiers = match ref_kind {
        ReferenceKind::Character => DEFAULT_QUALIFIERS,
        ReferenceKind::Prop => DEFAULT_PROP_QUALIFIERS,
    };
    let qualifiers = body
        .get("qualifiers")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_qualifiers);
    let explicit_slug = body
        .get("card_id")
        .or_else(|| body.get("slug"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let slug = match explicit_slug {
        Some(s) => sanitize_art_slug(s),
        None => match ref_kind {
            ReferenceKind::Character => sanitize_art_slug(character),
            ReferenceKind::Prop => {
                let s = sanitize_art_slug(subject);
                if s.starts_with("weapon_") {
                    s
                } else {
                    format!("weapon_{s}")
                }
            }
        },
    };
    let max_n = body
        .get("max_candidates")
        .and_then(Value::as_u64)
        .unwrap_or(6)
        .clamp(1, MAX_CANDIDATES as u64) as usize;
    let auto_pick = body.get("auto_pick").and_then(Value::as_bool).unwrap_or(true);
    let project_param = body.get("project").and_then(Value::as_str).unwrap_or("");

    let query = match ref_kind {
        ReferenceKind::Character => format!("{character} {show} {qualifiers}"),
        ReferenceKind::Prop => {
            if show.is_empty() {
                format!("{subject} {qualifiers}")
            } else {
                format!("{subject} {show} {qualifiers}")
            }
        }
    };
    let refs_dir = root.join("art").join("references");
    let cand_dir = refs_dir.join(".candidates").join(&slug);
    std::fs::create_dir_all(&cand_dir).or_else(|e| err(500, format!("create references dir: {e}")))?;

    let allow_http_fallback = body
        .get("use_http_fallback")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let (raw, search_backend) = fetch_candidates(&query, max_n, &cand_dir, allow_http_fallback)?;
    let ranked = rank_candidates_for_kind(raw, ref_kind);

    let mut saved_candidates = Vec::new();
    for sc in ranked {
        if saved_candidates.len() >= max_n {
            break;
        }
        let idx = saved_candidates.len();
        let stem = cand_dir.join(format!("cand_{idx}"));
        let Some((abs_path, ext, bytes)) = download_candidate(&sc.raw, &stem) else {
            continue;
        };
        let rel_path = format!("art/references/.candidates/{slug}/cand_{idx}.{ext}");
        let url = image_url(project_param, &rel_path);
        let sha = sha256_hex(&bytes);
        saved_candidates.push((
            abs_path,
            ext,
            bytes.len(),
            sha.clone(),
            sc.rejected_reason.is_none(),
            json!({
                "index": idx,
                "path": rel_path,
                "url": url,
                "image_url": sc.raw.image_url,
                "source_url": sc.raw.source_url,
                "title": sc.raw.title,
                "width": sc.raw.width,
                "height": sc.raw.height,
                "bytes": bytes.len(),
                "sha256": sha,
                "score": sc.score,
                "reasons": sc.reasons,
                "rejected_reason": sc.rejected_reason,
            }),
        ));
    }

    if saved_candidates.is_empty() {
        return err(502, format!("No usable image candidates found for '{query}'. Try different qualifiers."));
    }

    let chosen_index = if auto_pick {
        saved_candidates
            .iter()
            .position(|c| c.4)
            .unwrap_or(0)
    } else {
        0
    };
    let chosen = &saved_candidates[chosen_index];
    let active_rel = format!("art/references/{slug}.{}", chosen.1);
    let active_abs = safe_rel(root, &active_rel)?;
    std::fs::copy(&chosen.0, &active_abs)
        .or_else(|e| err(500, format!("save chosen reference: {e}")))?;

    let candidates_json: Vec<Value> = saved_candidates.iter().map(|c| c.5.clone()).collect();
    let meta_rel = format!("art/references/{slug}.meta.json");
    let meta_doc = json!({
        "version": 1,
        "slug": slug,
        "kind": ref_kind.as_str(),
        "subject": subject,
        "character": character,
        "show": show,
        "query": query,
        "search_backend": search_backend,
        "chosen_index": chosen_index,
        "reference_path": active_rel,
        "sha256": chosen.3,
        "bytes": chosen.2,
        "source_url": chosen.5.get("source_url"),
        "image_url": chosen.5.get("image_url"),
        "created_at": now_iso(),
        "candidates": candidates_json,
    });
    write_json_atomic(&root.join(&meta_rel), &meta_doc)?;

    Ok(json!({
        "ok": true,
        "slug": slug,
        "kind": ref_kind.as_str(),
        "subject": subject,
        "character": character,
        "show": show,
        "query": query,
        "search_backend": search_backend,
        "chosen_index": chosen_index,
        "reference_path": active_rel,
        "meta_path": meta_rel,
        "url": image_url(project_param, &active_rel),
        "sha256": chosen.3,
        "bytes": chosen.2,
        "candidates": meta_doc["candidates"],
    }))
}

fn image_url(project: &str, rel: &str) -> String {
    if project.is_empty() {
        format!("/api/art/image?path={rel}")
    } else {
        format!("/api/art/image?project={project}&path={rel}")
    }
}

/// `GET /api/art/references`
pub fn list(root: &Path) -> R<Value> {
    let dir = root.join("art").join("references");
    let mut items = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".meta.json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    items.push(v);
                }
            }
        }
    }
    Ok(json!({ "ok": true, "count": items.len(), "references": items }))
}

/// `POST /api/art/references/select` — owner/UI overrides the chosen candidate.
pub fn select(root: &Path, body: &Value) -> R<Value> {
    let raw_slug = body
        .get("slug")
        .or_else(|| body.get("card_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if raw_slug.is_empty() {
        return bad("Send 'slug' (or 'card_id') and 'candidate_index'.");
    }
    let slug = sanitize_art_slug(raw_slug);
    let Some(idx) = body.get("candidate_index").and_then(Value::as_u64).map(|n| n as usize) else {
        return bad("Send 'candidate_index' as a non-negative integer.");
    };
    let meta_rel = format!("art/references/{slug}.meta.json");
    let meta_abs = safe_rel(root, &meta_rel)?;
    let mut meta: Value = std::fs::read_to_string(&meta_abs)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| Oc { status: 404, msg: format!("No saved reference search found for '{slug}'.") })?;

    let cand = meta
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|arr| arr.get(idx))
        .cloned()
        .ok_or_else(|| Oc { status: 400, msg: format!("candidate_index {idx} is out of range.") })?;
    let cand_rel = cand
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Oc { status: 500, msg: "Candidate record is missing path.".into() })?;
    let cand_abs = safe_rel(root, cand_rel)?;
    let bytes = std::fs::read(&cand_abs)
        .map_err(|_| Oc { status: 404, msg: format!("Candidate file '{cand_rel}' is missing on disk.") })?;
    let (_, ext) = detect_image_mime(&bytes).map_err(|e| Oc { status: 400, msg: e })?;
    let active_rel = format!("art/references/{slug}.{ext}");
    let active_abs = safe_rel(root, &active_rel)?;
    std::fs::write(&active_abs, &bytes)
        .or_else(|e| err(500, format!("write selected reference: {e}")))?;

    let sha = sha256_hex(&bytes);
    meta["chosen_index"] = json!(idx);
    meta["reference_path"] = json!(active_rel);
    meta["sha256"] = json!(sha);
    meta["bytes"] = json!(bytes.len());
    meta["source_url"] = cand.get("source_url").cloned().unwrap_or(Value::Null);
    meta["image_url"] = cand.get("image_url").cloned().unwrap_or(Value::Null);
    meta["updated_at"] = json!(now_iso());
    write_json_atomic(&meta_abs, &meta)?;

    let project_param = body.get("project").and_then(Value::as_str).unwrap_or("");
    Ok(json!({
        "ok": true,
        "slug": slug,
        "chosen_index": idx,
        "reference_path": active_rel,
        "url": image_url(project_param, &active_rel),
        "sha256": sha,
        "bytes": bytes.len(),
        "width": cand.get("width"),
        "height": cand.get("height"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_action_scenes_above_standing_character_sheets() {
        let sheet = RawCandidate {
            image_url: "https://example.com/naruto-character-sheet.png".into(),
            source_url: "https://wiki.example.com/naruto".into(),
            title: "Naruto official character sheet standing pose".into(),
            width: 800,
            height: 1200,
            local_file: None,
        };
        let cutout = RawCandidate {
            image_url: "https://pngwing.com/naruto-render.png".into(),
            source_url: "https://pngwing.com/123".into(),
            title: "Naruto transparent png render".into(),
            width: 350,
            height: 900,
            local_file: None,
        };
        let action = RawCandidate {
            image_url: "https://cdn.example.com/naruto-rasengan-battle.jpg".into(),
            source_url: "https://anime.example.com/ep476".into(),
            title: "Naruto Shippuden dynamic battle action scene sakuga frame".into(),
            width: 1280,
            height: 720,
            local_file: None,
        };
        let ranked = rank_candidates(vec![sheet, cutout, action]);
        assert!(ranked[0].rejected_reason.is_none());
        assert!(ranked[0].raw.image_url.contains("rasengan-battle"));
        assert!(ranked[1].rejected_reason.is_some());
        assert!(ranked[2].rejected_reason.is_some());
    }

    #[test]
    fn parses_bing_html_and_stops_on_captcha() {
        let html = r#"
            <a class="iusc" m="{&quot;murl&quot;:&quot;https://cdn.example/goku-battle.png&quot;,&quot;purl&quot;:&quot;https://anime.example/ep&quot;,&quot;t&quot;:&quot;Goku dynamic battle action scene&quot;,&quot;w&quot;:1280,&quot;h&quot;:720}"></a>
            <a class="iusc" m="{&quot;murl&quot;:&quot;https://wiki.example/sheet.png&quot;,&quot;purl&quot;:&quot;https://wiki.example/goku&quot;,&quot;t&quot;:&quot;Goku official character sheet&quot;,&quot;w&quot;:600,&quot;h&quot;:1200}"></a>
        "#;
        let parsed = parse_http_image_candidates(html, 8).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].image_url, "https://cdn.example/goku-battle.png");
        assert_eq!(parsed[0].width, 1280);

        let captcha_html = "<html><body>Please verify you are human to continue</body></html>";
        assert!(parse_http_image_candidates(captcha_html, 8).is_err());
    }

    #[test]
    fn ranks_isolated_prop_refs_above_action_scenes_and_watermarks() {
        let action_scene = RawCandidate {
            image_url: "https://anime.example/ep12-battle-screenshot.jpg".into(),
            source_url: "https://anime.example/ep12".into(),
            title: "Sasuke battle scene episode screenshot sakuga clash".into(),
            width: 1280,
            height: 720,
            local_file: None,
        };
        let watermarked = RawCandidate {
            image_url: "https://shutterstock.com/katana-prop-watermark.jpg".into(),
            source_url: "https://shutterstock.com/123".into(),
            title: "Katana weapon prop reference sheet watermark".into(),
            width: 1200,
            height: 800,
            local_file: None,
        };
        let low_res = RawCandidate {
            image_url: "https://wiki.example/tiny-sword.png".into(),
            source_url: "https://fandom.com/wiki/Sword".into(),
            title: "Kusanagi sword wiki weapon render".into(),
            width: 120,
            height: 120,
            local_file: None,
        };
        let wiki_prop = RawCandidate {
            image_url: "https://static.wikia.nocookie.net/naruto/images/kusanagi-render.png".into(),
            source_url: "https://naruto.fandom.com/wiki/Sword_of_Kusanagi".into(),
            title: "Sword of Kusanagi isolated weapon prop reference sheet blade hilt scabbard".into(),
            width: 1100,
            height: 360,
            local_file: None,
        };

        let ranked = rank_candidates_for_kind(
            vec![action_scene, watermarked, low_res, wiki_prop],
            ReferenceKind::Prop,
        );
        assert!(ranked[0].rejected_reason.is_none());
        assert!(ranked[0].raw.image_url.contains("kusanagi-render"));
        assert!(ranked[0].score > ranked[1].score);
        // Watermarked and low-res candidates must be rejected
        assert!(ranked.iter().any(|c| c.raw.image_url.contains("shutterstock") && c.rejected_reason.is_some()));
        assert!(ranked.iter().any(|c| c.raw.image_url.contains("tiny-sword") && c.rejected_reason.is_some()));
    }
}

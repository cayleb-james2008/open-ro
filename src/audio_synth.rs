//! Zero-dependency pure-Rust procedural RIFF/WAVE (`.wav`) SFX synthesizer for OpenRO.
//!
//! Generates authentic 44.1 kHz, 16-bit signed PCM mono `.wav` game audio cues
//! completely offline (no external binaries or API keys required), plus atomic
//! storage and traversal-hardened reads under `<data_dir>/game-assets/audio/`.
//!
//! Presets:
//! - `summon_roll` — crystalline Torii shrine pentatonic arpeggio bell
//! - `mythic_fanfare` — grand major-triad harmonic fanfare swell + sparkle tail
//! - `battle_crit` — sub-bass impact drop + filtered metallic slash burst
//! - `potion_drink` — bubbling ascending harmonic chime
//! - `ui_click` — crisp tactile 75 ms UI interaction click
//! - `level_up` — triumphant 4-note ascending arcade fanfare.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

pub const SAMPLE_RATE: u32 = 44_100;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;

pub const MAX_DURATION_MS: u32 = 15_000;

/// Canonical list of procedural SFX presets.
pub const AUDIO_PRESETS: &[&str] = &[
    "summon_roll",
    "mythic_fanfare",
    "battle_crit",
    "potion_drink",
    "ui_click",
    "level_up",
    "death_shatter",
    "card_flip",
    "pack_open",
    "rare_chime",
    "legendary_drop",
    "secret_alarm",
    "transcendent_choir",
    "battle_hit",
    "button_hover",
    "error_buzz",
    "ambient_plaza",
];

/// In-memory synthesized `.wav` buffer and audio metadata.
#[derive(Clone, Debug)]
pub struct SynthWav {
    pub preset: &'static str,
    pub wav_bytes: Vec<u8>,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sha256: String,
}

/// Normalize a preset name or alias into one of the canonical presets.
pub fn normalize_audio_preset(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "summon_roll" | "roll_chime" | "chime" | "roll" => Ok("summon_roll"),
        "mythic_fanfare" | "fanfare" | "mythic" => Ok("mythic_fanfare"),
        "battle_crit" | "crit" | "crit_hit" | "clash" | "card_clash" => Ok("battle_crit"),
        "potion_drink" | "potion" | "luck_potion" => Ok("potion_drink"),
        "ui_click" | "click" | "button" => Ok("ui_click"),
        "level_up" | "levelup" | "coin" | "pickup" => Ok("level_up"),
        "death_shatter" | "shatter" | "ko" | "card_break" => Ok("death_shatter"),
        "card_flip" | "flip" | "whoosh" => Ok("card_flip"),
        "pack_open" | "pack" | "summon_open" => Ok("pack_open"),
        "rare_chime" | "rare" | "epic_chime" => Ok("rare_chime"),
        "legendary_drop" | "legendary" => Ok("legendary_drop"),
        "secret_alarm" | "secret" | "boss_alarm" => Ok("secret_alarm"),
        "transcendent_choir" | "transcendent" | "divine" => Ok("transcendent_choir"),
        "battle_hit" | "hit" | "impact" | "punch" => Ok("battle_hit"),
        "button_hover" | "hover" | "tick" => Ok("button_hover"),
        "error_buzz" | "error" | "denied" => Ok("error_buzz"),
        "ambient_plaza" | "ambient" | "plaza_loop" => Ok("ambient_plaza"),
        other => Err(format!(
            "unknown audio preset '{other}' (supported: {}; or pass a custom 'layers' recipe)",
            AUDIO_PRESETS.join(", ")
        )),
    }
}

fn finalize_pcm(preset: &'static str, duration_ms: u32, mut pcm: Vec<f64>) -> SynthWav {
    let sample_count = pcm.len();
    let edge = ((SAMPLE_RATE as f64) * 0.003) as usize;
    for (i, s) in pcm.iter_mut().enumerate() {
        let env = if i < edge {
            i as f64 / edge.max(1) as f64
        } else if i + edge > sample_count {
            (sample_count - i) as f64 / edge.max(1) as f64
        } else {
            1.0
        };
        *s *= env;
    }
    let peak = pcm
        .iter()
        .fold(0.0f64, |acc, &v| acc.max(v.abs()))
        .max(1e-6);
    let scale = if peak > 0.88 { 0.88 / peak } else { 1.0 };

    let mut samples_i16 = Vec::with_capacity(sample_count);
    for &s in &pcm {
        let clamped = (s * scale).clamp(-0.98, 0.98);
        samples_i16.push((clamped * 32_767.0) as i16);
    }

    let wav_bytes = encode_pcm16_wav(&samples_i16, SAMPLE_RATE);
    let mut hasher = Sha256::new();
    hasher.update(&wav_bytes);
    let digest = hasher.finalize();
    let sha256: String = digest.iter().map(|b| format!("{b:02x}")).collect();

    SynthWav {
        preset,
        wav_bytes,
        duration_ms,
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        bits_per_sample: BITS_PER_SAMPLE,
        sha256,
    }
}

/// Synthesize a valid 44.1 kHz 16-bit PCM mono RIFF/WAVE buffer for `preset`.
pub fn synthesize_wav(preset: &str, duration_override_ms: Option<u32>) -> Result<SynthWav, String> {
    let canonical = normalize_audio_preset(preset)?;
    let default_ms: u32 = match canonical {
        "summon_roll" => 1_100,
        "mythic_fanfare" => 1_800,
        "battle_crit" => 650,
        "potion_drink" => 750,
        "ui_click" => 75,
        "level_up" => 950,
        "death_shatter" => 900,
        "card_flip" => 260,
        "pack_open" => 1_300,
        "rare_chime" => 850,
        "legendary_drop" => 2_200,
        "secret_alarm" => 2_400,
        "transcendent_choir" => 3_600,
        "battle_hit" => 280,
        "button_hover" => 55,
        "error_buzz" => 320,
        "ambient_plaza" => 6_000,
        _ => 800,
    };
    let duration_ms = duration_override_ms.unwrap_or(default_ms).clamp(50, MAX_DURATION_MS);
    let sample_count = ((SAMPLE_RATE as u64) * (duration_ms as u64) / 1000).max(1) as usize;
    let mut pcm = vec![0.0f64; sample_count];

    match canonical {
        "summon_roll" => synth_summon_roll(&mut pcm),
        "mythic_fanfare" => synth_mythic_fanfare(&mut pcm),
        "battle_crit" => synth_battle_crit(&mut pcm),
        "potion_drink" => synth_potion_drink(&mut pcm),
        "ui_click" => synth_ui_click(&mut pcm),
        "level_up" => synth_level_up(&mut pcm),
        "death_shatter" => synth_death_shatter(&mut pcm),
        "card_flip" => synth_card_flip(&mut pcm),
        "pack_open" => synth_pack_open(&mut pcm),
        "rare_chime" => synth_rare_chime(&mut pcm),
        "legendary_drop" => synth_legendary_drop(&mut pcm),
        "secret_alarm" => synth_secret_alarm(&mut pcm),
        "transcendent_choir" => synth_transcendent_choir(&mut pcm),
        "battle_hit" => synth_battle_hit(&mut pcm),
        "button_hover" => synth_button_hover(&mut pcm),
        "error_buzz" => synth_error_buzz(&mut pcm),
        "ambient_plaza" => synth_ambient_plaza(&mut pcm),
        _ => synth_ui_click(&mut pcm),
    }

    Ok(finalize_pcm(canonical, duration_ms, pcm))
}

/// Synthesize a custom multi-layer DSP recipe:
/// each layer has `{ wave: "sine"|"triangle"|"square"|"saw"|"noise", start_ms?, duration_ms?,
/// freq_start?, freq_end?, gain?, attack_ms?, decay_ms?, sustain?, release_ms? }`.
pub fn synthesize_recipe(layers: &[Value], duration_override_ms: Option<u32>) -> Result<SynthWav, String> {
    if layers.is_empty() || layers.len() > 32 {
        return Err("custom audio recipe 'layers' must contain 1..=32 layers".to_string());
    }
    let mut inferred_end_ms: u32 = 100;
    for l in layers {
        let start = l.get("start_ms").and_then(Value::as_u64).unwrap_or(0) as u32;
        let dur = l.get("duration_ms").and_then(Value::as_u64).unwrap_or(350) as u32;
        inferred_end_ms = inferred_end_ms.max(start.saturating_add(dur));
    }
    let duration_ms = duration_override_ms
        .unwrap_or(inferred_end_ms)
        .clamp(50, MAX_DURATION_MS);
    let total_samples = ((SAMPLE_RATE as u64) * (duration_ms as u64) / 1000).max(1) as usize;
    let mut pcm = vec![0.0f64; total_samples];
    let sr = SAMPLE_RATE as f64;

    for (idx, layer) in layers.iter().enumerate() {
        let wave = layer
            .get("wave")
            .and_then(Value::as_str)
            .unwrap_or("sine")
            .trim()
            .to_ascii_lowercase();
        if !matches!(wave.as_str(), "sine" | "triangle" | "square" | "saw" | "noise") {
            return Err(format!(
                "layer {idx}: unsupported wave '{wave}' (expected sine, triangle, square, saw, or noise)"
            ));
        }
        let start_ms = layer.get("start_ms").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        let dur_ms = layer
            .get("duration_ms")
            .and_then(Value::as_f64)
            .unwrap_or(350.0)
            .clamp(5.0, MAX_DURATION_MS as f64);
        let f0 = layer.get("freq_start").and_then(Value::as_f64).unwrap_or(440.0).clamp(20.0, 18_000.0);
        let f1 = layer.get("freq_end").and_then(Value::as_f64).unwrap_or(f0).clamp(20.0, 18_000.0);
        let gain = layer.get("gain").and_then(Value::as_f64).unwrap_or(0.5).clamp(0.0, 2.0);
        let attack_s = layer.get("attack_ms").and_then(Value::as_f64).unwrap_or(10.0).max(0.5) / 1000.0;
        let decay_s = layer.get("decay_ms").and_then(Value::as_f64).unwrap_or(50.0).max(0.5) / 1000.0;
        let sustain = layer.get("sustain").and_then(Value::as_f64).unwrap_or(0.7).clamp(0.0, 1.0);
        let release_s = layer.get("release_ms").and_then(Value::as_f64).unwrap_or(80.0).max(0.5) / 1000.0;

        let start_sample = ((start_ms / 1000.0) * sr) as usize;
        let dur_s = dur_ms / 1000.0;
        let len_samples = (dur_s * sr) as usize;
        let mut phase = 0.0f64;
        let mut noise_state: u32 = 0x1234_5678u32.wrapping_add((idx as u32).wrapping_mul(0x9E37_79B9));

        for k in 0..len_samples {
            let pos = start_sample + k;
            if pos >= total_samples {
                break;
            }
            let t = k as f64 / sr;
            let progress = (t / dur_s).clamp(0.0, 1.0);
            let freq = f0 + (f1 - f0) * progress;
            phase += freq / sr;
            let frac = phase.fract();
            let raw_val = match wave.as_str() {
                "sine" => (2.0 * PI * phase).sin(),
                "triangle" => 4.0 * (frac - 0.5).abs() - 1.0,
                "square" => if frac < 0.5 { 1.0 } else { -1.0 },
                "saw" => 2.0 * frac - 1.0,
                _ => {
                    noise_state = noise_state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((noise_state >> 16) as f64 / 32_768.0) - 1.0
                }
            };
            let env = if t < attack_s {
                t / attack_s
            } else if t < attack_s + decay_s {
                1.0 - (1.0 - sustain) * ((t - attack_s) / decay_s)
            } else if t > (dur_s - release_s).max(0.0) {
                let rem = (dur_s - t).max(0.0);
                sustain * (rem / release_s).clamp(0.0, 1.0)
            } else {
                sustain
            };
            pcm[pos] += raw_val * env * gain;
        }
    }

    Ok(finalize_pcm("custom", duration_ms, pcm))
}

/// Strictly validate an audio filename (no traversal, separators, NULs, hidden files; `.wav` only).
pub fn sanitize_audio_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 96 {
        return Err("audio filename must be 1..=96 characters".to_string());
    }
    if trimmed.contains('\0')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.starts_with('.')
    {
        return Err(format!(
            "invalid audio filename '{trimmed}': path separators or traversal are forbidden"
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!(
            "invalid audio filename '{trimmed}': only ASCII letters, digits, '.', '-', '_' allowed"
        ));
    }
    if !trimmed.to_ascii_lowercase().ends_with(".wav") {
        return Err(format!("audio filename '{trimmed}' must end with .wav"));
    }
    Ok(trimmed.to_string())
}

fn slug_to_wav_filename(raw: Option<&str>, preset: &str) -> String {
    let base = raw.unwrap_or(preset).trim();
    let stripped = base
        .strip_suffix(".wav")
        .or_else(|| base.strip_suffix(".WAV"))
        .unwrap_or(base);
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let clean = slug.trim_matches('-');
    if clean.is_empty() {
        format!("{}.wav", preset.replace('_', "-"))
    } else {
        format!("{clean}.wav")
    }
}

pub fn audio_assets_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("game-assets").join("audio")
}

/// Generate a procedural `.wav` SFX cue and persist it atomically under `<data_dir>/game-assets/audio/`.
/// Returns the exact `#332` `POST /api/tools/audio/generate` JSON contract.
pub fn generate_audio_asset(data_dir: &Path, req: &Value) -> Result<Value, String> {
    generate_audio_asset_for_project(data_dir, None, req)
}

/// Generate a procedural `.wav` SFX cue (from a preset OR custom `layers` recipe) and persist it
/// under `<data_dir>/game-assets/audio/` AND (when `project_root` is provided) directly into
/// `<project_root>/assets/audio/<filename>` (or custom project-relative `out_path`).
pub fn generate_audio_asset_for_project(
    data_dir: &Path,
    project_root: Option<&Path>,
    req: &Value,
) -> Result<Value, String> {
    let duration_override = req
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let synth = if let Some(layers) = req.get("layers").and_then(|v| v.as_array()) {
        synthesize_recipe(layers, duration_override)?
    } else {
        let preset_raw = req
            .get("preset")
            .and_then(|v| v.as_str())
            .unwrap_or("summon_roll");
        synthesize_wav(preset_raw, duration_override)?
    };

    let raw_name = req.get("name").and_then(|v| v.as_str());
    let filename = sanitize_audio_filename(&slug_to_wav_filename(raw_name, synth.preset))?;

    let dir = audio_assets_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create audio assets dir: {e}"))?;
    let target = dir.join(&filename);
    let tmp = dir.join(format!(".{filename}.tmp"));
    std::fs::write(&tmp, &synth.wav_bytes).map_err(|e| format!("write wav tmp: {e}"))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("commit wav file: {e}"))?;

    let mut project_path_out: Option<String> = None;
    if let Some(root) = project_root {
        let rel = req
            .get("out_path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("assets/audio/{filename}"));
        if !rel.to_ascii_lowercase().ends_with(".wav") {
            return Err("out_path for audio must end with .wav".to_string());
        }
        let abs = crate::opencloud::safe_rel(root, &rel).map_err(|e| e.msg)?;
        if let Some(p) = abs.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("create project audio dir: {e}"))?;
        }
        let ptmp = abs.with_extension("wav.tmp-openro");
        std::fs::write(&ptmp, &synth.wav_bytes).map_err(|e| format!("write project wav: {e}"))?;
        std::fs::rename(&ptmp, &abs).map_err(|e| format!("commit project wav: {e}"))?;
        project_path_out = Some(rel);
    }

    Ok(json!({
        "ok": true,
        "preset": synth.preset,
        "filename": filename,
        "wav_url": format!("/api/assets/audio/{filename}"),
        "project_path": project_path_out,
        "bytes": synth.wav_bytes.len(),
        "duration_ms": synth.duration_ms,
        "sample_rate": synth.sample_rate,
        "channels": synth.channels,
        "bits_per_sample": synth.bits_per_sample,
        "sha256": synth.sha256,
        "presets": AUDIO_PRESETS,
    }))
}

/// Read a generated `.wav` file safely from `<data_dir>/game-assets/audio/` with symlink containment.
pub fn read_audio_asset(data_dir: &Path, raw_filename: &str) -> Result<Vec<u8>, String> {
    let filename = sanitize_audio_filename(raw_filename)?;
    let dir = audio_assets_dir(data_dir);
    let canon_dir = std::fs::canonicalize(&dir)
        .map_err(|_| format!("audio asset '{filename}' not found"))?;
    let candidate = dir.join(&filename);
    let canon_file = std::fs::canonicalize(&candidate)
        .map_err(|_| format!("audio asset '{filename}' not found"))?;
    if !canon_file.starts_with(&canon_dir) {
        return Err("audio asset path escapes audio directory".to_string());
    }
    std::fs::read(&canon_file).map_err(|e| format!("read audio asset: {e}"))
}

// ---------------------------------------------------------------------------
// DSP Waveform Generators (deterministic, zero external deps)
// ---------------------------------------------------------------------------

fn synth_summon_roll(buf: &mut [f64]) {
    // Japanese Yo/pentatonic shimmering bell arpeggio (D5, E5, G5, A5, D6)
    let notes: &[(f64, f64)] = &[
        (0.00, 587.33),
        (0.11, 659.25),
        (0.22, 783.99),
        (0.34, 880.00),
        (0.48, 1174.66),
    ];
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let mut acc = 0.0;
        for &(start, freq) in notes {
            if t >= start {
                let dt = t - start;
                let env = (dt / 0.008).min(1.0) * (-dt * 4.2).exp();
                let bell = (2.0 * PI * freq * dt).sin()
                    + 0.42 * (2.0 * PI * freq * 2.0 * dt).sin() * (-dt * 3.0).exp()
                    + 0.18 * (2.0 * PI * freq * 3.01 * dt).sin() * (-dt * 6.0).exp();
                acc += bell * env * 0.28;
            }
        }
        *sample = acc;
    }
}

fn synth_mythic_fanfare(buf: &mut [f64]) {
    // Heroic C5 -> G5 -> C6 brassy triad swell + sparkling harmonic overtone
    let notes: &[(f64, f64, f64)] = &[
        (0.00, 523.25, 0.30),
        (0.16, 659.25, 0.28),
        (0.32, 783.99, 0.30),
        (0.50, 1046.50, 0.36),
        (0.50, 523.25, 0.24),
        (0.50, 1318.51, 0.20),
    ];
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let mut acc = 0.0;
        for &(start, freq, amp) in notes {
            if t >= start {
                let dt = t - start;
                let attack = (dt / 0.035).min(1.0);
                let decay = (-dt * 1.45).exp();
                let vib = 1.0 + 0.004 * (2.0 * PI * 5.5 * dt).sin();
                let f = freq * vib;
                let brass = (2.0 * PI * f * dt).sin()
                    + 0.50 * (2.0 * PI * 2.0 * f * dt).sin()
                    + 0.24 * (2.0 * PI * 3.0 * f * dt).sin();
                acc += brass * attack * decay * amp * 0.34;
            }
        }
        *sample = acc;
    }
}

fn synth_battle_crit(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        // 1. Sub-bass punch dropping 180 Hz -> 48 Hz
        let bass_freq = 48.0 + 132.0 * (-t * 18.0).exp();
        let bass = (2.0 * PI * bass_freq * t).sin() * (-t * 9.0).exp() * 0.65;
        // 2. Metallic anime slash sweep 1800 Hz -> 420 Hz
        let slash_freq = 420.0 + 1380.0 * (-t * 14.0).exp();
        let slash = (2.0 * PI * slash_freq * t).sin()
            * (2.0 * PI * (slash_freq * 1.47) * t).sin()
            * (-t * 10.5).exp()
            * 0.55;
        // 3. Deterministic transient crackle
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let noise = ((rng >> 33) as f64 / (u32::MAX as f64)) * 2.0 - 1.0;
        let crack = noise * (-t * 36.0).exp() * 0.45;
        *sample = bass + slash + crack;
    }
}

fn synth_potion_drink(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let drops: &[(f64, f64)] = &[(0.00, 320.0), (0.14, 440.0), (0.28, 590.0), (0.43, 780.0)];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let mut acc = 0.0;
        for &(start, base_f) in drops {
            if t >= start {
                let dt = t - start;
                let freq = base_f * (1.0 + 0.55 * (1.0 - (-dt * 24.0).exp()));
                let env = (dt / 0.01).min(1.0) * (-dt * 12.0).exp();
                acc += (2.0 * PI * freq * dt).sin() * env * 0.48;
            }
        }
        *sample = acc;
    }
}

fn synth_ui_click(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let freq = 920.0 + 480.0 * (-t * 80.0).exp();
        let env = (-t * 68.0).exp();
        let tone = (2.0 * PI * freq * t).sin() + 0.35 * (2.0 * PI * freq * 2.4 * t).sin();
        *sample = tone * env * 0.7;
    }
}

fn synth_level_up(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let notes: &[(f64, f64)] = &[
        (0.00, 523.25), // C5
        (0.11, 659.25), // E5
        (0.22, 783.99), // G5
        (0.35, 1046.50), // C6
    ];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let mut acc = 0.0;
        for &(start, freq) in notes {
            if t >= start {
                let dt = t - start;
                let env = (dt / 0.006).min(1.0) * (-dt * 5.2).exp();
                let chime = (2.0 * PI * freq * dt).sin()
                    + 0.32 * (2.0 * PI * 2.0 * freq * dt).sin()
                    + 0.14 * (2.0 * PI * 3.0 * freq * dt).sin();
                acc += chime * env * 0.34;
            }
        }
        *sample = acc;
    }
}

fn synth_death_shatter(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let mut rng: u32 = 0x51A7_7E21;
    let shards: &[(f64, f64)] = &[(0.02, 1680.0), (0.06, 2340.0), (0.11, 1920.0), (0.16, 2890.0), (0.22, 1420.0)];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let sub_freq = 130.0 * (-t * 5.5).exp() + 34.0;
        let sub = (2.0 * PI * sub_freq * t).sin() * (-t * 4.5).exp() * 0.55;
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (((rng >> 16) as f64 / 32_768.0) - 1.0) * (-t * 9.0).exp() * 0.42;
        let mut glass = 0.0;
        for &(start, f) in shards {
            if t >= start {
                let dt = t - start;
                glass += (2.0 * PI * f * dt).sin() * (-dt * 12.0).exp() * 0.22;
            }
        }
        *sample = sub + noise + glass;
    }
}

fn synth_card_flip(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let mut rng: u32 = 0xC42D_F11B;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((rng >> 16) as f64 / 32_768.0) - 1.0;
        let whoosh_env = (t / 0.04).min(1.0) * (-t * 14.0).exp();
        let snap = if t > 0.07 {
            let dt = t - 0.07;
            (2.0 * PI * (1200.0 - 400.0 * dt) * dt).sin() * (-dt * 42.0).exp() * 0.45
        } else {
            0.0
        };
        *sample = n * whoosh_env * 0.35 + snap;
    }
}

fn synth_pack_open(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let mut rng: u32 = 0x0A0B_7711;
    let chord: &[f64] = &[523.25, 659.25, 783.99, 1046.50];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let tear = if t < 0.35 {
            (((rng >> 16) as f64 / 32_768.0) - 1.0) * (t / 0.12).min(1.0) * (1.0 - t / 0.35) * 0.4
        } else {
            0.0
        };
        let rise_f = 280.0 + 920.0 * (t / 0.45).min(1.0);
        let riser = if t < 0.45 {
            (2.0 * PI * rise_f * t).sin() * (t / 0.45) * 0.25
        } else {
            0.0
        };
        let mut burst = 0.0;
        if t >= 0.38 {
            let dt = t - 0.38;
            let env = (dt / 0.015).min(1.0) * (-dt * 3.4).exp();
            for &f in chord {
                burst += (2.0 * PI * f * dt).sin() * env * 0.2;
            }
        }
        *sample = tear + riser + burst;
    }
}

fn synth_rare_chime(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let notes: &[(f64, f64)] = &[(0.0, 659.25), (0.09, 880.0), (0.19, 1318.51)];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let mut acc = 0.0;
        for &(s, f) in notes {
            if t >= s {
                let dt = t - s;
                let env = (dt / 0.005).min(1.0) * (-dt * 4.5).exp();
                acc += ((2.0 * PI * f * dt).sin() + 0.3 * (2.0 * PI * 2.0 * f * dt).sin()) * env * 0.32;
            }
        }
        *sample = acc;
    }
}

fn synth_legendary_drop(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let notes: &[(f64, f64)] = &[(0.0, 293.66), (0.14, 440.0), (0.28, 587.33), (0.42, 739.99), (0.56, 880.0), (0.72, 1174.66)];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let bass = (2.0 * PI * 73.42 * t).sin() * (-t * 1.8).exp() * 0.35;
        let mut acc = bass;
        for &(s, f) in notes {
            if t >= s {
                let dt = t - s;
                let env = (dt / 0.01).min(1.0) * (-dt * 2.1).exp();
                acc += ((2.0 * PI * f * dt).sin() + 0.35 * (2.0 * PI * 2.0 * f * dt).sin()) * env * 0.22;
            }
        }
        *sample = acc;
    }
}

fn synth_secret_alarm(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let pulse = (2.0 * PI * 3.5 * t).sin() * 0.5 + 0.5;
        let freq = 220.0 + 440.0 * pulse;
        let sub = (2.0 * PI * 55.0 * t).sin() * 0.4;
        let siren = (2.0 * PI * freq * t).sin() * 0.35 * (1.0 - t / 3.0).max(0.15);
        let shimmer = (2.0 * PI * 1760.0 * t).sin() * pulse * 0.14;
        *sample = sub + siren + shimmer;
    }
}

fn synth_transcendent_choir(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let voices: &[f64] = &[261.63, 329.63, 392.0, 523.25, 659.25, 783.99, 1046.5];
    let dur = (buf.len() as f64 / sr).max(0.5);
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let swell = (t / 0.6).min(1.0) * ((dur - t) / 0.8).clamp(0.0, 1.0);
        let vib = 1.0 + 0.004 * (2.0 * PI * 5.2 * t).sin();
        let mut acc = 0.0;
        for (idx, &f) in voices.iter().enumerate() {
            let detune = 1.0 + ((idx as f64) - 3.0) * 0.0012;
            acc += (2.0 * PI * f * detune * vib * t).sin() * 0.16;
        }
        *sample = acc * swell;
    }
}

fn synth_battle_hit(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let mut rng: u32 = 0xB477_1E01;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let f = 180.0 * (-t * 18.0).exp() + 48.0;
        let thump = (2.0 * PI * f * t).sin() * (-t * 14.0).exp() * 0.72;
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let crack = (((rng >> 16) as f64 / 32_768.0) - 1.0) * (-t * 32.0).exp() * 0.35;
        *sample = thump + crack;
    }
}

fn synth_button_hover(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        *sample = (2.0 * PI * 1480.0 * t).sin() * (-t * 95.0).exp() * 0.45;
    }
}

fn synth_error_buzz(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let env = (-t * 7.5).exp();
        let raw = (2.0 * PI * 145.0 * t).sin() + 0.5 * (2.0 * PI * 155.0 * t).sin();
        *sample = raw.clamp(-0.6, 0.6) * env * 0.7;
    }
}

fn synth_ambient_plaza(buf: &mut [f64]) {
    let sr = SAMPLE_RATE as f64;
    let dur = (buf.len() as f64 / sr).max(1.0);
    let pad: &[f64] = &[146.83, 220.0, 293.66, 329.63, 440.0];
    for (i, sample) in buf.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let loop_env = (t / 0.4).min(1.0) * ((dur - t) / 0.4).clamp(0.0, 1.0);
        let lfo = 0.7 + 0.3 * (2.0 * PI * 0.25 * t).sin();
        let mut acc = 0.0;
        for &f in pad {
            acc += (2.0 * PI * f * t).sin() * 0.14;
        }
        *sample = acc * lfo * loop_env;
    }
}

/// Encode signed 16-bit mono PCM samples into a standards-compliant RIFF/WAVE file.
pub fn encode_pcm16_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let riff_len = 36u32.saturating_add(data_len);
    let byte_rate = sample_rate * (CHANNELS as u32) * (BITS_PER_SAMPLE as u32 / 8);
    let block_align = CHANNELS * (BITS_PER_SAMPLE / 8);

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

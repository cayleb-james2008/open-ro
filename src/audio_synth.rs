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

/// Canonical list of procedural SFX presets.
pub const AUDIO_PRESETS: &[&str] = &[
    "summon_roll",
    "mythic_fanfare",
    "battle_crit",
    "potion_drink",
    "ui_click",
    "level_up",
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
        other => Err(format!(
            "unknown audio preset '{other}' (expected summon_roll, mythic_fanfare, battle_crit, potion_drink, ui_click, or level_up)"
        )),
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
        _ => 800,
    };
    let duration_ms = duration_override_ms.unwrap_or(default_ms).clamp(50, 5_000);
    let sample_count = ((SAMPLE_RATE as u64) * (duration_ms as u64) / 1000).max(1) as usize;
    let mut pcm = vec![0.0f64; sample_count];

    match canonical {
        "summon_roll" => synth_summon_roll(&mut pcm),
        "mythic_fanfare" => synth_mythic_fanfare(&mut pcm),
        "battle_crit" => synth_battle_crit(&mut pcm),
        "potion_drink" => synth_potion_drink(&mut pcm),
        "ui_click" => synth_ui_click(&mut pcm),
        "level_up" => synth_level_up(&mut pcm),
        _ => synth_ui_click(&mut pcm),
    }

    // Soft-normalize and apply 3 ms fade-in/fade-out edges to eliminate DC clicks
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

    Ok(SynthWav {
        preset: canonical,
        wav_bytes,
        duration_ms,
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        bits_per_sample: BITS_PER_SAMPLE,
        sha256,
    })
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
    let preset_raw = req
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("summon_roll");
    let duration_override = req
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let synth = synthesize_wav(preset_raw, duration_override)?;

    let raw_name = req.get("name").and_then(|v| v.as_str());
    let filename = sanitize_audio_filename(&slug_to_wav_filename(raw_name, synth.preset))?;

    let dir = audio_assets_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create audio assets dir: {e}"))?;
    let target = dir.join(&filename);
    let tmp = dir.join(format!(".{filename}.tmp"));
    std::fs::write(&tmp, &synth.wav_bytes).map_err(|e| format!("write wav tmp: {e}"))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("commit wav file: {e}"))?;

    Ok(json!({
        "ok": true,
        "preset": synth.preset,
        "filename": filename,
        "wav_url": format!("/api/assets/audio/{filename}"),
        "bytes": synth.wav_bytes.len(),
        "duration_ms": synth.duration_ms,
        "sample_rate": synth.sample_rate,
        "channels": synth.channels,
        "bits_per_sample": synth.bits_per_sample,
        "sha256": synth.sha256,
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

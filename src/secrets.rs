//! Shared secrets store: one file per secret in a private directory.
//!
//! The directory is `$OPENRO_SECRETS_DIR` when set (so several OpenRO instances share
//! one set of keys), otherwise `<data_dir>/secrets`. The directory is `0700`, each file
//! `0600`. Keys are typed only into the Settings page; API replies show a mask, never
//! the value, and upstream error text is scrubbed before it is logged or returned.

use std::path::{Path, PathBuf};

pub const OPENCLOUD_KEY: &str = "opencloud_api_key";
pub const GEMINI_KEY: &str = "gemini_api_key";
pub const OPENAI_IMAGE_KEY: &str = "openai_image_api_key";

#[derive(Clone, Debug)]
pub struct SecretStore {
    dir: PathBuf,
}

impl SecretStore {
    pub fn at(dir: PathBuf) -> Self {
        SecretStore { dir }
    }

    /// `$OPENRO_SECRETS_DIR` or `<data_dir>/secrets`.
    pub fn for_data_dir(data_dir: &Path) -> Self {
        let dir = std::env::var("OPENRO_SECRETS_DIR")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("secrets"));
        SecretStore { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        let ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        ok.then(|| self.dir.join(name))
    }

    pub fn get(&self, name: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.file(name)?).ok()?;
        let t = text.trim();
        (!t.is_empty()).then(|| t.to_string())
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let path = self.file(name).ok_or("invalid secret name")?;
        let value = value.trim();
        if value.is_empty() {
            return self.clear(name);
        }
        if value.contains(['\n', '\r', '\0']) {
            return Err("a key cannot contain line breaks".to_string());
        }
        self.ensure_dir()?;
        let tmp = self.dir.join(format!(".{name}.tmp"));
        write_private(&tmp, value.as_bytes()).map_err(|e| format!("save key: {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("save key: {e}"))
    }

    pub fn clear(&self, name: &str) -> Result<(), String> {
        let path = self.file(name).ok_or("invalid secret name")?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("clear key: {e}")),
        }
    }

    fn ensure_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("create secrets dir: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("secure secrets dir: {e}"))?;
        }
        Ok(())
    }

    /// Move keys that older versions kept in `config.json` into the store. A value already in
    /// the store wins (another instance may have set a newer one). Returns the names moved
    /// so the caller can blank them in its config and save it.
    pub fn migrate(&self, legacy: &[(&str, Option<&str>)]) -> Vec<String> {
        let mut moved = Vec::new();
        for (name, value) in legacy {
            let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
                continue;
            };
            if !self.has(name) && self.set(name, v).is_err() {
                continue; // keep the old copy rather than lose the key
            }
            moved.push((*name).to_string());
        }
        moved
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// `••••abcd` (last four) for long keys, all dots for short ones.
pub fn mask(k: &str) -> String {
    let n = k.chars().count();
    if n <= 8 {
        "••••••••".to_string()
    } else {
        let tail: String = k.chars().skip(n - 4).collect();
        format!("••••{tail}")
    }
}

fn pct_encode_variant(s: &str, uppercase: bool, space_as_plus: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if space_as_plus && b == b' ' {
            out.push('+');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if uppercase {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

/// Remove every secret (plus its URL-encoded and JSON-escaped forms) from `text`.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for s in secrets.iter().filter(|s| s.len() >= 6) {
        out = out.replace(s, "••••");
        for variant in [
            pct_encode_variant(s, true, false),
            pct_encode_variant(s, false, false),
            pct_encode_variant(s, true, true),
            pct_encode_variant(s, false, true),
        ] {
            if variant != *s {
                out = out.replace(&variant, "••••");
            }
        }
        if let Ok(json_quoted) = serde_json::to_string(s) {
            let escaped = &json_quoted[1..json_quoted.len().saturating_sub(1)];
            if escaped != *s {
                out = out.replace(escaped, "••••");
            }
            if escaped.contains('/') {
                let slash_escaped = escaped.replace('/', "\\/");
                out = out.replace(&slash_escaped, "••••");
            }
        }
    }
    out
}

/// Recursively scrub secrets from any string inside a nested JSON value.
pub fn scrub_value(val: &mut serde_json::Value, secrets: &[&str]) {
    if secrets.iter().all(|s| s.len() < 6) {
        return;
    }
    match val {
        serde_json::Value::String(s) => {
            *s = scrub(s, secrets);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                scrub_value(item, secrets);
            }
        }
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let clean_k = scrub(&k, secrets);
                if let Some(mut v) = map.remove(&k) {
                    scrub_value(&mut v, secrets);
                    map.insert(clean_k, v);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear_and_modes() {
        let dir = std::env::temp_dir().join(format!("openro-secrets-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = SecretStore::at(dir.join("s"));
        assert_eq!(s.get(OPENCLOUD_KEY), None);
        s.set(OPENCLOUD_KEY, "  abcdef123456 ").unwrap();
        assert_eq!(s.get(OPENCLOUD_KEY).as_deref(), Some("abcdef123456"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let d = std::fs::metadata(s.dir()).unwrap().permissions().mode() & 0o777;
            let f = std::fs::metadata(s.dir().join(OPENCLOUD_KEY)).unwrap().permissions().mode() & 0o777;
            assert_eq!((d, f), (0o700, 0o600));
        }
        assert!(s.set("../evil", "x").is_err());
        s.clear(OPENCLOUD_KEY).unwrap();
        assert!(!s.has(OPENCLOUD_KEY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_keeps_existing_store_value() {
        let dir = std::env::temp_dir().join(format!("openro-secrets-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = SecretStore::at(dir.clone());
        s.set(GEMINI_KEY, "stored-key-1").unwrap();
        let moved = s.migrate(&[(GEMINI_KEY, Some("old-key-2")), (OPENAI_IMAGE_KEY, Some("oai-key-3")), ("x_none", None)]);
        assert_eq!(moved, vec![GEMINI_KEY, OPENAI_IMAGE_KEY]);
        assert_eq!(s.get(GEMINI_KEY).as_deref(), Some("stored-key-1"));
        assert_eq!(s.get(OPENAI_IMAGE_KEY).as_deref(), Some("oai-key-3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mask_and_scrub() {
        assert_eq!(mask("abcdefghijkl"), "••••ijkl");
        assert_eq!(mask("short"), "••••••••");
        assert_eq!(scrub("bad key secretKEY99 here", &["secretKEY99"]), "bad key •••• here");
        let special = "rbk+abc/def=123";
        assert_eq!(
            scrub("url=rbk%2Babc%2Fdef%3D123 lower=rbk%2babc%2fdef%3d123 json=rbk+abc\\/def=123", &[special]),
            "url=•••• lower=•••• json=••••"
        );
        let mut nested = serde_json::json!({
            "error": {
                "message": "upstream failed for rbk+abc/def=123",
                "details": ["token rbk%2Babc%2Fdef%3D123 invalid"]
            }
        });
        scrub_value(&mut nested, &[special]);
        let dumped = nested.to_string();
        assert!(!dumped.contains(special), "{dumped}");
        assert!(!dumped.contains("%2B"), "{dumped}");
    }
}

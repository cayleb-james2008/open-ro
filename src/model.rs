//! Model client: OpenAI-shaped `POST {base}/chat/completions`.
//!
//! Loopback OpenAI-compatible gateways (`127.0.0.1`, `localhost`, `[::1]`) work
//! without a bearer API key. When `OPENRO_MODEL_BASE_URL` and
//! `OPENRO_MODEL_API_KEY` are both unset (and `OPENRO_NO_AUTO_MODEL` is not
//! `"1"`), `ModelClient::from_env` probes local gateways running on this
//! machine (`8790`, `8789`, `11434`). Remote hosts still require an API key
//! and fail closed when none is configured.

use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
pub const DEFAULT_MODEL_ID: &str = "deepseek-v4.1-flash";
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 OpenRO/0.4";

#[derive(Clone, Debug)]
pub struct DetectedGateway {
    pub base_url: String,
    pub model_id: String,
    pub label: String,
}

const LOCAL_GATEWAYS: [(&str, &str, &str); 3] = [
    ("http://127.0.0.1:11434/v1", "qwen2.5:3b", "Ollama :11434 (Qwen 2.5 3B)"),
    ("http://127.0.0.1:8789/v1", "gemini-3.8-flash-high", "Local Gateway :8789 (Gemini 3.8 Flash)"),
    ("http://127.0.0.1:8790/v1", "deepseek-v4.1-flash-free", "Local Gateway :8790 (DeepSeek V4.1 Flash)"),
];

/// True when `url` targets loopback (`127.0.0.1`, `localhost`, or `[::1]`)
/// with a real authority boundary (`:`, `/`, or end of string), never a
/// look-alike host such as `http://localhost.evil.com`.
pub fn is_loopback_url(url: &str) -> bool {
    let trimmed = url.trim();
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// Probe local OpenAI-compatible gateways on this machine (`8790`, `8789`,
/// `11434`) with a short timeout and return every live gateway with its
/// preferred available model ID.
pub fn probe_local_gateways() -> Vec<DetectedGateway> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(600))
        .timeout_read(Duration::from_millis(900))
        .timeout(Duration::from_millis(1500))
        .build();
    let mut found = Vec::new();
    for (base, preferred_model, label) in LOCAL_GATEWAYS {
        let url = format!("{base}/models");
        let Ok(resp) = agent.get(&url).call() else {
            continue;
        };
        let Ok(val) = resp.into_json::<Value>() else {
            continue;
        };
        let ids: Vec<String> = val
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            continue;
        }
        let chosen = if ids.iter().any(|id| id == preferred_model) {
            preferred_model.to_string()
        } else {
            ids[0].clone()
        };
        found.push(DetectedGateway {
            base_url: base.to_string(),
            model_id: chosen,
            label: label.to_string(),
        });
    }
    found
}

pub struct ModelClient {
    base_url: String,
    model_id: String,
    api_key: Option<String>,
    session_id: String,
    agent: ureq::Agent,
}

impl ModelClient {
    pub fn new(base_url: String, model_id: String, api_key: Option<String>) -> Self {
        let session_id = format!("openro-{}-{}", std::process::id(), session_nonce());
        // A hung model endpoint must not hang the ask forever.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(90))
            .build();
        ModelClient {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            model_id: model_id.trim().to_string(),
            api_key: api_key.and_then(|k| {
                let t = k.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
            session_id,
            agent,
        }
    }

    /// Build from the environment (`OPENRO_MODEL_BASE_URL`, `OPENRO_MODEL_ID`,
    /// `OPENRO_MODEL_API_KEY`). When both URL and key are unset and
    /// `OPENRO_NO_AUTO_MODEL` is not `"1"`, probes local machine gateways
    /// (`8790`, `8789`, `11434`) before falling back to the unconfigured remote
    /// default.
    pub fn from_env() -> Self {
        let env_base = std::env::var("OPENRO_MODEL_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let env_model = std::env::var("OPENRO_MODEL_ID")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let env_key = std::env::var("OPENRO_MODEL_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let no_auto = std::env::var("OPENRO_NO_AUTO_MODEL")
            .ok()
            .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"));

        if env_base.is_none() && env_key.is_none() && !no_auto {
            if let Some(gw) = probe_local_gateways().into_iter().next() {
                return ModelClient::new(gw.base_url, env_model.unwrap_or(gw.model_id), None);
            }
        }

        let base = env_base.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = env_model.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string());
        ModelClient::new(base, model, env_key)
    }

    /// Loopback OpenAI-compatible endpoints work without an API key; remote
    /// endpoints require one.
    pub fn is_configured(&self) -> bool {
        self.api_key.is_some() || is_loopback_url(&self.base_url)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn is_loopback(&self) -> bool {
        is_loopback_url(&self.base_url)
    }

    pub fn api_key_masked(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| {
            let n = k.chars().count();
            if n <= 8 {
                "••••••••".to_string()
            } else {
                let tail: String = k.chars().skip(n - 4).collect();
                format!("••••{tail}")
            }
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// One chat completion. Returns the assistant message content.
    pub fn chat(&self, messages: &[Value]) -> Result<String, String> {
        if !self.is_configured() {
            return Err(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
            );
        }
        let url = format!("{}/chat/completions", self.base_url);
        let body = if is_loopback_url(&self.base_url) {
            json!({ "model": self.model_id, "messages": messages, "temperature": 0.1 })
        } else {
            json!({ "model": self.model_id, "messages": messages })
        };
        let mut req = self
            .agent
            .post(&url)
            .set("x-opencode-session", &self.session_id)
            .set("User-Agent", USER_AGENT);
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        let response = req
            .send_json(body)
            .map_err(|e| format!("The model did not answer, so nothing was built. Wait a minute and send it again. (Details: {e})"))?;
        let value: Value = response
            .into_json()
            .map_err(|e| format!("The model sent back something OpenRO could not read, so nothing was built. Send it again. (Details: {e})"))?;
        value
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "The model sent back an empty answer, so nothing was built. Send it again. (Details: no message content)".to_string())
    }
}

fn session_nonce() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 % 1_000_000_000)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_url_detection_is_strict() {
        assert!(is_loopback_url("http://127.0.0.1:8790/v1"));
        assert!(is_loopback_url("http://localhost:11434/v1"));
        assert!(is_loopback_url("http://[::1]:8789/v1"));
        assert!(!is_loopback_url("https://opencode.ai/zen/go/v1"));
        assert!(!is_loopback_url("http://localhost.evil.com/v1"));
        assert!(!is_loopback_url("http://127.0.0.1.evil.com/v1"));
    }

    #[test]
    fn loopback_works_without_api_key_while_remote_fails_closed() {
        let local = ModelClient::new(
            "http://127.0.0.1:8790/v1".to_string(),
            "deepseek-v4.1-flash-free".to_string(),
            None,
        );
        assert!(local.is_configured());
        assert!(local.is_loopback());
        assert!(!local.has_api_key());

        let remote = ModelClient::new(
            DEFAULT_BASE_URL.to_string(),
            DEFAULT_MODEL_ID.to_string(),
            None,
        );
        assert!(!remote.is_configured());
        let err = remote.chat(&[]).unwrap_err();
        assert!(err.contains("no model key configured"), "{err}");
    }
}

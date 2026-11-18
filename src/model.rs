//! Model client: OpenCode Go, OpenAI-shaped `POST {base}/chat/completions`.
//!
//! Every call carries an `x-opencode-session` header and a browser-like
//! User-Agent, and the bearer key comes from `OPENRO_MODEL_API_KEY`.
//! With no key configured the client refuses honestly — it never fabricates
//! a model reply.

use serde_json::{json, Value};

pub const DEFAULT_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
pub const DEFAULT_MODEL_ID: &str = "deepseek-v4.1-flash";
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 OpenRO/0.1";

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
            .timeout(std::time::Duration::from_secs(60))
            .build();
        ModelClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            model_id,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            session_id,
            agent,
        }
    }

    /// Build from the environment: `OPENRO_MODEL_BASE_URL`,
    /// `OPENRO_MODEL_ID`, `OPENRO_MODEL_API_KEY`.
    pub fn from_env() -> Self {
        let base = std::env::var("OPENRO_MODEL_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("OPENRO_MODEL_ID")
            .unwrap_or_else(|_| DEFAULT_MODEL_ID.to_string());
        let key = std::env::var("OPENRO_MODEL_API_KEY").ok();
        ModelClient::new(base, model, key)
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// One chat completion. Returns the assistant message content.
    pub fn chat(&self, messages: &[Value]) -> Result<String, String> {
        let key = match &self.api_key {
            Some(k) => k.clone(),
            None => {
                return Err(
                    "no model key configured: set OPENRO_MODEL_API_KEY to enable chat"
                        .to_string(),
                )
            }
        };
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({ "model": self.model_id, "messages": messages });
        let response = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {key}"))
            .set("x-opencode-session", &self.session_id)
            .set("User-Agent", USER_AGENT)
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

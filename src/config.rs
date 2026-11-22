//! Persistent runtime configuration (`data_dir/config.json`) for live Model
//! provider and Roblox Studio backend switching from the Settings UI.

use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct RuntimeConfig {
    pub base_url: Option<String>,
    pub model_id: Option<String>,
    pub api_key: Option<String>,
    pub mcp_command: Option<String>,
    /// `"auto"` | `"local_place"` | `"studio_mcp"`
    pub workspace_mode: Option<String>,
}

impl RuntimeConfig {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return RuntimeConfig::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return RuntimeConfig::default();
        };
        let str_opt = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        RuntimeConfig {
            base_url: str_opt("base_url"),
            model_id: str_opt("model_id"),
            api_key: str_opt("api_key"),
            mcp_command: str_opt("mcp_command"),
            workspace_mode: str_opt("workspace_mode"),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create config dir: {e}"))?;
        let path = dir.join("config.json");
        let tmp = dir.join("config.json.tmp");
        let val = json!({
            "base_url": self.base_url,
            "model_id": self.model_id,
            "api_key": self.api_key,
            "mcp_command": self.mcp_command,
            "workspace_mode": self.workspace_mode,
        });
        let body = serde_json::to_string_pretty(&val)
            .map_err(|e| format!("serialize config: {e}"))?;
        std::fs::write(&tmp, body).map_err(|e| format!("write config: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &path).map_err(|e| format!("commit config: {e}"))?;
        Ok(())
    }
}

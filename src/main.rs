//! OpenRO — you describe the game, it builds it.
//!
//! A proactive creative agent in a plain chat window, with full access to
//! Roblox Studio through an MCP bridge (stdio JSON-RPC) and Luau code
//! generation driven by Rune skills.
//!
//! Environment:
//!   OPENRO_MODEL_BASE_URL  default https://opencode.ai/zen/go/v1
//!   OPENRO_MODEL_ID        default deepseek-v4.1-flash
//!   OPENRO_MODEL_API_KEY   bearer key (unset = honest no-key state)
//!   OPENRO_MCP_COMMAND     Studio MCP server command (unset = not connected)
//!   OPENRO_GAME_NAME       default "Untitled Game"
//!   OPENRO_DATA_DIR        default "openro-data" (chats persist there)
//!   OPENRO_PORT            default 8077

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::server::{serve, Shared};
use std::net::TcpListener;

fn main() {
    let model = ModelClient::from_env();
    if !model.is_configured() {
        eprintln!("openro: no model key configured (OPENRO_MODEL_API_KEY unset) — chat will refuse honestly");
    }
    let studio = match McpClient::from_env() {
        Ok(Some(client)) => {
            eprintln!("openro: Studio connected via OPENRO_MCP_COMMAND");
            Some(client)
        }
        Ok(None) => {
            eprintln!("openro: Studio not connected (OPENRO_MCP_COMMAND unset)");
            None
        }
        Err(e) => {
            eprintln!("openro: Studio bridge failed to start: {e}");
            None
        }
    };
    let agent = Agent::new(model, studio);
    let game_name = std::env::var("OPENRO_GAME_NAME")
        .unwrap_or_else(|_| "Untitled Game".to_string());
    let port: u16 = std::env::var("OPENRO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8077);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("bind 127.0.0.1:{port}: {e}"));
    eprintln!("openro: serving http://127.0.0.1:{port}");
    let data_dir = std::env::var("OPENRO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("openro-data"));
    let shared = Shared::with_data_dir(Some(agent), game_name, data_dir);
    if let Err(e) = serve(listener, shared) {
        eprintln!("openro: server error: {e}");
        std::process::exit(1);
    }
}

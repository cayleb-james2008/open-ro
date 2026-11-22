//! OpenRO — you describe the game, it builds it.
//!
//! A proactive creative agent in a Studio Workbench, backed by Rune Luau code
//! generation, a real persistent Local Place Engine (persisting `place.json` +
//! Roblox Studio `game.rbxlx` XML), and live Roblox Studio MCP auto-detection.
//!
//! Environment:
//!   OPENRO_MODEL_BASE_URL   default auto-probed local gateway or OpenCode Go
//!   OPENRO_MODEL_ID         default gateway model or deepseek-v4.1-flash
//!   OPENRO_MODEL_API_KEY    optional for loopback URLs; required for remote
//!   OPENRO_NO_AUTO_MODEL    set to "1" to skip local gateway probing (fail-closed)
//!   OPENRO_MCP_COMMAND      explicit Studio MCP server command
//!   OPENRO_NO_LOCAL_PLACE   set to "1" to stay disconnected when MCP is absent
//!   OPENRO_GAME_NAME        default "Untitled Game"
//!   OPENRO_DATA_DIR         default "openro-data"
//!   OPENRO_PORT             default 8077

use openro_rs::agent::Agent;
use openro_rs::config::RuntimeConfig;
use openro_rs::mcp::McpClient;
use openro_rs::model::model_from_config;
use openro_rs::place::{LocalPlace, PlaceBackend};
use openro_rs::server::{serve, Shared};
use std::net::TcpListener;

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
}

fn main() {
    let cli_args: Vec<String> = std::env::args().collect();
    if cli_args
        .iter()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("openro-rs {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if cli_args.iter().skip(1).any(|a| a == "--help" || a == "-h") {
        println!(
            "openro-rs {} — Roblox Game Builder Studio Workbench\n\n\
             Usage: openro-rs [OPTIONS]\n\n\
             Options:\n  \
             --port <PORT>     Listen port (default: $OPENRO_PORT or 8077)\n  \
             --no-open         Start server without opening a desktop app window\n  \
             --headless        Alias for --no-open (headless/CI mode)\n  \
             -V, --version     Print version and exit\n  \
             -h, --help        Print help and exit",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }

    let cli_port: Option<u16> = cli_args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .or_else(|| {
            cli_args
                .iter()
                .find_map(|a| a.strip_prefix("--port=").and_then(|v| v.parse().ok()))
        });

    let data_dir = env_nonempty("OPENRO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("openro-data"));
    let _ = std::fs::create_dir_all(&data_dir);
    let saved_cfg = RuntimeConfig::load(&data_dir);

    let game_name = env_nonempty("OPENRO_GAME_NAME").unwrap_or_else(|| "Untitled Game".to_string());

    // 1. Model resolution: env -> saved config -> Antigravity CLI -> local gateway -> default
    let model = model_from_config(&saved_cfg, Some(&data_dir));
    if model.is_antigravity() {
        eprintln!("openro: model {} via Antigravity CLI", model.model_id());
        std::thread::spawn(|| {
            let _ = openro_rs::model::agy_models(false);
        });
    } else if model.is_loopback() {
        eprintln!("openro: connected local model {} @ {}", model.model_id(), model.base_url());
    }

    if !model.is_configured() {
        eprintln!(
            "openro: no model configured (set OPENRO_MODEL_API_KEY or pick a local gateway in Settings) — chat will refuse honestly"
        );
    }

    // 2. Studio / Place Backend resolution:
    //    explicit OPENRO_MCP_COMMAND -> saved config mode/mcp_command -> passive Vinegar detect -> real persistent LocalPlace
    let no_local_place = env_flag("OPENRO_NO_LOCAL_PLACE");
    let explicit_mcp = env_nonempty("OPENRO_MCP_COMMAND");
    let saved_mode = saved_cfg.workspace_mode.as_deref().unwrap_or("auto");

    let backend: Option<PlaceBackend> = if let Some(cmd) = explicit_mcp {
        match McpClient::spawn(&cmd) {
            Ok(client) => {
                eprintln!("openro: Studio connected via OPENRO_MCP_COMMAND");
                Some(PlaceBackend::Mcp(client))
            }
            Err(e) => {
                eprintln!("openro: Studio bridge failed to start: {e}");
                None
            }
        }
    } else if saved_mode == "local_place" {
        let place = LocalPlace::open(data_dir.clone(), game_name.clone());
        eprintln!("openro: Local Place Engine ready ({})", data_dir.join("game.rbxlx").display());
        Some(PlaceBackend::Local(place))
    } else if let Some(cmd) = saved_cfg
        .mcp_command
        .clone()
        .or_else(McpClient::detect_vinegar_studio_mcp)
    {
        match McpClient::spawn(&cmd) {
            Ok(client) => {
                eprintln!("openro: Studio connected via {cmd}");
                Some(PlaceBackend::Mcp(client))
            }
            Err(e) => {
                eprintln!("openro: Studio MCP ({cmd}) unavailable ({e}); falling back to Local Place Engine");
                if no_local_place {
                    None
                } else {
                    Some(PlaceBackend::Local(LocalPlace::open(
                        data_dir.clone(),
                        game_name.clone(),
                    )))
                }
            }
        }
    } else if no_local_place {
        eprintln!("openro: Studio not connected (OPENRO_NO_LOCAL_PLACE=1)");
        None
    } else {
        let place = LocalPlace::open(data_dir.clone(), game_name.clone());
        eprintln!("openro: Local Place Engine ready ({})", data_dir.join("game.rbxlx").display());
        Some(PlaceBackend::Local(place))
    };

    let mut agent = match backend {
        Some(PlaceBackend::Local(place)) => Agent::with_local_place(model, place),
        other => Agent::with_backend(model, other),
    };

    let port: u16 = cli_port
        .or_else(|| std::env::var("OPENRO_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8077);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("bind 127.0.0.1:{port}: {e}"));
    let url = format!("http://127.0.0.1:{port}");
    eprintln!("openro: serving {url}");

    // Change sets persist next to the chats, so a restart does not
    // silently orphan every revert.
    agent.set_changes_file(data_dir.join("changes.json"));
    let shared = Shared::with_data_dir(Some(agent), game_name, data_dir.clone());
    // Only the real binary checks GitHub in the background; tests never do.
    shared.updater.start_background();

    // Launch chromeless standalone desktop app window on Linux/Windows/macOS
    // unless --no-open / --headless / OPENRO_NO_OPEN=1 / CI / headless display.
    let desktop_outcome = openro_rs::desktop::launch_desktop_window(&url, &data_dir, &cli_args);
    if desktop_outcome.launched {
        eprintln!(
            "openro: launched desktop window ({})",
            desktop_outcome.reason
        );
    }

    if let Err(e) = serve(listener, shared) {
        eprintln!("openro: server error: {e}");
        std::process::exit(1);
    }
}

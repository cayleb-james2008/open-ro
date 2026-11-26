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

#[cfg(target_env = "gnu")]
unsafe extern "C" {
    fn mallopt(param: i32, value: i32) -> i32;
}

fn main() {
    #[cfg(target_env = "gnu")]
    unsafe {
        // Bound glibc heap arenas (M_ARENA_MAX = -8, M_MMAP_THRESHOLD = -3) so concurrent
        // worker threads share 1 compact arena and free JSON/HTML buffers (>=8 KiB) via munmap.
        let _ = mallopt(-8, 1);
        let _ = mallopt(-3, 8 * 1024);
    }
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
             --port <PORT>         Listen port (default: $OPENRO_PORT or 8077)\n  \
             --data-dir <PATH>     Data directory (default: $OPENRO_DATA_DIR or openro-data)\n  \
             --no-open             Start server without opening a desktop app window\n  \
             --headless            Alias for --no-open (headless/CI mode)\n  \
             -V, --version         Print version and exit\n  \
             -h, --help            Print help and exit",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }

    let mut cli_port: Option<u16> = None;
    let mut cli_data_dir: Option<std::path::PathBuf> = None;
    let mut i = 1usize;
    while i < cli_args.len() {
        let arg = &cli_args[i];
        if arg == "--no-open" || arg == "--headless" {
            i += 1;
        } else if arg == "--port" {
            let Some(val) = cli_args.get(i + 1) else {
                eprintln!("openro-rs: --port requires a port number");
                std::process::exit(2);
            };
            match val.parse::<u16>() {
                Ok(p) => cli_port = Some(p),
                Err(_) => {
                    eprintln!("openro-rs: invalid --port value '{val}'");
                    std::process::exit(2);
                }
            }
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--port=") {
            match val.parse::<u16>() {
                Ok(p) => cli_port = Some(p),
                Err(_) => {
                    eprintln!("openro-rs: invalid --port value '{val}'");
                    std::process::exit(2);
                }
            }
            i += 1;
        } else if arg == "--data-dir" {
            let Some(val) = cli_args.get(i + 1).filter(|v| !v.trim().is_empty()) else {
                eprintln!("openro-rs: --data-dir requires a directory path");
                std::process::exit(2);
            };
            cli_data_dir = Some(std::path::PathBuf::from(val));
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--data-dir=") {
            if val.trim().is_empty() {
                eprintln!("openro-rs: --data-dir requires a directory path");
                std::process::exit(2);
            }
            cli_data_dir = Some(std::path::PathBuf::from(val));
            i += 1;
        } else {
            eprintln!("openro-rs: unknown option '{arg}' (run with --help for valid options)");
            std::process::exit(2);
        }
    }

    let data_dir = cli_data_dir
        .or_else(|| env_nonempty("OPENRO_DATA_DIR").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("openro-data"));
    let _ = std::fs::create_dir_all(&data_dir);
    let saved_cfg = RuntimeConfig::load(&data_dir);

    let game_name = env_nonempty("OPENRO_GAME_NAME").unwrap_or_else(|| "Untitled Game".to_string());

    // Bind TCP listener early so HTTP health/UI responds in <50 ms while passive
    // Studio/gateway/plugin detection finishes asynchronously.
    let port: u16 = cli_port
        .or_else(|| std::env::var("OPENRO_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8077);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("bind 127.0.0.1:{port}: {e}"));
    let url = format!("http://127.0.0.1:{port}");

    // 1. Fast non-blocking model resolution (defer gateway network probes if needed).
    let env_base = env_nonempty("OPENRO_MODEL_BASE_URL");
    let env_key = env_nonempty("OPENRO_MODEL_API_KEY");
    let env_model = env_nonempty("OPENRO_MODEL_ID");
    let no_auto_model = env_flag("OPENRO_NO_AUTO_MODEL");
    let needs_gateway_probe = env_base.is_none()
        && env_key.is_none()
        && saved_cfg.provider.as_deref() != Some("antigravity")
        && saved_cfg.provider.as_deref() != Some("openai")
        && saved_cfg.base_url.is_none()
        && saved_cfg.api_key.is_none()
        && !no_auto_model
        && (env_flag("OPENRO_NO_AGY") || !openro_rs::model::agy_available());

    let model = if needs_gateway_probe {
        // Resolve quickly without stalling startup on TCP timeouts; probe_local_gateways
        // runs in the background right after listener bind.
        openro_rs::model::ModelClient::new(
            openro_rs::model::DEFAULT_BASE_URL.to_string(),
            env_model
                .clone()
                .unwrap_or_else(|| openro_rs::model::DEFAULT_MODEL_ID.to_string()),
            None,
        )
        .with_agy_dir(data_dir.join("agy-sandbox"))
    } else {
        model_from_config(&saved_cfg, Some(&data_dir))
    };

    let warmup_agy_models = model.is_antigravity();
    if warmup_agy_models {
        eprintln!("openro: model {} via Antigravity CLI", model.model_id());
    } else if model.is_loopback() {
        eprintln!("openro: connected local model {} @ {}", model.model_id(), model.base_url());
    } else if !model.is_configured() && !needs_gateway_probe {
        eprintln!(
            "openro: no model configured (set OPENRO_MODEL_API_KEY or pick a local gateway in Settings) — chat will refuse honestly"
        );
    }

    // 2. Studio / Place Backend resolution:
    //    explicit OPENRO_MCP_COMMAND stays synchronous (tests/CI); passive Vinegar/saved MCP
    //    detection + spawn runs asynchronously after listener bind so Cold Boot stays <50 ms.
    let no_local_place = env_flag("OPENRO_NO_LOCAL_PLACE");
    let explicit_mcp = env_nonempty("OPENRO_MCP_COMMAND");
    let saved_mode = saved_cfg.workspace_mode.as_deref().unwrap_or("auto").to_string();
    let saved_mcp_cmd = saved_cfg.mcp_command.clone();

    let mut defer_mcp_probe = false;
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
    } else if no_local_place {
        defer_mcp_probe = true;
        None
    } else {
        defer_mcp_probe = true;
        let place = LocalPlace::open(data_dir.clone(), game_name.clone());
        Some(PlaceBackend::Local(place))
    };

    let mut agent = match backend {
        Some(PlaceBackend::Local(place)) => Agent::with_local_place(model, place),
        other => Agent::with_backend(model, other),
    };

    eprintln!("openro: serving {url}");

    // Change sets persist next to the chats, so a restart does not
    // silently orphan every revert.
    agent.set_changes_file(data_dir.join("changes.json"));
    let shared = Shared::with_data_dir(Some(agent), game_name, data_dir.clone());
    // Defer passive Studio MCP spawn, Vinegar plugin install, local gateway probe, and agy CLI warmup
    // off the critical HTTP cold-boot path so Cold Boot stays fast even under CPU pinning.
    {
        let bg_shared = std::sync::Arc::clone(&shared);
        let bg_data_dir = data_dir.clone();
        let bg_env_model = env_model.clone();
        std::thread::spawn(move || {
            bg_shared.updater.start_background();
            std::thread::sleep(std::time::Duration::from_millis(200));
            openro_rs::server::prewarm_index_cache(&bg_shared);

            if warmup_agy_models {
                let _ = openro_rs::model::agy_models(false);
            }

            if needs_gateway_probe {
                if let Some(gw) = openro_rs::model::probe_local_gateways().into_iter().next() {
                    if let Ok(mut guard) = bg_shared.agent.lock() {
                        if let Some(a) = guard.as_mut() {
                            a.model = openro_rs::model::ModelClient::new(
                                gw.base_url.clone(),
                                bg_env_model.unwrap_or_else(|| gw.model_id.clone()),
                                None,
                            );
                        }
                    }
                    openro_rs::server::refresh_status_cache(&bg_shared);
                    eprintln!("openro: connected local model {} @ {}", gw.model_id, gw.base_url);
                }
            }

            if defer_mcp_probe {
                if let Some(cmd) = saved_mcp_cmd.or_else(McpClient::detect_vinegar_studio_mcp) {
                    if let Ok(mut client) = McpClient::spawn(&cmd) {
                        if client.has_live_studio() {
                            if let Ok(mut guard) = bg_shared.agent.lock() {
                                if let Some(a) = guard.as_mut() {
                                    a.studio = Some(PlaceBackend::Mcp(client));
                                    a.attach_studio_stream(std::sync::Arc::clone(&bg_shared.studio_stream));
                                }
                            }
                            openro_rs::server::refresh_status_cache(&bg_shared);
                            openro_rs::server::prewarm_index_cache(&bg_shared);
                            eprintln!("openro: Studio connected via {cmd}");
                        }
                    }
                }
            }

            if let Some(installed) = openro_rs::studio_plugin::maybe_auto_install_vinegar_plugin(
                &bg_shared.studio_plugin,
                &bg_data_dir,
                port,
            ) {
                eprintln!(
                    "openro: auto-installed {} into {}",
                    openro_rs::studio_plugin::COMPANION_PLUGIN_FILENAME,
                    installed.display()
                );
            }
        });
    }

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

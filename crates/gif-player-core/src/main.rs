use anyhow::{anyhow, Result};
use gif_player_core::{default_socket_path, serve, AnimationStore, WidgetManager};
use serde_json::json;
use std::path::PathBuf;

const DEFAULT_CACHE_BUDGET: usize = 160 * 1024 * 1024;
const DEFAULT_ASSET_BUDGET: usize = 64 * 1024 * 1024;

fn main() -> Result<()> {
    let mut socket = default_socket_path();
    let mut self_test = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--self-test" => self_test = true,
            "--socket" => {
                socket = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("--socket requires a path"))?,
                );
            }
            "-h" | "--help" => {
                println!("gif-playerd-beta [--socket PATH] [--self-test]");
                return Ok(());
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }

    if self_test {
        let manager = WidgetManager::default();
        let cache = AnimationStore::new(DEFAULT_CACHE_BUDGET, DEFAULT_ASSET_BUDGET);
        println!(
            "{}",
            json!({
                "ok": true,
                "component": "gif-player-core",
                "stage": "native-ipc-beta",
                "protocol": 2,
                "widgets": manager.len(),
                "cache_budget_bytes": cache.stats().budget,
                "max_asset_bytes": DEFAULT_ASSET_BUDGET,
                "socket": socket,
                "frontend_neutral": true
            })
        );
        return Ok(());
    }

    serve(&socket)
}

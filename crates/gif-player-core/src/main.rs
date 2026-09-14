use gif_player_core::{SharedCache, WidgetManager};
use serde_json::json;

const DEFAULT_CACHE_BUDGET: usize = 160 * 1024 * 1024;

fn main() {
    let self_test = std::env::args().any(|arg| arg == "--self-test");
    if self_test {
        let manager = WidgetManager::default();
        let cache: SharedCache<Vec<u8>> = SharedCache::new(DEFAULT_CACHE_BUDGET);
        println!(
            "{}",
            json!({
                "ok": true,
                "component": "gif-player-core",
                "stage": "beta-bootstrap",
                "protocol": 2,
                "widgets": manager.len(),
                "cache_budget_bytes": cache.stats().budget,
                "frontend_neutral": true
            })
        );
        return;
    }

    eprintln!(
        "gif-playerd-beta: native multi-surface daemon bootstrap is not complete; use --self-test for CI"
    );
    std::process::exit(2);
}

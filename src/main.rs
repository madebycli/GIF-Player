mod cli;
mod decode;
mod diagnostics;
mod frame_store;
mod ipc;
mod model;
mod noctalia;
mod paths;
mod plugin;
mod preview;
mod render;
mod store;
mod ui;
mod wayland;

fn main() {
    let result = (|| -> anyhow::Result<i32> {
        let args = cli::parse()?;
        if args.args[0] == "daemon" {
            cli::check_flags(&args.args, &["--headless", "--stay", "--no-restore"], &[])?;
            if let Err(error) = diagnostics::init(&args.paths) {
                eprintln!("gif-player diagnostics unavailable: {error:#}");
            }
            let headless = args.args.iter().any(|s| s == "--headless");
            let stay = args.args.iter().any(|s| s == "--stay");
            let restore = !args.args.iter().any(|s| s == "--no-restore");
            wayland::run(model::Engine::new(args.paths)?, stay, restore, headless)?;
            Ok(0)
        } else {
            cli::run(&args)
        }
    })();
    if let Err(ref error) = result {
        diag!(
            Error,
            "daemon",
            "fatal_error",
            serde_json::json!({"error":format!("{error:#}")})
        );
    } else {
        diag!(Info, "daemon", "shutdown", serde_json::json!({}));
    }
    let _ = diagnostics::flush();
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("gif-player: {error:#}");
            std::process::exit(1);
        }
    }
}

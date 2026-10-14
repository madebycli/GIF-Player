mod cli;
mod decode;
mod ipc;
mod model;
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
            let headless = args.args.iter().any(|s| s == "--headless");
            let stay = args.args.iter().any(|s| s == "--stay");
            let restore = !args.args.iter().any(|s| s == "--no-restore");
            wayland::run(model::Engine::new(args.paths)?, stay, restore, headless)?;
            Ok(0)
        } else {
            cli::run(&args)
        }
    })();
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("gif-player: {error:#}");
            std::process::exit(1);
        }
    }
}

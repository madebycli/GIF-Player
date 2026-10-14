use crate::{decode, ipc, paths::Paths};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    env,
    fs::OpenOptions,
    io::Write,
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Args {
    pub paths: Paths,
    pub args: Vec<String>,
}
pub fn parse() -> Result<Args> {
    let mut input = env::args().skip(1).collect::<Vec<_>>();
    let mut gif_dir = None;
    let mut i = 0;
    while i < input.len() {
        if input[i] == "--gif-dir" {
            ensure!(i + 1 < input.len(), "--gif-dir needs a path");
            gif_dir = Some(input.remove(i + 1));
            input.remove(i);
        } else if let Some(value) = input[i].strip_prefix("--gif-dir=") {
            gif_dir = Some(value.into());
            input.remove(i);
        } else {
            i += 1;
        }
    }
    let exe = env::args().next().unwrap_or_default();
    if input.is_empty() {
        input.push(
            if exe.ends_with("gif-control") {
                "control"
            } else {
                "picker"
            }
            .into(),
        );
    }
    let known = [
        "run",
        "ipc",
        "all",
        "list",
        "edit",
        "lock",
        "stop-all",
        "kill-all",
        "picker",
        "control",
        "app",
        "settings",
        "daemon",
        "doctor",
        "self-test",
        "request",
        "watch",
        "library",
        "catalog",
        "profile",
        "profiles",
        "bench",
        "preview",
        "install-plugin",
        "logs",
        "quit-daemon",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    if !known.contains(&input[0].as_str()) && !input[0].starts_with('-') {
        if input.len() == 1 {
            input.insert(0, "run".into());
        } else {
            input.insert(0, "ipc".into());
            if input[2] == "stop" {
                input[2] = "quit".into();
            }
        }
    }
    Ok(Args {
        paths: Paths::new(gif_dir.as_deref()),
        args: input,
    })
}
pub fn help() {
    println!(
        "Noctalia GIF Player — native Rust Wayland overlays\n\nUsage: gif-player [--gif-dir DIR] [COMMAND]\n\n  run GIF [--id ID] [--monitor N] [--state JSON]\n  ipc ID ACTION [ARGS]       all ACTION [ARGS]\n  list [--json]              edit | lock | stop-all | kill-all\n  picker | control | settings [--standalone]\n  app                       Normal xdg-shell application window\n  daemon [--headless] [--stay] [--no-restore]\n  profile list|save|apply|delete NAME | rename NAME NEW\n  library                   request JSON | watch [--reconnect]\n  doctor [--offline]         self-test | bench GIF\n  logs path | level LEVEL | export [NEW_DIRECTORY]\n  install-plugin [DEST] [--bar NAME | --no-bar]      quit-daemon\n\nWidget actions: status lock unlock toggle pause play resume reset quit\n  move X Y | move-by DX DY | corner tl/tr/bl/br/center | center\n  scale N | opacity N | speed N | flip none/h/v/hv/toggle-h/toggle-v\n  bounce | stop-bounce | hop | jump | jump-rate SECONDS\n  show | hide | visibility | canvas [true/false] | layer LAYER | output NAME\n\nWithout a command, open the picker. A GIF name launches a new instance.\nNoctalia 5 is optional; --standalone selects the Rust control surface."
    );
}
pub fn widget_command(id: &str, args: &[String]) -> Result<Value> {
    let action = args.first().context("Missing action")?;
    let mut value = json!({"action":action,"id":id});
    let keys: &[&str] = match action.as_str() {
        "move" => &["x", "y"],
        "move-by" => &["dx", "dy"],
        "scale" => &["scale"],
        "opacity" => &["opacity"],
        "speed" => &["speed"],
        "jump-rate" => &["seconds"],
        _ => &[],
    };
    if !keys.is_empty() {
        ensure!(
            args.len() == keys.len() + 1,
            "Wrong argument count for {action}"
        );
        for (i, key) in keys.iter().enumerate() {
            let n: f64 = args[i + 1]
                .parse()
                .with_context(|| format!("Invalid {key}"))?;
            ensure!(n.is_finite(), "Values must be finite");
            value[key] = json!(n);
        }
    } else {
        match action.as_str() {
            "corner" | "flip" | "layer" | "output" => {
                ensure!(args.len() == 2, "{action} needs one argument");
                let key = match action.as_str() {
                    "corner" => "position",
                    "flip" => "mode",
                    _ => action,
                };
                value[key] = json!(args[1]);
            }
            "canvas" => {
                if let Some(flag) = args.get(1) {
                    value["enabled"] = json!(flag.parse::<bool>()?);
                }
            }
            _ => ensure!(args.len() == 1, "Unexpected action arguments"),
        }
    }
    Ok(value)
}
pub fn ensure_daemon(paths: &Paths) -> Result<()> {
    if ipc::request(paths, &json!({"action":"ping"})).is_ok() {
        return Ok(());
    }
    paths.ensure_runtime()?;
    let logfile = paths.runtime_dir.join("daemon.log");
    if logfile.metadata().is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::rename(&logfile, paths.runtime_dir.join("daemon.log.1"));
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&logfile)?;
    let mut child = Command::new(env::current_exe()?)
        .arg("daemon")
        .arg("--gif-dir")
        .arg(&paths.gif_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(6) {
        if ipc::request(paths, &json!({"action":"ping"})).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!("Daemon exited ({status}); see {}", logfile.display());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("Daemon startup timed out; see {}", logfile.display())
}
pub fn run(args: &Args) -> Result<i32> {
    let a = &args.args;
    let paths = &args.paths;
    match a[0].as_str() {
        "logs" => return crate::diagnostics::cli(paths, a),
        "preview" => {
            let path = paths.resolve(a.get(1).context("preview needs a GIF")?)?;
            return print_result(crate::preview::generate(paths, &path)?);
        }
        "--help" | "-h" => {
            help();
            return Ok(0);
        }
        "--version" | "-V" => {
            println!("Noctalia GIF Player {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        "self-test" => {
            paths.ensure_runtime()?;
            let mut result = serde_json::to_value(paths)?;
            result["ok"] = json!(true);
            result["socket"] = json!(paths.socket());
            result["protocol"] = json!(2);
            result["runtime_mode"] = json!("0o700");
            result["engine"] = json!("rust");
            println!("{result}");
            return Ok(0);
        }
        "doctor" => {
            paths.ensure_runtime()?;
            let connected = ipc::request(paths, &json!({"action":"ping"})).is_ok();
            let protocols = if a.iter().any(|s| s == "--offline") {
                json!({"graphical_test":"skipped"})
            } else {
                crate::wayland::doctor()?
            };
            println!(
                "{}",
                json!({"ok":true,"paths":paths,"daemon_running":connected,"runtime":"native Rust / no GTK, Python or Quickshell","wayland":protocols,"fullscreen":"overlay stacking is compositor-dependent; see docs/acceptance.md"})
            );
            return Ok(0);
        }
        "install-plugin" => {
            let destination = a
                .get(1)
                .filter(|s| !s.starts_with("--"))
                .map(|s| crate::paths::expand(s))
                .unwrap_or_else(|| {
                    paths
                        .data_dir
                        .parent()
                        .unwrap()
                        .join("noctalia/plugins/gif-player")
                });
            crate::plugin::install(&destination)?;
            if !a.iter().any(|argument| argument == "--no-bar") {
                let selected = a
                    .iter()
                    .position(|argument| argument == "--bar")
                    .and_then(|index| a.get(index + 1));
                match crate::noctalia::attach_bar(selected.map(String::as_str)) {
                    Ok(Some(bar)) => println!(
                        "Enabled Noctalia GIF Player and attached its widget to bar '{bar}'."
                    ),
                    Ok(None) => {}
                    Err(error) => eprintln!("Plugin installed; bar attachment failed: {error:#}"),
                }
            }
            println!(
                "Plugin files installed in {}. Widget placement is available in Noctalia Settings → Bars.",
                destination.display()
            );
            return Ok(0);
        }
        "bench" => {
            let path = paths.resolve(a.get(1).context("bench needs a GIF")?)?;
            let start = Instant::now();
            let anim = decode::load(&path, decode::CACHE_LIMIT)?;
            let elapsed = start.elapsed();
            let start = Instant::now();
            let mut sum = 0;
            for i in 0..1_000_000 {
                sum += anim.frame_at(i as f64 * 1.37).0;
            }
            let lookup = start.elapsed();
            println!(
                "{}",
                json!({"gif":path,"frames":anim.frames.len(),"decoded_bytes":anim.bytes,"size":[anim.width,anim.height],"decode_ms":elapsed.as_secs_f64()*1000.0,"million_timeline_lookups_ms":lookup.as_secs_f64()*1000.0,"checksum":sum,"warning":anim.warning})
            );
            return Ok(0);
        }
        "watch" => {
            ipc::watch(
                paths,
                a.iter().any(|s| s == "--reconnect"),
                a.iter().any(|s| s == "--frontend"),
            )?;
            return Ok(0);
        }
        "picker" | "control" | "settings" | "app" => {
            if a[0] != "app" && !a.iter().any(|s| s == "--standalone") {
                let panel = format!("madebycli/gif-player:{}", a[0]);
                if Command::new("noctalia")
                    .args(["msg", "panel-toggle", &panel])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
                {
                    return Ok(0);
                }
            }
            ensure_daemon(paths)?;
            return print_result(ipc::request(
                paths,
                &json!({"action":if a[0] == "settings" { "settings-ui" } else { &a[0] }}),
            )?);
        }
        _ => {}
    }
    let command = match a[0].as_str() {
        "run" => {
            let path = paths.resolve(a.get(1).context("run needs a GIF")?)?;
            let mut value = json!({"action":"spawn","gif":path});
            let mut i = 2;
            while i < a.len() {
                ensure!(i + 1 < a.len(), "{} needs a value", a[i]);
                match a[i].as_str() {
                    "--id" => value["id"] = json!(a[i + 1]),
                    "--monitor" => value["monitor"] = json!(a[i + 1].parse::<u32>()?),
                    "--output" => value["output"] = json!(a[i + 1]),
                    "--state" => {
                        value["state"] = serde_json::from_str(&a[i + 1])?;
                        ensure!(value["state"].is_object(), "--state needs a JSON object");
                    }
                    _ => bail!("Unknown option {}", a[i]),
                }
                i += 2;
            }
            ensure_daemon(paths)?;
            value
        }
        "request" => {
            ensure!(a.len() == 2, "request needs one JSON argument");
            serde_json::from_str(&a[1])?
        }
        "ipc" => {
            ensure!(a.len() >= 3, "ipc needs ID and ACTION");
            widget_command(&a[1], &a[2..])?
        }
        "all" => widget_command("*", &a[1..])?,
        "list" => json!({"action":"list"}),
        "edit" => json!({"action":"unlock","id":"*"}),
        "lock" => json!({"action":"lock","id":"*"}),
        "stop-all" | "kill-all" => json!({"action":"stop-all"}),
        "quit-daemon" => json!({"action":"quit-daemon"}),
        "library" | "catalog" => {
            ensure_daemon(paths)?;
            json!({"action":"library"})
        }
        "profile" | "profiles" => {
            let operation = a.get(1).map(String::as_str).unwrap_or("list");
            let action = match operation {
                "list" => "profiles",
                "save" => "profile-save",
                "apply" => "profile-apply",
                "delete" => "profile-delete",
                "rename" => "profile-rename",
                _ => bail!("Unknown profile operation"),
            };
            let mut cmd = json!({"action":action});
            if action != "profiles" {
                cmd["name"] = json!(a.get(2).context("Missing profile name")?);
            }
            if action == "profile-rename" {
                cmd["new_name"] = json!(a.get(3).context("Missing new profile name")?);
            }
            ensure_daemon(paths)?;
            cmd
        }
        _ => bail!("Unknown command {}", a[0]),
    };
    let result = match ipc::request(paths, &command) {
        Ok(value) => value,
        Err(e) if ["list", "edit", "lock", "stop-all", "kill-all"].contains(&a[0].as_str()) => {
            if a[0] != "list" {
                println!("Keine Widgets aktiv");
            }
            let _ = e;
            return Ok(0);
        }
        Err(e) => return Err(e),
    };
    if a[0] == "list" && !a.iter().any(|s| s == "--json") {
        for w in result
            .get("widgets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            println!("{}", w["id"].as_str().unwrap_or("?"));
        }
        return Ok(0);
    }
    print_result(result)
}
pub(crate) fn print_result(result: Value) -> Result<i32> {
    let failed = result.get("error").is_some_and(|v| !v.is_null())
        || result.get("ok") == Some(&Value::Bool(false));
    println!("{result}");
    std::io::stdout().flush()?;
    Ok(i32::from(failed))
}

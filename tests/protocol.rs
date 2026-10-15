use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixStream,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Session {
    root: tempfile::TempDir,
    child: Child,
    socket: PathBuf,
    gif: PathBuf,
}
impl Session {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let gifs = root.path().join("gifs");
        fs::create_dir(&gifs).unwrap();
        let gif = gifs.join("spaces ' Unicode 鳥.gif");
        {
            let mut encoder = gif::Encoder::new(
                fs::File::create(&gif).unwrap(),
                3,
                2,
                &[0, 0, 0, 255, 0, 0, 0, 255, 0],
            )
            .unwrap();
            for i in 0..3 {
                encoder
                    .write_frame(&gif::Frame {
                        width: 3,
                        height: 2,
                        buffer: vec![if i % 2 == 0 { 1 } else { 2 }; 6].into(),
                        delay: 5,
                        ..Default::default()
                    })
                    .unwrap();
            }
        }
        // A genuine old state/profile document, with unrecognized fields that must survive.
        let legacy = root.path().join(".config/gif-widget");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("state.json"),serde_json::to_vec(&json!({"spaces ' Unicode 鳥":{"x":-33,"y":2222,"scale":1.25,"opacity":0.6,"custom":{"keep":true}}})).unwrap()).unwrap();
        fs::write(legacy.join("profiles.json"),serde_json::to_vec(&json!({"old":{"widgets":[{"gif":gif,"x":-200,"y":900,"jumping":true,"jump_rate":4}]}})).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_gif-player"));
        command
            .args(["daemon", "--headless", "--stay", "--no-restore"])
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("GIF_PLAYER_GIF_DIR", &gifs)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let socket = runtime.join("gif-player/daemon.sock");
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "daemon startup timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            root,
            child,
            socket,
            gif,
        }
    }
    fn request(&self, value: Value) -> Value {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        writeln!(stream, "{value}").unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn spawn(&self) -> Value {
        self.request(json!({"action":"spawn","gif":self.gif}))
    }
    fn wait_loaded(&self, id: &str) {
        let start = Instant::now();
        loop {
            let s = self.request(json!({"action":"status","id":id}));
            if s["loading"] == false {
                assert!(s.get("error").is_none(), "{s}");
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(mut socket) = UnixStream::connect(&self.socket) {
            let _ = socket.write_all(b"{\"action\":\"quit-daemon\"}\n");
        }
        let start = Instant::now();
        while self.child.try_wait().unwrap().is_none() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn legacy_protocol_and_shared_cache() {
    let s = Session::new();
    assert_eq!(s.request(json!({"action":"ping"}))["protocol"], 2);
    let first = s.spawn();
    let id = first["id"].as_str().unwrap();
    s.wait_loaded(id);
    assert_eq!(
        s.request(json!({"action":"status","id":id}))["base_position"],
        json!([-33.0, 2222.0])
    );
    let bytes = s.request(json!({"action":"snapshot"}))["cache_bytes"].clone();
    let duplicate = s.spawn();
    assert_ne!(first["id"], duplicate["id"]);
    assert_eq!(
        s.request(json!({"action":"snapshot"}))["cache_bytes"],
        bytes
    );
    for (action, params) in [
        ("unlock", json!({})),
        ("lock", json!({})),
        ("toggle", json!({})),
        ("pause", json!({})),
        ("play", json!({})),
        ("move", json!({"x":-300,"y":8000})),
        ("move-by", json!({"dx":11,"dy":-22})),
        ("scale", json!({"scale":1.7})),
        ("opacity", json!({"opacity":0.4})),
        ("speed", json!({"speed":2.5})),
        ("flip", json!({"mode":"both"})),
        ("bounce", json!({})),
        ("stop-bounce", json!({})),
        ("hop", json!({})),
        ("jump", json!({})),
        ("jump-rate", json!({"seconds":3})),
        ("corner", json!({"position":"center"})),
        ("hide", json!({})),
        ("show", json!({})),
        ("canvas", json!({"enabled":true})),
        ("layer", json!({"layer":"top"})),
        ("reset", json!({})),
    ] {
        let mut cmd = params;
        cmd["action"] = json!(action);
        cmd["id"] = json!(id);
        let result = s.request(cmd);
        assert_eq!(result["ok"], true, "{action}: {result}");
    }
    assert!(
        s.request(json!({"action":"move","id":id,"x":null,"y":3}))
            .get("error")
            .is_some()
    );
    assert!(
        s.request(json!({"action":"nonsense","id":id}))
            .get("error")
            .is_some()
    );
    assert_eq!(
        s.request(json!({"action":"pause","id":"*"}))["results"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(s.request(json!({"action":"stop-all"}))["stopped"], 2);
    assert_eq!(s.request(json!({"action":"snapshot"}))["cache_bytes"], 0);
    s.request(json!({"action":"quit-daemon"}));
    let start = Instant::now();
    while s.child.id() != 0
        && !s
            .root
            .path()
            .join("config/gif-player/session.json")
            .exists()
    {
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn profile_atomicity_and_persistence() {
    let mut s = Session::new();
    let id = s.spawn()["id"].as_str().unwrap().to_string();
    s.wait_loaded(&id);
    assert_eq!(
        s.request(json!({"action":"profile-save","name":"new"}))["ok"],
        true
    );
    assert_eq!(
        s.request(json!({"action":"profile-rename","name":"new","new_name":"renamed"}))["ok"],
        true
    );
    assert_eq!(
        s.request(json!({"action":"profiles"}))["profiles"]["old"]["widgets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let failed =
        s.request(json!({"action":"apply-setup","widgets":[{"gif":"/does/not/exist.gif"}]}));
    assert!(failed.get("error").is_some());
    assert_eq!(
        s.request(json!({"action":"list"}))["widgets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.request(json!({"action":"profile-apply","name":"old"}))["applied"],
        1
    );
    let status = s.request(json!({"action":"list"}));
    assert_eq!(
        status["widgets"][0]["base_position"],
        json!([-200.0, 900.0])
    );
    s.request(json!({"action":"quit-daemon"}));
    s.child.wait().unwrap();
    let state: Value = serde_json::from_slice(
        &fs::read(s.root.path().join("config/gif-player/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["spaces ' Unicode 鳥"]["custom"]["keep"], true);
    assert!(
        s.root
            .path()
            .join("config/gif-player/state.json.legacy-backup")
            .exists()
    );
    assert!(s.root.path().join(".config/gif-widget/state.json").exists());
    assert_eq!(
        fs::metadata(s.socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
#[test]
fn slow_client_does_not_block_daemon() {
    let s = Session::new();
    let mut slow = UnixStream::connect(&s.socket).unwrap();
    slow.write_all(b"{\"action\":").unwrap();
    let now = Instant::now();
    assert_eq!(s.request(json!({"action":"ping"}))["ok"], true);
    assert!(now.elapsed() < Duration::from_secs(1));
}
#[test]
fn runtime_symlinks_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.path().join("gif-player")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_gif-player"))
        .arg("self-test")
        .env("XDG_RUNTIME_DIR", root.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Unsafe directory"));
}

#[test]
fn native_theme_updates_validate_and_persist() {
    let session = Session::new();
    let result = session.request(json!({"action":"appearance","palette":{"primary":"#cbb9ff","surface":"#121217"},"font_family":"monospace"}));
    assert_eq!(result["ok"], true);
    let result = session.request(json!({"action":"appearance","palette":{"primary":"#bad"}}));
    assert_eq!(result["ok"], false);
    let snapshot = session.request(json!({"action":"snapshot"}));
    assert_eq!(
        snapshot["settings"]["appearance"]["palette"]["primary"],
        "#cbb9ff"
    );
    assert_eq!(
        snapshot["settings"]["appearance"]["font_family"],
        "monospace"
    );
}

#[test]
fn detailed_logs_export_live_and_after_shutdown() {
    let mut session = Session::new();
    assert_eq!(
        session.request(json!({"action":"logs-level","level":"trace"}))["ok"],
        true
    );
    assert_eq!(
        session.request(json!({"action":"logs-level","level":"invalid"}))["ok"],
        false
    );
    let spawned=session.request(json!({"action":"spawn","gif":session.gif,"state":{"password":"DO_NOT_EXPORT_PRIVATE_FIELD","custom_token":"DO_NOT_EXPORT_PRIVATE_FIELD"}}));
    let id = spawned["id"].as_str().unwrap();
    session.wait_loaded(id);
    session.request(json!({"action":"pause","id":id}));
    let report = session.root.path().join("report with spaces 鳥");
    let response = session.request(json!({"action":"logs-export","directory":report}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["directory"], json!(report));
    let started = Instant::now();
    loop {
        assert_eq!(session.request(json!({"action":"ping"}))["ok"], true);
        let state = session.request(json!({"action":"logs-export-status","directory":report}));
        if state["pending"] != true {
            assert_eq!(state["ok"], true, "{state}");
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::metadata(&report).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for name in [
        "manifest.json",
        "snapshot.json",
        "system.json",
        "configuration.json",
    ] {
        let file = report.join(name);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        let data = fs::read_to_string(file).unwrap();
        let _: Value = serde_json::from_str(&data).unwrap();
        assert!(!data.contains("DO_NOT_EXPORT_PRIVATE_FIELD"));
    }
    let logs = fs::read_to_string(report.join("daemon.jsonl")).unwrap();
    assert!(!logs.contains("DO_NOT_EXPORT_PRIVATE_FIELD"));
    let records = logs
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect::<Vec<_>>();
    for event in [
        "logging_started",
        "listening",
        "job_finished",
        "completed",
        "export_requested",
    ] {
        assert!(
            records.iter().any(|r| r["event"] == event),
            "missing {event}: {logs}"
        );
    }
    assert!(records.iter().any(|r| r["level"] == "trace"));
    let failed = session.request(json!({"action":"logs-export","directory":report}));
    assert_eq!(failed["ok"], false);
    session.request(json!({"action":"quit-daemon"}));
    session.child.wait().unwrap();
    let offline = session.root.path().join("offline report");
    let result = Command::new(env!("CARGO_BIN_EXE_gif-player"))
        .arg("logs")
        .arg("export")
        .arg(&offline)
        .env("HOME", session.root.path())
        .env("XDG_RUNTIME_DIR", session.root.path().join("runtime"))
        .env("XDG_CONFIG_HOME", session.root.path().join("config"))
        .env("XDG_STATE_HOME", session.root.path().join("state"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(offline.join("daemon.jsonl").exists());
    let snapshot: Value =
        serde_json::from_slice(&fs::read(offline.join("snapshot.json")).unwrap()).unwrap();
    assert_eq!(snapshot["connected"], false);
    let settings: Value = serde_json::from_slice(
        &fs::read(session.root.path().join("config/gif-player/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["log_level"], "trace");
}

fn isolated(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gif-player"))
        .args(args)
        .env("HOME", root)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("LANG", "en_US.UTF-8")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}
#[test]
fn command_help_never_runs_the_command_and_typos_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("runtime")).unwrap();
    fs::set_permissions(
        root.path().join("runtime"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    for command in ["install-plugin", "daemon", "picker", "app", "doctor", "run"] {
        let output = isolated(root.path(), &[command, "--help"]);
        assert!(output.status.success(), "{command} --help: {output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: gif-player"));
    }
    assert!(
        !root.path().join("data/noctalia").exists(),
        "--help must not install the plugin"
    );
    assert!(
        !root.path().join("runtime/gif-player/daemon.sock").exists(),
        "--help must not start a daemon"
    );
    let output = isolated(root.path(), &["install-plugin", "--hlep"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown option --hlep"));
    assert!(
        !root.path().join("data/noctalia").exists(),
        "a typo must not install anything"
    );
    let output = isolated(root.path(), &["daemon", "--headles"]);
    assert!(
        !output.status.success(),
        "a misspelled daemon flag must not start a daemon"
    );
    let output = isolated(root.path(), &["install-plugin", "--bar"]);
    assert!(!output.status.success());
    let dest = root.path().join("plugin copy");
    let output = isolated(
        root.path(),
        &["install-plugin", dest.to_str().unwrap(), "--no-bar"],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(dest.join("plugin.toml").exists());
}

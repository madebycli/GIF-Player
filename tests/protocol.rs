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

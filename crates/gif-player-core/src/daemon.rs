use crate::manager::{ManagedWidget, WidgetManager};
use crate::model::{
    PlayerState, DEFAULT_JUMP_RATE, DEFAULT_OPACITY, DEFAULT_SCALE, DEFAULT_SPEED, DEFAULT_X,
    DEFAULT_Y,
};
use crate::protocol::Request;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const MAX_REQUEST_BYTES: usize = 1024 * 1024;

pub struct DaemonCore {
    manager: WidgetManager,
}

pub struct DispatchOutcome {
    pub response: Value,
    pub shutdown: bool,
}

impl Default for DaemonCore {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonCore {
    pub fn new() -> Self {
        Self {
            manager: WidgetManager::default(),
        }
    }

    pub fn dispatch(&mut self, request: Request) -> DispatchOutcome {
        let response = match request.action.as_str() {
            "ping" => json!({
                "ok": true,
                "daemon": true,
                "widgets": self.manager.len(),
                "renderer": "native-wayland-beta"
            }),
            "list" => json!({"ok": true, "widgets": self.manager.statuses()}),
            "spawn" => self.spawn(&request),
            "stop-all" => {
                let stopped = self.manager.clear();
                json!({"ok": true, "stopped": stopped})
            }
            "quit-daemon" => {
                let stopped = self.manager.clear();
                return DispatchOutcome {
                    response: json!({"ok": true, "daemon": false, "stopped": stopped}),
                    shutdown: true,
                };
            }
            _ => self.widget_dispatch(&request),
        };
        DispatchOutcome {
            response,
            shutdown: false,
        }
    }

    fn spawn(&mut self, request: &Request) -> Value {
        let Some(path) = request.gif.as_ref() else {
            return json!({"error": "spawn requires gif"});
        };
        if !path.is_file() {
            return json!({"error": format!("GIF not found: {}", path.display())});
        }
        let file = match path.canonicalize() {
            Ok(path) => path,
            Err(error) => return json!({"error": format!("cannot resolve GIF: {error}")}),
        };
        let mut state = request.state.clone().unwrap_or_default();
        state.normalize();
        match self.manager.spawn(
            file,
            request.id.clone(),
            state,
            request.output.clone(),
            request.monitor,
        ) {
            Ok(widget) => json!(widget.status()),
            Err(error) => json!({"error": error}),
        }
    }

    fn widget_dispatch(&mut self, request: &Request) -> Value {
        let Some(id) = request.id.as_deref() else {
            return json!({"error": format!("action '{}' requires id", request.action)});
        };
        if id == "*" {
            let ids: Vec<String> = self
                .manager
                .statuses()
                .into_iter()
                .map(|status| status.id)
                .collect();
            let mut results = Map::new();
            for id in ids {
                let value = self.widget_action(&id, request);
                results.insert(id, value);
            }
            return json!({"ok": true, "results": results});
        }
        self.widget_action(id, request)
    }

    fn widget_action(&mut self, id: &str, request: &Request) -> Value {
        if request.action == "quit" {
            return if self.manager.remove(id).is_some() {
                json!({"ok": true, "shutdown": true})
            } else {
                json!({"error": format!("No widget '{id}' running")})
            };
        }

        let Some(widget) = self.manager.get_mut(id) else {
            return json!({"error": format!("No widget '{id}' running")});
        };
        match apply_state_action(widget, request) {
            Ok(()) => json!(widget.status()),
            Err(error) => json!({"error": error.to_string()}),
        }
    }
}

fn apply_state_action(widget: &mut ManagedWidget, request: &Request) -> Result<()> {
    match request.action.as_str() {
        "status" => {}
        "lock" => widget.state.locked = true,
        "unlock" => widget.state.locked = false,
        "toggle" => widget.state.locked = !widget.state.locked,
        "pause" => widget.state.paused = true,
        "play" => widget.state.paused = false,
        "move" => {
            let x = number(request, "x")?;
            let y = number(request, "y")?;
            widget.state.x = x;
            widget.state.y = y;
            widget.state.bouncing = false;
        }
        "move-by" => {
            widget.state.x += number(request, "dx")?;
            widget.state.y += number(request, "dy")?;
            widget.state.bouncing = false;
        }
        "scale" => {
            widget.state.scale = number(request, "scale")?;
            widget.state.normalize();
        }
        "opacity" => {
            widget.state.opacity = number(request, "opacity")?;
            widget.state.normalize();
        }
        "flip" => match text(request, "mode")?.to_ascii_lowercase().as_str() {
            "none" => {
                widget.state.flip_h = false;
                widget.state.flip_v = false;
            }
            "h" => {
                widget.state.flip_h = true;
                widget.state.flip_v = false;
            }
            "v" => {
                widget.state.flip_h = false;
                widget.state.flip_v = true;
            }
            "hv" | "vh" | "both" => {
                widget.state.flip_h = true;
                widget.state.flip_v = true;
            }
            "toggle-h" => widget.state.flip_h = !widget.state.flip_h,
            "toggle-v" => widget.state.flip_v = !widget.state.flip_v,
            other => return Err(anyhow!("unknown flip mode: {other}")),
        },
        "speed" => {
            widget.state.speed = number(request, "speed")?;
            widget.state.normalize();
        }
        "bounce" => widget.state.bouncing = !widget.state.bouncing,
        "stop-bounce" => widget.state.bouncing = false,
        "hop" => {}
        "jump" => widget.state.jumping = !widget.state.jumping,
        "jump-rate" => {
            widget.state.jump_rate = number(request, "seconds")?;
            widget.state.normalize();
        }
        "reset" => reset_state(&mut widget.state),
        "corner" => return Err(anyhow!("corner requires attached output geometry")),
        other => return Err(anyhow!("unknown action: {other}")),
    }
    Ok(())
}

fn reset_state(state: &mut PlayerState) {
    let locked = state.locked;
    state.x = DEFAULT_X;
    state.y = DEFAULT_Y;
    state.scale = DEFAULT_SCALE;
    state.opacity = DEFAULT_OPACITY;
    state.speed = DEFAULT_SPEED;
    state.flip_h = false;
    state.flip_v = false;
    state.paused = false;
    state.bouncing = false;
    state.jumping = false;
    state.jump_rate = DEFAULT_JUMP_RATE;
    state.locked = locked;
}

fn number(request: &Request, key: &str) -> Result<f64> {
    let value = request
        .extra
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("missing numeric parameter: {key}"))?;
    if !value.is_finite() {
        return Err(anyhow!("non-finite parameter: {key}"));
    }
    Ok(value)
}

fn text<'a>(request: &'a Request, key: &str) -> Result<&'a str> {
    request
        .extra
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string parameter: {key}"))
}

pub fn default_socket_path() -> PathBuf {
    let uid = unsafe { libc::geteuid() };
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(base) if Path::new(&base).is_absolute() => {
            PathBuf::from(base).join("gif-player").join("daemon.sock")
        }
        _ => PathBuf::from(format!("/tmp/gif-player-{uid}/daemon.sock")),
    }
}

pub fn serve(socket_path: &Path) -> Result<()> {
    let parent = socket_path
        .parent()
        .ok_or_else(|| anyhow!("socket path has no parent"))?;
    ensure_private_runtime_dir(parent)?;
    remove_stale_socket(socket_path)?;

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("bind Unix socket {}", socket_path.display()))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;

    let mut core = DaemonCore::new();
    for connection in listener.incoming() {
        let shutdown = match connection {
            Ok(mut stream) => handle_connection(&mut core, &mut stream),
            Err(error) => return Err(error).context("accept Unix socket connection"),
        }?;
        if shutdown {
            break;
        }
    }
    let _ = fs::remove_file(socket_path);
    Ok(())
}

fn handle_connection(core: &mut DaemonCore, stream: &mut UnixStream) -> Result<bool> {
    let bytes = read_bounded_request(stream)?;
    if bytes.is_none() {
        return write_response(
            stream,
            json!({"error": "request exceeds 1 MiB limit"}),
            false,
        );
    }
    let bytes = bytes.expect("bounded request checked above");
    let request: Request = serde_json::from_slice(&bytes).context("parse IPC request")?;
    let outcome = core.dispatch(request);
    write_response(stream, outcome.response, outcome.shutdown)
}

fn read_bounded_request(stream: &UnixStream) -> Result<Option<Vec<u8>>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut bytes = Vec::with_capacity(4096);
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(take) > MAX_REQUEST_BYTES {
            return Ok(None);
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    Ok(Some(bytes))
}

fn write_response(stream: &mut UnixStream, response: Value, shutdown: bool) -> Result<bool> {
    serde_json::to_writer(&mut *stream, &response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(shutdown)
}

fn ensure_private_runtime_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(anyhow!("unsafe runtime directory: {}", path.display()));
    }
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid {
        return Err(anyhow!(
            "runtime directory {} belongs to uid {}, expected {}",
            path.display(),
            metadata.uid(),
            uid
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn remove_stale_socket(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("inspect existing socket path"),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(anyhow!(
            "refusing to replace non-socket path: {}",
            path.display()
        ));
    }
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_gif_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("gif-player-{stamp}.gif"));
        fs::write(&path, b"GIF89a").expect("create fake GIF path");
        path
    }

    #[test]
    fn ping_and_list_use_ipc_v2_shapes() {
        let mut core = DaemonCore::new();
        let ping: Request = serde_json::from_value(json!({"action": "ping"})).expect("request");
        assert_eq!(core.dispatch(ping).response["ok"], true);
        let list: Request = serde_json::from_value(json!({"action": "list"})).expect("request");
        assert_eq!(core.dispatch(list).response["widgets"], json!([]));
    }

    #[test]
    fn spawn_duplicate_and_move_preserve_free_positions() {
        let path = test_gif_path();
        let mut core = DaemonCore::new();
        let first: Request =
            serde_json::from_value(json!({"action": "spawn", "gif": path})).expect("spawn request");
        let first_id = core.dispatch(first).response["id"]
            .as_str()
            .expect("first id")
            .to_string();
        let second: Request =
            serde_json::from_value(json!({"action": "spawn", "gif": path})).expect("spawn request");
        let second_id = core.dispatch(second).response["id"]
            .as_str()
            .expect("second id")
            .to_string();
        assert_eq!(second_id, format!("{first_id}-2"));

        let request: Request = serde_json::from_value(json!({
            "action": "move",
            "id": first_id,
            "x": -500.0,
            "y": 4000.0
        }))
        .expect("move request");
        let response = core.dispatch(request).response;
        assert_eq!(response["x"], -500.0);
        assert_eq!(response["y"], 4000.0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn oversized_request_is_rejected_without_unbounded_buffering() {
        let mut core = DaemonCore::new();
        let (mut client, mut server) = UnixStream::pair().expect("socket pair");
        let writer = std::thread::spawn(move || {
            client
                .write_all(&vec![b'x'; MAX_REQUEST_BYTES + 1])
                .expect("write request");
            client.write_all(b"\n").expect("write newline");
            let mut line = String::new();
            BufReader::new(client)
                .read_line(&mut line)
                .expect("read response");
            line
        });
        handle_connection(&mut core, &mut server).expect("handle request");
        let response = writer.join().expect("writer thread");
        assert!(response.contains("request exceeds 1 MiB limit"));
    }
}

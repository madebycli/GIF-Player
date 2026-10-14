use crate::{model::Engine, paths::Paths};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    time::{Duration, Instant},
};

pub const MAX_MESSAGE: usize = 1024 * 1024;
const MAX_REPLY: usize = 32 * 1024 * 1024;
pub struct Peer {
    socket: UnixStream,
    input: Vec<u8>,
    output: Vec<u8>,
    written: usize,
    pub watch: bool,
    frontend: bool,
    closing: bool,
    since: Instant,
    revision: u64,
}
pub struct Server {
    listener: UnixListener,
    pub peers: Vec<Peer>,
    _lock: File,
    paths: Paths,
}
impl Server {
    pub fn bind(paths: &Paths) -> Result<Self> {
        paths.ensure_runtime()?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(paths.runtime_dir.join("daemon.lock"))?;
        // The descriptor owns the advisory lock until Server is dropped.
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Daemon already running"
        );
        if let Ok(info) = fs::symlink_metadata(paths.socket()) {
            ensure!(
                info.file_type().is_socket(),
                "Refusing to replace a non-socket runtime file"
            );
            fs::remove_file(paths.socket())?;
        }
        let listener = UnixListener::bind(paths.socket())?;
        listener.set_nonblocking(true)?;
        fs::set_permissions(paths.socket(), fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            peers: Vec::new(),
            _lock: lock,
            paths: paths.clone(),
        })
    }
    pub fn fds(&self) -> Vec<(RawFd, i16)> {
        std::iter::once((self.listener.as_raw_fd(), libc::POLLIN))
            .chain(self.peers.iter().map(|p| {
                (
                    p.socket.as_raw_fd(),
                    libc::POLLIN
                        | if p.written < p.output.len() {
                            libc::POLLOUT
                        } else {
                            0
                        },
                )
            }))
            .collect()
    }
    pub fn service(&mut self, engine: &mut Engine) {
        while let Ok((socket, _)) = self.listener.accept() {
            if self.peers.len() >= 128 {
                continue;
            }
            if socket.set_nonblocking(true).is_err() {
                continue;
            }
            self.peers.push(Peer {
                socket,
                input: Vec::new(),
                output: Vec::new(),
                written: 0,
                watch: false,
                frontend: false,
                closing: false,
                since: Instant::now(),
                revision: 0,
            });
        }
        for peer in &mut self.peers {
            if !peer.watch && !peer.closing {
                let mut chunk = [0; 8192];
                loop {
                    match peer.socket.read(&mut chunk) {
                        Ok(0) => {
                            peer.closing = true;
                            break;
                        }
                        Ok(n) => {
                            peer.input.extend_from_slice(&chunk[..n]);
                            if peer.input.len() > MAX_MESSAGE {
                                peer.closing = true;
                                break;
                            }
                            if peer.input.contains(&b'\n') {
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => {
                            peer.closing = true;
                            break;
                        }
                    }
                }
                if let Some(end) = peer.input.iter().position(|b| *b == b'\n') {
                    let result = match serde_json::from_slice::<Value>(&peer.input[..end]) {
                        Ok(cmd) => {
                            peer.watch = cmd.get("action").and_then(Value::as_str) == Some("watch");
                            peer.frontend =
                                cmd.get("frontend").and_then(Value::as_bool) == Some(true);
                            engine.dispatch(&cmd)
                        }
                        Err(e) => json!({"error":format!("Invalid request: {e}")}),
                    };
                    peer.output = encode(&result);
                    peer.written = 0;
                    peer.closing = !peer.watch;
                    peer.revision = engine.revision;
                    peer.input.clear();
                }
            }
            if peer.watch && peer.revision != engine.revision {
                // Coalesce changes and bound backpressure; never block the Wayland thread.
                if peer.written == peer.output.len() {
                    peer.output = encode(&engine.snapshot());
                    peer.written = 0;
                    peer.revision = engine.revision;
                    peer.since = Instant::now();
                }
            }
            while peer.written < peer.output.len() {
                match peer.socket.write(&peer.output[peer.written..]) {
                    Ok(0) => {
                        peer.watch = false;
                        peer.closing = true;
                        peer.output.clear();
                        break;
                    }
                    Ok(n) => peer.written += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        peer.watch = false;
                        peer.closing = true;
                        peer.output.clear();
                        break;
                    }
                }
            }
            if peer.watch {
                let mut probe = [0; 1];
                match peer.socket.read(&mut probe) {
                    Ok(0) => {
                        peer.watch = false;
                        peer.closing = true;
                    }
                    Err(e) if e.kind() != std::io::ErrorKind::WouldBlock => {
                        peer.watch = false;
                        peer.closing = true;
                    }
                    _ => {}
                }
            }
        }
        self.peers.retain(|p| {
            !(p.closing && p.written >= p.output.len())
                && (p.watch || p.since.elapsed() < Duration::from_secs(5))
                && (p.written >= p.output.len() || p.since.elapsed() < Duration::from_secs(10))
        });
        engine.managed_frontend = self.peers.iter().any(|p| p.watch && p.frontend);
    }
    pub fn timeout(&self) -> Option<Duration> {
        self.peers
            .iter()
            .filter(|p| !p.watch || p.written < p.output.len())
            .map(|p| {
                Duration::from_secs(if p.watch { 10 } else { 5 }).saturating_sub(p.since.elapsed())
            })
            .min()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.paths.socket());
    }
}
fn encode(value: &Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(value).unwrap();
    line.push(b'\n');
    line
}
pub fn request(paths: &Paths, cmd: &Value) -> Result<Value> {
    let mut stream = UnixStream::connect(paths.socket()).context("Daemon not running")?;
    stream.set_read_timeout(Some(Duration::from_secs(6)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&encode(cmd))?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(MAX_REPLY as u64)
        .read_line(&mut line)?;
    ensure!(!line.is_empty(), "Daemon closed connection without a reply");
    Ok(serde_json::from_str(&line)?)
}
pub fn watch(paths: &Paths, reconnect: bool, frontend: bool) -> Result<()> {
    let mut first = true;
    loop {
        let result = (|| -> Result<()> {
            let mut stream = UnixStream::connect(paths.socket())?;
            stream.write_all(&encode(&json!({"action":"watch","frontend":frontend})))?;
            let mut reader = BufReader::new(stream);
            loop {
                let mut line = String::new();
                let n = reader
                    .by_ref()
                    .take(MAX_REPLY as u64)
                    .read_line(&mut line)?;
                ensure!(n > 0 && line.ends_with('\n'), "Daemon disconnected");
                let _: Value = serde_json::from_str(&line)?;
                std::io::stdout().write_all(line.as_bytes())?;
                std::io::stdout().flush()?;
                first = false;
            }
        })();
        if !reconnect {
            return result;
        }
        if first || result.is_err() {
            println!(
                "{}",
                json!({"connected":false,"error":"Daemon not running"})
            );
            std::io::stdout().flush()?;
        }
        first = false;
        std::thread::sleep(Duration::from_secs(2));
    }
}
pub fn poll(fds: &[(RawFd, i16)], timeout: Option<Duration>) -> Result<Vec<i16>> {
    let mut descriptors = fds
        .iter()
        .map(|(fd, events)| libc::pollfd {
            fd: *fd,
            events: *events,
            revents: 0,
        })
        .collect::<Vec<_>>();
    let ms = timeout.map_or(-1, |t| {
        t.as_millis()
            .saturating_add(u128::from(t.subsec_nanos() % 1_000_000 != 0))
            .min(i32::MAX as u128) as i32
    });
    // poll receives only owned/live descriptors and a valid contiguous array.
    let result = unsafe {
        libc::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as libc::nfds_t,
            ms,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
    Ok(descriptors.into_iter().map(|p| p.revents).collect())
}

mod gif;
mod model;

use anyhow::{anyhow, Context, Result};
use gif::Animation;
use image::{imageops::resize, imageops::FilterType, RgbaImage};
use model::{
    bounce_axis, fully_inside, jump_offset, random_jump_delay, scaled_size, snap_axis, PlayerState,
    DEFAULT_JUMP_RATE, DEFAULT_OPACITY, DEFAULT_SCALE, DEFAULT_SPEED, DEFAULT_X, DEFAULT_Y,
    HOP_DURATION, HOP_HEIGHT,
};
use serde::Deserialize;
use serde_json::{json, Value};
use smithay_client_toolkit::reexports::calloop::{channel, EventLoop};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_RIGHT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DRAG_THRESHOLD: f64 = 4.0;
const BOUNCE_SPEED: f64 = 360.0;
const INPUT_PAD: i32 = 4;

#[derive(Debug, Deserialize)]
struct Startup {
    id: String,
    gif: PathBuf,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    monitor: Option<usize>,
    #[serde(default)]
    state: PlayerState,
}

struct Request {
    value: Value,
    reply: mpsc::SyncSender<Value>,
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    layer: Option<LayerSurface>,
    pointer: Option<wl_pointer::WlPointer>,
    qh: QueueHandle<App>,

    id: String,
    gif_path: PathBuf,
    output_name: Option<String>,
    animation: Animation,
    state: PlayerState,

    configured_width: u32,
    configured_height: u32,
    bounds_width: u32,
    bounds_height: u32,
    canvas_mode: bool,
    first_configure: bool,
    dirty: bool,
    exit: bool,

    frame_index: usize,
    frame_deadline: Instant,
    bounce_vx: f64,
    bounce_vy: f64,
    last_motion_tick: Instant,
    hop_started: Option<Instant>,
    next_jump: Option<Instant>,

    drag_pending: bool,
    dragging: bool,
    drag_press: (f64, f64),
    drag_origin: (f64, f64),
    drag_was_bouncing: bool,
    last_left_click: Option<Instant>,
}

impl App {
    fn selected_output(&self, wanted: Option<&str>, monitor: Option<usize>) -> Option<wl_output::WlOutput> {
        if let Some(wanted) = wanted {
            for output in self.output_state.outputs() {
                if self
                    .output_state
                    .info(&output)
                    .and_then(|info| info.name)
                    .as_deref()
                    == Some(wanted)
                {
                    return Some(output);
                }
            }
        }
        if let Some(index) = monitor {
            return self.output_state.outputs().nth(index);
        }
        self.output_state.outputs().next()
    }

    fn initialize_layer(&mut self, output: Option<&str>, monitor: Option<usize>) {
        let selected = self.selected_output(output, monitor);
        if let Some(ref output) = selected {
            if let Some(info) = self.output_state.info(output) {
                self.output_name = info.name.clone();
                if let Some((width, height)) = info.logical_size {
                    if width > 0 && height > 0 {
                        self.bounds_width = width as u32;
                        self.bounds_height = height as u32;
                    }
                }
            }
        }

        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some(format!("gif-player-{}", self.id)),
            selected.as_ref(),
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_margin(0, 0, 0, 0);
        layer.commit();
        self.layer = Some(layer);
    }

    fn gif_size(&self) -> (u32, u32) {
        scaled_size(self.animation.width, self.animation.height, self.state.scale)
    }

    fn hop_offset(&self, now: Instant) -> f64 {
        let Some(started) = self.hop_started else {
            return 0.0;
        };
        let progress = now.duration_since(started).as_secs_f64() / HOP_DURATION;
        jump_offset(progress, HOP_HEIGHT)
    }

    fn wanted_canvas(&self) -> bool {
        let (width, height) = self.gif_size();
        !self.state.locked
            || self.state.bouncing
            || self.hop_started.is_some()
            || self.dragging
            || !fully_inside(
                self.state.x,
                self.state.y,
                width as f64,
                height as f64,
                self.bounds_width as f64,
                self.bounds_height as f64,
            )
    }

    fn configure_surface(&mut self) {
        let wanted = self.wanted_canvas();
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        let (gif_w, gif_h) = self.gif_size();
        if wanted {
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_margin(0, 0, 0, 0);
        } else {
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_size(gif_w, gif_h);
            layer.set_margin(
                self.state.y.round() as i32,
                0,
                0,
                self.state.x.round() as i32,
            );
        }
        let changed = wanted != self.canvas_mode;
        self.canvas_mode = wanted;
        self.update_input_region(false);
        layer.commit();
        self.dirty = true;
        if changed {
            self.configured_width = if wanted { self.bounds_width.max(1) } else { gif_w };
            self.configured_height = if wanted { self.bounds_height.max(1) } else { gif_h };
        }
    }

    fn update_input_region(&self, full_drag: bool) {
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        let region = self.compositor.wl_compositor().create_region(&self.qh, ());
        if !self.state.locked {
            if full_drag {
                region.add(
                    0,
                    0,
                    self.configured_width.max(1) as i32,
                    self.configured_height.max(1) as i32,
                );
            } else {
                let (width, height) = self.gif_size();
                let (x, y) = if self.canvas_mode {
                    (
                        self.state.x.floor() as i32 - INPUT_PAD,
                        (self.state.y - self.hop_offset(Instant::now())).floor() as i32 - INPUT_PAD,
                    )
                } else {
                    (-INPUT_PAD, -INPUT_PAD)
                };
                region.add(
                    x,
                    y,
                    width as i32 + INPUT_PAD * 2,
                    height as i32 + INPUT_PAD * 2,
                );
            }
        }
        layer.wl_surface().set_input_region(Some(&region));
        region.destroy();
    }

    fn point_in_gif(&self, point: (f64, f64)) -> bool {
        let (width, height) = self.gif_size();
        let (x, y) = if self.canvas_mode {
            (self.state.x, self.state.y - self.hop_offset(Instant::now()))
        } else {
            (0.0, 0.0)
        };
        point.0 >= x - INPUT_PAD as f64
            && point.0 <= x + width as f64 + INPUT_PAD as f64
            && point.1 >= y - INPUT_PAD as f64
            && point.1 <= y + height as f64 + INPUT_PAD as f64
    }

    fn start_bounce(&mut self) {
        if self.state.bouncing {
            return;
        }
        let angle = match fastrand::usize(0..4) {
            0 => 0.5,
            1 => std::f64::consts::PI - 0.5,
            2 => std::f64::consts::PI + 0.5,
            _ => std::f64::consts::TAU - 0.5,
        } + fastrand::f64() * 0.6 - 0.3;
        self.bounce_vx = angle.cos() * BOUNCE_SPEED;
        self.bounce_vy = angle.sin() * BOUNCE_SPEED;
        self.state.bouncing = true;
        self.last_motion_tick = Instant::now();
        self.configure_surface();
    }

    fn stop_bounce(&mut self) {
        self.state.bouncing = false;
        self.configure_surface();
    }

    fn start_hop(&mut self) {
        if self.hop_started.is_none() {
            self.hop_started = Some(Instant::now());
            self.configure_surface();
        }
    }

    fn schedule_jump(&mut self) {
        if self.state.jumping {
            self.next_jump = Some(
                Instant::now() + Duration::from_secs_f64(random_jump_delay(self.state.jump_rate)),
            );
        } else {
            self.next_jump = None;
        }
    }

    fn animation_delay(&self, index: usize) -> Duration {
        Duration::from_secs_f64(
            (self.animation.duration(index) as f64 / 1000.0) / self.state.speed.max(0.1),
        )
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_motion_tick).as_secs_f64().min(0.05);
        self.last_motion_tick = now;

        if self.state.bouncing {
            let (width, height) = self.gif_size();
            let x = bounce_axis(
                self.state.x,
                self.bounce_vx,
                dt,
                self.bounds_width as f64,
                width as f64,
            );
            let y = bounce_axis(
                self.state.y,
                self.bounce_vy,
                dt,
                self.bounds_height as f64,
                height as f64,
            );
            self.state.x = x.position;
            self.state.y = y.position;
            self.bounce_vx = x.velocity;
            self.bounce_vy = y.velocity;
            self.dirty = true;
            if !self.state.locked {
                self.update_input_region(false);
            }
        }

        if let Some(started) = self.hop_started {
            if now.duration_since(started).as_secs_f64() >= HOP_DURATION {
                self.hop_started = None;
                self.configure_surface();
            } else {
                self.dirty = true;
                if !self.state.locked {
                    self.update_input_region(false);
                }
            }
        }

        if self.state.jumping && self.next_jump.is_some_and(|deadline| now >= deadline) {
            if !self.dragging {
                self.start_hop();
            }
            self.schedule_jump();
        }

        if !self.state.paused && self.animation.frame_count() > 1 {
            let mut advanced = 0;
            while now >= self.frame_deadline && advanced < 8 {
                self.frame_index = (self.frame_index + 1) % self.animation.frame_count();
                self.frame_deadline += self.animation_delay(self.frame_index);
                advanced += 1;
            }
            if now >= self.frame_deadline {
                self.frame_deadline = now + self.animation_delay(self.frame_index);
            }
            if advanced > 0 {
                self.dirty = true;
            }
        }

        if self.dirty && !self.first_configure {
            if let Err(error) = self.draw() {
                eprintln!("gif-player-renderer draw failed: {error:#}");
                self.exit = true;
            }
        }
    }

    fn draw(&mut self) -> Result<()> {
        let Some(layer) = self.layer.as_ref() else {
            return Ok(());
        };
        let width = self.configured_width.max(1);
        let height = self.configured_height.max(1);
        let stride = width as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
            .context("create Wayland SHM buffer")?;
        canvas.fill(0);

        let (gif_w, gif_h) = self.gif_size();
        let source = &self.animation.frames[self.frame_index].pixels;
        let scaled: RgbaImage = if source.width() == gif_w && source.height() == gif_h {
            source.clone()
        } else {
            resize(source, gif_w, gif_h, FilterType::Triangle)
        };
        let hop = self.hop_offset(Instant::now());
        let origin_x = if self.canvas_mode { self.state.x.round() as i32 } else { 0 };
        let origin_y = if self.canvas_mode {
            (self.state.y - hop).round() as i32
        } else {
            0
        };
        composite(
            canvas,
            width,
            height,
            &scaled,
            origin_x,
            origin_y,
            self.state.opacity,
            self.state.flip_h,
            self.state.flip_v,
        );

        layer.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
        buffer
            .attach_to(layer.wl_surface())
            .context("attach Wayland SHM buffer")?;
        layer.commit();
        self.dirty = false;
        Ok(())
    }

    fn set_scale(&mut self, value: f64) {
        let old = self.gif_size();
        let center_x = self.state.x + old.0 as f64 / 2.0;
        let center_y = self.state.y + old.1 as f64 / 2.0;
        self.state.scale = value.clamp(0.1, 5.0);
        let new = self.gif_size();
        self.state.x = center_x - new.0 as f64 / 2.0;
        self.state.y = center_y - new.1 as f64 / 2.0;
        self.configure_surface();
    }

    fn set_position(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() || self.dragging {
            return;
        }
        if self.state.bouncing {
            self.stop_bounce();
        }
        self.state.x = x;
        self.state.y = y;
        self.configure_surface();
        self.dirty = true;
    }

    fn go_corner(&mut self, position: &str) -> Result<()> {
        let margin = 20.0;
        let (width, height) = self.gif_size();
        let max_x = self.bounds_width as f64 - width as f64;
        let max_y = self.bounds_height as f64 - height as f64;
        let (x, y) = match position {
            "tl" => (margin, margin),
            "tr" => (max_x - margin, margin),
            "bl" => (margin, max_y - margin),
            "br" => (max_x - margin, max_y - margin),
            "center" => (max_x / 2.0, max_y / 2.0),
            _ => return Err(anyhow!("unknown corner: {position}")),
        };
        self.set_position(x, y);
        Ok(())
    }

    fn reset(&mut self) {
        self.state.x = DEFAULT_X;
        self.state.y = DEFAULT_Y;
        self.state.scale = DEFAULT_SCALE;
        self.state.opacity = DEFAULT_OPACITY;
        self.state.speed = DEFAULT_SPEED;
        self.state.flip_h = false;
        self.state.flip_v = false;
        self.state.paused = false;
        self.state.bouncing = false;
        self.state.jumping = false;
        self.state.jump_rate = DEFAULT_JUMP_RATE;
        self.frame_index = 0;
        self.hop_started = None;
        self.next_jump = None;
        self.frame_deadline = Instant::now() + self.animation_delay(0);
        self.configure_surface();
        self.dirty = true;
    }

    fn status(&self) -> Value {
        let (width, height) = self.gif_size();
        json!({
            "ok": true,
            "id": self.id,
            "x": self.state.x.round() as i64,
            "y": self.state.y.round() as i64,
            "scale": round3(self.state.scale),
            "locked": self.state.locked,
            "paused": self.state.paused,
            "opacity": round3(self.state.opacity),
            "flip_h": self.state.flip_h,
            "flip_v": self.state.flip_v,
            "speed": round3(self.state.speed),
            "bouncing": self.state.bouncing,
            "jumping": self.state.jumping,
            "jump_rate": (self.state.jump_rate * 100.0).round() / 100.0,
            "size": [width, height],
            "screen": [self.bounds_width, self.bounds_height],
            "frames": [self.animation.frame_count(), self.animation.frame_count()],
            "loading": false,
            "file": self.gif_path,
            "output": self.output_name,
        })
    }

    fn command(&mut self, command: Value) -> Value {
        let Some(action) = command.get("action").and_then(Value::as_str) else {
            return json!({"error": "missing action"});
        };
        let result: Result<()> = (|| {
            match action {
                "status" => return Ok(()),
                "quit" => self.exit = true,
                "lock" => {
                    self.state.locked = true;
                    self.drag_pending = false;
                    self.dragging = false;
                    self.configure_surface();
                }
                "unlock" => {
                    self.state.locked = false;
                    self.configure_surface();
                }
                "toggle" => {
                    self.state.locked = !self.state.locked;
                    self.configure_surface();
                }
                "pause" => self.state.paused = true,
                "play" => {
                    self.state.paused = false;
                    self.frame_deadline = Instant::now() + self.animation_delay(self.frame_index);
                }
                "move" => self.set_position(number(&command, "x")?, number(&command, "y")?),
                "move-by" => {
                    let x = self.state.x + number(&command, "dx")?;
                    let y = self.state.y + number(&command, "dy")?;
                    self.set_position(x, y);
                }
                "scale" => self.set_scale(number(&command, "scale")?),
                "corner" => self.go_corner(text(&command, "position")?)?,
                "opacity" => {
                    self.state.opacity = number(&command, "opacity")?.clamp(0.05, 1.0);
                    self.dirty = true;
                }
                "flip" => {
                    match text(&command, "mode")?.to_ascii_lowercase().as_str() {
                        "none" => { self.state.flip_h = false; self.state.flip_v = false; }
                        "h" => { self.state.flip_h = true; self.state.flip_v = false; }
                        "v" => { self.state.flip_h = false; self.state.flip_v = true; }
                        "hv" | "vh" | "both" => { self.state.flip_h = true; self.state.flip_v = true; }
                        "toggle-h" => self.state.flip_h = !self.state.flip_h,
                        "toggle-v" => self.state.flip_v = !self.state.flip_v,
                        other => return Err(anyhow!("unknown flip mode: {other}")),
                    }
                    self.dirty = true;
                }
                "speed" => {
                    self.state.speed = number(&command, "speed")?.clamp(0.1, 10.0);
                    self.frame_deadline = Instant::now() + self.animation_delay(self.frame_index);
                }
                "bounce" => {
                    if self.state.bouncing { self.stop_bounce(); } else { self.start_bounce(); }
                }
                "stop-bounce" => self.stop_bounce(),
                "hop" => self.start_hop(),
                "jump" => {
                    self.state.jumping = !self.state.jumping;
                    self.schedule_jump();
                }
                "jump-rate" => {
                    self.state.jump_rate = number(&command, "seconds")?.clamp(0.5, 60.0);
                    self.schedule_jump();
                }
                "reset" => self.reset(),
                other => return Err(anyhow!("unknown action: {other}")),
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.status(),
            Err(error) => json!({"error": error.to_string()}),
        }
    }

    fn open_noctalia_panel() {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "madebycli/gif-player:manager"])
            .spawn();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let width = configure.new_size.0;
        let height = configure.new_size.1;
        if self.canvas_mode {
            if width > 0 && height > 0 {
                self.bounds_width = width;
                self.bounds_height = height;
            }
            self.configured_width = if width > 0 { width } else { self.bounds_width.max(1) };
            self.configured_height = if height > 0 { height } else { self.bounds_height.max(1) };
        } else {
            let (gif_w, gif_h) = self.gif_size();
            self.configured_width = if width > 0 { width } else { gif_w };
            self.configured_height = if height > 0 { height } else { gif_h };
        }
        self.first_configure = false;
        self.update_input_region(false);
        self.dirty = true;
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(pointer);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        for event in events {
            if &event.surface != layer.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    if self.state.locked || !self.point_in_gif(event.position) {
                        continue;
                    }
                    let now = Instant::now();
                    if self.last_left_click.is_some_and(|previous| now.duration_since(previous) <= DOUBLE_CLICK) {
                        self.last_left_click = None;
                        self.drag_pending = false;
                        Self::open_noctalia_panel();
                        continue;
                    }
                    self.last_left_click = Some(now);
                    self.drag_was_bouncing = self.state.bouncing;
                    if self.state.bouncing {
                        self.stop_bounce();
                    }
                    self.hop_started = None;
                    self.drag_pending = true;
                    self.drag_press = event.position;
                    self.drag_origin = (self.state.x, self.state.y);
                }
                PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                    if !self.state.locked {
                        self.state.locked = true;
                        self.drag_pending = false;
                        self.dragging = false;
                        self.configure_surface();
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if self.drag_pending && !self.dragging {
                        let dx = event.position.0 - self.drag_press.0;
                        let dy = event.position.1 - self.drag_press.1;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            self.dragging = true;
                            self.last_left_click = None;
                            self.update_input_region(true);
                        }
                    }
                    if self.dragging {
                        let (width, height) = self.gif_size();
                        let x = self.drag_origin.0 + event.position.0 - self.drag_press.0;
                        let y = self.drag_origin.1 + event.position.1 - self.drag_press.1;
                        self.state.x = snap_axis(x, width as f64, self.bounds_width as f64);
                        self.state.y = snap_axis(y, height as f64, self.bounds_height as f64);
                        self.dirty = true;
                    }
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.drag_pending = false;
                    if self.dragging {
                        self.dragging = false;
                        self.update_input_region(false);
                        self.configure_surface();
                    }
                    if self.drag_was_bouncing {
                        self.drag_was_bouncing = false;
                        self.start_bounce();
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    if self.state.locked || !self.point_in_gif(event.position) {
                        continue;
                    }
                    let amount = if vertical.value120 != 0 {
                        vertical.value120 as f64 / 120.0
                    } else if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        vertical.absolute.signum()
                    };
                    if amount != 0.0 {
                        let factor = 1.05_f64.powf(-amount);
                        self.set_scale(self.state.scale * factor);
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

delegate_registry!(App);
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(App);

fn composite(
    target: &mut [u8],
    target_width: u32,
    target_height: u32,
    source: &RgbaImage,
    origin_x: i32,
    origin_y: i32,
    opacity: f64,
    flip_h: bool,
    flip_v: bool,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    for sy in 0..source.height() as i32 {
        let dy = origin_y + sy;
        if dy < 0 || dy >= target_height as i32 { continue; }
        for sx in 0..source.width() as i32 {
            let dx = origin_x + sx;
            if dx < 0 || dx >= target_width as i32 { continue; }
            let source_x = if flip_h { source.width() as i32 - 1 - sx } else { sx } as u32;
            let source_y = if flip_v { source.height() as i32 - 1 - sy } else { sy } as u32;
            let pixel = source.get_pixel(source_x, source_y).0;
            let alpha = ((pixel[3] as f64 * opacity).round() as u32).min(255);
            let red = (pixel[0] as u32 * alpha + 127) / 255;
            let green = (pixel[1] as u32 * alpha + 127) / 255;
            let blue = (pixel[2] as u32 * alpha + 127) / 255;
            let offset = ((dy as u32 * target_width + dx as u32) * 4) as usize;
            target[offset] = blue as u8;
            target[offset + 1] = green as u8;
            target[offset + 2] = red as u8;
            target[offset + 3] = alpha as u8;
        }
    }
}

fn number<'a>(value: &'a Value, key: &str) -> Result<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
        .ok_or_else(|| anyhow!("missing or invalid parameter: {key}"))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing parameter: {key}"))
}

fn round3(value: f64) -> f64 { (value * 1000.0).round() / 1000.0 }

fn spawn_stdin_thread(sender: channel::Sender<Request>) {
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdout = io::stdout().lock();
        for line in BufReader::new(stdin.lock()).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    eprintln!("gif-player-renderer stdin failed: {error}");
                    break;
                }
            };
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(error) => {
                    let _ = writeln!(stdout, "{}", json!({"error": format!("bad request: {error}")}));
                    let _ = stdout.flush();
                    continue;
                }
            };
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            if sender.send(Request { value, reply: reply_tx }).is_err() {
                break;
            }
            match reply_rx.recv() {
                Ok(reply) => {
                    let _ = writeln!(stdout, "{reply}");
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
    });
}

fn parse_startup() -> Result<Startup> {
    let mut args = env::args().skip(1);
    let mut config_json = None;
    while let Some(arg) = args.next() {
        if arg == "--config-json" {
            config_json = args.next();
        }
    }
    let config_json = config_json.ok_or_else(|| anyhow!("usage: gif-player-renderer --config-json JSON"))?;
    let mut startup: Startup = serde_json::from_str(&config_json).context("invalid startup JSON")?;
    startup.state.normalize();
    if !startup.gif.is_file() {
        return Err(anyhow!("GIF not found: {}", startup.gif.display()));
    }
    Ok(startup)
}

fn main() -> Result<()> {
    let startup = parse_startup()?;
    let animation = Animation::load(Path::new(&startup.gif))?;
    let conn = Connection::connect_to_env().context("connect to Wayland compositor")?;
    let (globals, mut event_queue) = registry_queue_init(&conn).context("initialize Wayland registry")?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor is unavailable")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell is unavailable")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is unavailable")?;
    let pool = SlotPool::new(4 * 1024 * 1024, &shm).context("create Wayland SHM pool")?;
    let now = Instant::now();
    let initial_delay = Duration::from_secs_f64(
        (animation.duration(0) as f64 / 1000.0) / startup.state.speed.max(0.1),
    );

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        shm,
        pool,
        layer: None,
        pointer: None,
        qh: qh.clone(),
        id: startup.id,
        gif_path: startup.gif,
        output_name: None,
        animation,
        state: startup.state,
        configured_width: 1,
        configured_height: 1,
        bounds_width: 1920,
        bounds_height: 1080,
        canvas_mode: true,
        first_configure: true,
        dirty: true,
        exit: false,
        frame_index: 0,
        frame_deadline: now + initial_delay,
        bounce_vx: 0.0,
        bounce_vy: 0.0,
        last_motion_tick: now,
        hop_started: None,
        next_jump: None,
        drag_pending: false,
        dragging: false,
        drag_press: (0.0, 0.0),
        drag_origin: (0.0, 0.0),
        drag_was_bouncing: false,
        last_left_click: None,
    };

    event_queue.roundtrip(&mut app).context("read Wayland outputs")?;
    app.initialize_layer(startup.output.as_deref(), startup.monitor);
    if app.state.bouncing {
        app.state.bouncing = false;
        app.start_bounce();
    }
    app.schedule_jump();

    let mut event_loop: EventLoop<App> = EventLoop::try_new().context("create event loop")?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .context("insert Wayland source")?;
    let (request_tx, request_rx) = channel::channel::<Request>();
    event_loop
        .handle()
        .insert_source(request_rx, |event, _, app| match event {
            channel::Event::Msg(request) => {
                let response = app.command(request.value);
                let _ = request.reply.send(response);
            }
            channel::Event::Closed => app.exit = true,
        })
        .context("insert command source")?;
    spawn_stdin_thread(request_tx);

    while !app.exit {
        event_loop
            .dispatch(Duration::from_millis(16), &mut app)
            .context("dispatch renderer event loop")?;
        app.tick();
    }
    Ok(())
}

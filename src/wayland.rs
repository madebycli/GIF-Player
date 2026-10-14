use crate::{
    ipc::{self, Server},
    model::{Engine, Output},
    render::{self, Item, Rect, Scene, Signature},
    ui::Ui,
};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Read,
    os::fd::{AsFd, AsRawFd},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use wayland_client::{
    Connection, EventQueue, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
pub fn signals() {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            stop_signal as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, stop_signal as *const () as libc::sighandler_t);
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Spec {
    key: String,
    output: String,
    layer: String,
    canvas: bool,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    widgets: Vec<String>,
    ui: bool,
}
struct Swap {
    buffer: Buffer,
    scene: Scene,
    initialized: bool,
}
enum Presentation {
    Layer(LayerSurface),
    Window(Window),
}
impl Presentation {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Self::Layer(layer) => layer.wl_surface(),
            Self::Window(window) => window.wl_surface(),
        }
    }
    fn commit(&self) {
        match self {
            Self::Layer(layer) => layer.commit(),
            Self::Window(window) => window.commit(),
        }
    }
    fn set_size(&self, width: u32, height: u32) {
        if let Self::Layer(layer) = self {
            layer.set_size(width, height);
        }
    }
    fn set_layer(&self, value: Layer) {
        if let Self::Layer(layer) = self {
            layer.set_layer(value);
        }
    }
    fn set_margin(&self, top: i32, right: i32, bottom: i32, left: i32) {
        if let Self::Layer(layer) = self {
            layer.set_margin(top, right, bottom, left);
        }
    }
    fn is_layer(&self, other: &LayerSurface) -> bool {
        matches!(self, Self::Layer(layer) if layer == other)
    }
}
struct Surface {
    spec: Spec,
    layer: Presentation,
    pool: SlotPool,
    buffers: Vec<Swap>,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    scale: f64,
    configured: bool,
    ready: bool,
    needs_draw: bool,
    presented: bool,
    presented_scene: Scene,
    width: u32,
    height: u32,
    frames: u64,
    input: Vec<Rect>,
}
impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(f) = &self.fractional {
            f.destroy();
        }
        if let Some(v) = &self.viewport {
            v.destroy();
        }
    }
}
struct ProtocolData;
struct FractionalData(wl_surface::WlSurface);
struct Drag {
    id: String,
    start: (f64, f64),
    origin: (f64, f64),
    pointer: wl_pointer::WlPointer,
}
pub struct State {
    pub engine: Engine,
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    xdg_shell: Option<XdgShell>,
    shm: Shm,
    viewporter: Option<WpViewporter>,
    fractional: Option<WpFractionalScaleManagerV1>,
    surfaces: BTreeMap<String, Surface>,
    pointers: HashMap<wl_seat::WlSeat, wl_pointer::WlPointer>,
    keyboards: HashMap<wl_seat::WlSeat, wl_keyboard::WlKeyboard>,
    ui: Ui,
    drag: Option<Drag>,
    ui_focus: bool,
    modifiers: Modifiers,
    output_dirty: bool,
    last_click: Option<(String, u32)>,
}
fn layer(name: &str) -> Layer {
    match name {
        "background" => Layer::Background,
        "bottom" => Layer::Bottom,
        "top" => Layer::Top,
        _ => Layer::Overlay,
    }
}
pub fn connect(engine: Engine) -> Result<(Connection, EventQueue<State>, State)> {
    let connection = Connection::connect_to_env()
        .context("A running Wayland session is required (WAYLAND_DISPLAY)")?;
    let (globals, queue) = registry_queue_init::<State>(&connection)?;
    crate::diag!(
        Info,
        "wayland",
        "connected",
        json!({"globals":globals.contents().clone_list().iter().map(|g|json!({"interface":g.interface,"version":g.version})).collect::<Vec<_>>()})
    );
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).context("Compositor does not support wlr-layer-shell")?;
    let shm = Shm::bind(&globals, &qh)?;
    let viewporter = globals
        .bind::<WpViewporter, _, _>(&qh, 1..=1, ProtocolData)
        .ok();
    let fractional = if viewporter.is_some() {
        globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ProtocolData)
            .ok()
    } else {
        None
    };
    let state = State {
        engine,
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        xdg_shell: XdgShell::bind(&globals, &qh).ok(),
        shm,
        viewporter,
        fractional,
        surfaces: BTreeMap::new(),
        pointers: HashMap::new(),
        keyboards: HashMap::new(),
        ui: Ui::new(),
        drag: None,
        ui_focus: false,
        modifiers: Modifiers::default(),
        output_dirty: true,
        last_click: None,
    };
    Ok((connection, queue, state))
}
pub fn doctor() -> Result<serde_json::Value> {
    let connection = Connection::connect_to_env()
        .context("WAYLAND_DISPLAY is unavailable; use doctor --offline for filesystem checks")?;
    let (globals, _queue) = registry_queue_init::<State>(&connection)?;
    let list = globals.contents().clone_list();
    ensure!(
        list.iter().any(|g| g.interface == "zwlr_layer_shell_v1"),
        "wlr-layer-shell is unavailable"
    );
    Ok(
        json!({"connected":true,"globals":list.iter().map(|g|json!({"interface":g.interface,"version":g.version})).collect::<Vec<_>>()}),
    )
}
pub fn run(mut engine: Engine, stay: bool, restore: bool, headless: bool) -> Result<()> {
    signals();
    crate::diag!(
        Info,
        "daemon",
        "starting",
        json!({"headless":headless,"stay":stay,"restore":restore})
    );
    let mut server = Server::bind(&engine.paths)?;
    if headless {
        if restore {
            engine.restore()?;
        }
        loop {
            engine.tick();
            server.service(&mut engine);
            if engine.quit || STOP.load(Ordering::Relaxed) {
                break;
            }
            let mut fds = server.fds();
            fds.push((engine.wake.as_raw_fd(), libc::POLLIN));
            let mut timeout = engine
                .deadline
                .map(|d| Duration::from_secs_f64((d - engine.now()).max(0.0)));
            if let Some(peer) = server.timeout() {
                timeout = Some(timeout.map_or(peer, |deadline| deadline.min(peer)));
            }
            ipc::poll(&fds, timeout)?;
            let mut bytes = [0; 128];
            let _ = engine.wake.read(&mut bytes);
        }
        engine.save()?;
        return Ok(());
    }
    let (connection, mut queue, mut state) = connect(engine)?;
    let qh = queue.handle();
    queue.roundtrip(&mut state)?;
    state.refresh_outputs();
    if restore {
        state.engine.restore()?;
    }
    let mut empty_since = Some(state.engine.now());
    let mut ever_active = false;
    loop {
        queue.dispatch_pending(&mut state)?;
        if state.output_dirty {
            state.refresh_outputs();
        }
        state.engine.tick();
        ever_active |= !state.engine.widgets.is_empty();
        server.service(&mut state.engine);
        if let Some(mode) = state.engine.ui_request.take() {
            if mode == "close" {
                state.ui.open = false;
            } else if let Err(e) = state.ui.show(&mode, &mut state.engine) {
                state.engine.error = Some(e.to_string());
                state.engine.revision += 1;
            }
        }
        state.ui.sync(&state.engine);
        state.ui.request_hover(&mut state.engine);
        state.ui.prepare(&mut state.engine);
        state.sync_surfaces(&qh)?;
        state.draw(&qh)?;
        if state.engine.quit || STOP.load(Ordering::Relaxed) {
            break;
        }
        if state.engine.widgets.is_empty() && !state.ui.open {
            let now = state.engine.now();
            let since = *empty_since.get_or_insert(now);
            if !stay
                && !crate::diagnostics::export_busy()
                && now - since > if ever_active { 2.0 } else { 10.0 }
            {
                break;
            }
        } else {
            empty_since = None;
        }
        let mut timeout = state
            .engine
            .deadline
            .map(|d| Duration::from_secs_f64((d - state.engine.now()).max(0.0)));
        if state
            .surfaces
            .values()
            .filter(|s| !s.spec.ui)
            .all(|s| !s.ready || !s.configured)
        {
            timeout = state
                .engine
                .next_save
                .map(|d| Duration::from_secs_f64((d - state.engine.now()).max(0.0)));
        }
        if let Some(peer) = server.timeout() {
            timeout = Some(timeout.map_or(peer, |t| t.min(peer)));
        }
        if !stay && !crate::diagnostics::export_busy() {
            if let Some(since) = empty_since {
                let empty = Duration::from_secs_f64(
                    (since + if ever_active { 2.0 } else { 10.0 } - state.engine.now()).max(0.0),
                );
                timeout = Some(timeout.map_or(empty, |t| t.min(empty)));
            }
        }
        if let Some(animation) = state.ui.preview_deadline(&state.engine) {
            timeout = Some(timeout.map_or(animation, |t| t.min(animation)));
            state.ui.dirty = true;
        }
        let Some(read) = queue.prepare_read() else {
            continue;
        };
        match connection.flush() {
            Ok(()) => {}
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        let mut fds = vec![
            (connection.as_fd().as_raw_fd(), libc::POLLIN),
            (state.engine.wake.as_raw_fd(), libc::POLLIN),
        ];
        fds.extend(server.fds());
        let events = ipc::poll(&fds, timeout)?;
        if events[0] & libc::POLLIN != 0 {
            read.read()?;
        } else {
            drop(read);
        }
        if events[1] & libc::POLLIN != 0 {
            let mut bytes = [0; 128];
            while state.engine.wake.read(&mut bytes).is_ok() {}
        }
    }
    state.engine.save()?;
    connection.flush()?;
    Ok(())
}
impl State {
    fn refresh_outputs(&mut self) {
        let outputs = self
            .output
            .outputs()
            .filter_map(|proxy| self.output.info(&proxy))
            .map(|info| {
                let mode = info
                    .modes
                    .iter()
                    .find(|m| m.current)
                    .or_else(|| info.modes.first());
                let scale = info.scale_factor.max(1);
                let (w, h) = info.logical_size.unwrap_or_else(|| {
                    mode.map(|m| (m.dimensions.0 / scale, m.dimensions.1 / scale))
                        .unwrap_or((1920, 1080))
                });
                Output {
                    name: info.name.unwrap_or_else(|| format!("output-{}", info.id)),
                    width: w.max(1) as u32,
                    height: h.max(1) as u32,
                    scale: mode
                        .map(|m| {
                            ((m.dimensions.0 as f64 * m.dimensions.1 as f64)
                                / (w.max(1) as f64 * h.max(1) as f64))
                                .sqrt()
                        })
                        .filter(|scale| scale.is_finite() && *scale >= 1.0)
                        .unwrap_or(scale as f64),
                }
            })
            .collect::<Vec<_>>();
        if serde_json::to_value(&outputs).ok() != serde_json::to_value(&self.engine.outputs).ok() {
            crate::diag!(
                Info,
                "wayland",
                "outputs_changed",
                json!({"previous":self.engine.outputs,"current":outputs})
            );
            self.engine.outputs = outputs;
            self.engine.revision += 1;
            for s in self.surfaces.values_mut() {
                s.needs_draw = true;
            }
        }
        self.output_dirty = false;
    }
    fn output_proxy(&self, name: &str) -> Option<wl_output::WlOutput> {
        self.output.outputs().find(|p| {
            self.output.info(p).is_some_and(|i| {
                i.name.as_deref() == Some(name) || format!("output-{}", i.id) == name
            })
        })
    }
    fn specs(&self) -> BTreeMap<String, Spec> {
        let mut specs: BTreeMap<String, Spec> = BTreeMap::new();
        for widget in self.engine.widgets.values() {
            if !widget.settings.visible || widget.settings.opacity == 0.0 {
                continue;
            }
            let Some(output) = self
                .engine
                .outputs
                .iter()
                .find(|o| o.name == widget.settings.output)
                .or_else(|| self.engine.outputs.first())
            else {
                continue;
            };
            let canvas = widget.canvas();
            let (key, x, y, width, height) = if canvas {
                (
                    format!("canvas:{}:{}", output.name, widget.settings.layer),
                    0,
                    0,
                    output.width,
                    output.height,
                )
            } else {
                let rect = Rect {
                    x: widget.position.0.floor() as i32,
                    y: widget.position.1.floor() as i32,
                    w: widget.size.0 as i32,
                    h: widget.size.1 as i32,
                };
                let Some(clip) = rect.clip(output.width as i32, output.height as i32) else {
                    continue;
                };
                (
                    format!("widget:{}:{}", widget.id, output.name),
                    clip.x,
                    clip.y,
                    clip.w as u32,
                    clip.h as u32,
                )
            };
            let spec = specs.entry(key.clone()).or_insert(Spec {
                key,
                output: output.name.clone(),
                layer: widget.settings.layer.clone(),
                canvas,
                x,
                y,
                width,
                height,
                widgets: Vec::new(),
                ui: false,
            });
            spec.widgets.push(widget.id.clone());
        }
        if self.ui.open {
            if let Some(output) = self.engine.outputs.first() {
                specs.insert(
                    if self.ui.windowed { "app" } else { "ui" }.into(),
                    Spec {
                        key: if self.ui.windowed { "app" } else { "ui" }.into(),
                        output: output.name.clone(),
                        layer: "overlay".into(),
                        canvas: false,
                        x: 0,
                        y: 0,
                        width: 900.min(output.width.saturating_sub(32)).max(320),
                        height: 740.min(output.height.saturating_sub(32)).max(320),
                        widgets: Vec::new(),
                        ui: true,
                    },
                );
            }
        }
        specs
    }
    fn sync_surfaces(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        let specs = self.specs();
        for (key, spec) in &specs {
            if !self.surfaces.contains_key(key) {
                crate::diag!(
                    Debug,
                    "surface",
                    "created",
                    json!({"key":key,"output":spec.output,"layer":spec.layer,"position":[spec.x,spec.y],"size":[spec.width,spec.height],"ui":spec.ui,"role":if spec.key=="app"{"xdg-toplevel"}else{"layer-shell"},"widgets":spec.widgets})
                );
                let output = self.output_proxy(&spec.output);
                let surface = self.compositor.create_surface(qh);
                let layer = if spec.key == "app" {
                    let shell = self
                        .xdg_shell
                        .as_ref()
                        .context("xdg-shell is required for the application window")?;
                    let window =
                        shell.create_window(surface.clone(), WindowDecorations::RequestServer, qh);
                    window.set_title("Noctalia GIF Player");
                    window.set_app_id("io.github.madebycli.NoctaliaGifPlayer");
                    window.set_min_size(Some((640, 420)));
                    Presentation::Window(window)
                } else {
                    let layer = self.layer_shell.create_layer_surface(
                        qh,
                        surface.clone(),
                        layer(&spec.layer),
                        Some("noctalia-gif-player"),
                        output.as_ref(),
                    );
                    layer.set_anchor(if spec.ui {
                        Anchor::empty()
                    } else {
                        Anchor::TOP | Anchor::LEFT
                    });
                    layer.set_exclusive_zone(-1);
                    layer.set_keyboard_interactivity(if spec.ui {
                        KeyboardInteractivity::OnDemand
                    } else {
                        KeyboardInteractivity::None
                    });
                    layer.set_size(spec.width, spec.height);
                    layer.set_margin(spec.y, 0, 0, spec.x);
                    Presentation::Layer(layer)
                };
                let region = Region::new(&self.compositor)?;
                if spec.ui {
                    region.add(0, 0, spec.width as i32, spec.height as i32);
                }
                surface.set_input_region(Some(region.wl_region()));
                let viewport = self
                    .viewporter
                    .as_ref()
                    .map(|v| v.get_viewport(&surface, qh, ProtocolData));
                let fractional = self
                    .fractional
                    .as_ref()
                    .map(|f| f.get_fractional_scale(&surface, qh, FractionalData(surface.clone())));
                let scale = self
                    .engine
                    .outputs
                    .iter()
                    .find(|o| o.name == spec.output)
                    .map_or(1.0, |o| o.scale);
                let pool = SlotPool::new(4, &self.shm)?;
                layer.commit();
                self.surfaces.insert(
                    key.clone(),
                    Surface {
                        spec: spec.clone(),
                        layer,
                        pool,
                        buffers: Vec::new(),
                        viewport,
                        fractional,
                        scale,
                        configured: false,
                        ready: true,
                        needs_draw: true,
                        presented: false,
                        presented_scene: Scene::new(),
                        width: spec.width,
                        height: spec.height,
                        frames: 0,
                        input: Vec::new(),
                    },
                );
            } else {
                let surface = self.surfaces.get_mut(key).unwrap();
                if surface.spec != *spec {
                    crate::diag!(
                        Trace,
                        "surface",
                        "geometry_changed",
                        json!({"key":key,"position":[spec.x,spec.y],"size":[spec.width,spec.height],"layer":spec.layer})
                    );
                    if surface.spec.width != spec.width || surface.spec.height != spec.height {
                        surface.layer.set_size(spec.width, spec.height);
                        surface.width = spec.width;
                        surface.height = spec.height;
                        surface.buffers.clear();
                        surface.pool = SlotPool::new(4, &self.shm)?;
                    }
                    surface.layer.set_layer(layer(&spec.layer));
                    surface.layer.set_margin(spec.y, 0, 0, spec.x);
                    surface.spec = spec.clone();
                    surface.needs_draw = true;
                }
            }
        }
        // Input commits must not wait for a frame callback. A covered/frozen surface can still
        // receive input; locking it must become click-through immediately.
        for surface in self.surfaces.values_mut() {
            let mut input = Vec::new();
            if surface.spec.ui {
                input.push(Rect {
                    x: 0,
                    y: 0,
                    w: surface.width as i32,
                    h: surface.height as i32,
                });
            } else {
                for id in &surface.spec.widgets {
                    if let Some(widget) = self.engine.widgets.get(id) {
                        if !widget.settings.locked {
                            let rect = Rect {
                                x: (widget.position.0 - surface.spec.x as f64).floor() as i32,
                                y: (widget.position.1 - surface.spec.y as f64).floor() as i32,
                                w: widget.size.0 as i32,
                                h: widget.size.1 as i32,
                            };
                            if let Some(rect) =
                                rect.clip(surface.width as i32, surface.height as i32)
                            {
                                input.push(rect);
                            }
                        }
                    }
                }
            }
            if input != surface.input {
                crate::diag!(
                    Trace,
                    "surface",
                    "input_region",
                    json!({"key":surface.spec.key,"click_through":input.is_empty(),"rectangles":input.iter().map(|r|[r.x,r.y,r.w,r.h]).collect::<Vec<_>>()})
                );
                let region = Region::new(&self.compositor)?;
                for rect in &input {
                    region.add(rect.x, rect.y, rect.w, rect.h);
                }
                surface
                    .layer
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
                surface.layer.commit();
                surface.input = input;
            }
        }
        // Keep the previous surface until its replacement has presented an initialized buffer.
        if specs
            .keys()
            .all(|key| self.surfaces.get(key).is_some_and(|s| s.presented))
        {
            self.surfaces.retain(|key, surface| {
                let retained = specs.contains_key(key);
                if !retained {
                    crate::diag!(
                        Debug,
                        "surface",
                        "destroyed",
                        json!({"key":key,"frames_committed":surface.frames})
                    );
                }
                retained
            });
        }
        Ok(())
    }
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        let mut drawn = HashSet::new();
        for surface in self.surfaces.values_mut() {
            let spec = &surface.spec;
            if !surface.configured || !surface.ready {
                continue;
            }
            let needs_ui = spec.ui && self.ui.dirty;
            let scale = if surface.viewport.is_some() {
                surface.scale
            } else {
                surface.scale.ceil()
            };
            let bw = (surface.width as f64 * scale).ceil().max(1.0) as u32;
            let bh = (surface.height as f64 * scale).ceil().max(1.0) as u32;
            ensure!(
                bw <= 16384 && bh <= 16384 && (bw as u64 * bh as u64 * 8) <= 512 * 1024 * 1024,
                "Surface buffer exceeds 512 MiB or 16K dimensions"
            );
            let items = spec
                .widgets
                .iter()
                .filter_map(|id| {
                    let widget = self.engine.widgets.get(id)?;
                    let animation = self.engine.assets.get(&widget.path)?.animation.clone()?;
                    let rect = Rect {
                        x: ((widget.position.0 - spec.x as f64) * scale).round() as i32,
                        y: ((widget.position.1 - spec.y as f64) * scale).round() as i32,
                        w: (widget.size.0 as f64 * scale).round().max(1.0) as i32,
                        h: (widget.size.1 as f64 * scale).round().max(1.0) as i32,
                    };
                    Some(Item {
                        id: id.clone(),
                        animation,
                        signature: Signature {
                            rect,
                            frame: widget.frame,
                            opacity: widget.settings.opacity,
                            flip_h: widget.settings.flip_h,
                            flip_v: widget.settings.flip_v,
                            editing: !widget.settings.locked,
                            edit_color: 0xff000000
                                | self.engine.store.settings["appearance"]["palette"]["primary"]
                                    .as_str()
                                    .and_then(|v| v.strip_prefix('#'))
                                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                                    .unwrap_or(0xcbb9ff),
                        },
                    })
                })
                .collect::<Vec<_>>();
            let next = render::scene(&items);
            let changed = spec
                .widgets
                .iter()
                .any(|id| self.engine.widgets.get(id).is_some_and(|w| w.dirty));
            if !surface.needs_draw && !changed && !needs_ui {
                continue;
            }
            let render_started = std::time::Instant::now();
            let index = if let Some(index) = surface
                .buffers
                .iter()
                .position(|b| b.buffer.canvas(&mut surface.pool).is_some())
            {
                index
            } else if surface.buffers.len() < 2 {
                crate::diag!(
                    Debug,
                    "renderer",
                    "buffer_allocated",
                    json!({"surface":spec.key,"size":[bw,bh],"bytes":bw as u64*bh as u64*4,"slot":surface.buffers.len()})
                );
                let (buffer, bytes) = surface.pool.create_buffer(
                    bw as i32,
                    bh as i32,
                    bw as i32 * 4,
                    wl_shm::Format::Argb8888,
                )?;
                bytes.fill(0);
                surface.buffers.push(Swap {
                    buffer,
                    scene: Scene::new(),
                    initialized: false,
                });
                surface.buffers.len() - 1
            } else {
                crate::diag!(
                    Trace,
                    "renderer",
                    "waiting_for_buffer",
                    json!({"surface":spec.key,"slots":surface.buffers.len()})
                );
                continue;
            };
            let swap = &mut surface.buffers[index];
            let dirty = if !swap.initialized || surface.needs_draw || spec.ui {
                Some(Rect {
                    x: 0,
                    y: 0,
                    w: bw as i32,
                    h: bh as i32,
                })
            } else {
                // SHM upload damage is relative to the last committed image, while recycled
                // buffer repair is relative to that buffer's older contents. Both matter.
                let repair = render::damage(&swap.scene, &next);
                let presentation = render::damage(&surface.presented_scene, &next);
                match (repair, presentation) {
                    (Some(a), Some(b)) => Some(a.union(&b)),
                    (a, b) => a.or(b),
                }
            };
            let bytes = swap
                .buffer
                .canvas(&mut surface.pool)
                .context("Released buffer became unavailable")?;
            if let Some(ref dirty) = dirty {
                if spec.ui {
                    let image = self
                        .ui
                        .draw(&self.engine, surface.width, surface.height, scale);
                    for (src, dst) in image.data().chunks_exact(4).zip(bytes.chunks_exact_mut(4)) {
                        dst.copy_from_slice(
                            &(((src[3] as u32) << 24)
                                | ((src[0] as u32) << 16)
                                | ((src[1] as u32) << 8)
                                | src[2] as u32)
                                .to_ne_bytes(),
                        );
                    }
                } else {
                    render::paint(bytes, bw, bh, &items, dirty)?;
                }
                if let Some(d) = dirty.clip(bw as i32, bh as i32) {
                    surface.layer.wl_surface().damage_buffer(d.x, d.y, d.w, d.h);
                }
            }
            surface.presented_scene = next.clone();
            swap.scene = next;
            swap.initialized = true;
            if let Some(viewport) = &surface.viewport {
                surface.layer.wl_surface().set_buffer_scale(1);
                viewport.set_destination(surface.width as i32, surface.height as i32);
            } else {
                surface.layer.wl_surface().set_buffer_scale(scale as i32);
            }
            swap.buffer.attach_to(surface.layer.wl_surface())?;
            surface
                .layer
                .wl_surface()
                .frame(qh, FrameCallbackData(surface.layer.wl_surface().clone()));
            surface.layer.commit();
            surface.ready = false;
            surface.presented = true;
            surface.needs_draw = false;
            surface.frames += 1;
            crate::diag!(
                Trace,
                "renderer",
                "frame_committed",
                json!({"surface":spec.key,"number":surface.frames,"buffer_slot":index,"buffer_size":[bw,bh],"scale":scale,"render_us":render_started.elapsed().as_micros(),"damage":dirty.as_ref().map(|r|[r.x,r.y,r.w,r.h]),"widgets":items.iter().map(|i|json!({"id":i.id,"frame":i.signature.frame,"rect":[i.signature.rect.x,i.signature.rect.y,i.signature.rect.w,i.signature.rect.h]})).collect::<Vec<_>>()})
            );
            drawn.extend(spec.widgets.iter().cloned());
        }
        for widget in self.engine.widgets.values_mut() {
            if drawn.contains(&widget.id) {
                widget.dirty = false;
            }
        }
        Ok(())
    }
    fn find_surface(&mut self, proxy: &wl_surface::WlSurface) -> Option<&mut Surface> {
        self.surfaces
            .values_mut()
            .find(|s| s.layer.wl_surface() == proxy)
    }
    fn key(&mut self, event: KeyEvent) {
        if !self.ui_focus || !self.ui.open {
            return;
        }
        if event.keysym == Keysym::Escape {
            self.ui.open = false;
        } else if event.keysym == Keysym::BackSpace {
            self.ui.backspace();
        } else if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            self.ui.submit(&mut self.engine);
        } else if event.keysym == Keysym::Page_Down {
            self.ui
                .activate(crate::ui::Action::Page(1), &mut self.engine);
        } else if event.keysym == Keysym::Page_Up {
            self.ui
                .activate(crate::ui::Action::Page(-1), &mut self.engine);
        } else if self.modifiers.ctrl && (event.keysym == Keysym::f || event.keysym == Keysym::F) {
            self.ui.focus = Some("search".into());
            self.ui.tab = 0;
            self.ui.dirty = true;
        } else if event.keysym == Keysym::Tab {
            let keys = self
                .ui
                .hits
                .iter()
                .filter_map(|h| {
                    if let crate::ui::Action::Field(key) = &h.action {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if !keys.is_empty() {
                let current = self
                    .ui
                    .focus
                    .as_ref()
                    .and_then(|key| keys.iter().position(|k| k == key))
                    .map_or(0, |n| (n + 1) % keys.len());
                self.ui.focus = Some(keys[current].clone());
                self.ui.dirty = true;
            }
        } else if !self.modifiers.ctrl && !self.modifiers.alt {
            if let Some(text) = event.utf8 {
                self.ui.text_input(&text, &mut self.engine);
            }
        }
    }
}
impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        proxy: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(s) = self.find_surface(proxy) {
            if s.fractional.is_none() {
                crate::diag!(
                    Debug,
                    "surface",
                    "scale_changed",
                    json!({"surface":s.spec.key,"source":"integer","scale":factor.max(1)})
                );
                s.scale = factor.max(1) as f64;
                s.buffers.clear();
                s.needs_draw = true;
            }
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        proxy: &wl_surface::WlSurface,
        callback_time: u32,
    ) {
        if let Some(s) = self.find_surface(proxy) {
            crate::diag!(
                Trace,
                "wayland",
                "frame_callback",
                json!({"surface":s.spec.key,"compositor_time_ms":callback_time,"committed_frames":s.frames})
            );
            s.ready = true;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.output_dirty = true;
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.output_dirty = true;
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.output_dirty = true;
    }
}
impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        let key = self
            .surfaces
            .iter()
            .find(|(_, s)| s.layer.is_layer(layer))
            .map(|(k, _)| k.clone());
        if let Some(key) = key {
            crate::diag!(
                Debug,
                "surface",
                "compositor_closed",
                json!({"surface":key})
            );
            if key == "ui" || key == "app" {
                self.ui.open = false;
            }
            self.surfaces.remove(&key);
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(s) = self.surfaces.values_mut().find(|s| s.layer.is_layer(layer)) {
            crate::diag!(
                Debug,
                "surface",
                "layer_configure",
                json!({"surface":s.spec.key,"requested_size":[s.spec.width,s.spec.height],"configured_size":[configure.new_size.0,configure.new_size.1]})
            );
            let width = if configure.new_size.0 > 0 {
                configure.new_size.0
            } else {
                s.spec.width
            };
            let height = if configure.new_size.1 > 0 {
                configure.new_size.1
            } else {
                s.spec.height
            };
            if width != s.width || height != s.height {
                s.width = width;
                s.height = height;
                s.buffers.clear();
            }
            s.configured = true;
            s.ready = true;
            s.needs_draw = true;
        }
    }
}
impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window) {
        if self
            .surfaces
            .values()
            .any(|surface| surface.layer.wl_surface() == window.wl_surface())
        {
            self.ui.open = false;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        if let Some(surface) = self
            .surfaces
            .values_mut()
            .find(|surface| surface.layer.wl_surface() == window.wl_surface())
        {
            let width = configure
                .new_size
                .0
                .map_or(surface.spec.width, |width| width.get());
            let height = configure
                .new_size
                .1
                .map_or(surface.spec.height, |height| height.get());
            crate::diag!(
                Debug,
                "surface",
                "window_configure",
                json!({"surface":surface.spec.key,"size":[width,height]})
            );
            if (width, height) != (surface.width, surface.height) {
                surface.width = width;
                surface.height = height;
                surface.buffers.clear();
                if let Ok(pool) = SlotPool::new(4, &self.shm) {
                    surface.pool = pool;
                }
            }
            surface.configured = true;
            surface.ready = true;
            surface.needs_draw = true;
            self.ui.dirty = true;
        }
    }
}
impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Ok(pointer) = self.seat.get_pointer(qh, &seat) {
                    self.pointers.insert(seat, pointer);
                }
            }
            Capability::Keyboard => {
                if let Ok(keyboard) = self.seat.get_keyboard(qh, &seat, None) {
                    self.keyboards.insert(seat, keyboard);
                }
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Some(pointer) = self.pointers.remove(&seat) {
                    pointer.release();
                }
                self.drag = None;
            }
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboards.remove(&seat) {
                    keyboard.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if let Some(pointer) = self.pointers.remove(&seat) {
            pointer.release();
        }
        if let Some(keyboard) = self.keyboards.remove(&seat) {
            keyboard.release();
        }
        self.drag = None;
    }
}
impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let Some(spec) = self
                .surfaces
                .values()
                .find(|s| s.layer.wl_surface() == &event.surface)
                .map(|s| s.spec.clone())
            else {
                continue;
            };
            let (x, y) = event.position;
            crate::diag!(
                Trace,
                "input",
                "pointer_event",
                json!({"surface":spec.key,"position":[x,y],"kind":format!("{:?}",event.kind)})
            );
            if spec.ui {
                match event.kind {
                    PointerEventKind::Press { button: 0x110, .. } => {
                        self.ui.click(x, y, &mut self.engine)
                    }
                    PointerEventKind::Release { .. } => self.ui.active_slider = None,
                    PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                        self.ui.motion(x, y);
                        self.ui.slider(x, &mut self.engine);
                    }
                    PointerEventKind::Leave { .. } => {
                        self.ui.hovered = None;
                        self.ui.dirty = true;
                    }
                    PointerEventKind::Axis { vertical, .. }
                        if vertical.absolute != 0.0 || vertical.discrete != 0 =>
                    {
                        self.ui.scroll(if vertical.absolute != 0.0 {
                            vertical.absolute
                        } else {
                            vertical.discrete as f64 * 15.0
                        });
                    }
                    _ => {}
                }
                continue;
            }
            let position = (x + spec.x as f64, y + spec.y as f64);
            let hit = spec
                .widgets
                .iter()
                .rev()
                .find(|id| {
                    self.engine.widgets.get(*id).is_some_and(|w| {
                        !w.settings.locked
                            && Rect {
                                x: w.position.0 as i32,
                                y: w.position.1 as i32,
                                w: w.size.0 as i32,
                                h: w.size.1 as i32,
                            }
                            .contains(position.0, position.1)
                    })
                })
                .cloned();
            match event.kind {
                PointerEventKind::Press {
                    button: 0x110,
                    time,
                    ..
                } => {
                    if let Some(id) = hit {
                        if self
                            .last_click
                            .as_ref()
                            .is_some_and(|(old, last)| old == &id && time.wrapping_sub(*last) < 400)
                        {
                            self.engine.dispatch(&json!({"action":"lock","id":id}));
                            self.last_click = None;
                            self.drag = None;
                        } else if let Some(w) = self.engine.widgets.get(&id) {
                            crate::diag!(
                                Debug,
                                "input",
                                "drag_started",
                                json!({"id":id,"pointer_position":position,"origin":w.position})
                            );
                            self.drag = Some(Drag {
                                id: id.clone(),
                                start: position,
                                origin: w.position,
                                pointer: pointer.clone(),
                            });
                            self.last_click = Some((id, time));
                        }
                    }
                }
                PointerEventKind::Press { button: 0x111, .. } => {
                    if let Some(id) = hit {
                        if self.engine.managed_frontend {
                            self.engine.revision += 1;
                            self.engine.frontend_request = Some(
                                json!({"kind":"control","id":id,"nonce":self.engine.revision}),
                            );
                        } else {
                            self.ui.selected = Some(id);
                            self.engine.ui_request = Some("control".into());
                        }
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if let Some(drag) = &self.drag {
                        if &drag.pointer == pointer {
                            let id = drag.id.clone();
                            let target = (
                                drag.origin.0 + position.0 - drag.start.0,
                                drag.origin.1 + position.1 - drag.start.1,
                            );
                            self.engine.drag_to(&id, target);
                        }
                    }
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    if let Some(drag) = self.drag.take() {
                        self.engine.finish_drag(&drag.id);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    if let Some(id) = hit {
                        if let Some(w) = self.engine.widgets.get(&id) {
                            let delta = if vertical.discrete != 0 {
                                vertical.discrete as f64
                            } else {
                                vertical.absolute / 15.0
                            };
                            let scale = w.settings.scale * 1.1f64.powf(-delta);
                            self.engine
                                .dispatch(&json!({"action":"scale","id":id,"scale":scale}));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.ui_focus = self
            .surfaces
            .get("ui")
            .is_some_and(|s| s.layer.wl_surface() == surface);
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.ui_focus = false;
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}
impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl Dispatch2<WpFractionalScaleV1, State> for FractionalData {
    fn event(
        &self,
        state: &mut State,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(surface) = state.find_surface(&self.0) {
                crate::diag!(
                    Debug,
                    "surface",
                    "scale_changed",
                    json!({"surface":surface.spec.key,"source":"fractional","scale":scale as f64/120.0})
                );
                surface.scale = scale as f64 / 120.0;
                surface.buffers.clear();
                surface.needs_draw = true;
                state.ui.dirty = true;
            }
        }
    }
}
macro_rules! no_events{($($proxy:ty),*)=>{$(impl Dispatch2<$proxy,State> for ProtocolData{fn event(&self,_:&mut State,_:&$proxy,_:<$proxy as wayland_client::Proxy>::Event,_:&Connection,_:&QueueHandle<State>){}})*}}
no_events!(WpFractionalScaleManagerV1, WpViewporter, WpViewport);
delegate_registry!(State);
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(State);

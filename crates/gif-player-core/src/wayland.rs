use crate::input::{apply_input_region, InputRegionMode};
use anyhow::{anyhow, Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use std::collections::HashMap;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, EventQueue, QueueHandle,
};

const SURFACE_NAMESPACE: &str = "gif-player";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDescriptor {
    pub index: usize,
    pub name: Option<String>,
    pub logical_size: Option<(i32, i32)>,
    pub scale_factor: i32,
}

#[derive(Debug)]
pub struct SurfaceRecord {
    pub id: String,
    pub output_name: Option<String>,
    pub configured_size: (u32, u32),
    pub configured: bool,
    pub closed: bool,
    layer: LayerSurface,
}

impl SurfaceRecord {
    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        self.layer.wl_surface()
    }

    pub fn layer_surface(&self) -> &LayerSurface {
        &self.layer
    }
}

pub struct WaylandRuntime {
    connection: Connection,
    event_queue: EventQueue<WaylandState>,
    state: WaylandState,
}

struct WaylandState {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    surfaces: HashMap<String, SurfaceRecord>,
}

impl WaylandRuntime {
    pub fn connect() -> Result<Self> {
        let connection = Connection::connect_to_env().context("connect to Wayland compositor")?;
        let (globals, mut event_queue) =
            registry_queue_init(&connection).context("initialize Wayland registry")?;
        let qh = event_queue.handle();
        let compositor =
            CompositorState::bind(&globals, &qh).context("wl_compositor is unavailable")?;
        let layer_shell =
            LayerShell::bind(&globals, &qh).context("wlr-layer-shell is unavailable")?;
        let shm = Shm::bind(&globals, &qh).context("wl_shm is unavailable")?;
        let mut state = WaylandState {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            compositor,
            layer_shell,
            shm,
            surfaces: HashMap::new(),
        };
        event_queue
            .roundtrip(&mut state)
            .context("read initial Wayland output state")?;
        Ok(Self {
            connection,
            event_queue,
            state,
        })
    }

    pub fn outputs(&self) -> Vec<OutputDescriptor> {
        self.state.output_descriptors()
    }

    pub fn surface(&self, id: &str) -> Option<&SurfaceRecord> {
        self.state.surfaces.get(id)
    }

    pub fn create_surface(
        &mut self,
        id: impl Into<String>,
        output_name: Option<&str>,
        monitor: Option<usize>,
    ) -> Result<()> {
        let id = id.into();
        if self.state.surfaces.contains_key(&id) {
            return Err(anyhow!("Wayland surface '{id}' already exists"));
        }

        let selected_output = self.state.resolve_output(output_name, monitor)?;
        let resolved_name = selected_output
            .as_ref()
            .and_then(|output| self.state.output_state.info(output))
            .and_then(|info| info.name);
        let qh = self.event_queue.handle();
        let wl_surface = self.state.compositor.create_surface(&qh);
        let layer = self.state.layer_shell.create_layer_surface(
            &qh,
            wl_surface,
            Layer::Overlay,
            Some(SURFACE_NAMESPACE),
            selected_output.as_ref(),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        // -1 means this layer surface extends through areas reserved by other layer
        // surfaces. It does not reserve space itself and is required for fullscreen
        // overlay behavior matching the existing implementation.
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        apply_input_region(
            &self.state.compositor,
            layer.wl_surface(),
            InputRegionMode::Empty,
        )?;
        layer.commit();

        self.state.surfaces.insert(
            id.clone(),
            SurfaceRecord {
                id,
                output_name: resolved_name.or_else(|| output_name.map(ToOwned::to_owned)),
                configured_size: (0, 0),
                configured: false,
                closed: false,
                layer,
            },
        );
        self.connection.flush().context("flush new layer surface")?;
        Ok(())
    }

    pub fn remove_surface(&mut self, id: &str) -> bool {
        self.state.surfaces.remove(id).is_some()
    }

    pub fn set_input_region(&mut self, id: &str, mode: InputRegionMode) -> Result<()> {
        let record = self
            .state
            .surfaces
            .get(id)
            .ok_or_else(|| anyhow!("No Wayland surface '{id}'"))?;
        apply_input_region(&self.state.compositor, record.layer.wl_surface(), mode)?;
        record.layer.commit();
        self.connection
            .flush()
            .context("flush input-region update")?;
        Ok(())
    }

    pub fn roundtrip(&mut self) -> Result<()> {
        self.event_queue
            .roundtrip(&mut self.state)
            .context("Wayland roundtrip")?;
        Ok(())
    }

    pub fn blocking_dispatch(&mut self) -> Result<()> {
        self.event_queue
            .blocking_dispatch(&mut self.state)
            .context("dispatch Wayland event")?;
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.connection
            .flush()
            .context("flush Wayland connection")?;
        Ok(())
    }
}

impl WaylandState {
    fn output_descriptors(&self) -> Vec<OutputDescriptor> {
        self.output_state
            .outputs()
            .enumerate()
            .map(|(index, output)| {
                let info = self.output_state.info(&output);
                OutputDescriptor {
                    index,
                    name: info.as_ref().and_then(|info| info.name.clone()),
                    logical_size: info.as_ref().and_then(|info| info.logical_size),
                    scale_factor: info.map_or(1, |info| info.scale_factor),
                }
            })
            .collect()
    }

    fn resolve_output(
        &self,
        output_name: Option<&str>,
        monitor: Option<usize>,
    ) -> Result<Option<wl_output::WlOutput>> {
        let outputs: Vec<_> = self.output_state.outputs().collect();
        if let Some(wanted) = output_name.filter(|name| !name.is_empty()) {
            let output = outputs.iter().find(|output| {
                self.output_state
                    .info(output)
                    .and_then(|info| info.name)
                    .as_deref()
                    == Some(wanted)
            });
            return output
                .cloned()
                .map(Some)
                .ok_or_else(|| anyhow!("Wayland output '{wanted}' not found"));
        }
        if let Some(index) = monitor {
            return outputs
                .get(index)
                .cloned()
                .map(Some)
                .ok_or_else(|| anyhow!("Wayland monitor index {index} not found"));
        }
        Ok(None)
    }

    fn record_for_layer_mut(&mut self, layer: &LayerSurface) -> Option<&mut SurfaceRecord> {
        self.surfaces
            .values_mut()
            .find(|record| &record.layer == layer)
    }
}

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

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

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(record) = self.record_for_layer_mut(layer) {
            record.closed = true;
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
        if let Some(record) = self.record_for_layer_mut(layer) {
            let (width, height) = configure.new_size;
            if width > 0 {
                record.configured_size.0 = width;
            }
            if height > 0 {
                record.configured_size.1 = height;
            }
            record.configured = true;
        }
    }
}

impl ShmHandler for WaylandState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(WaylandState);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_descriptor_is_frontend_neutral() {
        let descriptor = OutputDescriptor {
            index: 0,
            name: Some("DP-1".into()),
            logical_size: Some((2560, 1440)),
            scale_factor: 1,
        };
        assert_eq!(descriptor.name.as_deref(), Some("DP-1"));
        assert_eq!(descriptor.logical_size, Some((2560, 1440)));
    }
}

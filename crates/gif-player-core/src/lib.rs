pub mod cache;
pub mod daemon;
pub mod gif;
pub mod input;
pub mod manager;
pub mod model;
pub mod protocol;
pub mod wayland;

pub use cache::{CacheStats, SharedCache};
pub use daemon::{default_socket_path, serve, DaemonCore, DispatchOutcome};
pub use gif::{Animation, AnimationStore, FrameData};
pub use input::{apply_input_region, input_region_mode, InputGeometry, InputRegionMode, Rect};
pub use manager::{ManagedWidget, WidgetManager};
pub use model::PlayerState;
pub use protocol::{Request, WidgetStatus};
pub use wayland::{OutputDescriptor, SurfaceRecord, WaylandRuntime};

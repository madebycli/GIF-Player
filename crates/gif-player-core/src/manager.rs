use crate::model::PlayerState;
use crate::protocol::WidgetStatus;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ManagedWidget {
    pub id: String,
    pub file: PathBuf,
    pub state: PlayerState,
    pub output: Option<String>,
    pub monitor: Option<u32>,
}

impl ManagedWidget {
    pub fn status(&self) -> WidgetStatus {
        WidgetStatus {
            ok: true,
            id: self.id.clone(),
            file: self.file.clone(),
            x: self.state.x,
            y: self.state.y,
            scale: self.state.scale,
            locked: self.state.locked,
            paused: self.state.paused,
            opacity: self.state.opacity,
            flip_h: self.state.flip_h,
            flip_v: self.state.flip_v,
            speed: self.state.speed,
            bouncing: self.state.bouncing,
            jumping: self.state.jumping,
            jump_rate: self.state.jump_rate,
            output: self.output.clone(),
        }
    }
}

#[derive(Default)]
pub struct WidgetManager {
    widgets: HashMap<String, ManagedWidget>,
}

impl WidgetManager {
    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&ManagedWidget> {
        self.widgets.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ManagedWidget> {
        self.widgets.get_mut(id)
    }

    pub fn statuses(&self) -> Vec<WidgetStatus> {
        let mut values: Vec<_> = self.widgets.values().map(ManagedWidget::status).collect();
        values.sort_by(|a, b| a.id.cmp(&b.id));
        values
    }

    pub fn spawn(
        &mut self,
        file: PathBuf,
        requested_id: Option<String>,
        mut state: PlayerState,
        output: Option<String>,
        monitor: Option<u32>,
    ) -> Result<&ManagedWidget, String> {
        state.normalize();
        let base = file_stem(&file);
        let id = match requested_id {
            Some(id) if id.trim().is_empty() => return Err("widget id must not be empty".into()),
            Some(id) if self.widgets.contains_key(&id) => {
                return Err(format!("widget '{id}' already exists"));
            }
            Some(id) => id,
            None => self.allocate_id(&base),
        };
        self.widgets.insert(
            id.clone(),
            ManagedWidget {
                id: id.clone(),
                file,
                state,
                output,
                monitor,
            },
        );
        Ok(self.widgets.get(&id).expect("inserted widget must exist"))
    }

    pub fn remove(&mut self, id: &str) -> Option<ManagedWidget> {
        self.widgets.remove(id)
    }

    pub fn clear(&mut self) -> usize {
        let count = self.widgets.len();
        self.widgets.clear();
        count
    }

    fn allocate_id(&self, base: &str) -> String {
        if !self.widgets.contains_key(base) {
            return base.to_string();
        }
        let mut suffix = 2_u32;
        loop {
            let candidate = format!("{base}-{suffix}");
            if !self.widgets.contains_key(&candidate) {
                return candidate;
            }
            suffix += 1;
        }
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("gif")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_instances_get_stable_suffixes() {
        let mut manager = WidgetManager::default();
        let path = PathBuf::from("/gifs/cat.gif");
        let first = manager
            .spawn(path.clone(), None, PlayerState::default(), None, None)
            .expect("first spawn")
            .id
            .clone();
        let second = manager
            .spawn(path, None, PlayerState::default(), None, None)
            .expect("second spawn")
            .id
            .clone();
        assert_eq!(first, "cat");
        assert_eq!(second, "cat-2");
    }

    #[test]
    fn output_connector_is_preserved_in_status() {
        let mut manager = WidgetManager::default();
        let status = manager
            .spawn(
                PathBuf::from("/gifs/cat.gif"),
                None,
                PlayerState::default(),
                Some("DP-1".into()),
                Some(1),
            )
            .expect("spawn")
            .status();
        assert_eq!(status.output.as_deref(), Some("DP-1"));
    }

    #[test]
    fn spawn_does_not_clamp_manual_positions() {
        let mut manager = WidgetManager::default();
        let state = PlayerState {
            x: -250.0,
            y: 3000.0,
            ..PlayerState::default()
        };
        let widget = manager
            .spawn(PathBuf::from("/gifs/cat.gif"), None, state, None, None)
            .expect("spawn");
        assert_eq!(widget.state.x, -250.0);
        assert_eq!(widget.state.y, 3000.0);
    }
}

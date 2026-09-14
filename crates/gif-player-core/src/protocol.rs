use crate::model::PlayerState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub gif: Option<PathBuf>,
    #[serde(default)]
    pub state: Option<PlayerState>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub monitor: Option<u32>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Request {
    pub fn action(&self) -> &str {
        self.action.as_str()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WidgetStatus {
    pub ok: bool,
    pub id: String,
    pub file: PathBuf,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub locked: bool,
    pub paused: bool,
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub speed: f64,
    pub bouncing: bool,
    pub jumping: bool,
    pub jump_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_shape_deserializes_without_rejecting_extra_widget_fields() {
        let request: Request = serde_json::from_str(
            r#"{"action":"move","id":"cat","x":120,"y":240}"#,
        )
        .expect("request should parse");
        assert_eq!(request.action(), "move");
        assert_eq!(request.id.as_deref(), Some("cat"));
        assert_eq!(request.extra.get("x").and_then(Value::as_i64), Some(120));
    }

    #[test]
    fn spawn_accepts_connector_name_and_monitor_index() {
        let request: Request = serde_json::from_str(
            r#"{"action":"spawn","gif":"/tmp/a.gif","output":"DP-1","monitor":1}"#,
        )
        .expect("request should parse");
        assert_eq!(request.output.as_deref(), Some("DP-1"));
        assert_eq!(request.monitor, Some(1));
    }
}

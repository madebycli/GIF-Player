//! Native host configuration integration: preserve existing state and target a visible bar.
use anyhow::{Context, Result, ensure};
use std::{fs, path::PathBuf, process::Command};
use toml_edit::{Array, DocumentMut, value};

pub fn attach_bar(requested: Option<&str>) -> Result<Option<String>> {
    let output = match Command::new("noctalia")
        .args(["config", "export", "full"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Ok(None),
    };
    let merged: DocumentMut = String::from_utf8(output.stdout)?.parse()?;
    let Some(target) = selected_bar(&merged, requested)? else {
        return Ok(None);
    };
    let bars = merged["bar"].as_table_like().unwrap();
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::paths::expand("~/.local/state"));
    let directory = state_home.join("noctalia");
    fs::create_dir_all(&directory)?;
    let path = directory.join("settings.toml");
    let original = match fs::read_to_string(&path) {
        Ok(original) => original,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut state: DocumentMut = original
        .parse()
        .context("Invalid Noctalia settings.toml; left unchanged")?;
    let widget_type = "madebycli/gif-player:player";
    let existing = merged
        .get("widget")
        .and_then(|v| v.as_table_like())
        .and_then(|widgets| {
            widgets
                .iter()
                .find(|(_, entry)| entry.get("type").and_then(|v| v.as_str()) == Some(widget_type))
        })
        .map(|(name, _)| name.to_string());
    let mut instance = existing.unwrap_or_else(|| "gif_player".into());
    if merged
        .get("widget")
        .and_then(|widgets| widgets.get(&instance))
        .is_some_and(|widget| widget.get("type").and_then(|v| v.as_str()) != Some(widget_type))
    {
        for index in 2..1000 {
            let candidate = format!("gif_player_{index}");
            if merged
                .get("widget")
                .and_then(|widgets| widgets.get(&candidate))
                .is_none()
            {
                instance = candidate;
                break;
            }
        }
    }
    let mut enabled = merged
        .get("plugins")
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !enabled
        .iter()
        .any(|v| v.as_str() == Some("madebycli/gif-player"))
    {
        enabled.push("madebycli/gif-player");
    }
    state["plugins"]["enabled"] = value(enabled);
    state["widget"][&instance]["type"] = value(widget_type);
    let bar = bars.get(&target).unwrap();
    let already_attached = ["start", "center", "end"].iter().any(|lane| {
        bar.get(lane)
            .and_then(|v| v.as_array())
            .is_some_and(|lane| lane.iter().any(|v| v.as_str() == Some(&instance)))
    });
    if !already_attached {
        let mut lane: Array = bar
            .get("end")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        lane.push(instance);
        state["bar"][&target]["end"] = value(lane);
    }
    let updated = state.to_string();
    if updated != original {
        if path.exists() {
            fs::copy(&path, directory.join("settings.toml.gif-player-backup"))?;
        }
        let temporary = directory.join(format!(".gif-player-{}.toml", std::process::id()));
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
    }
    Ok(Some(target))
}

fn selected_bar(merged: &DocumentMut, requested: Option<&str>) -> Result<Option<String>> {
    let bars = merged
        .get("bar")
        .and_then(|bars| bars.as_table_like())
        .context("No Noctalia bars configured")?;
    let target = if let Some(name) = requested {
        ensure!(
            bars.get(name)
                .is_some_and(|bar| bar.as_table_like().is_some()),
            "No Noctalia bar named {name}"
        );
        name.to_string()
    } else {
        let Some((name, _)) = bars.iter().find(|(_, bar)| {
            bar.as_table_like().is_some()
                && bar.get("enabled").and_then(|v| v.as_bool()) != Some(false)
        }) else {
            return Ok(None);
        };
        name.to_string()
    };
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_enabled_bar_not_metadata_or_disabled_default() {
        let config: DocumentMut = "[bar]\norder = ['default', 'flying']\n[bar.default]\nenabled = false\n[bar.flying]\nenabled = true\n".parse().unwrap();
        assert_eq!(selected_bar(&config, None).unwrap(), Some("flying".into()));
        assert!(selected_bar(&config, Some("order")).is_err());
        let inline: DocumentMut =
            "bar = {order=['default','flying'], default={enabled=false}, flying={enabled=true}}"
                .parse()
                .unwrap();
        assert_eq!(selected_bar(&inline, None).unwrap(), Some("flying".into()));
        assert_eq!(
            selected_bar(&config, Some("flying")).unwrap(),
            Some("flying".into())
        );
    }
}

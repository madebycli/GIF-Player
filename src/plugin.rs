use anyhow::Result;
use std::{fs, path::Path};

const FILES: &[(&str, &str)] = &[
    ("host.luau", include_str!("../plugins/gif-player/host.luau")),
    (
        "plugin.toml",
        include_str!("../plugins/gif-player/plugin.toml"),
    ),
    (
        "settings.luau",
        include_str!("../plugins/gif-player/settings.luau"),
    ),
    ("bar.luau", include_str!("../plugins/gif-player/bar.luau")),
    (
        "service.luau",
        include_str!("../plugins/gif-player/service.luau"),
    ),
    (
        "panel.luau",
        include_str!("../plugins/gif-player/panel.luau"),
    ),
    (
        "picker.luau",
        include_str!("../plugins/gif-player/picker.luau"),
    ),
    (
        "control.luau",
        include_str!("../plugins/gif-player/control.luau"),
    ),
    (
        "shortcut.luau",
        include_str!("../plugins/gif-player/shortcut.luau"),
    ),
    (
        "translations/en.json",
        include_str!("../plugins/gif-player/translations/en.json"),
    ),
    (
        "translations/de.json",
        include_str!("../plugins/gif-player/translations/de.json"),
    ),
    ("README.md", include_str!("../plugins/gif-player/README.md")),
    ("NOTICE.md", include_str!("../NOTICE.md")),
];
pub fn install(destination: &Path) -> Result<()> {
    for (name, contents) in FILES {
        let path = destination.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, contents)?;
    }
    Ok(())
}

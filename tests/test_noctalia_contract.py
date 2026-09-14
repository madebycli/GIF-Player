from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - CI/Nix currently use 3.11+
    tomllib = None

import gif_player_cli
from gif_player_paths import get_paths


ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / "noctalia"


def nested(data: dict, dotted: str):
    value = data
    for part in dotted.split("."):
        if not isinstance(value, dict) or part not in value:
            return None
        value = value[part]
    return value


class NoctaliaContractTests(unittest.TestCase):
    @unittest.skipIf(tomllib is None, "tomllib unavailable on Python 3.10")
    def test_manifest_uses_split_noctalia_frontend(self):
        manifest = tomllib.loads((PLUGIN / "plugin.toml").read_text(encoding="utf-8"))
        self.assertEqual(manifest["id"], "madebycli/gif-player")
        self.assertEqual(manifest["plugin_api"], 28)
        self.assertIn("gif-player", manifest["dependencies"])

        services = {entry["id"]: entry for entry in manifest.get("service", [])}
        panels = {entry["id"]: entry for entry in manifest.get("panel", [])}
        widgets = {entry["id"]: entry for entry in manifest.get("widget", [])}

        self.assertEqual(services["bridge"]["entry"], "service.luau")
        self.assertEqual(panels["picker"]["entry"], "picker.luau")
        self.assertEqual(panels["controls"]["entry"], "controls.luau")
        self.assertEqual(widgets["gif-player"]["entry"], "widget.luau")
        self.assertNotIn("manager", panels)

        for entry in [*services.values(), *panels.values(), *widgets.values()]:
            self.assertTrue((PLUGIN / entry["entry"]).is_file(), entry["entry"])

    @unittest.skipIf(tomllib is None, "tomllib unavailable on Python 3.10")
    def test_manifest_translation_keys_exist(self):
        manifest = tomllib.loads((PLUGIN / "plugin.toml").read_text(encoding="utf-8"))
        translations = {
            language: json.loads((PLUGIN / "translations" / f"{language}.json").read_text(encoding="utf-8"))
            for language in ("en", "de")
        }

        owners = [manifest]
        for entry_type in ("widget", "panel"):
            owners.extend(manifest.get(entry_type, []))

        settings = list(manifest.get("setting", []))
        for owner in owners[1:]:
            settings.extend(owner.get("setting", []))

        for setting in settings:
            for field in ("label_key", "description_key"):
                key = setting.get(field)
                if key is None:
                    continue
                for language, data in translations.items():
                    self.assertIsNotNone(nested(data, key), f"missing {language}:{key}")

    def test_noctalia_scripts_do_not_own_wayland_rendering(self):
        scripts = list(PLUGIN.glob("*.luau"))
        self.assertGreaterEqual(len(scripts), 4)
        for path in scripts:
            source = path.read_text(encoding="utf-8")
            self.assertNotIn("GtkLayerShell", source)
            self.assertNotIn("gi.repository", source)
            self.assertNotIn("wayland-client", source)

        widget = (PLUGIN / "widget.luau").read_text(encoding="utf-8")
        service = (PLUGIN / "service.luau").read_text(encoding="utf-8")
        self.assertNotIn("runAsync", widget)
        self.assertIn("gif-player watch", service)

    def test_dynamic_commands_use_argv_form(self):
        for filename in ("picker.luau", "controls.luau", "service.luau"):
            source = (PLUGIN / filename).read_text(encoding="utf-8")
            self.assertNotIn('noctalia.runAsync("', source, filename)

    def test_catalog_exposes_category_and_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            category = root / "cats"
            category.mkdir()
            gif = category / "wave.gif"
            gif.write_bytes(b"GIF89a")
            catalog = gif_player_cli._catalog(root)
            self.assertEqual(len(catalog), 1)
            self.assertEqual(catalog[0]["category"], "cats")
            self.assertEqual(catalog[0]["relative"], "cats/wave.gif")
            self.assertGreaterEqual(catalog[0]["size"], 6)

    def test_profile_store_roundtrip_uses_xdg_profile_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = {
                "XDG_RUNTIME_DIR": str(root / "runtime"),
                "XDG_CONFIG_HOME": str(root / "config"),
                "XDG_CACHE_HOME": str(root / "cache"),
                "XDG_DATA_HOME": str(root / "data"),
            }
            paths = get_paths(env=env, home=root, allow_legacy=False)
            store = gif_player_cli.ProfileStore(paths.profile_file)
            store.write({"desk": {"widgets": [{"gif": "/tmp/a.gif", "scale": 0.7}]}})
            self.assertEqual(store.load()["desk"]["widgets"][0]["scale"], 0.7)


if __name__ == "__main__":
    unittest.main()

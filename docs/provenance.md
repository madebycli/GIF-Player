# References and copyright

No third-party GIFs or artwork are bundled. The old UI is not reused.
NOTICE.md is retained verbatim. No new repository or plugin license is granted.
The plugin declares `LicenseRef-All-Rights-Reserved` explicitly because an omitted
Noctalia license defaults to MIT. Publication to a community catalog needs the
rights holder's licensing decision; the locally installed plugin works without
catalog publication.

Examined references:

| Source | Revision / purpose |
|---|---|
| madebycli/GIF-Player main | 786e6fe: all Python sources, runtime patches, tests, CLI, profiles, paths, packaging |
| madebycli/GIF-Player beta/native-noctalia-v2 | existing frontend-neutral core, invariants and prior migration context |
| noctalia-dev/noctalia | fc2af27c0e616006bf858cad3b7c9711a028e501, version 5.2.1: manifest, API versions, UI primitives, module isolation, plugin lifecycle |
| noctalia-dev/official-plugins | 9aec34181e46789b281263942092ac552f661d1a: example, mpvpaper, world clock and timer integration patterns |
| mangowm/mangobar | 04949d533d7ad8a776c23b3fe28cfa33f0b35e23: layer-shell, namespace, exclusive zones, popup input |
| mangowm/mango | 0.18.0 and current source: layer ordering, renderer requirements |
| Smithay/client-toolkit | 0.21.1: released protocol bindings, Region, SlotPool, output/pointer/keyboard dispatch |
| SunqdXX/GifEngine | feature review only: independent media instances, flip, lock, overlay/desktop choice; no Electron/Quickshell code used |

The originally supplied xnixjoyer/GIF-Player URL returned 404. The user identified
madebycli/GIF-Player as the authoritative reference through the GitHub connector.
Implementation is written independently against documented interfaces.

Cargo.lock fixes dependency versions. Rust dependencies keep their own licenses;
system libxkbcommon and installed fonts keep their package copyright notices.
Neither GTK, Python, Electron, Qt, Chromium nor Quickshell is required to run the
Rust executable. Build/test scripts and upstream reference builds are separate
from the application runtime.

# Acceptance evidence — 2026-10-14

The Python implementation at `786e6fe1baf0ef31380bdd5e60c318f5aa3cb095` is retained on the GitHub branch `legacy/python-before-rust`. The user explicitly instructed publication of the Rust rebuild on `main`, with all project context files exclusively in `madebycli/master-context`. Hardware checks below remain separate from publication.

## Executed checks

| Area | Result and method |
|---|---|
| Rust | rustfmt; Clippy all targets with warnings denied; locked unit/integration tests; release binary/examples |
| GIF composition | generated GIFs exercise transparent deltas, local palettes, disposal Background/Previous, precise normalized delays, long-time timeline lookup, native pixels above 512 under cache pressure, lossless backing with intact timing, cancellation and truncated prefix warnings |
| IPC v2 | genuine headless daemon and Unix clients exercise legacy actions, wildcards, independent IDs/shared frames, quoted/Unicode paths, partial slow clients and unsafe runtime symlinks |
| Persistence | old state/profile shapes, unknown fields, byte-preserving migration backup, profile layout validation and atomic file replacement |
| Sway 1.10.1 | fifteen real Wayland acceptance checks described below; no fake compositor or mocked input |
| Noctalia 5.2.1 | unmodified host loaded native service, bar widget, attached control panel and floating picker; native UI screenshot inspected; real playback and image-toggle clicks change daemon state; official plugin lint: 0 errors, 0 warnings; Luau bytecode compiler accepts all entries |
| Nix x86_64 | locked package build and flake checks, CLI, native runtime closure and native Noctalia manifest lint |
| Installation | staged Make/Cargo installation and embedded plugin payload; Arch recipe syntax/check/package functions inspected and staged payload exercised |
| Performance | real previous Python player and Rust player measured sequentially on the same Sway output/workload; process CPU and RSS from /proc |

Rust test tools and the acceptance runner are development dependencies, not installed runtime processes. Aarch64 flake outputs are evaluated but not compiled in this x86_64 environment. Arch's package manager is unavailable on Debian; an actual `makepkg -si` run has not been performed.

## Real Wayland acceptance

`tools/headless_acceptance.py` starts unchanged Sway with its headless backend and pixman renderer. It uses a genuine fullscreen XDG client (`examples/input_probe.rs`), persistent virtual pointer/keyboard protocol clients, actual SHM surfaces, a synthetic GIF and grim screenshots. It asserts:

1. Overlay pixels remain visible over the fullscreen client's blue background.
2. Pointer clicks at locked GIF pixels reach the fullscreen client.
3. The locked transparent fullscreen canvas passes clicks outside the GIF.
4. Keyboard and scroll events reach the fullscreen client.
5. Unlocking captures only the GIF rectangle; transparent edit canvas areas remain interactive for underlying windows.
6. Dragging changes the logical base position by the actual injected movement.
7. Explicit negative/offscreen positions are retained.
8. Two distinct outputs are reported and output selection works.
9. One output uses 1920×1080 physical pixels at scale 1.5, with 1280×720 logical geometry.
10. The actual standalone Rust control surface renders at fractional scale and its Pause all button changes daemon state.
11. A normal xdg-shell app window is mapped as a tiled client.
12. A 1024×1024 animation retains native pixels with a 32 MiB cache budget via lossless backing.
13. Held-pointer motion remains unsnapped, presents its new edge and clears the trailing edge across buffer switches.
14. Disabling that output updates daemon output state without crashing.
15. A live asynchronous export contains real decoder frames, render commits, frame callbacks, input regions and drag motion events.

Reproduce as a non-root user with Sway and grim installed:

```sh
cargo build --release --locked --examples
cargo build --release --locked --bin gif-player
python3 tools/headless_acceptance.py --artifacts artifacts
```

The runner writes compositor/player/input logs, `fullscreen-overlay.png` and a result JSON; CI uploads them. These are compositor-specific software tests. They do not prove behavior of physical monitor changes, a GPU renderer or other compositors.

## Native Noctalia integration

The real Noctalia 5.2.1 host ran under Sway using Mesa software EGL, with `madebycli/gif-player` enabled and `madebycli/gif-player:player` installed in the native bar. Both actual panel entries were opened through Noctalia IPC. Collection thumbnails and the native widget tree were rendered by the host, not by a standalone mock. Manifest entries use Overlay (requires API 30) to avoid hiding the panels under fullscreen windows. Empty icon names and isolated-module callback scope were fixed against the actual host. Native `noctalia plugins lint plugins/gif-player` reports no errors or warnings.

Noctalia was also compiled from the examined upstream source without changing its implementation. The plugin's UI comes from host `ui.*` components and host design roles; the independent Rust surface is a separate fallback frontend.

## 1.1.0 frontend and drag checks

Actual Noctalia 5.2.1 checks used a disabled `default` bar and enabled vertical `flying` bar (35 px). Installation selected `flying`, validated the resulting config and displayed the zero-GIF widget; clicking it opened native controls. Clicking a gallery image started its GIF, and clicking again stopped it. A 32-GIF collection scrolled to its final rows without pagination. Compact settings and the normal Rust app were visually inspected. Changing the real host from Catppuccin to Dracula updated the daemon appearance and the visible app colors automatically, including the configured monospace font.

The native 1024-pixel drag test samples newly covered and vacated screenshot pixels, not just IPC coordinates. `docs/drag-results.json` records end-to-end test times including compositor capture, PNG encoding and a separate PNG-reader process; these are not mouse-to-photon latency measurements. Mango hardware interaction remains unverified.

## Historical 1.0.0 performance

Exact data is in [performance.json](performance.json). The generated media contains three separate 512×512 GIFs, each with 24 frames and 50 ms delays. All are visible at scale 0.7 on HEADLESS-1, scale 1, without motion effects. Each warm process was measured for 60 seconds using `tools/measure.py`, sequentially to avoid covering one implementation with the other. The previous Python implementation was run with its actual GTK/Pillow/GtkLayerShell runtime, not estimated.

| Process / condition | CPU, % of one core | Peak RSS |
|---|---:|---:|
| Python, 3 active GIFs | 27.18% | 129.4 MiB |
| Rust, same GIFs | 4.48% | 82.4 MiB |
| Rust, 3 paused GIFs, 20 seconds | 0% | 82.4 MiB |
| Rust, all stopped, `--stay`, 10 seconds | 0% | 4.6 MiB |

That workload measured approximately 83.5% less player CPU and 36.3% less peak RSS. The 1.0.0 release executable was about 2 MiB on x86_64. Figures exclude compositor CPU, disk preview storage and hardware energy use; RSS includes the player's media caches and mapped buffers. Fully transparent fullscreen motion canvases cost more buffer memory than compact stationary surfaces. Cache limits are not a promise that total RSS always stays below the same number. No universal performance or power claim is inferred. These numbers describe the 1.0.0 renderer; native-resolution/lossless storage and bilinear resampling changed that workload in 1.1.0, so they must not be quoted as fresh 1.1.0 measurements.

## MangoWM and remaining graphical limits

The cloud environment is Debian 13 without a GPU or `/dev/dri`. MangoWM 0.18.0 was obtained as the unchanged Nixpkgs package with wlroots 0.20.2 and scenefx 0.5. Starting it with the headless backend fails:

```text
drmGetDevices2 failed: No such file or directory
Cannot create GLES2 renderer: no DRM FD available
couldn't create renderer
```

Mango uses `fx_renderer_create` directly. `WLR_RENDERER=pixman` cannot substitute its renderer. No patched Mango build was presented as a successful target test. Source examination shows Overlay maps to `LyrSpecialOverlay`, above `LyrFullscreen` and `LyrSpecialFullscreen`; this is source evidence only.

**Real Mango fullscreen, game mouse/keyboard pass-through, GPU presentation, physical hotplug and hardware energy consumption are unverified.** Niri and Hyprland have not been run. An unchanged GPU-backed Mango session must perform those checks before claiming target hardware acceptance. Run the fullscreen probe alongside locked GIFs and locked/edit canvases, observe mouse, keyboard and scroll input, test output scale/hotplug and inspect `gif-player doctor`. Session locking always remains controlled by the compositor.

## 1.2.0 diagnostics checks

Sixteen locked Rust unit/integration tests pass, including log rotation, valid JSONL/truncation, private export permissions, unknown-field exclusion, asynchronous live export with continued IPC, existing-directory rejection, persisted levels and stopped-daemon export. The real Sway runner now enables trace in an isolated XDG state directory and checks a completed report from genuine decode/render/input events. Logging/export use bounded writer/job queues and the existing wake stream; no mock event source is used. Native plugin API lint, Nix build/check and staged installation are repeated for the new package version. Current CPU/power figures for trace mode have not been benchmarked; historical 1.0 performance is not a 1.2 claim.

In an additional unmodified Noctalia 5.2.1 software-EGL session, actual pointer clicks changed info → debug → trace and the native Export logs button created a complete report. The normal Rust app settings export button created another report and the footer displayed its completed path. These were real host/app interactions, not mocked callbacks. Host-only sound/PipeWire warnings from the audio-less cloud session are unrelated to plugin execution.

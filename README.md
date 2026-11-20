# Lumenic (`lumenic`)

Cross-platform immediate-mode UI and application runtime in Rust targeting Windows (x64, ARM64), Linux (x86_64, ARM64), macOS, and WebAssembly (`wasm32-unknown-unknown` + WebGL 2).

Lumenic combines a 2-pass flex/grid layout engine, fluent `StyleBuilder` customization, an instanced SDF OpenGL renderer, dynamic `cosmic-text` glyph shaping, batched `Canvas2D` + custom GLSL passes, a depth-buffered forward `Scene3D` pipeline, sandboxed storage, non-blocking HTTP/WebSocket clients, and built-in headless/live MCP inspection in a single crate.

> **Status:** `0.1.0` (pre-1.0; public APIs may evolve across minor releases). Requires OpenGL 3.3 Core, OpenGL ES 3.0, or WebGL 2.

## Design Principles

- **Unopinionated Styling:** No preset widget themes or baked-in design systems. `Theme` is a plain `Copy` struct of color and metric tokens (`Theme::dark()`, `Theme::light()`, or custom), and every node/widget accepts `StyleBuilder` overrides (box-model padding/margin, flex alignment and distribution, per-corner radii, linear/radial gradients, state modifiers, 2D transforms, sticky positioning, and cursor icons).
- **Strict Crate Encapsulation:** Public APIs expose only Lumenic types (`UiContext`, `App`, `WindowConfig`, `Theme`, `Color`, `Key`, `TextureId`, `MeshId`, `NodeId`, `Camera3D`, `Error`). Underlying crates (`winit`, `glutin`, `glow`, `cosmic-text`, `rustls`, `rfd`, `arboard`, `gltf`) remain behind internal module boundaries.
- **Minimal Dependency & Safety Footprint:** Built without `tokio`, `serde`, or `wgpu`. `#![forbid(unsafe_code)]` is enforced in `ui/`, `gfx/`, and `sys/` (with `unsafe` quarantined to `os/` platform bindings and `sys/arena.rs`).
- **Event-Driven Idle & Pixel Snapping:** `LoopMode::Idle` returns to `ControlFlow::Wait` once springs settle (`CursorMoved` skips redraws unless hover changes, a drag is active, or `cursor_glow` is enabled). `LoopMode::FixedTimestep` drives continuous 2D/3D viewports. Layout edges and glyph baselines snap to physical pixels across fractional DPI scales (100%, 125%, 150%, 200%).


## Quick Start

Add `lumenic` to `Cargo.toml`:

```toml
[dependencies]
lumenic = "0.1"
```

Minimal application:

```rust
use lumenic::{LoopMode, Theme, WindowConfig};

fn main() -> Result<(), lumenic::Error> {
    let mut enabled = true;
    let mut gain = 0.75_f32;
    let theme = Theme::default();

    lumenic::run(WindowConfig::new("Lumenic Demo"), LoopMode::Idle, move |ui, app| {
        let csd = ui.csd_titlebar("Lumenic Demo");
        app.perform_csd(csd);

        ui.col(|ui| {
            ui.txt("Audio Control").sz(20.0);
            ui.toggle("Enable Output", &mut enabled);
            ui.slider("Master Gain", &mut gain, 0.0..=1.0);
            if ui.btn("Reset").clicked() {
                gain = 0.75;
            }
        })
        .p(16.0)
        .gap(10.0)
        .bg(theme.surface)
        .r(12.0);
    })
}
```

## Platform Prerequisites

- **Windows & macOS:** Standard Rust toolchain; no external C/C++ libraries required.
- **Linux:** Wayland/XKB development headers and Mesa/OpenGL drivers for building, plus `xdg-desktop-portal` at runtime when the `dialogs` feature is enabled:
  ```sh
  sudo apt-get install libwayland-dev libxkbcommon-dev libgl1-mesa-dev
  ```
- **WebAssembly:**
  ```sh
  rustup target add wasm32-unknown-unknown
  cargo build --target wasm32-unknown-unknown --release --lib
  ```

## Architecture & Capabilities

- **Rendering (`gfx/`):** Unified `#version 300 es` / OpenGL 3.3 Core instanced SDF shader (rounded rects with independent 4-corner radii, linear/radial gradients, borders, drop shadows, Dual-Kawase frosted `glass`, sparklines, and pointer ripples), `cosmic-text` dynamic glyph atlas with an embedded `< 45 KB` subsetted fallback font + runtime `register_font`, `Canvas2D` vector/SVG/custom-GLSL/instanced passes, and forward depth-buffered `Scene3D`.
- **Layout & Widgets (`ui/`):** 2-pass flex + responsive `grid`/`grid_auto` layout, deferred overlay pass, `interact` gesture recognizer, spring/tween/FLIP animations, virtualized lists (`vlist`, `vlist_dynamic`), split panes, tables, tabs, markdown + syntax-highlighted code blocks, gap-buffer `TextEditState` with IME + undo/redo, and `toasts`.
- **Platform & I/O (`os/`, `sys/`):** `SafeDir` path-traversal/symlink sandbox with atomic writes (`localStorage` on `wasm32`), worker-threaded native file dialogs waking via `Waker`, `HttpClient` and RFC 6455 `WsClient`, browser URL routing (`app.route()`, `app.navigate(...)`), and panic-hook flush callbacks.

## Cargo Features

| Feature | Default | Purpose |
| :--- | :--- | :--- |
| `network` | Yes | Native `rustls` + `webpki-roots` RFC 6455 `ws://`/`wss://` WebSocket + HTTP/1.1 (`http://`/`https://`) clients (`XmlHttpRequest` / browser `WebSocket` on `wasm32`) |
| `dialogs` | Yes | Worker-threaded native open/save dialogs via `rfd` (`xdg-portal` on Linux) |
| `clipboard` | Yes | Cross-platform clipboard integration via `arboard` (DOM clipboard on `wasm32`) |
| `tray` | Yes | Native system tray icon and menu support via `tray-icon` (`ksni` on Linux) |
| `gltf` | No | `.glb`/`.gltf` 3D mesh and node hierarchy loader for `Scene3D` |

Build minimal binaries with `--no-default-features` when dialogs, clipboard, system tray, or TLS networking are not needed.

## CLI Inspection & MCP Automation

Applications launched via `lumenic::run` support built-in diagnostics and JSON-RPC 2.0 UI automation on native targets:

- `F12`: Toggle the live telemetry HUD (layout/UI/GPU timings, draw calls, SDF instance count, arena usage, glyph atlas occupancy).
- `--inspect [--out-png <path>] [--out-json <path>]`: Render a single frame offscreen and write PNG + semantic `layout.json`.
- `--mcp`: Run the headless stdio JSON-RPC 2.0 MCP server (`ui_tree`, `ui_screenshot`, `ui_click`, `ui_type`, `ui_key`, `ui_scroll`, `ui_set_theme`, `ui_step_frames`, `ui_perf`, `ui_close`).
- `--mcp-live` (or `--mcp --headed`): Open the desktop window and accept live JSON-RPC 2.0 commands over stdio.
- `--mcp-port <port>`: Open the desktop window and serve live JSON-RPC 2.0 commands on `127.0.0.1:<port>`.
- `--headless-smoke`: Step two headless frames and exit `0` for CI smoke checks.

## Non-Goals

- Not a general-purpose async runtime (no `tokio`; background I/O uses OS threads waking the UI via `Waker`).
- Not a PBR/raytracing AAA 3D engine (`Scene3D` is a forward depth-buffered pipeline for viewports, model previews, and stylized games).
- Not an HTML/CSS browser engine (layout uses a deterministic 2-pass flex/grid model with explicit builder methods).
- Not a GTK/Qt wrapper (`os/` talks directly to `winit`, `glutin`, `glow`, and XDG portals).

## Documentation

Full API documentation is maintained in rustdoc comments across the crate:

```sh
cargo doc -p lumenic --no-deps --open
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

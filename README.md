# Wallman

A lightweight, native Wayland wallpaper manager written in Rust.

Wallman renders wallpapers directly through Wayland instead of relying on external wallpaper backends such as `swaybg`. It also provides a blurred backdrop layer for desktop environments/compositors that expose an overview-style interface, such as Niri.

## Features

* Native Wayland rendering
* Lightweight background daemon
* No dependency on `swaybg`
* Multi-monitor support
* Per-monitor wallpapers
* Automatic wallpaper restoration after restart
* Blurred overview/backdrop layer
* Configurable blur processing
* Multiple scaling modes:

  * `fill`
  * `fit`
  * `stretch`
  * `center`
* Background image processing using a worker thread
* Color extraction from wallpapers
* IPC-based wallpaper updates
* CLI for controlling the daemon
* Graceful handling of invalid images
* Native shared-memory (`wl_shm`) buffers
* Designed specifically for Linux/Wayland

## How It Works

Wallman consists of three main parts:

```text
CLI
 │
 │ IPC
 ▼
Daemon
 │
 ├── Worker thread
 │    └── Image decoding / resizing / blur / color extraction
 │
 ▼
Wayland Renderer
 │
 ├── Wallpaper surface
 │
 └── Blurred backdrop surface
```

The Wayland renderer remains responsible for compositor communication and buffer management, while expensive image processing is performed outside the Wayland event loop.

This prevents large images from blocking compositor communication while they are being decoded, resized, or blurred.

## Installation

Clone the repository:

```bash
git clone https://github.com/boxodirhifi/wallman.git
cd wallman
```

Install with Cargo:

```bash
cargo install --path .
```

Verify the installation:

```bash
wallman --help
```

## Usage

Start the daemon:

```bash
wallman daemon
```

Set a wallpaper:

```bash
wallman set ~/Pictures/wallpaper.jpg
```

Choose a scaling mode:

```bash
wallman set ~/Pictures/wallpaper.jpg --mode fill
wallman set ~/Pictures/wallpaper.jpg --mode fit
wallman set ~/Pictures/wallpaper.jpg --mode stretch
wallman set ~/Pictures/wallpaper.jpg --mode center
```

Reload the current wallpaper:

```bash
wallman reload
```

Check daemon status:

```bash
wallman status
```

Stop the daemon:

```bash
wallman stop
```

## Multi-Monitor Support

Wallman supports multiple Wayland outputs and can assign wallpapers independently to different monitors.

For example:

```bash
wallman set ~/Pictures/main.jpg --monitor DP-1
wallman set ~/Pictures/secondary.jpg --monitor HDMI-A-1
```

Each monitor gets its own wallpaper surface and rendering state.

This allows different wallpapers, scaling modes, and processing results to be maintained independently per output.

## Scaling Modes

Wallman supports four wallpaper scaling modes.

### `fill`

Scales the image while preserving its aspect ratio and crops the excess so the entire surface is filled.

### `fit`

Scales the image while preserving its aspect ratio and places it inside the surface without cropping.

### `stretch`

Stretches the image to exactly match the output dimensions.

### `center`

Displays the image at its original dimensions, centered on the output.

The default mode is `fill`.

## Blurred Backdrop

Wallman can maintain a second Wayland layer containing a blurred version of the current wallpaper.

```text
┌──────────────────────────────┐
│                              │
│      Desktop Wallpaper       │
│          sharp               │
│                              │
├──────────────────────────────┤
│                              │
│     Overview / Backdrop      │
│          blurred             │
│                              │
└──────────────────────────────┘
```

The backdrop is processed independently from the sharp wallpaper and is intended for compositor interfaces such as Niri's overview.

## Image Processing

Image processing is performed by a worker thread so expensive operations do not block the Wayland event loop.

The processing pipeline includes:

```text
Image
  │
  ├── Decode
  │
  ├── Resize
  │
  ├── Wallpaper output
  │
  ├── Backdrop resize
  │
  ├── Blur
  │
  └── Color extraction
```

The worker produces processed pixel data while the renderer remains responsible for creating and attaching Wayland buffers.

This architecture is particularly useful when dealing with large 4K/8K source images.

## Color Extraction

Wallman extracts representative colors from wallpapers and stores the resulting information for use by other components or future theming features.

This allows Wallman to eventually provide wallpaper-aware theming without requiring another image-processing application.

## Architecture

Wallman is intentionally split into separate responsibilities:

```text
src/
├── cli.rs
├── commands/
│   ├── set.rs
│   └── config.rs
├── config/
├── daemon/
├── ipc/
└── renderer/
```

### CLI

Provides the user-facing interface for setting wallpapers, managing configuration, and communicating with the daemon.

### Daemon

Runs continuously in the background and receives wallpaper commands through IPC.

### IPC

Provides local communication between the CLI and the daemon through a Unix socket.

### Renderer

Handles Wayland objects, layer-shell surfaces, shared-memory buffers, output management, and compositor events.

### Worker

Handles CPU-intensive image processing without blocking the Wayland event loop.

## Why Rust?

Wallman is written in Rust because it provides:

* Low memory overhead
* Native performance
* Strong memory and thread safety
* Excellent Linux/Wayland ecosystem support
* No garbage collector
* A good fit for a long-running system daemon

The goal is to keep Wallman small, predictable, and efficient.

## Compositor Compatibility

Wallman is built around standard Wayland protocols and `wlr-layer-shell`.

It is primarily designed for Wayland compositors supporting:

* `wl_shm`
* `wlr-layer-shell`

Some features, particularly the blurred backdrop, depend on how the compositor handles layer-shell surfaces.

Niri is currently the primary environment used for development and testing.

## Configuration

Wallman provides configuration through its CLI:

```bash
wallman config
```

For example, the default scaling mode can be changed with:

```bash
wallman config mode fill
```

Run:

```bash
wallman config --help
```

for the available configuration options.

## Development

Build:

```bash
cargo build
```

Check the project:

```bash
cargo check
```

Run directly:

```bash
cargo run -- daemon
```

## Roadmap

### v1.0

* [x] Native Wayland renderer
* [x] Desktop wallpaper surface
* [x] Blurred backdrop
* [x] Scaling modes
* [x] Error handling
* [x] Buffer lifecycle cleanup
* [x] Stable daemon/IPC architecture

### v1.1

* [x] Worker-thread image processing
* [x] Color extraction
* [x] Multi-monitor support
* [x] Per-monitor wallpapers
* [x] Per-output rendering architecture

### Future

* [ ] More compositor-specific integrations
* [ ] Additional image-processing options
* [ ] Further performance optimizations
* [ ] More advanced per-monitor configuration
* [ ] Additional theming integrations

The project intentionally avoids adding unnecessary features simply for the sake of complexity. The priority is keeping Wallman lightweight, native, reliable, and fast.

## License

Wallman is licensed under the GNU General Public License v3.0.

See [`LICENSE`](LICENSE) for the full license text.

## Repository

GitHub:

https://github.com/boxodirhifi/wallman

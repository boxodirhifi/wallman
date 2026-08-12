# Wallman 🖼️

A lightning-fast, native Wayland wallpaper manager built in Rust. 

Wallman was designed specifically to solve the "Overview Backdrop" problem in compositors like Niri, while providing a robust, multi-monitor architecture for power users.

## ✨ Features

* **Native Wayland Rendering:** Zero reliance on legacy X11 tools or heavy external daemons like `swaybg`. Uses `zwlr_layer_shell_v1` directly.
* **The Niri Overview Backdrop:** Automatically generates a blurred version of your wallpaper and renders it on a separate background layer, giving the Niri Overview a beautiful, native blurred backdrop.
* **Configurable Blur Radius:** Tune the blur intensity to your exact taste (1–30) via the `--blur` flag.
* **Stutter-Free Architecture:** Heavy image processing (Lanczos3 resizing, Gaussian blurring) runs on a dedicated background worker thread. Your Wayland event loop never freezes, even with massive 4K/8K images.
* **Color Extraction:** Automatically extracts a 5-color palette from your wallpaper and writes it to `~/.cache/wallman/colors.toml` for easy integration with Waybar, terminals, and window borders.
* **Per-Monitor Wallpapers:** Set different wallpapers for different screens using the `--monitor` flag.
* **Smart Scaling:** Supports `fill`, `fit`, `stretch`, and `center` modes natively.

## 🚀 Installation

Currently installable via Cargo:

```bash
cargo install --git https://github.com/boxodirhifi/wallman.git
```

*(Ensure your Cargo `bin` directory is in your `$PATH`)*

## 🛠️ Usage

### Start the Daemon
Wallman runs a lightweight daemon to manage the Wayland surfaces. Add this to your compositor's autostart:
```bash
wallman daemon
```

### Set a Global Wallpaper
Applies the wallpaper to all connected monitors.
```bash
wallman set ~/Pictures/landscape.jpg --mode fill
```

### Customize the Blur
Controls the blur intensity for the Niri Overview backdrop (1–30, default is 8).
```bash
# Subtle blur
wallman set ~/Pictures/landscape.jpg --blur 3

# Heavy blur
wallman set ~/Pictures/landscape.jpg --blur 15
```

### Set a Per-Monitor Wallpaper
Target a specific output (e.g., `eDP-1`, `DP-2`). Other monitors will keep their global wallpaper.
```bash
wallman set ~/Pictures/portrait.jpg --mode fit --monitor eDP-1
```

### Reload & Stop
```bash
wallman reload
wallman stop
```

## 🎨 Color Extraction & Theming

Every time you set a wallpaper, Wallman extracts the dominant colors and saves them to `~/.cache/wallman/colors.toml`:

```toml
# ~/.cache/wallman/colors.toml
primary = "#233b28"
secondary = "#b4a88d"
tertiary = "#57826b"
quaternary = "#7aa88d"
quinary = "#8d9c9c"
```

You can use a simple script in your compositor config to reload your tools (like Waybar or your terminal) whenever this file changes to instantly theme your desktop to match your wallpaper!

## 🐧 Compositor Compatibility

* **Niri:** Fully supported. Wallman's signature blurred backdrop layer integrates perfectly with the Niri Overview.
* **Hyprland / Sway:** Fully supported via the standard `layer-shell` protocol.

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

### v1.2

* [x] Configurable blur radius
* [ ] Monitor hotplugging support
* [ ] Additional image-processing options
* [ ] Further performance optimizations
* [ ] More advanced per-monitor configuration
* [ ] Additional theming integrations

The project intentionally avoids adding unnecessary features simply for the sake of complexity. The priority is keeping Wallman lightweight, native, reliable, and fast.

---

**License:** GPL-3.0

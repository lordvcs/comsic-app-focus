# COSMIC App Focus

This repository contains two binaries:

- `cosmic-app-focus`: a CLI helper that focuses (or launches) an app by Wayland app_id.
- `cosmic-app-focus-applet`: a COSMIC panel applet that mirrors GNOME's dash-to-panel behavior with Super+1…0 mapping.

## Requirements

- Pop!_OS 24.04 COSMIC desktop with the new panel.
- Rust toolchain (>= 1.77), `cargo`.
- Dev packages: `sudo apt install build-essential pkg-config libwayland-dev wayland-protocols libxkbcommon-dev`.

## Build

```bash
cargo build --release
```

Binaries will appear under `target/release/`.

## Installing the helper

```bash
sudo install -Dm755 target/release/cosmic-app-focus /usr/local/bin/cosmic-app-focus
```

You can still call the helper directly if you want ad-hoc bindings, but the panel applet manages Super+number shortcuts automatically.

## Installing the panel applet

1. Install the binary:
   ```bash
   sudo install -Dm755 target/release/cosmic-app-focus-applet /usr/local/bin/cosmic-app-focus-applet
   ```

2. Copy the desktop entry and icon:
   ```bash
   sudo install -Dm644 data/applications/com.system76.CosmicAppFocusApplet.desktop \
     /usr/local/share/applications/com.system76.CosmicAppFocusApplet.desktop
   sudo install -Dm644 data/icons/hicolor/symbolic/apps/com.system76.CosmicAppFocusApplet-symbolic.svg \
     /usr/local/share/icons/hicolor/symbolic/apps/com.system76.CosmicAppFocusApplet-symbolic.svg
   sudo update-icon-caches /usr/local/share/icons/hicolor -f
   ```

3. Reload COSMIC panel (log out/in or restart `cosmic-panel`).

4. In COSMIC Settings → Panel → Applets, add "COSMIC App Focus". Arrange it wherever you want on the panel.

## Configuration

The applet reuses COSMIC's existing favorites list (`com.system76.CosmicAppList`). Pinned apps (first in the applet) come from the "Favorites" section in COSMIC's Dock settings. Super+1…0 shortcuts are rewritten automatically to match the first ten favorites; additional running apps appear to the right and are still clickable.

## Development

Run the applet directly:

```bash
cargo run --release --bin cosmic-app-focus-applet
```

Logs: set `RUST_LOG=info` before running.

## License

GPL-3.0-only (see [LICENSE](LICENSE)).

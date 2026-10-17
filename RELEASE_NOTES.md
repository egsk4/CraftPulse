CraftPulse v0.4.1 is an alpha update focused on Linux, Arch and ArtCraft AppImage support. PhotoCraft and LightCraft remain the main development focus.

### Changes

- Detect versioned ArtCraft AppImages and generic payload executables, including apps started from ArtCraft Launcher.
- Preserve window information and focus across an app's helper processes.
- Support window and idle detection through X11 and supported Wayland compositors, plus native Linux settings rendered with WebKitGTK.
- Configure idle status in seconds; zero disables it. Diagnostics report measured idle time when available.
- Apply activity rules to tools, panels and views reported by the apps' live control APIs.
- Show card buttons safely in the settings preview and distinguish saved session variables from successfully applied runtime settings.
- Improve settings validation, configuration persistence and startup handling. Bound desktop helper commands so stalled helpers do not block presence updates.
- Add Arch packaging, automated Arch GUI checks, cross-platform release builds and downloadable checksums.

### Downloads

| Platform | Asset |
| --- | --- |
| Windows x86-64 | `craftpulse-v0.4.1-windows-x86_64.zip` |
| Linux x86-64 | `craftpulse-v0.4.1-linux-x86_64.tar.gz` |
| Arch Linux x86-64 | `craftpulse-v0.4.1-archlinux-x86_64.tar.gz` |
| macOS Apple Silicon | `craftpulse-v0.4.1-macos-arm64.tar.gz` |

Extract the archive and run `craftpulse.exe` on Windows or `./craftpulse` on Linux/macOS. Linux needs GTK 3, WebKitGTK 4.1, Ayatana AppIndicator and libxdo; on Arch, install `gtk3 webkit2gtk-4.1 libayatana-appindicator xdotool`. Run the Discord desktop app and a supported ArtCraft app.

### Validation and limits

Publication requires passing tests, strict Clippy and optimized builds on Windows, Linux and macOS, plus rolling Arch Wayland idle, native settings rendering and an end-to-end AppImage/Discord fixture test. Real launcher 0.4.1 and PhotoCraft 0.9.0 AppImages were separately tested on a Debian/Sway host, including PhotoCraft's authenticated live API; this does not certify every ArtCraft app or desktop.

Linux and Windows share core features but differ in window-inspection capabilities. GNOME Wayland needs the apps' live APIs for detailed activity. After enabling live detection, restart the launcher and its apps; Linux may require logging out and in first. AppImage tests used extraction where FUSE was unavailable. macOS and Windows builds are CI-tested; real ArtCraft interaction was tested on Linux.

See [README](https://github.com/egsk4/CraftPulse/blob/CraftPulse-v0.4.1/README.md), [development notes](https://github.com/egsk4/CraftPulse/blob/CraftPulse-v0.4.1/DEVELOPING.md) and [Linux testing details](https://github.com/egsk4/CraftPulse/blob/CraftPulse-v0.4.1/LINUX-TESTING.md).

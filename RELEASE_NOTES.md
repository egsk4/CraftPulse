CraftPulse v0.4.2 alpha fixes Windows idle status and improves the idle-time setting.

### Fixed

- **Windows idle status:** clicking around Discord or another app no longer makes an already-idle ArtCraft app appear active. Idle clears when you return to that ArtCraft app. Selecting a different app, closing it or disabling idle also clears the latch.
- **Idle-time control:** replaced the plain number box with a themed, keyboard-accessible seconds field and labelled −/+ buttons. Buttons adjust by 30 seconds, clamp values to 0–86400, and debounce saving during repeated clicks. Zero disables idle.
- Isolated the X11 test helper from the process-wide PATH to avoid environment-dependent fixture failures.

### Downloads

| Platform | Asset |
| --- | --- |
| Windows x86-64 | `craftpulse-v0.4.2-windows-x86_64.zip` |
| Linux x86-64 | `craftpulse-v0.4.2-linux-x86_64.tar.gz` |
| Arch Linux x86-64 | `craftpulse-v0.4.2-archlinux-x86_64.tar.gz` |
| macOS Apple Silicon | `craftpulse-v0.4.2-macos-arm64.tar.gz` |

Quit CraftPulse from its tray menu, extract the new archive and replace your executable. Your configuration is retained. Checksums are in `SHA256SUMS.txt`.

### Validation

The release workflow requires passing tests, strict Clippy and optimized builds on Windows, Linux and macOS, plus Arch Wayland idle, native settings rendering and end-to-end AppImage/Discord smoke checks. Regression tests simulate the Windows idle/focus transitions; they do not replace an interactive test with real Discord on a Windows desktop.

Linux keeps its existing system-idle behavior. Desktop-specific detection limits and installation dependencies are documented in the [README](https://github.com/egsk4/CraftPulse/blob/CraftPulse-v0.4.2/README.md).

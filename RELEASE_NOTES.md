CraftPulse v0.5.0 — first official release

Reliability
- Atomic config and stats writes; previous valid config backup and explicit recovery.
- Invalid startup config disables sharing and shows a warning instead of silently using sharing-enabled defaults.
- Failed settings saves leave running settings unchanged.
- Usage stats exclude disabled sharing/apps, idle time and all pause modes.
- Stopping sharing clears the preview; test cards respect global disable and pauses.
- OS-held single-instance protection prevents duplicate tray apps.

Settings
- Activity rule editor with validated ordered rules.
- Per-app idle timeout, including inherit and disabled options.
- Stable updates by default; optional prerelease channel.
- CPU/RAM/uptime monitor, sampled every five seconds for CraftPulse only.
- Sanitized support-report export excluding names, paths, tokens and logs.

Performance
- Minimal process refresh fields on Windows/macOS.
- Cached privacy regexes; stripped, LTO-optimized release builds.
- Resource monitor refreshes only this PID, not the process tree.

Downloads
- Windows x86-64: craftpulse-v0.5.0-windows-x86_64.zip
- Linux x86-64: craftpulse-v0.5.0-linux-x86_64.tar.gz
- Arch Linux x86-64: craftpulse-v0.5.0-archlinux-x86_64.tar.gz
- macOS Apple Silicon: craftpulse-v0.5.0-macos-arm64.tar.gz
- SHA256SUMS.txt

Platform capabilities differ. Linux GNOME Wayland uses process/live-API detection for native apps; window enumeration is unavailable there. Native GUI libraries are required on Linux. Windows/macOS CI is automated coverage, not interactive testing with real Discord. See README.md and LINUX-TESTING.md for installation, checks and limitations.

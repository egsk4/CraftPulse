Unreleased — local changes

- Add a detection inspector with selected-app reason, activity source, detection channel, pause state and IPC write status.
- Add a read-only draft rule tester with live/API and Windows selected-control matching modes.
- Share live matching logic between runtime and rule previews; keep inspection/control data out of support exports.

- Fix issue #2 locally: recognise WordCraft, DeckCraft, SoundCraft, GridCraft, CADCraft and FigCraft, with specific default activities and published icon URLs.
- Migrate older configs by adding missing built-in apps without changing existing app preferences.
- Add App to share pinning; closing or disabling the pinned app clears sharing instead of selecting another app.
- Exclude disabled apps before automatic selection.
- Add Up/Down rule priority controls; Save rules applies the draft order.

- Replaced the cramped per-app idle field with responsive app cards and an Idle status menu: Use global, Never show Idle or Custom timeout.
- Added Classic, Studio and Playful card styles. Choosing a style fills draft text; Save applies it.
- Kept card text/button drafts out of unrelated settings autosaves.
- Added local creative prompts that do not repeat consecutively or change Discord activity.
- Expanded the settings source into readable HTML, CSS and JavaScript without adding runtime dependencies.

---

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

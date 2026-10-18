CraftPulse v0.6.0

- Add a detection inspector with selected-app reason, activity source, detection channel, pause state and IPC write status.
- Add a read-only draft rule tester with live/API and Windows selected-control matching modes.
- Share live matching logic between runtime and rule previews; keep inspection/control data out of support exports.

- Fix the missing app registry entries reported in issue #2: recognise WordCraft, DeckCraft, SoundCraft, GridCraft, CADCraft and FigCraft, with specific default activities and published icon URLs.
- Migrate older configs by adding missing built-in apps without changing existing app preferences.
- Add App to share pinning; closing or disabling the pinned app clears sharing instead of selecting another app.
- Exclude disabled apps before automatic selection.
- Add Up/Down rule priority controls; Save rules applies the draft order.

- Replaced the cramped per-app idle field with responsive app cards and an Idle status menu: Use global, Never show Idle or Custom timeout.
- Added Classic, Studio and Playful card styles. Choosing a style fills draft text; Save applies it.
- Kept card text/button drafts out of unrelated settings autosaves.
- Added local creative prompts that do not repeat consecutively or change Discord activity.
- Expanded the settings source into readable HTML, CSS and JavaScript without adding runtime dependencies.


Downloads
- Windows x86-64: craftpulse-v0.6.0-windows-x86_64.zip
- Linux x86-64: craftpulse-v0.6.0-linux-x86_64.tar.gz
- Arch Linux x86-64: craftpulse-v0.6.0-archlinux-x86_64.tar.gz
- macOS Apple Silicon: craftpulse-v0.6.0-macos-arm64.tar.gz
- SHA256SUMS.txt

Verification: 199 local tests, strict Clippy, native WebKit/browser checks and simulated AppImage/Discord integration passed. Windows, macOS, Ubuntu and Arch CI passed for the feature commit; release CI repeats the platform gates before publishing. Real Omarchy/Hyprland and interactive Windows/macOS sessions remain unverified. The six new apps default to process/window detection, without assumed live API support.

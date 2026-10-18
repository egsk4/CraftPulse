# Developing CraftPulse

Notes for working on the code. For using the app, see [README.md](README.md).

CraftPulse is early and has bugs. The current focus is **PhotoCraft** and **LightCraft**, so fixes and improvements for those two are the most useful.

## Setup

You need the [Rust toolchain](https://rustup.rs/) (stable) and Git. Windows provides the broadest window-accessibility integration (`build = "src/build.rs"` in `Cargo.toml` is what embeds the exe icon; without it the exe has the generic icon), while Linux uses compositor helpers and app control APIs.

```powershell
git clone https://github.com/egsk4/CraftPulse.git
cd CraftPulse
cargo run
```

Run the Discord desktop app and one of the Craft apps to see presence working end to end.

| Command | What it does |
| --- | --- |
| `cargo run` | Debug build. Keeps a console window |
| `cargo build --release --locked` | Optimised build (LTO, stripped). Has **no console window** |
| `cargo test --locked` | Runs the unit tests |
| `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` | Format and lint before opening a pull request |

Some tests only run on Linux (a fake Discord socket and a fake `xprop`), so they are skipped on Windows.

## How it works

`main.rs` runs a loop every 500 ms:

1. Reload `config.toml` if it changed on disk.
2. `ProcessDetector::detect` finds the Craft app to show, its project name and any detected activity.
3. Read the app's live state from the control channel (`ControlHub`), then an optional hint file. Live state wins over window guessing.
4. `privacy::make_card` builds the Discord card and applies the hiding rules.
5. If the card changed, send it to Discord through the rate limiter. Otherwise resend it every 10 seconds, which also restores it after Discord is refreshed. While Discord is unreachable, retry every 3 seconds without giving up.

### Threads

| Thread | Job |
| --- | --- |
| Main | The polling loop above |
| `craftpulse-discord-rpc` | Owns the Discord IPC connection. Requests time out after 4 seconds, and the worker is recreated if it hangs |
| `craftpulse-control` | Reads the chosen app's state over its control channel, so a slow app can't stall the main loop |
| `craftpulse-settings` | Local HTTP server for the settings page |
| `craftpulse-tray` | Tray icon and menu (a `tao` event loop, created with `with_any_thread(true)` on Windows) |

### Source layout

| File | Purpose |
| --- | --- |
| `src/main.rs` | Main loop, retry and timing logic, default activity per app |
| `src/control.rs` | Live detection: talks to each app's JSON-lines control channel (read-only), maps tools, dialogs and documents to activity text, and powers the settings page's live status |
| `src/update.rs` | Background check of GitHub releases (`/releases`); only notifies, never installs. The crate version in `Cargo.toml` must match the release you ship |
| `src/quiet.rs` | Quiet-hours window check (uses local time via `chrono`) |
| `src/milestones.rs` | Hour marks and export counts for the optional card notes |
| `src/history.rs` | In-memory recent activity and the open document (for "Hide this document") |
| `src/pause.rs` | Shared pause state (tray menu, settings page, main loop) |
| `src/stats.rs` | Local per-app usage time, saved to `stats.json` |
| `src/notify.rs` | Windows toast notifications (restart reminder) |
| `src/detection.rs` | App, window title, dialog and UI Automation detection |
| `src/config.rs` | `config.toml` structs, loading, comment-preserving saving, live reload |
| `src/privacy.rs` | Builds `PresenceCard`, applies hide settings and regex patterns |
| `src/presence.rs` | Discord IPC worker and `PresenceCard` |
| `src/settings.rs` | Local settings server and JSON API |
| `src/hints.rs` | Reads hint files from the `hints` folder |
| `src/rate_limit.rs` | Sliding-window limiter for Discord updates |
| `src/startup.rs` | Launch at login for Windows, macOS and Linux |
| `src/tray.rs` | System tray |
| `src/tests.rs` | Linux-only fake Discord socket test |
| `web/settings.html` | The settings page, embedded into the binary at build time |

Because `settings.html` is embedded with `include_str!`, you must rebuild after editing it.

### Detection in more detail

`ProcessDetector::detect` does this:

1. Lists running processes and matches them to the supported app names (`PhotoCraft` matches `PhotoCraft.exe`, but not `PhotoCraftBeta.exe`).
2. Picks the app with focus. If a non-Craft window has focus, it stays on the last app while it's running, otherwise it picks the newest one.
3. Reads the visible window titles for that process. A title that starts with a keyword in `DIALOG_ACTIVITIES` (and doesn't contain the app name) is treated as a dialog, which sets the activity (for example `Export As` becomes "Exporting"). The main window's title, minus the app name, is the project name.
4. On Windows, every 2 seconds, reads the window's accessibility tree to find:
   - open document tabs, for the project name (labels like `Poster.png @ 78.9% (RGB/8)*` are cleaned to `Poster.png`)
   - the selected button that matches an activity rule
   - for LightCraft, the selected photo and the file name shown in the Info panel
5. Marks the user idle after the configured timeout (5 minutes by default; zero disables idle).

Activity priority is set in `main.rs`: hint file, per-app activity from settings, dialog, button rule, idle, then the default.

macOS reads window titles through `osascript`. Linux uses compositor helpers on
Wayland and `xprop` on X11; it gets detailed tools, panels and documents from
the apps' live control APIs instead of Windows UI Automation.

## Common changes

### Add a built-in button rule

Add a row to `BUILTIN_RULES` in `src/detection.rs`, and update the array length in its type (`[(&str, &str, &str); N]`). Earlier rows win. `element` is matched as a case-insensitive prefix of the button name.

Only these UI Automation control types are considered: button, checkbox, list item, radio button and tab item. The control must also report that it is selected or toggled on.

### Add a dialog keyword

Add a row to `DIALOG_ACTIVITIES` in `src/detection.rs` and update its length. Rows are checked in order, so put more specific keywords (like `save as`) before general ones (`save`).

### Support a new Craft app

1. Add the name to `APP_NAMES` in `src/main.rs` and update the array length.
2. Add it to `Config::default` and to the TOML in `default_document` in `src/config.rs`.
3. Add its default activity text in `default_activity` in `src/main.rs`.
4. Add a test module with `app_suffix_tests!("NewCraft")` at the bottom of `src/detection.rs`.
5. Update the app count asserted in the `config.rs` tests.
6. Make sure the matching `large_image_key` image exists as an asset on the Discord application (ID `1558510318513033329`).

### Add a setting

1. Add the field to `Config` in `src/config.rs`, and write it in `save_to` and `default_document`.
2. Add it to `SettingsUpdate` and `SettingsResponse`, and apply it in the `PUT /api/settings` handler in `src/settings.rs`.
3. Add the control in `web/settings.html`.

Use `#[serde(default)]` behaviour so older `config.toml` files without the new key still load.

## Settings server API

The server binds to `127.0.0.1` on a random port. The URL, including a random token, is built in `SettingsServer::url` and opened from the tray. `GET /` serves the page, and every `/api/*` route needs the token in the `X-CraftPulse-Token` header or a `?token=` query parameter.

| Route | Purpose |
| --- | --- |
| `GET /api/settings` | Current settings |
| `PUT /api/settings` | Update settings (invalid regular expressions are rejected with a 400) |
| `GET /api/diagnostics` | Config path, hints path and the last card sent |
| `GET /api/log` | Recent log lines (the last 200 are kept in memory) |
| `POST /api/test-action` | Sends a test card to Discord for 10 seconds |

## Discord behaviour

- Application ID `1558510318513033329` (`application_id` in `config.toml`, also a constant in `presence.rs`).
- Updates are limited to 4 per 20 seconds (`rate_limit.rs`). A changed card is sent at most every 5 seconds, and an unchanged one is refreshed every 10 seconds.
- If Discord isn't available, sending is retried every 3 seconds without a cutoff; RPC requests time out after 4 seconds.
- Document names are truncated to 128 characters, and states to 100 plus the `in <App> — ` prefix.

## Debugging

- Open the settings page from the tray and check **Diagnostics & activity log**. It shows the windows seen, UI Automation notes (such as `UIA: no tabs found`), Discord retries and unrecognised processes with "craft" in the name.
- To force a state quickly without the app, drop a hint file in the `hints` folder (see the README).
- `config.toml` reloads automatically, so you can change rules and privacy settings while the app is running.

## Releasing

1. Update `version` in `Cargo.toml`.
2. Run `cargo test` and `cargo build --release --locked`.
3. Create a GitHub release with a tag like `v0.1.0` and attach `target/release/craftpulse.exe`.

Don't commit `target/` or your own `config.toml`.

## Pull requests

- Keep changes small and focused.
- Run `cargo fmt`, `cargo clippy` and `cargo test`.
- Say which Craft app and version you tested against, since detection depends on each app's UI.
- Bug reports are welcome as [issues](https://github.com/egsk4/CraftPulse/issues).

## Live detection (control channel)

Each Craft app can open a loopback JSON-lines control channel. CraftPulse reads it with only read-only queries (`ui.inspect`, `ui.state`, and `engine.execute` with `photo.inspect` / `project.inspect`). The protocol comes from each app's own `docs/control-protocol.md`.

| App | Port (default) | Auth | How it is turned on |
| --- | --- | --- | --- |
| LightCraft | 7980 | none | `LIGHTCRAFT_CONTROL_PORT` or `--control` |
| VectorCraft | 7979 | none | `VECTORCRAFT_CONTROL_PORT` |
| DesignCraft | 7981 | none | `DESIGNCRAFT_CONTROL_PORT` |
| FilmCraft | 9876 | none | `FILMCRAFT_CONTROL_PORT` |
| EffectCraft | 9877 | none | `EFFECTCRAFT_CONTROL_PORT` |
| PhotoCraft | 7878 | bearer token file | `PHOTOCRAFT_CONTROL_PORT` + `PHOTOCRAFT_CONTROL_TOKEN_FILE` |
| PdfCraft | random | token in a JSON file | `pdfcraft --control <file>` only |

The settings page's **Turn on live detection** button (and the first-run prompt) sets these variables with `setx` on Windows. Apps read them at launch, so they must be restarted once. `ControlHub::needs_restart` detects an app that is running but unreachable and triggers the reminder.

Reply shapes differ per app, so `state_from_inspect` in `control.rs` matches fields loosely. The activity log prints each app's `ui.inspect` field names on first connect, which is the fastest way to fix a wrong mapping. Cloning an app's repo and reading its `crates/ui-egui/src/control.rs` `inspect` function shows the exact fields.


## Linux regression checks

Linux builds require WebKitGTK 4.1 development files in addition to GTK 3, Ayatana AppIndicator and libxdo. On Debian: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev`.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
```

With a running Wayland compositor implementing ext-idle-notify (tested on headless Sway):

```sh
cargo test --locked wayland_idle -- --ignored --test-threads=1
```

With a display, D-Bus session and working WebKitGTK subprocess paths:

```sh
dbus-run-session -- cargo test --locked linux_settings_window -- --ignored --test-threads=1
dbus-run-session -- python3 scripts/smoke_linux.py
```

The native-window test loads the real authenticated settings page in WebKitGTK and verifies safe button rendering. The Python smoke test starts an isolated config, a simulated AppImage payload and a fake Discord IPC server. On Wayland it verifies idle status, then disables idle and verifies normal activity plus button data. It also checks settings validation, persistence and reporting a failed runtime environment update. These simulations do not replace tests with real ArtCraft applications or Discord.

Live user rules can match the active tool, panel or view returned by the control channel. Linux does not emulate the entire Windows accessibility tree. Helper commands have bounded execution/output; a missing or stuck helper cannot block the main polling loop.


## Creating a release

The current release is **v0.6.0**. GitHub Actions publishes the release and
four executable archives only after the cross-platform and Arch checks pass.

1. Update the package version in `Cargo.toml` and its root entry in `Cargo.lock`.
2. Update `RELEASE_NOTES.md`, the README download table and release links.
3. Run `cargo fmt --check`, `cargo test --locked` and
   `cargo clippy --locked --all-targets -- -D warnings`. For Linux GUI and
   AppImage checks, run `bash scripts/check_arch.sh` on Arch as an ordinary user.
4. Commit and push to main, then tag that exact commit. Tags follow the existing
   naming convention, for example `CraftPulse-v0.6.0`, and must match Cargo.toml.

```sh
git tag -s CraftPulse-v0.6.0 -m "CraftPulse v0.6.0"
git push origin CraftPulse-v0.6.0
```

`.github/workflows/release.yml` calls the reusable build and Arch workflows.
`scripts/package_release.py` validates the tag, packages each compiled executable
with its license, preserves executable permissions in Unix archives, and writes
`SHA256SUMS.txt`. The publication job uses GitHub's job-scoped token with
`contents: write`; no personal access token is required. Builds and tests use
read-only repository access. Tags without a suffix publish an official release; tags with a hyphenated version suffix publish a prerelease.

Publication keeps an existing release's downloads and notes unchanged. Release
runs for the same tag are serialized to avoid concurrent publication.

To add a signature to an existing tag, keep its target commit unchanged. Historical
tags run the workflow stored at their target commit, so updating this workflow on
main does not protect older tags. Temporarily disable the **Release** workflow in
GitHub Actions before pushing replacement signatures, then enable it again after
both pushes succeed. Use an explicit `--force-with-lease` for each tag so another
person's tag update cannot be overwritten accidentally.

If a check fails, fix the failure before publishing. Do not replace an existing
release tag to retry a source change; use a new patch version. A failed workflow
can be rerun for a transient service problem. Windows/macOS CI builds do not
substitute for testing real ArtCraft applications on those platforms.

Windows idle is latched for the selected ArtCraft process while another app has
focus. Global input in Discord must not resume its presence. Focus returning to
ArtCraft, selecting a different app, closing the app or disabling idle resets the
latch. Regression tests cover these transitions in `src/detection.rs`.

## Stable-release reliability checks

- `storage::atomic_write` syncs a same-directory temporary file before replacing its destination; `tempfile` handles replacement on Windows too. Config saves keep a valid previous version in `config.toml.bak`. Stats use the same atomic-write helper.
- Failed settings saves keep the running config unchanged. Invalid startup configs disable sharing and show a warning. Explicit backup restoration preserves the old file and leaves sharing disabled.
- `instance` holds an OS file lock for the app lifetime and explicitly unlocks on drop. Test it with a second instance and after normal exit. Never delete the lock file to force another running instance.
- Usage counting must require enabled sharing, an enabled app, nonidle input, no manual pause and no automatic pause. Test cards must not bypass global disable or pause.
- Per-app `idle_after_secs` overrides the global threshold; null/omitted means inherit. The API validates 0–86400 seconds.
- Activity rules are limited to 100, with a supported app and 1–100 character control/activity text. The editor preserves list priority. General settings autosave excludes unsaved rule edits.
- The resource monitor uses existing `sysinfo` support and samples only this PID every five seconds. Its CPU percentage is per logical core and excludes WebView children. The first sample is marked unavailable until a CPU interval exists.
- Support export uses an allowlist of fields, never the full config or event log. Do not add document names, user paths, tokens or private button URLs.

`scripts/smoke_linux.py` verifies rules, app idle overrides, update channels, monitor data, sanitized report fields, second-instance exit, failed-save rollback and config backup restoration, alongside the existing simulated AppImage/Discord checks. Native WebKit tests exercise the rendered rule controls and monitor UI. Run `scripts/check_arch.sh` for the complete Arch validation including Sway idle and GUI checks.

The detection loop backs off from 500 ms to 2 seconds when no app is detected or sharing is disabled. Settings changes and newly started apps may take up to that interval to reach the detection loop.

For performance measurements, use an optimized build with isolated config, update checks disabled, no ArtCraft app or Discord and the settings window closed. Allow startup to settle, then collect at least six monitor samples five seconds apart. Report the desktop, build, sampling window and excluded helper processes; avoid universal RAM/CPU promises from one machine.

## Settings UI maintenance

Keep `web/settings.html` readable; do not minify the source. Format it with
Prettier 3.6.2 using `--html-whitespace-sensitivity ignore`. No formatter package
is required at runtime.

The app-card idle selector maps Use global to null, Never to zero and Custom to
1–86400 seconds. This preserves the existing config/API representation. Check
both a full settings window and the narrow tray popup when changing this layout.
Card-style buttons only populate draft template fields; Save persists them.
Creative sparks are local UI text and must never send settings or activity requests.

## App registry and pinning (v0.6.0)

`APP_NAMES` is the detection registry and source of default app settings. Startup
and config reload fill missing built-in app entries while preserving existing
ones. The six additions from issue #2 default to window detection, control port
zero and published HTTPS icons. Verify a real app's protocol before enabling or
claiming live API support. Never invent a port to make a settings badge look ready.

`preferred_app` in TOML is optional. The settings API accepts a supported app name
or an empty string to restore Automatic. Pinning selects only that app, retains
its current process when possible and clears presence if it closes or is disabled.
Windows idle resumes only when the selected logical app has focus, including its
helper-process windows. Disabled apps are excluded before selection.

The extended Linux smoke test runs all six additional names as generic AppImage
payloads, checks their default activity/icon on a mock Discord socket and checks
that closing a pinned app cannot fall back to another running app. These fixtures
are not real Omarchy, Hyprland, ArtCraft or Discord desktop sessions.

## Detection inspection and rule previews (v0.6.0)

The detector records selection reasons from the same branch that chooses an app.
The main loop records activity source alongside the chosen activity, preserving
hint/override/busy/dialog/idle/rule/fallback precedence. It publishes one owned
`DetectionInspection` snapshot per loop via the settings server, with no additional
polling thread. Sharing and IPC write status are distinct from detection.

`RequestState` groups settings-handler shared state. The authenticated diagnostics
endpoint includes the inspection snapshot; the allowlisted support export does not.
Do not add live control names to support reports or include the full inspection.

`rules::matching_rule` is shared by live runtime matching and the read-only
`POST /api/rules/test` endpoint. Window mode models the case-insensitive literal
prefix used for selected UI Automation controls; live mode normalizes punctuation
and parenthesized shortcut labels. Draft tests do not inspect selection flags,
include built-in rules, persist configuration or send Discord activity. Requests
are limited to 64 KiB, 100 rules and 32 controls of up to 200 characters.

# CraftPulse

**Discord Rich Presence for the [ArtCraft](https://getartcraft.com/apps) creative apps.**

CraftPulse runs quietly in your system tray, notices which Craft app you're using, and shows it on your Discord profile: the project you're working on and what you're doing in it.

```
Poster concept
in PhotoCraft — Editing
```

Everything happens locally on your machine. The only thing sent anywhere is the presence card to your own Discord client.

> [!WARNING]
> **CraftPulse is in very early development and has a lot of bugs right now.** Detection can be wrong, miss things, or lag behind what you're doing. We're focusing on **PhotoCraft** and **LightCraft** first, and will get to the rest of the Craft apps after that. If you hit a bug, please [open an issue](https://github.com/egsk4/CraftPulse/issues), or send a [pull request](https://github.com/egsk4/CraftPulse/pulls) if you've fixed it yourself.

## Supported apps

The seven apps below are recognised, but **PhotoCraft and LightCraft are the current focus**. The others work at a basic level (app, project name, dialogs) and will get more attention later.

| App | What it is | Built-in activity |
| --- | --- | --- |
| [PhotoCraft](https://getartcraft.com/apps/photocraft) | Image editor (current focus) | Editing an image |
| [VectorCraft](https://getartcraft.com/apps/vectorcraft) | Vector illustration | Drawing vectors |
| [FilmCraft](https://getartcraft.com/apps/filmcraft) | Video editor | Editing a video |
| [LightCraft](https://getartcraft.com/apps/lightcraft) | Photo library and raw developer (current focus) | Developing photos, plus [panel-aware activities](#built-in-rules-lightcraft) |
| [PdfCraft](https://getartcraft.com/apps/pdfcraft) | PDF workbench | Working on a PDF |
| [EffectCraft](https://getartcraft.com/apps/effectcraft) | Motion graphics and VFX | Making motion graphics |
| [DesignCraft](https://getartcraft.com/apps/designcraft) | Page layout and publishing | Designing a layout |

## Features

- **Automatic detection** of the Craft app you're using. If several are open, the one with focus wins.
- **Project name on your card**, read from the window title and, on Windows, from the app's open document tabs.
- **Activity detection**: dialogs like Export, Render, Save, Print, Import and Open are recognised, and you can map selected buttons or tabs to your own activity text.
- **Idle status** after 5 minutes without keyboard or mouse input.
- **Privacy controls**: hide project names everywhere, per app, or whenever a name matches a regular expression.
- **Local settings page** with a live card preview, per-app overrides, diagnostics and an activity log.
- **Tray app**: no console window. Left-click the tray icon to open settings, right-click for the menu.
- **Launch at login** (Windows registry, macOS LaunchAgent or Linux autostart entry).
- **Live config reload**: edit `config.toml` and changes are picked up automatically.

## Install

### Prebuilt (recommended)

1. Download the latest `craftpulse` executable from the [Releases page](https://github.com/egsk4/CraftPulse/releases).
2. Run it. A CraftPulse icon appears in your system tray.
3. Make sure the Discord desktop app is running, then open one of the Craft apps.

### Build from source

Requires the [Rust toolchain](https://rustup.rs/) (stable).

```powershell
git clone https://github.com/egsk4/CraftPulse.git
cd CraftPulse
cargo build --release
```

The binary is written to `target/release/craftpulse`. To stop a running copy before rebuilding, quit it from the tray menu (or run `Stop-Process -Name craftpulse -Force`).

On Linux, the tray icon libraries typically need GTK and appindicator development packages installed.

## Platform support

| Platform | Support |
| --- | --- |
| Windows | Full: app detection, window titles, document tabs, dialog activity, button/tab rules, idle detection |
| macOS | App detection and window title only |
| Linux | App detection and window title only (uses `xprop` under X11) |

Button and tab rules, document-tab names and idle detection use Windows UI Automation, so they are Windows only.

## Settings

Open the settings page from the tray icon (left-click, or **Open CraftPulse settings** in the menu). It runs on a random local port protected by a one-time token, so only your own browser session can use it.

| Setting | What it does |
| --- | --- |
| **Enable Rich Presence** | Pauses or resumes all CraftPulse activity |
| **Hide project names** | Shows "Private project" in place of document names |
| **Launch at login** | Starts CraftPulse when you sign in |
| **Default activity** | Fallback text when nothing more specific is detected. Left as `Creating`, each app uses its own built-in default (see the table above) |
| **Hide names matching these regular expressions** | One pattern per line, case-insensitive. Matching document names show as "Private project" |
| **Test an activity** | Sends your text to Discord as a test card for 10 seconds, and shows it in the preview |
| **Per-app overrides** | Per app: turn it on or off, set privacy to *inherit*, *always hide* or *show name*, and set a fixed activity text |

A per-app activity text always replaces the detected activity for that app, so leave it blank if you want automatic detection.

The page also has a **Diagnostics & activity log** panel showing the config path, the last card sent and recent events (windows seen, Discord connection retries, unrecognised Craft processes).

## How the card is built

| Discord field | Content |
| --- | --- |
| Top line | Project name, or `Private project` if hidden, or `Working` if none was found |
| Second line | `in <App> — <activity>` |
| Large image | The app's icon (`large_image_key`) with the app name as hover text |
| Small image | ArtCraft badge |

The activity is chosen in this order, first match wins:

1. An **activity hint file** (see [Integrations](#integrations))
2. The **per-app activity text** from settings
3. A detected **dialog**: Export, Render, Save, Print, Import or Open
4. A matching **button or tab rule**
5. **Idle**, after 5 minutes without input
6. The **default activity**

To respect Discord's limits, updates are rate limited to 4 per 20 seconds, a changed card is sent at most every 5 seconds, and the presence is refreshed every 30 seconds. If Discord isn't running, CraftPulse retries every 30 seconds for up to 5 minutes after a Craft app appears.

## Configuration

Settings are stored in `config.toml`:

| OS | Location |
| --- | --- |
| Windows | `%APPDATA%\CraftPulse\config.toml` |
| macOS | `~/Library/Application Support/CraftPulse/config.toml` |
| Linux | `~/.config/CraftPulse/config.toml` |

The file is created on first run. Saving from the settings page keeps your comments and formatting.

```toml
enabled = true
hide_project_name = false
hide_patterns = ["confidential", "client-.*"]
action = "Creating"
launch_at_login = false

[apps.PhotoCraft]
enabled = true
large_image_key = "photocraft"
# hide_project_name = true   # per-app privacy override
# action = "Painting"        # fixed activity for this app

[[rules]]
app = "LightCraft"
element = "Info (I)"
activity = "Viewing photo info"
```

### Button and tab rules (Windows)

A rule shows `activity` while a button or tab in `app` is selected and its name **starts with** `element` (case-insensitive).

- Rules are added by editing `config.toml`; the settings page doesn't manage them.
- Your rules are checked before the built-in ones, and earlier rules win.
- Only button-like controls (buttons, checkboxes, radio buttons, list items and tabs) that are currently selected or toggled on are matched.
- To find the exact button names an app exposes, use a UI Automation inspector such as [Accessibility Insights for Windows](https://accessibilityinsights.io/).

### Built-in rules: LightCraft

Listed in priority order. The first selected match wins, so the grid views take precedence over Edit.

| Button | Activity |
| --- | --- |
| Compare | Comparing photos |
| Survey | Surveying photos |
| People | Organising people |
| Photo Grid, Square Grid | Browsing the library |
| Crop & Rotate | Cropping a photo |
| Remove (H) | Retouching a photo |
| Masking (M) | Masking a photo |
| Red Eye | Fixing red eye |
| Versions | Managing versions |
| History & Activity | Reviewing edit history |
| Keywords | Tagging photos |
| Info (I) | Viewing photo info |
| Presets | Applying presets |
| Edit (E) | Editing a photo |

LightCraft only exposes the open photo's file name while its **Info** panel (`I`) is open. CraftPulse remembers the last name it saw, so it can be stale if you switch photos with Info closed.

## Integrations

Apps and scripts can push a status directly by writing a small JSON file to the `hints` folder next to `config.toml`:

```
%APPDATA%\CraftPulse\hints\PhotoCraft.json
```

```json
{ "action": "Exporting", "document": "Cover" }
```

Both fields are optional and limited to 100 characters. A hint is only used while the file was modified in the last 2 minutes, and its values take priority over everything detected automatically.

## Privacy

- Detection and settings are entirely local. There is no account, telemetry or cloud service.
- The settings server only listens on `127.0.0.1` and requires a random per-launch token.
- Discord receives only the card described above. Turn on **Hide project names** or add hide patterns to keep project names off it.

## Troubleshooting

- **Nothing shows on Discord**: the Discord desktop app must be running, **Enable Rich Presence** must be on, and the app must be enabled in per-app overrides. Check the activity log for `Discord is unavailable; will retry in 30 seconds`.
- **Card shows "Working"**: no project name was found. Opening or saving a document usually fixes it.
- **Wrong or missing activity**: add a rule in `config.toml`, or clear the per-app activity text, which overrides detection.
- **A Craft app isn't detected**: the log lists unrecognised processes with "craft" in their name. Only the seven apps above are matched.

## Development

```
src/
  main.rs         Main loop, presence timing and retry logic
  detection.rs    Process, window title, dialog and UI Automation detection
  config.rs       config.toml loading, saving and live reload
  privacy.rs      Builds the card and applies hiding rules
  presence.rs     Discord IPC worker
  settings.rs     Local settings server and API
  hints.rs        Hint file reader
  rate_limit.rs   Discord update rate limiter
  startup.rs      Launch-at-login on Windows, macOS and Linux
  tray.rs         System tray icon and menu
web/
  settings.html   Settings page
```

```powershell
cargo test
```

## Reporting bugs and contributing

CraftPulse is early, so bugs are expected and reports help a lot.

- **Found a bug?** [Open an issue](https://github.com/egsk4/CraftPulse/issues). Include which Craft app and version you were using, what you were doing, what the Discord card showed versus what you expected, and the recent events from the settings page's activity log.
- **Fixed something or added a feature?** Send a [pull request](https://github.com/egsk4/CraftPulse/pulls). Small, focused changes are easiest to review. Run `cargo test` first.
- **Want better activity text for an app?** Rules for more apps and buttons are especially welcome.

## License

MIT

## Live detection (recommended)

CraftPulse can ask each Craft app directly what you're doing (tool, dialog, open document, and in LightCraft the selected photo) instead of guessing from window titles. Click **Turn on live detection** in the settings page, then restart your Craft apps once. This sets the apps' own `*_CONTROL_PORT` environment variables (PhotoCraft also gets a private token file). PdfCraft has no variable: start it with `pdfcraft --control "%USERPROFILE%\.pdfcraft-control.json"`. Only read-only queries are ever sent, and the channel listens on `127.0.0.1` only. If an app isn't reachable, CraftPulse falls back to window detection.

## Timer and themes

The settings page has an **Elapsed timer** switch (Off, Session, Document) and a **Theme** switch (System, Dark, Light, Mono). Session counts from when the app opened; Document restarts when you switch files.

## Updates and extra details

CraftPulse checks GitHub for a newer release (switch it off under **Check for updates**) and tells you when one exists. It never installs anything by itself. **Show extra details** adds things like "Page 3 of 12" (PdfCraft), image size (PhotoCraft) and size plus star rating (LightCraft) to the document line.

## Card text, buttons, pause and stats

- **Card:** change the two text lines with placeholders `{app}`, `{activity}`, `{document}` and `{detail}`, and add up to two link buttons (https links only).
- **Pause sharing:** from the tray menu or the settings page (1 hour, or until you resume).
- **Time in apps:** shows how long you were active in each Craft app today and over 7 days. It is stored only in `stats.json` next to `config.toml`; use **Clear** to erase it.

## More controls

- **Quiet hours:** stop sharing automatically during a daily window (it can cross midnight, e.g. 22:00 to 07:00).
- **Pause while streaming or recording:** stops sharing while OBS, Streamlabs, XSplit, Twitch Studio, Zoom screen share or Medal is running.
- **Hide this document:** one click on the settings page hides the open document's name from now on.
- **Milestone notes:** optional short notes such as "2h in" or "export #5".
- **Recent activity:** the last things your card showed, kept in memory only.
- **Export / Import layout:** share your card text, buttons, timer and theme as a small `.json` file. It never includes your hide patterns or app settings.

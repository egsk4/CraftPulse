# Linux changes

Review notes for the Linux branch. Using the app is still described in [README.md](README.md). Day-to-day development is still described in [DEVELOPING.md](DEVELOPING.md). This file is the list of what changed, why, and how it was checked.

Base commit: `7d29a15` (`CraftPulse v0.4.0-alpha`) on `main`.

Linux support was already in the tree. This branch does not replace it. It fixes three gaps that showed up when the Linux build was actually run: the tray would not start, most AppImages were invisible, and Discord drew the ArtCraft card for any app whose icon had not been uploaded.

## What was already there

Leave these alone unless a bug in them is reproduced.

- Discord IPC on Linux is a Unix socket. `discord-rich-presence` 0.2.5 looks in `$XDG_RUNTIME_DIR/discord-ipc-N`, including the Flatpak and snap paths.
- Window titles on Linux already come from `xprop` (`_NET_CLIENT_LIST`). macOS uses `osascript`. Windows uses UI Automation.
- Launch at login on Linux already writes an autostart `.desktop` entry (`src/startup.rs`).
- `src/tests.rs` already has a Linux-only fake Discord socket test. `src/detection.rs` already has a Linux-only fake `xprop` test.
- `workflows/build.yml` already has an Ubuntu job. That file lives at `workflows/build.yml`, not `.github/workflows/`, and its path filter is `craftpulse/**` with `working-directory: craftpulse`. In this repository the crate is at the repo root, so that workflow does not run as GitHub Actions here. It was not moved.

Windows-only pieces were left Windows-only: UI Automation (document tabs, button and tab rules), idle time, toast notifications, and the WebView settings popup. On Linux the foreground PID is `None`, so when several Craft apps are open the newest one is chosen. That selection rule was already there.

## Bug 1: the tray did not start

`src/tray.rs` runs the tray on a background thread. `tao` 0.34 refuses to build an event loop off the main thread unless `with_any_thread(true)` is set. Windows already did that through `EventLoopBuilderExtWindows`. Linux now does the same through `tao::platform::unix::EventLoopBuilderExtUnix`.

`tray-icon` 0.21 has one default feature, `libxdo`. It only feeds the predefined Cut, Copy, and Paste menu items in `muda`. This app does not use those items. The feature links `-lxdo`. A machine can have GTK 3 and libappindicator and still have no `libxdo` (it may have `libxdot` instead). `Cargo.toml` turns the default features off:

```toml
tray-icon = { version = "0.21", default-features = false }
```

`libappindicator` is a normal Linux dependency of `tray-icon`, not a feature, so the tray icon still builds. `Cargo.lock` drops `libxdo` and `libxdo-sys`. The release binary links `libgtk-3` and `libgio` and does not link `libxdo`.

`workflows/build.yml` still installs `libxdo-dev` on Ubuntu. That package is unused by this build. It was left in place so the workflow diff stays out of this change.

Build packages that are still required on Linux: GTK 3 and appindicator (`libayatana-appindicator3-dev` or `libappindicator3-dev`, plus the GTK and X11 packages the README already names).

## Bug 2: only PhotoCraft was detected

`sysinfo` 0.33 `Process::name()` is the kernel comm from `/proc/<pid>/comm`. That string is at most 15 bytes. `Process::exe()` is the resolved executable. `refresh_processes` fills `exe` and does not fill the command line by default.

ArtCraft ships AppImages.

- PhotoCraft's `AppRun` is a shell script that execs `photocraft`. Its comm is `photocraft`, so the old exact-name match worked.
- LightCraft, WordCraft, GridCraft, DeckCraft, and the other apps in the same packaging use an `AppRun` symlink to `usr/bin/<app>`. Their comm is the literal string `AppRun`. The real name is the exe path, for example `/tmp/.mount_.../usr/bin/lightcraft`. The old matcher never saw them.
- A versioned file name looks like `lightcraft-0.8.0-linux-x86_64.AppImage`. Truncated to 15 bytes, the comm is `lightcraft-0.8.`.

`process_matches` in `src/detection.rs` now:

1. Trims a trailing ` (deleted)`.
2. Uses the file name when the string is a path.
3. Strips a trailing `.exe` or `.AppImage`, case-insensitively.
4. Accepts an exact match.
5. Accepts a version suffix: a hyphen and then an ASCII digit (`lightcraft-0.8.0-linux-x86_64`, and the truncated `lightcraft-0.8.`).

It rejects a different program that only shares a prefix. `LightCraftBeta`, `lightcraft-cli`, and `AppRun` do not match. The digit requirement is what keeps a name like `artcraft-launcher-0.3.2` from matching an app named `ArtCraft`. There is no `ArtCraft` app in the list. Do not relax this to "any string that starts with the app name".

`ProcessDetector` matches `process.name()` or `process.exe()`. An unmatched process whose name or exe contains `craft`, and does not contain `craftpulse`, is still reported as unknown.

## Bug 3: six ArtCraft apps were missing

The recognised list is the 13 apps on [getartcraft.com/apps](https://getartcraft.com/apps), in that order. The first seven were already present.

| App | Slug | Default activity | In the old list |
| --- | --- | --- | --- |
| PhotoCraft | photocraft | Editing an image | yes |
| VectorCraft | vectorcraft | Drawing vectors | yes |
| FilmCraft | filmcraft | Editing a video | yes |
| LightCraft | lightcraft | Developing photos | yes |
| PdfCraft | pdfcraft | Working on a PDF | yes |
| EffectCraft | effectcraft | Making motion graphics | yes |
| DesignCraft | designcraft | Designing a layout | yes |
| FigCraft | figcraft | Designing an interface | added |
| WordCraft | wordcraft | Writing a document | added |
| SoundCraft | soundcraft | Editing audio | added |
| DeckCraft | deckcraft | Editing a presentation | added |
| CADCraft | cadcraft | Drafting | added |
| GridCraft | gridcraft | Editing a spreadsheet | added |

The same 13 names are in `APP_NAMES` (`src/main.rs`), `Config::default` (`src/config.rs`), the embedded default `config.toml` (`default_document` in `src/config.rs`), and the README table. `default_activity` has the six new phrases. CADCraft's slug is `cadcraft` (letters only, lowercased).

`Config::ensure_builtin_apps` runs from `load_or_create` and `reload_if_changed`. An older `config.toml` that only lists the original apps gains the missing ones in memory. Apps already in the file are not overwritten, including a user who turned one off. The file is not rewritten until a settings save. A missing app defaults to enabled through `app_config`.

### Control ports were not guessed

`default_control_port` is unchanged. These are the ports the apps document:

| App | Port |
| --- | --- |
| PhotoCraft | 7878 |
| VectorCraft | 7979 |
| LightCraft | 7980 |
| DesignCraft | 7981 |
| FilmCraft | 9876 |
| EffectCraft | 9877 |
| PdfCraft | 0 (the app picks a random port and writes a control file) |
| FigCraft, WordCraft, SoundCraft, DeckCraft, CADCraft, GridCraft | 0 |

Port 0 means the control channel is not opened. Live tool, dialog, and document updates from the app do not run. Process detection and the default activity still do. WordCraft, GridCraft, and DeckCraft binaries contain control-channel environment variables, and GridCraft's help text has used `--control 7979`, which is VectorCraft's port. Those were not copied into `default_control_port`. Do not set port 0 into the environment, and do not invent a port to make the control test "cover" the new apps.

`control_ports_are_distinct_and_overridable` still expects 7 distinct ports: the six fixed ports plus the shared 0.

## Bug 4: Word, Deck, and Grid showed the ArtCraft icon and "Playing ArtCraft"

Discord application id is still `1558510318513033329`. There is one Discord application for every Craft app. The header "Playing ArtCraft" is the application name. The header "Playing GridCraft" is the activity `name` field with activity type 0 (`Playing`). The small badge stays the uploaded `artcraft` asset.

`discord-rich-presence`'s `Activity` builder has no `name` field. `set_activity` sends `SET_ACTIVITY` and returns `Ok` without reading Discord's reply. An unknown image key is not an error reply. Discord accepts the activity and removes `large_image`. The card then falls back to the application icon and the application name.

That was measured against a running Discord desktop client on the local IPC socket, by sending `SET_ACTIVITY` and reading the reply.

Keys Discord keeps, and rewrites to a numeric asset id:

- `artcraft` (the small image)
- `photocraft`
- `vectorcraft`
- `filmcraft`
- `lightcraft`
- `pdfcraft`
- `effectcraft`
- `designcraft`

Keys Discord drops, which is why those cards showed ArtCraft:

- `figcraft`
- `wordcraft`
- `soundcraft`
- `deckcraft`
- `cadcraft`
- `gridcraft`

A raw `https://` URL is kept. Discord rewrites it to `mp:external/<hash>/https/<host>/<path>`. A hand-built `mp:external/...` string with a made-up hash is dropped. Do not pre-build that prefix. Send the plain `https://` URL.

The icon used for an app with no uploaded asset is the PNG in that app's repository:

`https://raw.githubusercontent.com/storytold/{slug}/main/assets/app-icon/{slug}-1024.png`

Each of those URLs returned HTTP 200 and a real PNG when this was written (about 30 KB to 300 KB). `https://getartcraft.com/images/apps/{slug}/icon.webp` also returned 200. The code uses the PNG.

A GET of Discord's media CDN copy of the proxied image returned 403. That check does not tell you whether the client will draw the image. The acceptance signal is the `SET_ACTIVITY` reply still containing `large_image`.

### How the payload is built

`discord_large_image(app_name, key)` in `src/presence.rs`:

1. A key that already starts with `https://`, `http://`, or `mp:` is sent unchanged.
2. A key in `UPLOADED_ASSETS` is sent lowercased. Photo, Vector, Film, Light, Pdf, Effect, and Design stay on the uploaded art.
3. An empty key, or a key equal to the app slug, becomes the GitHub PNG above. The slug is the app name with only ASCII letters and digits, lowercased. `CADCraft` becomes `cadcraft`.
4. Any other key is sent unchanged, so a custom key in `config.toml` is still honoured.

`activity_message` builds the JSON itself:

- `name`: the display name, such as `GridCraft`
- `type`: `0`
- `details`, `state`
- `assets.large_image` from `discord_large_image`
- `assets.large_text`: the display name
- `assets.small_image`: the card's small key, normally `artcraft`
- `assets.small_text`: `ArtCraft`
- `timestamps.start` when the card has one
- at most two buttons, `{label, url}`

The worker sends that value with `DiscordIpc::send(payload, 1)`. Opcode 1 is the Discord frame opcode. The same payload is reused for the one reconnect attempt. `clear_activity` and `close` are unchanged.

The production worker does not call `recv` after `send`. The fake Discord server in `src/tests.rs` does not answer `SET_ACTIVITY`. If the worker waited for a reply, that test would block. The ignored live test below does call `recv`, because it is talking to the real client.

An existing config can keep `large_image_key = "gridcraft"`. The rewrite happens at send time, so the config file does not need a new key for the icon to change.

## Tests

`cargo test --offline` on Rust 1.98: 279 passed, 1 ignored.

New or updated checks:

| Test | What it locks |
| --- | --- |
| `presence::tests::uploaded_icons_stay_asset_keys_and_the_rest_use_the_published_png` | Uploaded keys stay keys. Word, Grid, Deck, Fig, Sound, and CAD become the GitHub PNG. An `https://` key and a custom key pass through. |
| `presence::tests::activity_names_the_craft_app_and_points_at_its_icon` | A GridCraft card sends `name` `GridCraft`, type 0, and a large image whose URL contains `gridcraft-1024.png`. The small image stays `artcraft`. |
| `config::tests::default_config_has_all_apps` | 13 apps. |
| `config::tests::default_document_parses` | The embedded toml has 13 apps. |
| `config::tests::older_config_gains_apps_added_later` | A file that only contains PhotoCraft gains Word, Grid, Deck, Fig, Sound, and CAD, and does not turn PhotoCraft off. |
| `config::tests::control_ports_are_distinct_and_overridable` | Still 7 distinct port values. |
| `tests::fake_discord_socket_receives_presence_and_clear_commands` | Also asserts `args.activity.name` is `PhotoCraft`. PhotoCraft's large image stays the key `photocraft`. |
| `detection` suffix modules `word_craft`, `grid_craft`, `deck_craft`, `fig_craft`, `sound_craft`, `cad_craft` | The shared `app_suffix_tests!` cases, including the new AppImage and exe-path cases. |

The ignored test is Linux-only:

`presence::tests::live_discord_keeps_icons_for_apps_without_uploaded_art`

```text
cargo test --offline -- --ignored live_discord --nocapture
```

It needs the Discord desktop app. It connects, sends Word, Grid, Deck, Fig, Sound, CAD, and Photo, reads each reply, and checks:

- `evt` is missing or null (Discord accepted it)
- `data.name` equals the app display name
- `data.assets.large_image` starts with `mp:external/` or is all digits

It waits 300 ms between apps, then clears the activity. This test passed against a local Discord client (about 2 seconds). It checks the IPC reply. It does not screenshot the Discord window. After it runs, presence is cleared until CraftPulse sets it again.

`cargo fmt` was run on the branch.

`cargo clippy --offline --all-targets -- -D warnings` still fails, on lints that were already on `main`: unread fields in `SettingsUpdate` (`src/settings.rs`), `manual_pattern_char_comparison` in `src/control.rs`, and `unnecessary_map_or` plus `type_complexity` in `src/detection.rs` and `src/main.rs`. None of those lines are part of this change, and they were not silenced here. The GitHub workflow that runs clippy is `workflows/build.yml`. It is not in `.github/workflows`, and its path filter points at a nested `craftpulse/` directory this repo does not have, so it does not run on this pull request.

## Known limits

- On a GNOME Wayland session, `xprop` does not see native Wayland window titles. `org.gnome.Shell.Eval` is not used. The document line can stay on the default, which is `Working`, even when the app name and icon are right.
- "Playing GridCraft" is the activity name inside the existing CraftPulse Discord application. It is not a separate Discord application id. The small ArtCraft badge remains.
- The six new apps have no control port here, so live in-app status (selected tool, open document from the control channel) is not available for them yet.
- If Discord cannot fetch the GitHub PNG, it can still accept the activity and then fail to draw the picture. The reply test is the check this branch has. A visual look at the Discord profile is the remaining check.

## Files in this branch

| File | Change |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | `tray-icon` default features off. Lockfile drops `libxdo` and `libxdo-sys`. |
| `src/tray.rs` | Linux `with_any_thread(true)`. |
| `src/detection.rs` | Match the exe path and versioned AppImage names. Tests for the six new apps. |
| `src/main.rs` | `APP_NAMES` and `default_activity` include all 13 apps. |
| `src/config.rs` | Default apps, embedded toml, and `ensure_builtin_apps`. |
| `src/presence.rs` | Activity JSON, icon choice, unit tests, ignored live Discord test. |
| `src/tests.rs` | Fake socket test expects the activity name. |
| `README.md` | Table rows for the six apps. "The seven apps" wording now says "The apps". |
| `DEVELOPING.md` | Tray note includes Linux. Pointer to this file. |
| `LINUX.md` | This file. |

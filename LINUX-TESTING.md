# Linux compatibility and testing

CraftPulse 0.6.0 supports ArtCraft AppImage payloads launched directly or from
ArtCraft Launcher. Detection groups helper processes with their application and
preserves the focused child's window information. The launcher itself is not
reported as a creative app.

See the [README's Linux section](README.md#linux-arch-appimages-and-wayland) for
Arch packages, desktop-specific helpers, live API setup and limitations. Linux
uses `idle_after_secs` (default 300); zero disables idle presence. Its window
inspection features depend on the desktop; app control channels provide tool,
panel and document detail without depending on Windows UI Automation.

## Automated checks

`.github/workflows/build.yml` runs tests, strict Clippy and release builds on
Linux x86-64, Windows x86-64 and macOS ARM64. `.github/workflows/arch.yml` uses
rolling Arch packages and also exercises native settings rendering, Wayland idle
and end-to-end AppImage detection with a mock Discord peer. These checks run
against this repository's code, not the separate CraftPulseee fork.

To run the Arch checks as an ordinary user:

```sh
sudo pacman -S --needed base-devel rust gtk3 webkit2gtk-4.1 libayatana-appindicator xdotool git python sway dbus
bash scripts/check_arch.sh
```

For optional tests against real PhotoCraft, start it with control port 7878 and
a private token file, then run:

```sh
CRAFTPULSE_TEST_PHOTOCRAFT_TOKEN_FILE=/path/to/token cargo test --locked real_photocraft_control_roundtrip -- --ignored --test-threads=1
```

This authenticates and reads `ui.inspect`; it does not modify documents.
`CRAFTPULSE_USE_RUNNING_PHOTOCRAFT=1 python3 scripts/smoke_linux.py` instead
checks an already running PhotoCraft's presence using a mock Discord peer.
The default smoke test creates a simulated AppImage payload.

## Evidence and limits

The integrated 0.4.x code passed 186 local regression tests, strict Clippy and a
debug build. Separate Wayland idle, native WebKit settings rendering and
AppImage/Discord smoke checks also passed against this integrated code.
Three integration tests are ignored by default because they need
a desktop or a running real application; their commands are documented above
and in DEVELOPING.md.

The same Linux detection and control fixes previously passed full Arch CI in
[the fork](https://github.com/egsk4/CraftPulseee/actions/runs/39026769086).
Official launcher AppImages 0.3.2 and 0.4.1 and PhotoCraft 0.9.0 were separately
verified against their published SHA256 values and opened on a Debian/Sway host.
Real PhotoCraft presence and authenticated control API checks passed there.
Those AppImages were extracted because the host has no FUSE; installation
through the offline launcher was not tested. These results do not certify
every desktop, ArtCraft release or kernel. An Arch container shares its host's
kernel. See this repository's Actions runs for the new integration's CI results.

## v0.5.0 stable-release checks

The default suite covers single-instance locking, atomic replacement, backup preservation, invalid startup config fail-closed behavior, rule/idle persistence, update-channel filtering and all usage-counting gates. The smoke test also checks recovery and transaction behavior through the actual local HTTP server. These simulated AppImage and Discord fixtures do not replace testing with every ArtCraft app or desktop.

The built-in resource monitor measures only the CraftPulse process. Native WebView child processes are excluded. An optimized Debian/headless-Sway measurement is recorded below when available; no equivalent Windows/macOS performance measurement has been made in this environment.

### Resource measurement for v0.5.0

On Debian 13 with a headless Sway compositor, an optimized build running through
`proot` for native WebKit library paths, no ArtCraft app or Discord, updates off,
and the settings window closed: six samples at five-second intervals showed
**2.13% mean CPU (100% = one core)** and **51.11 MiB maximum resident memory**.
The stripped executable was **6.46 MiB**. This includes startup in a short sample;
WebView child processes and the compositor are excluded. An earlier run before
background polling backoff observed about 6.84% mean CPU under the same setup.
These are observations from this container, not targets for every system.

### Published release verification

[v0.5.0 release CI](https://github.com/egsk4/CraftPulse/actions/runs/39049795128)
passed the Ubuntu, Windows, macOS and Arch gates before publishing the official
release. Local Linux validation passed 192 default tests; the Wayland idle and
native WebKit integration tests were run separately and passed. The real
PhotoCraft API integration remains an optional, separately invoked test.

All four public binary archives were downloaded and checked against SHA256SUMS,
archive integrity, expected entries and executable format/mode. The downloaded
Linux executable also passed the extended simulated AppImage/Discord smoke test,
including settings authentication, rules, per-app idle, update channels, resource
samples, report export, duplicate-instance exit, failed-save rollback and recovery.
Windows/macOS results are CI coverage; interactive testing on those desktops was
not performed in this environment. This evidence does not imply zero remaining
bugs or support for every ArtCraft app/compositor/version combination.

### v0.6.0 issue #2 coverage

The local build adds the six apps reported missing on Omarchy. The extended
smoke fixture runs WordCraft, DeckCraft, SoundCraft, GridCraft, CADCraft and
FigCraft as generic AppImage payloads and verifies their app-specific activity
and HTTPS icon on a mock Discord socket. It also verifies that a closed pinned
app does not fall back to another running app. Default ports remain zero and
live detection is off for the six additions.

Local checks passed 199 default tests, native WebKit rendering and browser
interaction checks for pinning and draft/saved rule priority. Responsive checks
use embedded WebKit viewports of exactly 1100, 420 and 360 CSS pixels. Published
v0.5.0 and its historical checks above are unchanged. The build matrix and Arch
workflows passed for commit `2e548d1` (confirmed in GitHub Actions by the user).
Windows/macOS coverage is automated CI, not interactive desktop testing. These
changes have not been tested on a real Omarchy/Hyprland desktop.

The same smoke run verifies the detection inspector's selection reason,
hint/override activity priority, closed pinned app and sharing-disabled states.
Draft rule tests cover live versus window matching, authentication, input limits
and unchanged saved rules. Support exports exclude inspector control names.
WebKit interaction checks also cover the draft rule tester and copying detected
controls. Strict Clippy, Rust formatting and settings-page formatting passed.
The resource measurement above predates these local changes; it was not repeated.

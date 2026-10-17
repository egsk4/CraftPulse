# Linux compatibility and testing

CraftPulse 0.4.1 supports ArtCraft AppImage payloads launched directly or from
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

The integrated 0.4.x code passed 184 local regression tests, strict Clippy and a
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

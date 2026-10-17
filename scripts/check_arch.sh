#!/usr/bin/env bash
# Run as an ordinary user in an Arch desktop, or after installing the CI packages.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
cat > "$XDG_RUNTIME_DIR/sway.conf" <<'CONFIG'
output * resolution 1280x720
seat seat0 fallback true
CONFIG
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$XDG_RUNTIME_DIR/sway.log" 2>&1 &
sway_pid=$!
cleanup() {
  status=$?
  if (( status != 0 )); then cat "$XDG_RUNTIME_DIR/sway.log"; fi
  kill "$sway_pid" 2>/dev/null || true
  rm -rf "$XDG_RUNTIME_DIR"
}
trap cleanup EXIT
for attempt in {1..100}; do
  for socket in "$XDG_RUNTIME_DIR"/wayland-*; do
    if [[ -S "$socket" ]]; then export WAYLAND_DISPLAY="$socket"; break 2; fi
  done
  kill -0 "$sway_pid"
  sleep 0.1
done
: "${WAYLAND_DISPLAY:?Sway did not create a socket}"
export XDG_CURRENT_DESKTOP=sway
cargo test --locked wayland_idle_notification_arrives -- --ignored --test-threads=1
dbus-run-session -- cargo test --locked linux_settings_window_renders_preview_buttons -- --ignored --test-threads=1
dbus-run-session -- python scripts/smoke_linux.py
cargo build --locked --release

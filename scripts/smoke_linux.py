#!/usr/bin/env python3
"""Linux functional smoke test; needs a running X11 or Wayland display.

Uses a simulated AppImage payload and Discord IPC server, never real Discord.
All configuration and launcher environment files live in a temporary directory.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


def wait_for(check, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError("Timed out waiting for expected behavior")


def main():
    repo = Path(__file__).resolve().parents[1]
    binary = Path(os.environ.get("CRAFTPULSE_BINARY", repo / "target/debug/craftpulse"))
    assert binary.is_file(), "Run cargo build --locked first"
    assert os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY"), "A display is required"
    with tempfile.TemporaryDirectory(prefix="craftpulse smoke ") as directory:
        root = Path(directory)
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        helpers = root / "bin"
        helpers.mkdir()
        urlfile = root / "settings-url"
        for name, content in {
            "xdg-open": '#!/bin/sh\nprintf "%s" "$1" > "$CRAFTPULSE_SMOKE_URL"\n',
            "systemctl": "#!/bin/sh\nexit 1\n",
        }.items():
            helper = helpers / name
            helper.write_text(content)
            helper.chmod(0o755)
        config = root / "config/CraftPulse/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text('check_updates = false\nlaunch_at_login = false\nidle_after_secs = 1\n')
        appdir = root / "appdir"
        appdir.mkdir()
        payload = appdir / "payload"
        shutil.copy2(shutil.which("sleep"), payload)
        env = dict(os.environ)
        # Preserve a compositor socket even while isolating Discord's runtime dir.
        if env.get("WAYLAND_DISPLAY") and not env["WAYLAND_DISPLAY"].startswith("/"):
            env["WAYLAND_DISPLAY"] = str(Path(env["XDG_RUNTIME_DIR"]) / env["WAYLAND_DISPLAY"])
        env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), CRAFTPULSE_SMOKE_URL=str(urlfile),
                   PATH=str(helpers) + os.pathsep + env["PATH"])
        listener = socket.socket(socket.AF_UNIX)
        listener.bind(str(runtime / "discord-ipc-0"))
        listener.listen()
        listener.settimeout(0.2)
        stopping = threading.Event()
        cards = []
        server_errors = []

        def serve():
            def read_exact(connection, count):
                data = b""
                while len(data) < count:
                    chunk = connection.recv(count - len(data))
                    if not chunk:
                        raise EOFError
                    data += chunk
                return data
            while not stopping.is_set():
                try:
                    connection, _ = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                try:
                    with connection:
                        connection.settimeout(0.3)
                        while not stopping.is_set():
                            try:
                                opcode, length = struct.unpack("<II", read_exact(connection, 8))
                                data = json.loads(read_exact(connection, length))
                            except socket.timeout:
                                continue
                            if opcode == 0:
                                assert data["client_id"] == "1558510318513033329"
                                greeting = json.dumps({"v": 1, "user": {"id": "fake"}}).encode()
                                connection.sendall(struct.pack("<II", 1, len(greeting)) + greeting)
                            elif opcode == 1 and data.get("cmd") == "SET_ACTIVITY":
                                cards.append(data["args"].get("activity"))
                except (EOFError, ConnectionError):
                    pass
                except Exception as error:
                    server_errors.append(error)
        server = threading.Thread(target=serve, daemon=True)
        server.start()
        fake_app = None if os.environ.get("CRAFTPULSE_USE_RUNNING_PHOTOCRAFT") else subprocess.Popen([str(payload), "120"], env={**env, "APPDIR": str(appdir),
            "APPIMAGE": str(root / "PhotoCraft-1.2-x86_64.AppImage")})
        with (root / "app.log").open("w") as log:
            process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
            try:
                wait_for(urlfile.exists)
                url = urllib.parse.urlsplit(urlfile.read_text())
                base = f"http://{url.netloc}"
                token = urllib.parse.parse_qs(url.query)["token"][0]

                def call(path, body=None, authenticated=True):
                    headers = {"X-CraftPulse-Token": token} if authenticated else {}
                    if body is not None:
                        headers["Content-Type"] = "application/json"
                    request = urllib.request.Request(base + path, headers=headers,
                        data=json.dumps(body).encode() if body is not None else None,
                        method="PUT" if path == "/api/settings" and body is not None else None)
                    with urllib.request.urlopen(request, timeout=3) as response:
                        return json.load(response)

                with urllib.request.urlopen(base, timeout=3) as response:
                    assert b'preview-buttons' in response.read()
                try:
                    call("/api/settings", authenticated=False)
                    raise AssertionError("Unauthenticated API request accepted")
                except urllib.error.HTTPError as error:
                    assert error.code == 401
                call("/api/settings", {"idle_after_secs": 1, "buttons": [
                    {"label": "Portfolio", "url": "https://example.com/me"}]})
                assert call("/api/settings")["idle_after_secs"] == 1
                for invalid in (86401, -1):
                    try:
                        call("/api/settings", {"idle_after_secs": invalid})
                        raise AssertionError("Invalid timeout accepted")
                    except urllib.error.HTTPError as error:
                        assert error.code == 400
                assert call("/api/settings")["idle_after_secs"] == 1
                setup = call("/api/live/enable", {})
                assert setup["persisted"] is True and setup["applied"] is False
                saved = (root / "config/environment.d/50-craftpulse.conf").read_text()
                assert 'PHOTOCRAFT_CONTROL_TOKEN_FILE="' in saved
                # Resolve the actual path reported by the setup response.
                token_path = Path(next(line.split("=", 1)[1] for line in setup["lines"]
                    if line.startswith("PHOTOCRAFT_CONTROL_TOKEN_FILE=")))
                assert token_path.stat().st_mode & 0o777 == 0o600
                assert (config.read_text().find("idle_after_secs = 1")) >= 0
                if env.get("WAYLAND_DISPLAY"):
                    wait_for(lambda: any(card and card.get("state") == "in PhotoCraft — Idle" for card in cards), 15)
                call("/api/settings", {"idle_after_secs": 0})
                wait_for(lambda: any(card and card.get("state") == "in PhotoCraft — Editing an image"
                    and card.get("buttons", [{}])[0].get("label") == "Portfolio" for card in cards), 20)
                # Stable-release regressions: new settings, safe reporting, duplicate
                # instance protection, failed-save rollback and explicit recovery.
                rules = [{"app": "PhotoCraft", "element": "Crop", "activity": "Cropping"}]
                settings = call("/api/settings")
                settings["apps"]["PhotoCraft"]["idle_after_secs"] = 0
                call("/api/settings", {"rules": rules, "apps": settings["apps"], "include_prereleases": True})
                settings = call("/api/settings")
                assert settings["rules"] == rules and settings["include_prereleases"]
                assert settings["apps"]["PhotoCraft"]["idle_after_secs"] == 0
                try:
                    call("/api/settings", {"rules": [{"app": "PhotoCraft", "element": "", "activity": "Bad"}]})
                    raise AssertionError("Empty rule accepted")
                except urllib.error.HTTPError as error:
                    assert error.code == 400
                call("/api/settings", {"enabled": False})
                wait_for(lambda: call("/api/diagnostics")["last_presence"] is None)
                before = call("/api/stats")
                time.sleep(2)
                assert call("/api/stats") == before, "Disabled sharing must not add usage"
                wait_for(lambda: call("/api/monitor")["sampled"])
                report = call("/api/support")
                assert report["resources"]["memory_bytes"] > 0
                assert not any(key in report for key in ("config_path", "last_presence", "token", "rules", "logs"))
                duplicate = subprocess.run([str(binary)], env=env, timeout=5, capture_output=True)
                assert duplicate.returncode == 0, duplicate.stderr
                config.write_text("invalid = [")
                try:
                    call("/api/settings", {"enabled": True})
                    raise AssertionError("Invalid file overwritten")
                except urllib.error.HTTPError as error:
                    assert error.code == 500
                assert config.read_text() == "invalid = ["
                assert call("/api/settings")["enabled"] is False, "Failed save changed running settings"
                call("/api/config/restore", {})
                assert call("/api/settings")["enabled"] is False
                assert list(config.parent.glob("config-recovered-*.toml")), "Original damaged config was not preserved"
                assert "invalid = [" not in config.read_text()
                assert process.poll() is None
                assert not server_errors, server_errors
                print("PASS:", "running real PhotoCraft" if fake_app is None else "simulated AppImage payload", "settings authentication,", "idle presence," if env.get("WAYLAND_DISPLAY") else "idle configuration,", "button IPC, rules, per-app idle, update channels, monitor, support export, single instance, save rollback, backup recovery and partial session-setup failure")
            except Exception:
                print("Observed cards (mock Discord peer):", cards)
                print("Mock server errors:", server_errors)
                print((root / "app.log").read_text())
                raise
            finally:
                process.terminate()
                process.wait(timeout=10)
                if fake_app is not None:
                    fake_app.terminate()
                    fake_app.wait(timeout=5)
                stopping.set()
                listener.close()
                server.join(timeout=2)


if __name__ == "__main__":
    main()

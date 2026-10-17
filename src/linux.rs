//! Linux support, for both X11 and Wayland.
//!
//! Wayland apps cannot look at each other's windows, so this asks the
//! compositor instead: Hyprland (`hyprctl`), Sway (`swaymsg`), niri (`niri msg`),
//! KDE Plasma (`kdotool`, focused window only) and X11/XWayland (`xprop`). GNOME
//! on Wayland offers nothing of the kind; there CraftPulse relies on live
//! detection (the apps' control port), which works everywhere.
//!
//! One background thread refreshes the window list and the idle time every
//! second, so the 500 ms main loop never waits on a child process.

use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        Mutex, Once, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// A visible top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinInfo {
    pub pid: u32,
    pub title: String,
    pub focused: bool,
}

struct Snapshot {
    at: Option<Instant>,
    windows: Vec<WinInfo>,
}

static SNAPSHOT: Mutex<Snapshot> = Mutex::new(Snapshot {
    at: None,
    windows: Vec::new(),
});
static STARTED: Once = Once::new();
const POLL: Duration = Duration::from_secs(1);
/// A snapshot older than this is ignored (the helper command is stuck).
const STALE: Duration = Duration::from_secs(5);
const IDLE_UNKNOWN: u64 = u64::MAX;
static IDLE_MS: AtomicU64 = AtomicU64::new(IDLE_UNKNOWN);

fn start() {
    STARTED.call_once(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            let _ = thread::Builder::new()
                .name("craftpulse-wayland-idle".to_string())
                .spawn(|| {
                    let _ = wayland::run(1000);
                    wayland::AVAILABLE.store(false, Ordering::Relaxed);
                });
        }
        let _ = thread::Builder::new()
            .name("craftpulse-linux".to_string())
            .spawn(|| loop {
                refresh();
                thread::sleep(POLL);
            });
    });
}

fn refresh() {
    let windows = list_windows();
    *SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()) = Snapshot {
        at: Some(Instant::now()),
        windows,
    };
    IDLE_MS.store(poll_idle().unwrap_or(IDLE_UNKNOWN), Ordering::Relaxed);
}

fn windows() -> Vec<WinInfo> {
    start();
    let snapshot = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner());
    match snapshot.at {
        Some(at) if at.elapsed() < STALE => snapshot.windows.clone(),
        _ => Vec::new(),
    }
}

/// Process id of the window that has keyboard focus, when the compositor tells us.
pub fn focused_pid() -> Option<u32> {
    windows()
        .iter()
        .find(|window| window.focused)
        .map(|window| window.pid)
}

pub fn titles_for(pid: u32) -> Vec<String> {
    windows()
        .into_iter()
        .filter(|window| window.pid == pid)
        .map(|window| window.title)
        .collect()
}

/// Seconds without keyboard or mouse input, when the system can tell.
pub fn idle_seconds() -> Option<u64> {
    start();
    let ms = IDLE_MS.load(Ordering::Relaxed);
    (ms != IDLE_UNKNOWN).then_some(ms / 1000)
}

// Bound execution and output: compositor helpers must never stall detection.
fn run(program: &str, args: &[&str]) -> Option<String> {
    run_with_timeout(program, args, Duration::from_millis(750))
}

fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_OUTPUT + 1).read_to_end(&mut bytes);
        let _ = sender.send(
            result
                .ok()
                .filter(|_| bytes.len() <= MAX_OUTPUT as usize)
                .map(|_| bytes),
        );
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                let bytes = receiver.recv_timeout(remaining).ok()??;
                return String::from_utf8(bytes).ok();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn desktop_is(name: &str) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|value| value.to_uppercase().contains(name))
}

// ---------------------------------------------------------------------------
// Windows

fn list_windows() -> Vec<WinInfo> {
    let found = if env_set("HYPRLAND_INSTANCE_SIGNATURE") {
        parse_hyprland(
            &run("hyprctl", &["clients", "-j"]).unwrap_or_default(),
            &run("hyprctl", &["activewindow", "-j"]).unwrap_or_default(),
        )
    } else if env_set("SWAYSOCK") {
        parse_sway(&run("swaymsg", &["-t", "get_tree"]).unwrap_or_default())
    } else if env_set("NIRI_SOCKET") {
        parse_niri(&run("niri", &["msg", "--json", "windows"]).unwrap_or_default())
    } else {
        Vec::new()
    };
    if !found.is_empty() {
        return found;
    }
    if desktop_is("KDE") {
        let focused = kde_focused();
        if !focused.is_empty() {
            return focused;
        }
    }
    if env_set("DISPLAY") {
        return x11_windows();
    }
    Vec::new()
}

fn parse_hyprland(clients: &str, active: &str) -> Vec<WinInfo> {
    let active_address = serde_json::from_str::<Value>(active)
        .ok()
        .and_then(|value| value["address"].as_str().map(str::to_string));
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(clients) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|window| {
            let title = window["title"].as_str().filter(|title| !title.is_empty())?;
            Some(WinInfo {
                pid: u32::try_from(window["pid"].as_u64()?).ok()?,
                title: title.to_string(),
                focused: active_address.is_some()
                    && window["address"].as_str() == active_address.as_deref(),
            })
        })
        .collect()
}

fn parse_sway(tree: &str) -> Vec<WinInfo> {
    fn walk(node: &Value, out: &mut Vec<WinInfo>) {
        let mut has_children = false;
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                has_children = true;
                walk(child, out);
            }
        }
        if has_children {
            return;
        }
        let title = node["name"].as_str().filter(|title| !title.is_empty());
        if let (Some(pid), Some(title)) = (node["pid"].as_u64(), title) {
            if let Ok(pid) = u32::try_from(pid) {
                out.push(WinInfo {
                    pid,
                    title: title.to_string(),
                    focused: node["focused"].as_bool() == Some(true),
                });
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(root) = serde_json::from_str::<Value>(tree) {
        walk(&root, &mut out);
    }
    out
}

fn parse_niri(windows: &str) -> Vec<WinInfo> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(windows) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|window| {
            let title = window["title"].as_str().filter(|title| !title.is_empty())?;
            Some(WinInfo {
                pid: u32::try_from(window["pid"].as_u64()?).ok()?,
                title: title.to_string(),
                focused: window["is_focused"].as_bool() == Some(true),
            })
        })
        .collect()
}

/// Plasma has no window list for ordinary programs; `kdotool` (extra repo)
/// can read the focused window through KWin.
fn kde_focused() -> Vec<WinInfo> {
    let Some(id) = run("kdotool", &["getactivewindow"]) else {
        return Vec::new();
    };
    let id = id.trim();
    let pid = run("kdotool", &["getwindowpid", id]).and_then(|text| text.trim().parse().ok());
    let title = run("kdotool", &["getwindowname", id]).map(|text| text.trim().to_string());
    match (pid, title) {
        (Some(pid), Some(title)) if !title.is_empty() => vec![WinInfo {
            pid,
            title,
            focused: true,
        }],
        _ => Vec::new(),
    }
}

fn x11_windows() -> Vec<WinInfo> {
    let Some(root) = run(
        "xprop",
        &["-root", "_NET_CLIENT_LIST", "_NET_ACTIVE_WINDOW"],
    ) else {
        return Vec::new();
    };
    let line_ids = |key: &str| {
        root.lines()
            .find(|line| line.starts_with(key))
            .map(parse_x11_window_ids)
            .unwrap_or_default()
    };
    let active = line_ids("_NET_ACTIVE_WINDOW").into_iter().next();
    let started = Instant::now();
    line_ids("_NET_CLIENT_LIST")
        .into_iter()
        .take(80)
        .filter_map(|id| {
            if started.elapsed() >= Duration::from_secs(1) {
                return None;
            }
            let details = run(
                "xprop",
                &["-id", &id, "_NET_WM_PID", "_NET_WM_NAME", "WM_NAME"],
            )?;
            let (pid, title) = parse_x11_window(&details)?;
            Some(WinInfo {
                pid,
                title,
                focused: active.as_deref() == Some(id.as_str()),
            })
        })
        .collect()
}

fn parse_x11_window_ids(line: &str) -> Vec<String> {
    line.split_once('#')
        .map(|(_, value)| {
            value
                .split(|ch: char| ch == ',' || ch.is_whitespace())
                .filter(|id| id.starts_with("0x"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_x11_window(properties: &str) -> Option<(u32, String)> {
    let pid = properties
        .lines()
        .find(|line| line.contains("_NET_WM_PID"))
        .and_then(|line| line.split('=').nth(1))
        .and_then(|part| part.trim().parse::<u32>().ok())?;
    let title = properties
        .lines()
        .find(|line| line.contains("WM_NAME"))
        .and_then(|line| line.split_once('='))
        .map(|(_, title)| title.trim().trim_matches('"').to_string())
        .filter(|title| !title.is_empty())?;
    Some((pid, title))
}

// ---------------------------------------------------------------------------
// Idle time

fn poll_idle() -> Option<u64> {
    if wayland::AVAILABLE.load(Ordering::Relaxed) {
        let since = wayland::IDLED_AT.load(Ordering::Relaxed);
        return Some(if since == 0 {
            0
        } else {
            1000 + (unix_seconds() - since).max(0) as u64 * 1000
        });
    }
    if desktop_is("GNOME") {
        let reply = run(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.gnome.Mutter.IdleMonitor",
                "--object-path",
                "/org/gnome/Mutter/IdleMonitor/Core",
                "--method",
                "org.gnome.Mutter.IdleMonitor.GetIdletime",
            ],
        );
        if let Some(ms) = reply.as_deref().and_then(parse_gdbus_uint) {
            return Some(ms);
        }
    }
    if env_set("DISPLAY") {
        // Optional `xprintidle` package; prints milliseconds.
        return run("xprintidle", &[])?.trim().parse().ok();
    }
    None
}

/// `(uint64 12345,)` -> 12345
fn parse_gdbus_uint(reply: &str) -> Option<u64> {
    let digits: String = reply
        .split_once("uint64 ")?
        .1
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.as_secs() as i64)
}

/// Idle time through the Wayland `ext-idle-notify-v1` protocol, which Hyprland,
/// Sway, niri, KDE Plasma and COSMIC support.
mod wayland {
    use super::{unix_seconds, AtomicBool, AtomicI64, Ordering};
    use wayland_client::{
        globals::{registry_queue_init, GlobalListContents},
        protocol::{wl_registry::WlRegistry, wl_seat::WlSeat},
        Connection, Dispatch, QueueHandle,
    };
    use wayland_protocols::ext::idle_notify::v1::client::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
    };

    pub static AVAILABLE: AtomicBool = AtomicBool::new(false);
    /// Unix seconds when the idle threshold was reached; 0 while active.
    pub static IDLED_AT: AtomicI64 = AtomicI64::new(0);

    struct State;

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: <WlRegistry as wayland_client::Proxy>::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<WlSeat, ()> for State {
        fn event(
            _: &mut Self,
            _: &WlSeat,
            _: <WlSeat as wayland_client::Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotifierV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotifierV1,
            _: ext_idle_notifier_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotificationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                ext_idle_notification_v1::Event::Idled => {
                    IDLED_AT.store(unix_seconds().max(1), Ordering::Relaxed)
                }
                ext_idle_notification_v1::Event::Resumed => IDLED_AT.store(0, Ordering::Relaxed),
                _ => {}
            }
        }
    }

    /// Runs until the compositor goes away. Fails at once on compositors
    /// without the protocol (GNOME), and the caller falls back to other ways.
    pub fn run(timeout_ms: u32) -> Result<(), Box<dyn std::error::Error>> {
        let connection = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
        let handle = queue.handle();
        let seat: WlSeat = globals.bind(&handle, 1..=1, ())?;
        let notifier: ExtIdleNotifierV1 = globals.bind(&handle, 1..=1, ())?;
        let _notification = notifier.get_idle_notification(timeout_ms, &seat, &handle, ());
        AVAILABLE.store(true, Ordering::Relaxed);
        let mut state = State;
        loop {
            queue.blocking_dispatch(&mut state)?;
        }
    }
}

// ---------------------------------------------------------------------------
// Recognising AppImages

/// Extra names a process answers to. AppImage payloads often have an odd
/// process name (truncated to 15 characters by the kernel), so also look at
/// the real executable, argv[0], and the AppImage file the process was started from.
pub fn extra_names(process: &sysinfo::Process) -> Vec<String> {
    let file_name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    let mut names = Vec::new();
    names.extend(process.exe().and_then(file_name));
    names.extend(
        process
            .cmd()
            .first()
            .and_then(|arg| file_name(Path::new(arg))),
    );
    // Only trust APPIMAGE for processes that run from inside the image, because
    // every program the app launches inherits that variable.
    let env = |key: &str| {
        process
            .environ()
            .iter()
            .find_map(|pair| pair.to_str()?.strip_prefix(key)?.strip_prefix('='))
            .map(str::to_string)
    };
    if let (Some(dir), Some(image), Some(exe)) = (env("APPDIR"), env("APPIMAGE"), process.exe()) {
        if exe.starts_with(&dir) {
            let image = file_name(Path::new(&image)).unwrap_or_default();
            let stem = image.len().checked_sub(".AppImage".len()).and_then(|at| {
                image
                    .get(at..)
                    .filter(|tail| tail.eq_ignore_ascii_case(".AppImage"))
                    .map(|_| image[..at].to_string())
            });
            names.extend(stem);
        }
    }
    names
}

/// "photocraft-bin", "PhotoCraft_x86_64" or "photocraft-1.2.0" count as PhotoCraft.
/// The AppImage file itself does not: that process is only the launcher stub.
pub fn variant_name_matches(name: &str, app: &str) -> bool {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"^([a-z0-9]+?)(?:[-_.](?:v?\d[\w.]*|x86_64|amd64|aarch64|arm64|linux|bin|app))+$",
        )
        .expect("valid pattern")
    });
    let lower = name.trim().to_lowercase();
    if lower.ends_with(".appimage") {
        return false;
    }
    pattern
        .captures(&lower)
        .is_some_and(|found| found[1].eq_ignore_ascii_case(app))
}

// ---------------------------------------------------------------------------
// Live detection: the apps read these variables when they start

/// Makes the control-port variables visible to apps started from now on
/// (including the ArtCraft launcher and the AppImages it starts):
/// `environment.d` for future logins, and the running systemd user session for
/// programs started from the desktop's menu straight away.
pub fn set_session_environment(pairs: &[(String, String)]) -> (bool, bool) {
    let Some(dir) = dirs::config_dir().map(|dir| dir.join("environment.d")) else {
        return (false, false);
    };
    let written = write_environment_file(&dir, pairs);
    let assignments: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut args = vec!["--user", "set-environment"];
    args.extend(assignments.iter().map(String::as_str));
    let applied = pairs.is_empty() || run("systemctl", &args).is_some();
    (written, applied)
}

fn write_environment_file(dir: &Path, pairs: &[(String, String)]) -> bool {
    let text: String = pairs
        .iter()
        .map(|(k, v)| {
            let value = v
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`");
            format!("{k}=\"{value}\"\n")
        })
        .collect();
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join("50-craftpulse.conf"), text))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_timeout_and_failure_are_bounded() {
        let started = Instant::now();
        assert!(
            run_with_timeout("sh", &["-c", "exec sleep 5"], Duration::from_millis(40)).is_none()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(run_with_timeout("sh", &["-c", "exit 1"], Duration::from_secs(1)).is_none());
        assert_eq!(
            run_with_timeout("sh", &["-c", "printf 'hello'"], Duration::from_secs(1)).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn helper_drains_output_larger_than_a_pipe() {
        let output = run_with_timeout(
            "sh",
            &["-c", "head -c 100000 /dev/zero"],
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output.len(), 100000);
    }

    #[test]
    fn helper_rejects_excessive_output() {
        assert!(run_with_timeout(
            "sh",
            &["-c", "head -c 5000000 /dev/zero"],
            Duration::from_secs(2)
        )
        .is_none());
    }

    #[test]
    fn parses_hyprland_clients() {
        let clients = r#"[{"address":"0xa1","pid":42,"title":"Poster — PhotoCraft"},
            {"address":"0xb2","pid":7,"title":""},{"address":"0xc3","pid":9,"title":"Terminal"}]"#;
        let windows = parse_hyprland(clients, r#"{"address":"0xa1"}"#);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].pid, 42);
        assert!(windows[0].focused && !windows[1].focused);
        assert!(parse_hyprland("garbage", "{}").is_empty());
        // Nothing focused: Hyprland answers with an empty object.
        assert!(parse_hyprland(clients, "{}").iter().all(|w| !w.focused));
    }

    #[test]
    fn parses_sway_tree() {
        let tree = r#"{"type":"root","nodes":[{"type":"output","nodes":[{"type":"workspace",
            "nodes":[{"type":"con","pid":42,"name":"Poster — PhotoCraft","focused":true,"nodes":[]}],
            "floating_nodes":[{"type":"floating_con","pid":43,"name":"Export","focused":false,"nodes":[]}]},
            {"type":"workspace","name":"2","nodes":[],"floating_nodes":[]}]}]}"#;
        let windows = parse_sway(tree);
        assert_eq!(
            windows,
            vec![
                WinInfo {
                    pid: 42,
                    title: "Poster — PhotoCraft".into(),
                    focused: true
                },
                WinInfo {
                    pid: 43,
                    title: "Export".into(),
                    focused: false
                },
            ]
        );
    }

    #[test]
    fn parses_niri_windows() {
        let windows = parse_niri(
            r#"[{"id":1,"title":"Poster","app_id":"photocraft","pid":42,"is_focused":true},
                {"id":2,"title":"Notes","pid":8,"is_focused":false}]"#,
        );
        assert_eq!(windows.len(), 2);
        assert!(windows[0].focused);
    }

    #[test]
    fn parses_x11_properties() {
        let root = "_NET_CLIENT_LIST(WINDOW): window id # 0x10, 0x20\n_NET_ACTIVE_WINDOW(WINDOW): window id # 0x20\n";
        let ids = parse_x11_window_ids(root.lines().next().unwrap());
        assert_eq!(ids, ["0x10", "0x20"]);
        let props =
            "_NET_WM_PID(CARDINAL) = 42\n_NET_WM_NAME(UTF8_STRING) = \"Poster — PhotoCraft\"\n";
        assert_eq!(
            parse_x11_window(props),
            Some((42, "Poster — PhotoCraft".to_string()))
        );
        assert_eq!(parse_x11_window("WM_NAME(STRING) = \"x\""), None);
    }

    #[test]
    fn parses_gnome_idle_reply() {
        assert_eq!(parse_gdbus_uint("(uint64 123456,)\n"), Some(123_456));
        assert_eq!(parse_gdbus_uint("oops"), None);
    }

    #[test]
    fn recognises_appimage_style_names() {
        assert!(variant_name_matches("photocraft-bin", "PhotoCraft"));
        assert!(variant_name_matches("PhotoCraft_x86_64", "PhotoCraft"));
        assert!(variant_name_matches(
            "photocraft-1.2.0-x86_64",
            "PhotoCraft"
        ));
        assert!(!variant_name_matches("photocraft-launcher", "PhotoCraft"));
        assert!(!variant_name_matches(
            "PhotoCraft-1.2.AppImage",
            "PhotoCraft"
        ));
        assert!(!variant_name_matches("videocraft-bin", "PhotoCraft"));
    }

    #[test]
    fn session_environment_file_is_written() {
        let dir = std::env::temp_dir().join(format!("craftpulse-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(write_environment_file(
            &dir,
            &[("LIGHTCRAFT_CONTROL_PORT".into(), "7980".into())]
        ));
        let text = std::fs::read_to_string(dir.join("50-craftpulse.conf")).unwrap();
        assert_eq!(text, "LIGHTCRAFT_CONTROL_PORT=\"7980\"\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fake_xprop_binary_lists_x11_windows() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let temp = std::env::temp_dir().join(format!("craftpulse-fake-x11-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        let fake_xprop = temp.join("xprop");
        fs::write(
            &fake_xprop,
            "#!/bin/sh\nif [ \"$1\" = \"-root\" ]; then\n  printf '_NET_CLIENT_LIST(WINDOW): window id # 0xabc\\n_NET_ACTIVE_WINDOW(WINDOW): window id # 0xabc\\n'\nelse\n  printf '_NET_WM_PID(CARDINAL) = 42\\n_NET_WM_NAME(UTF8_STRING) = \"Mock Project — PhotoCraft\"\\n'\nfi\n",
        )
        .unwrap();
        fs::set_permissions(&fake_xprop, fs::Permissions::from_mode(0o755)).unwrap();
        let original_path = std::env::var_os("PATH");
        let mut paths = vec![temp.clone()];
        paths.extend(std::env::split_paths(
            &original_path.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        let windows = x11_windows();
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        }
        assert_eq!(
            windows,
            vec![WinInfo {
                pid: 42,
                title: "Mock Project — PhotoCraft".into(),
                focused: true
            }]
        );
        let _ = fs::remove_dir_all(temp);
    }

    /// Needs a running Wayland compositor with ext-idle-notify-v1 (Sway, Hyprland,
    /// niri, KDE): `cargo test -- --ignored wayland_idle`.
    #[test]
    #[ignore]
    fn wayland_idle_notification_arrives() {
        thread::spawn(|| {
            let _ = wayland::run(1000);
        });
        thread::sleep(Duration::from_secs(3));
        assert!(wayland::AVAILABLE.load(Ordering::Relaxed));
        assert_ne!(wayland::IDLED_AT.load(Ordering::Relaxed), 0);
    }
}

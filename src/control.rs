//! Live detection through the Craft apps' JSON-lines control channel.
//!
//! Every Craft app can expose a loopback-only control channel. The apps'
//! `<app>-cli mcp --connect` servers are only a wrapper around this same
//! channel, so reading it directly gives CraftPulse the app's own view of its
//! state (tool, dialogs, document, photo) without an MCP middleman.
//!
//! How each app is reached (taken from the apps' own protocol docs):
//!   * LightCraft, VectorCraft, DesignCraft, EffectCraft, FilmCraft:
//!     plain TCP port, no authentication (`<APP>_CONTROL_PORT` or `--control <port>`).
//!   * PhotoCraft: TCP port plus a bearer token. First frame must be `auth`
//!     (`--control <port> --control-token-file <file>`).
//!   * PdfCraft: `--control <file>` picks a random port and writes
//!     `{"port","token","pid"}` to that file; the first frame must be `auth`.
//!
//! Requests are `{"id":1,"method":"ui.inspect","params":{}}`; replies are
//! `{"id":1,"ok":true,"result":{...}}` or `{"id":1,"ok":false,"error":"..."}`.
//!
//! CraftPulse only sends `auth` and read-only queries (`ui.inspect`,
//! `ui.state`, and `engine.execute` with `photo.inspect` / `project.inspect`).
//! Nothing here can change a document.

use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_secs(3);
/// How often to retry while the app isn't reachable.
const RETRY_EVERY: Duration = Duration::from_secs(3);
/// How often to ask the app for its state while connected.
const POLL_EVERY: Duration = Duration::from_millis(1000);
/// How often to re-read the open document/project name (heavier queries).
const DOCUMENT_EVERY: Duration = Duration::from_secs(5);
const MAX_REPLY_BYTES: u64 = 4 * 1024 * 1024;

/// How an app's control channel is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Plain loopback port.
    Open,
    /// Loopback port + bearer token read from a token file.
    TokenFile,
    /// A JSON file the app writes with `port` and `token`.
    ControlFile,
}

pub fn access_for(app: &str) -> Access {
    match app.to_ascii_lowercase().as_str() {
        "photocraft" => Access::TokenFile,
        "pdfcraft" => Access::ControlFile,
        _ => Access::Open,
    }
}

/// Environment variable that turns the control channel on at launch, if the
/// app has one (PdfCraft only takes a command-line flag).
pub fn control_env_var(app: &str) -> Option<&'static str> {
    match app.to_ascii_lowercase().as_str() {
        "photocraft" => Some("PHOTOCRAFT_CONTROL_PORT"),
        "vectorcraft" => Some("VECTORCRAFT_CONTROL_PORT"),
        "filmcraft" => Some("FILMCRAFT_CONTROL_PORT"),
        "lightcraft" => Some("LIGHTCRAFT_CONTROL_PORT"),
        "effectcraft" => Some("EFFECTCRAFT_CONTROL_PORT"),
        "designcraft" => Some("DESIGNCRAFT_CONTROL_PORT"),
        _ => None,
    }
}

/// What the settings page shows about one app's control channel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveStatus {
    pub name: String,
    /// "open", "token" or "file".
    pub access: &'static str,
    pub port: u16,
    pub env_var: Option<&'static str>,
    pub token_file: String,
    /// The app is listening right now.
    pub reachable: bool,
    /// Command-line way to start the app with the channel on.
    pub launch_hint: String,
}

/// Checks (quickly) whether an app's control channel is listening.
pub fn probe(config: &crate::config::Config, app: &str) -> LiveStatus {
    let access = access_for(app);
    let token_file = config.control_token_file(app);
    let mut port = config.control_port(app);
    if access == Access::ControlFile {
        port = std::fs::read_to_string(&token_file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|info| info.get("port").and_then(Value::as_u64))
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or(0);
    }
    let reachable = port != 0
        && TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            Duration::from_millis(150),
        )
        .is_ok();
    let exe = app.to_ascii_lowercase();
    let launch_hint = match access {
        Access::Open => format!("{exe} --control {port}"),
        Access::TokenFile => format!(
            "{exe} --control {port} --control-token-file \"{}\"",
            token_file.display()
        ),
        Access::ControlFile => format!("{exe} --control \"{}\"", token_file.display()),
    };
    LiveStatus {
        name: app.to_string(),
        access: match access {
            Access::Open => "open",
            Access::TokenFile => "token",
            Access::ControlFile => "file",
        },
        port,
        env_var: control_env_var(app),
        token_file: token_file.display().to_string(),
        reachable,
        launch_hint,
    }
}

/// Makes the apps start with their control channel on, by setting the
/// per-user environment variables the apps read at launch. On Windows this
/// runs `setx` (no admin rights needed; apps must be restarted). Elsewhere it
/// only returns the lines to add to a shell profile. Returns (applied, lines).
pub fn enable_environment(
    config: &crate::config::Config,
    apps: &[&str],
) -> (bool, bool, Vec<String>) {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for app in apps {
        if !config.app_config(app).live {
            continue;
        }
        let Some(var) = control_env_var(app) else {
            continue;
        };
        pairs.push((var.to_string(), config.control_port(app).to_string()));
        if access_for(app) == Access::TokenFile {
            let token_file = config.control_token_file(app);
            ensure_token_file(&token_file);
            pairs.push((
                "PHOTOCRAFT_CONTROL_TOKEN_FILE".to_string(),
                token_file.display().to_string(),
            ));
        }
    }
    let lines: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut all_ok = true;
        for (key, value) in &pairs {
            let ok = std::process::Command::new("setx")
                .arg(key)
                .arg(value)
                .creation_flags(0x0800_0000)
                .status()
                .is_ok_and(|status| status.success());
            all_ok &= ok;
        }
        (all_ok, all_ok, lines)
    }
    #[cfg(target_os = "linux")]
    {
        {
            let (persisted, applied) = crate::linux::set_session_environment(&pairs);
            (applied, persisted, lines)
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        (false, false, lines)
    }
}

/// What the app told us about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveState {
    /// Active tool id, for example "brush".
    pub tool: Option<String>,
    /// First open dialog, for example "export" or "cameraRaw".
    pub dialog: Option<String>,
    /// Open document / photo / project name.
    pub document: Option<String>,
    /// Main view (LightCraft: "detail", "compare", ...).
    pub view: Option<String>,
    /// Open side panel / mode (LightCraft: "edit", "crop", ...).
    pub panel: Option<String>,
    /// Ready-made text for long-running work ("Exporting photos").
    pub busy: Option<String>,
    /// Playback or preview is running.
    pub playing: bool,
    /// Extra info for the document line ("Page 3 of 12", "6000×4000").
    pub detail: Option<String>,
}

impl LiveState {
    /// Apply user rules to the active tool, panel or view reported by the app.
    /// This supplies a cross-platform equivalent for these controls without
    /// pretending that Linux exposes Windows UI Automation.
    pub fn rule_activity(
        &self,
        app: &str,
        rules: &[crate::config::ActivityRule],
    ) -> Option<String> {
        fn key(text: &str) -> String {
            text.split('(')
                .next()
                .unwrap_or(text)
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        }
        rules
            .iter()
            .filter(|rule| rule.app.eq_ignore_ascii_case(app))
            .find_map(|rule| {
                let prefix = key(&rule.element);
                (!prefix.is_empty()
                    && [&self.tool, &self.panel, &self.view]
                        .into_iter()
                        .flatten()
                        .any(|value| key(value).starts_with(&prefix)))
                .then(|| rule.activity.clone())
            })
    }

    /// Activity for an open dialog ("Exporting", "Using Camera Raw").
    pub fn dialog_activity(&self) -> Option<String> {
        let pretty = prettify(self.dialog.as_deref()?);
        if pretty.is_empty() {
            return None;
        }
        Some(
            crate::detection::dialog_activity(&pretty)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Using {pretty}")),
        )
    }

    /// Activity for the active tool ("Painting", "Using Magic Wand").
    pub fn tool_activity(&self) -> Option<String> {
        self.tool.as_deref().map(tool_activity)
    }

    /// The best single description of what the user is doing right now.
    /// Long-running work wins, then dialogs, then panels and tools.
    pub fn activity(&self, app: &str) -> Option<String> {
        if let Some(busy) = &self.busy {
            return Some(busy.clone());
        }
        if let Some(dialog) = self.dialog_activity() {
            return Some(dialog);
        }
        let app = app.to_ascii_lowercase();
        if app == "lightcraft" {
            return self.lightcraft_activity();
        }
        if self.playing {
            return Some(
                match app.as_str() {
                    "filmcraft" => "Previewing the timeline",
                    "effectcraft" => "Previewing an animation",
                    _ => "Playing back",
                }
                .to_string(),
            );
        }
        self.tool_activity().or_else(|| {
            if app == "pdfcraft" {
                if let Some(text) = self.panel.as_deref().and_then(pdf_mode_activity) {
                    return Some(text);
                }
            }
            self.panel
                .as_deref()
                .map(prettify)
                .filter(|p| !p.is_empty())
                .map(|p| format!("Using {p}"))
        })
    }

    fn lightcraft_activity(&self) -> Option<String> {
        let view = self.view.as_deref().map(|v| v.to_ascii_lowercase());
        let panel = self.panel.as_deref().map(|p| p.to_ascii_lowercase());
        match view.as_deref() {
            Some("photogrid") | Some("squaregrid") => return Some("Browsing the library".into()),
            Some("compare") => return Some("Comparing photos".into()),
            Some("survey") => return Some("Surveying photos".into()),
            Some("people") => return Some("Organising people".into()),
            _ => {}
        }
        let from_panel = match panel.as_deref() {
            Some("edit") | Some("profiles") => Some("Editing a photo"),
            Some("crop") => Some("Cropping a photo"),
            Some("remove") => Some("Retouching a photo"),
            Some("masking") => Some("Masking a photo"),
            Some("redeye") => Some("Fixing red eye"),
            Some("versions") => Some("Managing versions"),
            Some("activity") => Some("Reviewing edit history"),
            Some("keywords") => Some("Tagging photos"),
            Some("info") => Some("Viewing photo info"),
            _ => None,
        };
        if let Some(text) = from_panel {
            return Some(text.to_string());
        }
        match view.as_deref() {
            Some("reference") => Some("Comparing with a reference".into()),
            Some("detail") => Some("Viewing a photo".into()),
            _ => None,
        }
    }
}

fn pdf_mode_activity(mode: &str) -> Option<String> {
    Some(
        match mode.to_ascii_lowercase().as_str() {
            "read" => "Reading a PDF",
            "edit" => "Editing a PDF",
            "convert" => "Converting a PDF",
            "sign" => "Signing a PDF",
            _ => return None,
        }
        .to_string(),
    )
}

struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Connection {
    /// Sends one request line and returns the reply that matches `id`.
    fn call(&mut self, request: &Value, id: Option<u64>) -> Result<Value, String> {
        let mut line = request.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .map_err(|error| format!("connection lost ({error})"))?;
        for _ in 0..8 {
            let mut reply = String::new();
            let read = (&mut self.reader)
                .take(MAX_REPLY_BYTES)
                .read_line(&mut reply)
                .map_err(|error| format!("no reply ({error})"))?;
            if read == 0 {
                return Err("connection closed by the app".to_string());
            }
            let value: Value = serde_json::from_str(reply.trim())
                .map_err(|_| "the app sent an unreadable reply".to_string())?;
            if let (Some(want), Some(got)) = (id, value.get("id").and_then(Value::as_u64)) {
                if want != got {
                    continue;
                }
            }
            return Ok(value);
        }
        Err("no matching reply from the app".to_string())
    }
}

/// The app's error text, when the reply is a failure.
fn has_error(reply: &Value) -> Option<String> {
    if let Some(error) = reply.get("error").filter(|error| !error.is_null()) {
        return Some(match error.as_str() {
            Some(text) => text.to_string(),
            None => error.to_string(),
        });
    }
    (reply.get("ok").and_then(Value::as_bool) == Some(false)).then(|| "request failed".to_string())
}

/// Reads one app's state over its control channel. Create one per app.
pub struct ControlClient {
    app: String,
    access: Access,
    port: u16,
    token_file: PathBuf,
    connection: Option<Connection>,
    next_id: u64,
    last_attempt: Option<Instant>,
    last_poll: Option<Instant>,
    last_document: Option<Instant>,
    state: Option<LiveState>,
    /// Document name cache: (key it was read for, name).
    document_cache: Option<(String, Option<String>, Option<String>)>,
    last_failure: String,
    described: bool,
    logs: Vec<String>,
}

impl ControlClient {
    pub fn new(app: &str, port: u16, token_file: PathBuf) -> Self {
        Self {
            app: app.to_string(),
            access: access_for(app),
            port,
            token_file,
            connection: None,
            next_id: 0,
            last_attempt: None,
            last_poll: None,
            last_document: None,
            state: None,
            document_cache: None,
            last_failure: String::new(),
            described: false,
            logs: Vec::new(),
        }
    }

    /// Applies new settings; a changed port or token file reconnects.
    pub fn configure(&mut self, port: u16, token_file: PathBuf) {
        if self.port != port || self.token_file != token_file {
            self.port = port;
            self.token_file = token_file;
            self.connection = None;
            self.state = None;
            self.last_attempt = None;
            self.last_failure.clear();
        }
    }

    /// Messages worth showing in the activity log (connects, failures).
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// Returns the app's current state, or `None` when it can't be reached.
    /// Cheap to call every loop: it only talks to the app about once a second.
    pub fn poll(&mut self) -> Option<LiveState> {
        if self.connection.is_none() {
            if self
                .last_attempt
                .is_some_and(|at| at.elapsed() < RETRY_EVERY)
            {
                return None;
            }
            self.last_attempt = Some(Instant::now());
            match self.connect() {
                Ok((connection, port)) => {
                    self.connection = Some(connection);
                    self.last_failure.clear();
                    self.last_poll = None;
                    self.last_document = None;
                    self.document_cache = None;
                    self.logs.push(format!(
                        "Live detection: connected to {} on port {port}",
                        self.app
                    ));
                }
                Err(message) => {
                    self.report_failure(message);
                    self.state = None;
                    return None;
                }
            }
        }
        if self.last_poll.is_some_and(|at| at.elapsed() < POLL_EVERY) {
            return self.state.clone();
        }
        self.last_poll = Some(Instant::now());
        match self.read_state() {
            Ok(state) => {
                self.state = Some(state.clone());
                Some(state)
            }
            Err(message) => {
                self.connection = None;
                self.state = None;
                self.last_attempt = None;
                self.report_failure(message);
                None
            }
        }
    }

    /// Logs a failure once, not on every retry.
    fn report_failure(&mut self, message: String) {
        if message != self.last_failure {
            self.logs.push(format!(
                "Live detection ({}): {message}. Using window detection instead",
                self.app
            ));
            self.last_failure = message;
        }
    }

    /// Port and token for this connection attempt.
    fn endpoint(&self) -> Result<(u16, Option<String>), String> {
        match self.access {
            Access::Open => Ok((self.port, None)),
            Access::TokenFile => {
                ensure_token_file(&self.token_file);
                let token = std::fs::read_to_string(&self.token_file)
                    .map(|text| text.trim().to_string())
                    .map_err(|_| {
                        format!("token file not found at {}", self.token_file.display())
                    })?;
                if token.is_empty() {
                    return Err("the token file is empty".to_string());
                }
                Ok((self.port, Some(token)))
            }
            Access::ControlFile => {
                let text = std::fs::read_to_string(&self.token_file).map_err(|_| {
                    format!(
                        "control file not found at {} (start {} with --control <that file>)",
                        self.token_file.display(),
                        self.app
                    )
                })?;
                let info: Value = serde_json::from_str(&text)
                    .map_err(|_| "the control file isn't valid JSON".to_string())?;
                let port = info
                    .get("port")
                    .and_then(Value::as_u64)
                    .and_then(|port| u16::try_from(port).ok())
                    .ok_or("the control file has no port")?;
                let token = info
                    .get("token")
                    .and_then(Value::as_str)
                    .ok_or("the control file has no token")?;
                Ok((port, Some(token.to_string())))
            }
        }
    }

    fn connect(&self) -> Result<(Connection, u16), String> {
        let (port, token) = self.endpoint()?;
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|_| {
            format!(
                "not reachable on port {port} (start {} with its control channel on, see the Apps tab)",
                self.app
            )
        })?;
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_nodelay(true);
        let writer = stream
            .try_clone()
            .map_err(|error| format!("could not use the connection ({error})"))?;
        let mut connection = Connection {
            reader: BufReader::new(stream),
            writer,
        };
        if let Some(token) = token {
            let reply = connection.call(
                &json!({"id": "auth", "method": "auth", "params": {"token": token}}),
                None,
            )?;
            if has_error(&reply).is_some() {
                return Err("the app rejected the token (is it using this token file?)".to_string());
            }
        }
        Ok((connection, port))
    }

    /// One read-only request; returns the `result` value.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let connection = self.connection.as_mut().ok_or("not connected")?;
        self.next_id += 1;
        let id = self.next_id;
        let reply = connection.call(
            &json!({"id": id, "method": method, "params": params}),
            Some(id),
        )?;
        if let Some(error) = has_error(&reply) {
            return Err(format!("{method} failed: {error}"));
        }
        Ok(reply.get("result").cloned().unwrap_or(reply))
    }

    fn read_state(&mut self) -> Result<LiveState, String> {
        let app = self.app.to_ascii_lowercase();
        let method = if app == "pdfcraft" {
            "ui.state"
        } else {
            "ui.inspect"
        };
        let result = self.request(method, json!({}))?;
        let mut state = state_from_inspect(&app, &result);
        if !self.described {
            self.described = true;
            let keys: Vec<&String> = result
                .as_object()
                .map(|object| object.keys().collect())
                .unwrap_or_default();
            self.logs.push(format!(
                "Live detection ({}): {method} fields {keys:?}; tool {:?}, dialog {:?}, document {:?}",
                self.app, state.tool, state.dialog, state.document
            ));
        }
        if state.document.is_none() {
            let (name, detail) = self.document_name(&app, &result);
            state.document = name;
            if detail.is_some() {
                state.detail = detail;
            }
        }
        Ok(state)
    }

    /// Reads the open document's name for apps whose `ui.inspect` doesn't
    /// carry it. Cached, and only re-read when something changes.
    fn document_name(&mut self, app: &str, inspect: &Value) -> (Option<String>, Option<String>) {
        let (key, command, params): (String, &str, Value) = match app {
            "lightcraft" => {
                // The photo's id is in `ui.inspect`; its file name is one query away.
                let Some(active) = inspect.get("active").filter(|id| !id.is_null()) else {
                    return (None, None);
                };
                (active.to_string(), "photo.inspect", json!({}))
            }
            "filmcraft" => (String::new(), "project.inspect", json!({})),
            _ => return (None, None),
        };
        let cached_ok = self
            .document_cache
            .as_ref()
            .is_some_and(|(cached_key, _, _)| *cached_key == key);
        let fresh = self
            .last_document
            .is_some_and(|at| at.elapsed() < DOCUMENT_EVERY);
        if cached_ok && (fresh || app == "lightcraft") {
            return self
                .document_cache
                .as_ref()
                .map(|(_, name, detail)| (name.clone(), detail.clone()))
                .unwrap_or((None, None));
        }
        self.last_document = Some(Instant::now());
        // A failed lookup isn't fatal: window detection still works.
        let result = self
            .request(
                "engine.execute",
                json!({"command": command, "params": params}),
            )
            .ok();
        let name = result.as_ref().and_then(name_in);
        let detail = if app == "lightcraft" {
            result.as_ref().and_then(photo_detail)
        } else {
            None
        };
        self.document_cache = Some((key, name.clone(), detail.clone()));
        (name, detail)
    }
}

/// Reads the live state of whichever app is currently chosen, on a worker
/// thread so a slow app can never stall CraftPulse's main loop.
#[derive(Clone)]
pub struct ControlHub {
    shared: Arc<Mutex<HubShared>>,
}

#[derive(Default)]
struct HubShared {
    /// (app, port, token/control file, live detection enabled)
    target: Option<(String, u16, PathBuf)>,
    /// When the current target was first chosen.
    target_since: Option<Instant>,
    state: Option<(String, LiveState)>,
    logs: Vec<String>,
}

impl ControlHub {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(HubShared::default()));
        let worker = Arc::clone(&shared);
        let _ = thread::Builder::new()
            .name("craftpulse-control".to_string())
            .spawn(move || run_hub(worker));
        Self { shared }
    }

    /// Chooses which app to read (`None`: none, disconnect everything).
    pub fn set_target(&self, target: Option<(String, u16, PathBuf)>) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if shared.target != target {
            shared.target_since = target.as_ref().map(|_| Instant::now());
            shared.target = target;
        }
    }

    /// Latest state for `app`, if the app is reachable.
    pub fn state_for(&self, app: &str) -> Option<LiveState> {
        let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared
            .state
            .as_ref()
            .filter(|(name, _)| name.eq_ignore_ascii_case(app))
            .map(|(_, state)| state.clone())
    }

    /// True when `app` has been running for a while but its control channel
    /// never answered: it was probably started before live detection was on.
    pub fn needs_restart(&self, app: &str) -> bool {
        let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.state.is_none()
            && shared
                .target
                .as_ref()
                .is_some_and(|(name, _, _)| name.eq_ignore_ascii_case(app))
            && shared
                .target_since
                .is_some_and(|since| since.elapsed() >= Duration::from_secs(20))
    }

    pub fn take_logs(&self) -> Vec<String> {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut shared.logs)
    }
}

fn run_hub(shared: Arc<Mutex<HubShared>>) {
    let mut clients: HashMap<String, ControlClient> = HashMap::new();
    loop {
        thread::sleep(Duration::from_millis(250));
        let target = shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .target
            .clone();
        let Some((app, port, token_file)) = target else {
            clients.clear();
            let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
            guard.state = None;
            continue;
        };
        // Only the chosen app keeps a connection; the rest are closed.
        clients.retain(|name, _| *name == app);
        let client = clients
            .entry(app.clone())
            .or_insert_with(|| ControlClient::new(&app, port, token_file.clone()));
        client.configure(port, token_file);
        let state = client.poll();
        let logs = client.take_logs();
        let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
        guard.state = state.map(|state| (app.clone(), state));
        guard.logs.extend(logs);
    }
}

/// Makes sure a bearer-token file exists, creating it with a fresh 256-bit
/// token if not. The apps read the same file (`--control-token-file`).
pub fn ensure_token_file(path: &Path) {
    if path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        return;
    }
    let mut random = [0_u8; 32];
    if getrandom::fill(&mut random).is_err() {
        return;
    }
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    #[cfg(unix)]
    {
        // Only you may read it: the token is the password to PhotoCraft's control port.
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            let _ = file.write_all(token.as_bytes());
        }
    }
    #[cfg(not(unix))]
    let _ = std::fs::write(path, token);
}

// ---------------------------------------------------------------------------
// Reading the app's reply. Field names are matched loosely, because each app
// reports its state in slightly different shapes.

/// A string, or the first of a few name-like keys of an object.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => ["kind", "id", "name", "title", "label"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str).map(str::to_string)),
        _ => None,
    }
    .filter(|text| !text.trim().is_empty())
}

/// The active tool. Most apps put it at the top level; EffectCraft and
/// FilmCraft keep it inside `ui`.
fn tool_name(result: &Value) -> Option<String> {
    result
        .get("tool")
        .and_then(text_of)
        .or_else(|| {
            result
                .get("ui")
                .and_then(|ui| ui.get("tool"))
                .and_then(text_of)
        })
        .or_else(|| result.get("quick_tool").and_then(text_of))
}

/// Dialog names that aren't real dialogs.
fn is_real_dialog(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "none" | "null" | "false"
    )
}

fn dialog_name(result: &Value) -> Option<String> {
    let found = ["dialogs", "dialog"]
        .iter()
        .find_map(|key| match result.get(*key)? {
            Value::Array(items) => items.iter().find_map(text_of),
            other => text_of(other),
        });
    found
        .or_else(|| {
            result
                .get("fileDialog")
                .and_then(text_of)
                .map(|kind| format!("file {kind}"))
        })
        .filter(|name| is_real_dialog(name))
}

/// The last path component, for "C:\\Art\\poster.psd" -> "poster.psd".
fn file_name(text: &str) -> Option<String> {
    let text = text.trim();
    let name = text.rsplit(['\\', '/']).next().unwrap_or(text).trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn name_in(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => file_name(text),
        Value::Object(map) => ["file_name", "fileName", "name", "title", "path", "file"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str).and_then(file_name)),
        _ => None,
    }
}

/// From `ui.inspect`: only explicit document keys, so an unrelated "name"
/// (a theme, a window) is never mistaken for the document.
fn document_from_ui(result: &Value) -> Option<String> {
    let direct = ["document", "doc", "activeDocument", "active_document"]
        .iter()
        .find_map(|key| result.get(*key).and_then(name_in));
    direct.or_else(|| document_from_list(result))
}

/// VectorCraft, DesignCraft and PdfCraft list `documents` and say which is active.
fn document_from_list(result: &Value) -> Option<String> {
    let documents = result.get("documents")?.as_array()?;
    let active_index = result
        .get("activeDocument")
        .and_then(Value::as_u64)
        .map(|index| index as usize);
    let flagged = documents
        .iter()
        .find(|doc| doc.get("active").and_then(Value::as_bool) == Some(true));
    flagged
        .or_else(|| active_index.and_then(|index| documents.get(index)))
        .and_then(name_in)
}

fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .filter(|text| !text.is_empty() && *text != "none")
}

/// Turns one app's `ui.inspect` / `ui.state` reply into a `LiveState`.
fn state_from_inspect(app: &str, result: &Value) -> LiveState {
    let mut state = LiveState {
        tool: tool_name(result),
        dialog: dialog_name(result),
        document: document_from_ui(result),
        ..LiveState::default()
    };
    match app {
        "lightcraft" => {
            state.view = string_at(result, &["ui", "view"]).map(str::to_string);
            state.panel = string_at(result, &["ui", "right"]).map(str::to_string);
            state.busy = lightcraft_busy(result);
        }
        "vectorcraft" => {
            state.busy = result
                .get("background")
                .and_then(Value::as_array)
                .and_then(|jobs| jobs.iter().find_map(Value::as_str))
                .map(busy_label);
        }
        "photocraft" => {
            state.detail = result
                .get("document")
                .and_then(|doc| size_text(doc.get("width")?, doc.get("height")?));
            state.busy = result
                .get("jobs")
                .and_then(Value::as_array)
                .and_then(|jobs| jobs.first())
                .and_then(|job| job.get("label").or_else(|| job.get("name")))
                .and_then(Value::as_str)
                .map(busy_label);
        }
        "effectcraft" => {
            state.playing = result.get("playing").and_then(Value::as_bool) == Some(true);
        }
        "filmcraft" => {
            state.playing = result
                .get("playback")
                .and_then(|playback| playback.get("playing"))
                .and_then(Value::as_bool)
                == Some(true);
        }
        "pdfcraft" => {
            state.detail = pdf_page_detail(result);
            state.tool = result
                .get("quick_tool")
                .and_then(Value::as_str)
                .filter(|tool| *tool != "select" && *tool != "hand")
                .map(str::to_string);
            state.dialog = result
                .get("dialog")
                .and_then(text_of)
                .filter(|name| is_real_dialog(name));
            state.panel = result
                .get("mode")
                .and_then(Value::as_str)
                .map(str::to_string);
            state.busy = result
                .get("progress")
                .and_then(|progress| progress.get("label"))
                .and_then(Value::as_str)
                .map(busy_label);
        }
        _ => {}
    }
    state
}

fn size_text(width: &Value, height: &Value) -> Option<String> {
    let (w, h) = (width.as_u64()?, height.as_u64()?);
    (w > 0 && h > 0).then(|| format!("{w}×{h}"))
}

/// "Page 3 of 12" from PdfCraft's `ui.state`.
fn pdf_page_detail(result: &Value) -> Option<String> {
    let page = result.get("active")?.get("page")?.as_u64()?;
    let pages = result
        .get("documents")?
        .as_array()?
        .iter()
        .find(|doc| doc.get("active").and_then(Value::as_bool) == Some(true))?
        .get("pages")?
        .as_u64()?;
    Some(format!("Page {page} of {pages}"))
}

/// "6000×4000 · ★★★" from LightCraft's `photo.inspect`.
fn photo_detail(photo: &Value) -> Option<String> {
    let size = size_text(photo.get("width")?, photo.get("height")?);
    let stars = photo
        .get("rating")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(5) as usize;
    let rating = (stars > 0).then(|| "★".repeat(stars));
    match (size, rating) {
        (Some(size), Some(rating)) => Some(format!("{size} · {rating}")),
        (size, rating) => size.or(rating),
    }
}

/// "Exporting 3 of 10" style labels from the apps become the activity as-is.
fn busy_label(label: &str) -> String {
    let label = label.trim();
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn lightcraft_busy(result: &Value) -> Option<String> {
    let non_null = |key: &str| result.get(key).is_some_and(|value| !value.is_null());
    if result
        .get("export")
        .and_then(|export| export.get("running"))
        .is_some_and(|running| !running.is_null())
    {
        return Some("Exporting photos".to_string());
    }
    if non_null("import") {
        return Some("Importing photos".to_string());
    }
    if non_null("scan") {
        return Some("Scanning the library".to_string());
    }
    result
        .get("activity")
        .and_then(Value::as_array)
        .and_then(|tasks| tasks.iter().find_map(|task| task.get("label")?.as_str()))
        .map(busy_label)
}

/// "magic_wand", "camera-raw", "cameraRaw" or "filter.blur.gaussianBlur"
/// -> "Magic Wand", "Camera Raw", "Camera Raw", "Gaussian Blur".
pub fn prettify(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut spaced = String::new();
    let mut previous_lower = false;
    for ch in last.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !spaced.ends_with(' ') {
                spaced.push(' ');
            }
            previous_lower = false;
            continue;
        }
        if ch.is_uppercase() && previous_lower {
            spaced.push(' ');
        }
        spaced.push(ch);
        previous_lower = ch.is_lowercase() || ch.is_ascii_digit();
    }
    spaced
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tool words and what they mean. The last word of a tool name is checked
/// first ("Paint Bucket" is a bucket, "Magic Wand" is a wand).
const TOOL_ACTIVITIES: &[(&str, &str)] = &[
    ("eraser", "Erasing"),
    ("brush", "Painting"),
    ("pencil", "Painting"),
    ("paint", "Painting"),
    ("crop", "Cropping"),
    ("slice", "Slicing"),
    ("type", "Adding text"),
    ("text", "Adding text"),
    ("pen", "Drawing paths"),
    ("path", "Drawing paths"),
    ("bezier", "Drawing paths"),
    ("node", "Editing nodes"),
    ("anchor", "Editing nodes"),
    ("gradient", "Applying a gradient"),
    ("bucket", "Filling an area"),
    ("fill", "Filling an area"),
    ("clone", "Retouching"),
    ("stamp", "Retouching"),
    ("heal", "Retouching"),
    ("patch", "Retouching"),
    ("spot", "Retouching"),
    ("blur", "Retouching"),
    ("sharpen", "Retouching"),
    ("smudge", "Retouching"),
    ("dodge", "Dodging and burning"),
    ("burn", "Dodging and burning"),
    ("sponge", "Adjusting color"),
    ("eyedropper", "Picking a color"),
    ("picker", "Picking a color"),
    ("zoom", "Zooming in"),
    ("hand", "Panning around"),
    ("pan", "Panning around"),
    ("lasso", "Making a selection"),
    ("marquee", "Making a selection"),
    ("wand", "Making a selection"),
    ("select", "Making a selection"),
    ("selection", "Making a selection"),
    ("rect", "Drawing shapes"),
    ("rectangle", "Drawing shapes"),
    ("ellipse", "Drawing shapes"),
    ("polygon", "Drawing shapes"),
    ("shape", "Drawing shapes"),
    ("line", "Drawing shapes"),
    ("razor", "Cutting clips"),
    ("blade", "Cutting clips"),
    ("cut", "Cutting clips"),
    ("trim", "Trimming clips"),
    ("ripple", "Trimming clips"),
    ("slip", "Trimming clips"),
    ("slide", "Trimming clips"),
    ("move", "Moving layers"),
    ("transform", "Transforming"),
    ("rotate", "Rotating"),
    ("measure", "Measuring"),
    ("ruler", "Measuring"),
    ("redact", "Redacting"),
    ("comment", "Adding comments"),
    ("highlight", "Highlighting"),
    ("sign", "Signing"),
    ("link", "Adding links"),
    ("snapshot", "Taking a snapshot"),
    ("field", "Building a form"),
];

pub fn tool_activity(tool: &str) -> String {
    let pretty = prettify(tool);
    for word in pretty.to_lowercase().split_whitespace().rev() {
        let found = TOOL_ACTIVITIES
            .iter()
            .find(|(key, _)| word == *key || (key.len() >= 5 && word.starts_with(key)));
        if let Some((_, activity)) = found {
            return (*activity).to_string();
        }
    }
    format!("Using {pretty}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn user_rules_match_live_controls_on_any_platform() {
        use crate::config::ActivityRule;
        let rule = ActivityRule {
            app: "LightCraft".into(),
            element: "Info (I)".into(),
            activity: "My photo info".into(),
        };
        let mut state = super::LiveState {
            panel: Some("info".into()),
            ..Default::default()
        };
        assert_eq!(
            state
                .rule_activity("lightcraft", std::slice::from_ref(&rule))
                .as_deref(),
            Some("My photo info")
        );
        assert_eq!(
            state.rule_activity("PhotoCraft", std::slice::from_ref(&rule)),
            None
        );
        state.panel = Some("edit".into());
        assert_eq!(state.rule_activity("LightCraft", &[rule]), None);
    }

    #[cfg(unix)]
    #[test]
    fn token_permissions_are_repaired_without_changing_token() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let path =
            std::env::temp_dir().join(format!("craftpulse-permissions-{}", std::process::id()));
        fs::write(&path, "existing-token").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        super::ensure_token_file(&path);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing-token");
        fs::remove_file(path).unwrap();
    }
    use super::*;

    #[test]
    fn prettifies_ids() {
        assert_eq!(prettify("magic_wand"), "Magic Wand");
        assert_eq!(prettify("camera-raw"), "Camera Raw");
        assert_eq!(prettify("cameraRaw"), "Camera Raw");
        assert_eq!(prettify("filter.blur.gaussianBlur"), "Gaussian Blur");
        assert_eq!(prettify(""), "");
    }

    #[test]
    fn maps_tools_to_activities() {
        assert_eq!(tool_activity("brush"), "Painting");
        assert_eq!(tool_activity("paint_bucket"), "Filling an area");
        assert_eq!(tool_activity("magic_wand"), "Making a selection");
        assert_eq!(tool_activity("quick_selection"), "Making a selection");
        assert_eq!(tool_activity("move"), "Moving layers");
        assert_eq!(tool_activity("frobnicate"), "Using Frobnicate");
    }

    #[test]
    fn reply_errors_in_both_shapes_are_noticed() {
        assert!(has_error(&json!({"id": 1, "ok": false, "error": "no"})).is_some());
        assert!(has_error(&json!({"id": 1, "ok": false})).is_some());
        assert!(has_error(&json!({"id": 1, "ok": true, "result": {}})).is_none());
        assert!(has_error(&json!({"id": 1, "result": {}, "error": null})).is_none());
    }

    #[test]
    fn photocraft_reply_gives_tool_dialog_and_document() {
        let result = json!({
            "tool": "eraser",
            "dialogs": [{"id": 3, "kind": "export", "title": "Export As", "fields": {}}],
            "document": {"name": "poster.psd", "path": "C:\\Art\\poster.psd"},
        });
        let state = state_from_inspect("photocraft", &result);
        assert_eq!(state.tool.as_deref(), Some("eraser"));
        assert_eq!(state.dialog.as_deref(), Some("export"));
        assert_eq!(state.document.as_deref(), Some("poster.psd"));
        assert_eq!(state.activity("PhotoCraft").as_deref(), Some("Exporting"));
    }

    #[test]
    fn ui_inspect_does_not_mistake_other_names_for_the_document() {
        let result = json!({"name": "Dark theme", "tool": "move"});
        assert_eq!(document_from_ui(&result), None);
    }

    #[test]
    fn vector_and_design_documents_follow_the_active_index() {
        let result = json!({
            "tool": "pen",
            "documents": [{"title": "a.vcraft", "dirty": false}, {"title": "logo.vcraft", "dirty": true}],
            "activeDocument": 1,
            "background": [],
        });
        let state = state_from_inspect("vectorcraft", &result);
        assert_eq!(state.document.as_deref(), Some("logo.vcraft"));
        assert_eq!(
            state.activity("VectorCraft").as_deref(),
            Some("Drawing paths")
        );
    }

    #[test]
    fn lightcraft_reply_uses_view_and_panel() {
        let result = json!({
            "ui": {"view": "detail", "right": "masking"},
            "active": 7,
            "export": {"running": null, "last": null},
            "import": null,
            "activity": [],
        });
        let state = state_from_inspect("lightcraft", &result);
        assert_eq!(
            state.activity("LightCraft").as_deref(),
            Some("Masking a photo")
        );
        let grid = json!({"ui": {"view": "photoGrid", "right": "none"}});
        assert_eq!(
            state_from_inspect("lightcraft", &grid)
                .activity("LightCraft")
                .as_deref(),
            Some("Browsing the library")
        );
        let exporting = json!({"ui": {"view": "detail"}, "export": {"running": {"total": 3}}});
        assert_eq!(
            state_from_inspect("lightcraft", &exporting)
                .activity("LightCraft")
                .as_deref(),
            Some("Exporting photos")
        );
    }

    #[test]
    fn photo_inspect_reply_names_the_photo() {
        let reply = json!({"id": 7, "file_name": "IMG_0042.CR3", "rating": 3});
        assert_eq!(name_in(&reply).as_deref(), Some("IMG_0042.CR3"));
    }

    #[test]
    fn film_effect_and_pdf_replies() {
        let film = json!({"ui": {"tool": "razor"}, "playback": {"playing": true}, "dialog": null});
        let state = state_from_inspect("filmcraft", &film);
        assert_eq!(
            state.activity("FilmCraft").as_deref(),
            Some("Previewing the timeline")
        );
        let effect = json!({"ui": {"tool": "pen"}, "playing": false, "dialog": "CommandPalette"});
        let state = state_from_inspect("effectcraft", &effect);
        assert_eq!(
            state.activity("EffectCraft").as_deref(),
            Some("Using Command Palette")
        );
        let pdf = json!({
            "documents": [{"tab": 0, "name": "report.pdf", "active": true}],
            "quick_tool": "redact", "mode": "Edit", "dialog": null, "progress": null,
        });
        let state = state_from_inspect("pdfcraft", &pdf);
        assert_eq!(state.document.as_deref(), Some("report.pdf"));
        assert_eq!(state.activity("PdfCraft").as_deref(), Some("Redacting"));
    }

    #[test]
    fn details_for_pdf_photo_and_image_size() {
        let pdf = json!({
            "documents": [{"name": "a.pdf", "pages": 12, "active": true}],
            "active": {"page": 3},
        });
        assert_eq!(
            state_from_inspect("pdfcraft", &pdf).detail.as_deref(),
            Some("Page 3 of 12")
        );
        let photo = json!({"width": 6000, "height": 4000, "rating": 3});
        assert_eq!(photo_detail(&photo).as_deref(), Some("6000×4000 · ★★★"));
        assert_eq!(
            photo_detail(&json!({"width": 10, "height": 5, "rating": 0})).as_deref(),
            Some("10×5")
        );
        let psd = json!({"document": {"name": "p.psd", "width": 1920, "height": 1080}});
        assert_eq!(
            state_from_inspect("photocraft", &psd).detail.as_deref(),
            Some("1920×1080")
        );
    }

    #[test]
    fn dialog_activity_uses_known_dialogs_first() {
        let state = LiveState {
            dialog: Some("export".to_string()),
            ..LiveState::default()
        };
        assert_eq!(state.dialog_activity().as_deref(), Some("Exporting"));
        let state = LiveState {
            dialog: Some("cameraRaw".to_string()),
            ..LiveState::default()
        };
        assert_eq!(state.dialog_activity().as_deref(), Some("Using Camera Raw"));
    }

    /// Start real PhotoCraft with control port 7878 and the token file supplied here.
    #[test]
    #[ignore = "requires a running real PhotoCraft control server"]
    fn real_photocraft_control_roundtrip() {
        let token = std::env::var_os("CRAFTPULSE_TEST_PHOTOCRAFT_TOKEN_FILE")
            .expect("set CRAFTPULSE_TEST_PHOTOCRAFT_TOKEN_FILE");
        let mut client = ControlClient::new("PhotoCraft", 7878, PathBuf::from(token));
        let (connection, _) = client.connect().expect("authenticate with real PhotoCraft");
        client.connection = Some(connection);
        let state = client.read_state().expect("read real ui.inspect response");
        assert!(
            state.tool.is_some(),
            "PhotoCraft should report its selected tool"
        );
    }

    #[test]
    fn access_modes_match_the_apps() {
        assert_eq!(access_for("PhotoCraft"), Access::TokenFile);
        assert_eq!(access_for("pdfcraft"), Access::ControlFile);
        assert_eq!(access_for("LightCraft"), Access::Open);
    }
}

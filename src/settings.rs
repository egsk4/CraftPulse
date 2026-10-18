use crate::{
    config::{
        ActivityRule, AppConfig, CardButton, Config, RawCardButton, SharedConfig,
        DEFAULT_DETAILS_TEMPLATE, DEFAULT_STATE_TEMPLATE,
    },
    presence::PresenceCard,
    startup,
};
use chrono::NaiveTime;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    thread,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Clone, Deserialize)]
struct SettingsUpdate {
    enabled: Option<bool>,
    preferred_app: Option<String>,
    idle_after_secs: Option<u64>,
    hide_project_name: Option<bool>,
    hide_patterns: Option<Vec<String>>,
    action: Option<String>,
    launch_at_login: Option<bool>,
    live_prompted: Option<bool>,
    timer_mode: Option<String>,
    theme: Option<String>,
    check_updates: Option<bool>,
    include_prereleases: Option<bool>,
    rules: Option<Vec<ActivityRule>>,
    show_details: Option<bool>,
    buttons: Option<Vec<RawCardButton>>,
    quiet_hours: Option<bool>,
    #[serde(default, deserialize_with = "crate::quiet::hh_mm::deserialize_opt")]
    quiet_start: Option<NaiveTime>,
    #[serde(default, deserialize_with = "crate::quiet::hh_mm::deserialize_opt")]
    quiet_end: Option<NaiveTime>,
    pause_while_streaming: Option<bool>,
    milestones: Option<bool>,
    details_template: Option<String>,
    state_template: Option<String>,
    apps: Option<std::collections::BTreeMap<String, AppConfig>>,
}

#[derive(Debug, Clone, Serialize)]
struct SettingsResponse {
    enabled: bool,
    preferred_app: Option<String>,
    idle_after_secs: u64,
    hide_project_name: bool,
    hide_patterns: Vec<String>,
    action: String,
    launch_at_login: bool,
    live_prompted: bool,
    timer_mode: String,
    theme: String,
    check_updates: bool,
    include_prereleases: bool,
    rules: Vec<ActivityRule>,
    show_details: bool,
    buttons: Vec<CardButton>,
    quiet_hours: bool,
    #[serde(with = "crate::quiet::hh_mm")]
    quiet_start: NaiveTime,
    #[serde(with = "crate::quiet::hh_mm")]
    quiet_end: NaiveTime,
    pause_while_streaming: bool,
    milestones: bool,
    details_template: String,
    state_template: String,
    apps: std::collections::BTreeMap<String, AppConfig>,
}

#[derive(Clone, Default, Serialize)]
pub struct DetectionInspection {
    pub app: Option<String>,
    pub selection: &'static str,
    pub activity: Option<String>,
    pub activity_source: &'static str,
    pub live_status: &'static str,
    pub live_controls: Vec<String>,
    pub sharing: &'static str,
    pub rpc: &'static str,
}

struct RequestState {
    config: SharedConfig,
    log: Arc<Mutex<Vec<String>>>,
    preview: Arc<Mutex<Option<PresenceCard>>>,
    test_request: Arc<Mutex<Option<PresenceCard>>>,
    restart_needed: Arc<Mutex<Vec<String>>>,
    inspection: Arc<Mutex<DetectionInspection>>,
}

pub struct SettingsServer {
    address: SocketAddr,
    token: String,
    inspection: Arc<Mutex<DetectionInspection>>,
}

impl SettingsServer {
    pub fn start(
        config: SharedConfig,
        log: Arc<Mutex<Vec<String>>>,
        preview: Arc<Mutex<Option<PresenceCard>>>,
        test_request: Arc<Mutex<Option<PresenceCard>>>,
        restart_needed: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        let server = Server::http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .expect("could not start local settings server");
        let address = server.server_addr().to_ip().expect("local server address");
        let token = make_token();
        let thread_token = token.clone();
        let inspection = Arc::new(Mutex::new(DetectionInspection::default()));
        let state = RequestState {
            config,
            log,
            preview,
            test_request,
            restart_needed,
            inspection: Arc::clone(&inspection),
        };
        thread::Builder::new()
            .name("craftpulse-settings".to_string())
            .spawn(move || {
                for mut request in server.incoming_requests() {
                    let response = handle_request(&mut request, &thread_token, &state);
                    let _ = request.respond(response);
                }
            })
            .expect("could not start settings handler");
        Self {
            address,
            token,
            inspection,
        }
    }

    pub fn set_inspection(&self, inspection: DetectionInspection) {
        *self
            .inspection
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = inspection;
    }

    pub fn url(&self) -> String {
        format!("http://{}/?token={}", self.address, self.token)
    }
}

fn handle_request(
    request: &mut tiny_http::Request,
    token: &str,
    state: &RequestState,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let RequestState {
        config,
        log,
        preview,
        test_request,
        restart_needed,
        inspection,
    } = state;
    let path = request.url().split('?').next().unwrap_or("/");
    if path == "/" {
        let html = include_str!("../web/settings.html");
        return html_response(html, "text/html; charset=utf-8");
    }
    let provided = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("X-CraftPulse-Token"))
        .map(|header| header.value.as_str())
        .or_else(|| {
            request
                .url()
                .split_once("?token=")
                .map(|(_, value)| value.split('&').next().unwrap_or(""))
        });
    if provided != Some(token) {
        return Response::from_string("Unauthorized").with_status_code(StatusCode(401));
    }
    match (request.method(), path) {
        (&Method::Get, "/api/settings") => {
            let config = config.lock().unwrap_or_else(|e| e.into_inner());
            let body = serde_json::to_string(&SettingsResponse {
                enabled: config.enabled,
                preferred_app: config.preferred_app.clone(),
                idle_after_secs: config.idle_after_secs,
                hide_project_name: config.hide_project_name,
                hide_patterns: config.hide_patterns.clone(),
                action: config.action.clone(),
                launch_at_login: config.launch_at_login,
                live_prompted: config.live_prompted,
                timer_mode: config.timer_mode.clone(),
                theme: config.theme.clone(),
                check_updates: config.check_updates,
                include_prereleases: config.include_prereleases,
                rules: config.rules.clone(),
                show_details: config.show_details,
                buttons: config.buttons.clone(),
                quiet_hours: config.quiet_hours,
                quiet_start: config.quiet_start,
                quiet_end: config.quiet_end,
                pause_while_streaming: config.pause_while_streaming,
                milestones: config.milestones,
                details_template: config.details_template.clone(),
                state_template: config.state_template.clone(),
                apps: config.apps.clone(),
            })
            .unwrap_or_else(|_| "{}".to_string());
            json_response(body)
        }
        (&Method::Put, "/api/settings") => {
            let mut body = String::new();
            if request
                .as_reader()
                .take(65536)
                .read_to_string(&mut body)
                .is_err()
            {
                return Response::from_string("Invalid request body")
                    .with_status_code(StatusCode(400));
            }
            let update: SettingsUpdate = match serde_json::from_str(&body) {
                Ok(update) => update,
                Err(_) => {
                    return Response::from_string("Invalid settings")
                        .with_status_code(StatusCode(400))
                }
            };
            if let Some(patterns) = &update.hide_patterns {
                if let Some(pattern) = patterns.iter().find(|pattern| Regex::new(pattern).is_err())
                {
                    return Response::from_string(format!("Invalid regular expression: {pattern}"))
                        .with_status_code(StatusCode(400));
                }
            }
            let buttons = match update
                .buttons
                .map(|raw| {
                    raw.into_iter()
                        .map(CardButton::try_from)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
            {
                Ok(Some(buttons)) if buttons.len() > 2 => {
                    return Response::from_string("Discord allows at most two buttons")
                        .with_status_code(StatusCode(400));
                }
                Ok(buttons) => buttons,
                Err(message) => {
                    return Response::from_string(message).with_status_code(StatusCode(400));
                }
            };
            if update.idle_after_secs.is_some_and(|v| v > crate::detection::MAX_IDLE_SECS) {
                return Response::from_string("Idle timeout must be between 0 and 86400 seconds").with_status_code(StatusCode(400));
            }
            if let Some(rules) = &update.rules {
                let invalid = rules.iter().any(|rule| {
                    let supported = crate::APP_NAMES.iter()
                        .any(|name| name.eq_ignore_ascii_case(&rule.app));
                    !supported || rule.element.trim().is_empty()
                        || rule.activity.trim().is_empty()
                        || rule.element.chars().count() > 100
                        || rule.activity.chars().count() > 100
                });
                if rules.len() > 100 || invalid {
                    return Response::from_string("Use up to 100 rules with a supported app and nonempty control/activity text of at most 100 characters")
                        .with_status_code(StatusCode(400));
                }
            }
            if update.apps.as_ref().is_some_and(|apps| apps.values().any(|app| app.idle_after_secs.is_some_and(|seconds| seconds > crate::detection::MAX_IDLE_SECS))) {
                return Response::from_string("Per-app idle timeout must be between 0 and 86400 seconds").with_status_code(StatusCode(400));
            }
            if update.preferred_app.as_ref().is_some_and(|name| {
                !name.is_empty() && !crate::APP_NAMES.iter()
                    .any(|app| app.eq_ignore_ascii_case(name))
            }) {
                return Response::from_string("Choose a supported app or Automatic")
                    .with_status_code(StatusCode(400));
            }
            let mut guard = config.lock().unwrap_or_else(|e| e.into_inner());
            let mut config = guard.clone();
            if let Some(name) = update.preferred_app {
                config.preferred_app = crate::APP_NAMES.iter()
                    .find(|app| app.eq_ignore_ascii_case(&name))
                    .map(|app| app.to_string());
            }
            if let Some(value) = update.include_prereleases { config.include_prereleases = value; }
            if let Some(value) = update.rules { config.rules = value; }
            let launch_changed = update.launch_at_login.is_some_and(|v| v != config.launch_at_login);
            if let Some(value) = update.idle_after_secs { config.idle_after_secs = value; }
            if let Some(value) = update.quiet_hours {
                config.quiet_hours = value;
            }
            if let Some(value) = update.quiet_start {
                config.quiet_start = value;
            }
            if let Some(value) = update.quiet_end {
                config.quiet_end = value;
            }
            if let Some(value) = update.pause_while_streaming {
                config.pause_while_streaming = value;
            }
            if let Some(value) = update.check_updates {
                config.check_updates = value;
            }
            if let Some(value) = update.show_details {
                config.show_details = value;
            }
            if let Some(value) = update.milestones {
                config.milestones = value;
            }
            if let Some(buttons) = buttons {
                config.buttons = buttons;
            }
            let template = |value: String, default: &str| {
                let value: String = value.chars().take(200).collect();
                if value.trim().is_empty() { default.to_string() } else { value }
            };
            if let Some(value) = update.details_template {
                config.details_template = template(value, DEFAULT_DETAILS_TEMPLATE);
            }
            if let Some(value) = update.state_template {
                config.state_template = template(value, DEFAULT_STATE_TEMPLATE);
            }
            if let Some(value) = update.enabled {
                config.enabled = value;
            }
            if let Some(value) = update.hide_project_name {
                config.hide_project_name = value;
            }
            if let Some(value) = update.hide_patterns {
                config.hide_patterns = value;
            }
            if let Some(value) = update.action {
                config.action = value.chars().take(100).collect();
            }
            if let Some(value) = update.launch_at_login {
                config.launch_at_login = value;
            }
            if let Some(value) = update.live_prompted {
                config.live_prompted = value;
            }
            if let Some(value) = update
                .timer_mode
                .filter(|v| matches!(v.as_str(), "off" | "session" | "document"))
            {
                config.timer_mode = value;
            }
            if let Some(value) = update
                .theme
                .filter(|v| matches!(v.as_str(), "system" | "dark" | "light" | "mono"))
            {
                config.theme = value;
            }
            if let Some(value) = update.apps {
                config.apps = value
                    .into_iter()
                    .map(|(name, mut app)| {
                        if let Some(action) = app.action.take() {
                            app.action = Some(action.chars().take(100).collect());
                        }
                        (name, app)
                    })
                    .collect();
            }
            let launch_at_login = config.launch_at_login;
            if let Err(error) = config.save() {
                return Response::from_string(error).with_status_code(StatusCode(500));
            }
            config.load_error = None;
            *guard = config;
            drop(guard);
            if launch_changed {
                if let Err(error) = startup::set_enabled(launch_at_login) {
                    return Response::from_string(error).with_status_code(StatusCode(500));
                }
            }
            json_response(r#"{"saved":true}"#.to_string())
        }
        (&Method::Post, "/api/config/restore") => {
            let mut guard = config.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(error) = Config::restore_backup() { return Response::from_string(error).with_status_code(StatusCode(400)); }
            *guard = Config::load_or_create();
            json_response(r#"{"restored":true}"#.into())
        }
        (&Method::Get, "/api/monitor") => json_response(serde_json::to_string(&crate::monitor::sample()).unwrap_or_else(|_| "{}".into())),
        (&Method::Get, "/api/support") => {
            let cfg = config.lock().unwrap_or_else(|e| e.into_inner());
            json_response(serde_json::json!({
                "version": crate::update::current_version(), "platform": std::env::consts::OS,
                "architecture": std::env::consts::ARCH, "sharing_enabled": cfg.enabled,
                "config_error": cfg.load_error.is_some(), "resources": crate::monitor::sample(),
                "session_type": safe_session("XDG_SESSION_TYPE"),
                "desktop": safe_session("XDG_CURRENT_DESKTOP"),
                "privacy": "No project names, paths, tokens, rules, buttons or event logs included"
            }).to_string())
        }
        (&Method::Post, "/api/rules/test") => test_rules(request),
        (&Method::Get, "/api/diagnostics") => {
            let response = serde_json::json!({
                "platform": std::env::consts::OS,
                "config_error": config.lock().unwrap_or_else(|e| e.into_inner()).load_error,
                "resources": crate::monitor::sample(),
                "idle_seconds": Some(crate::detection::LAST_IDLE_SECONDS.load(std::sync::atomic::Ordering::Relaxed)).filter(|seconds| *seconds != u64::MAX),
                "config_path": Config::path().to_string_lossy(),
                "hints_path": Config::path().parent().unwrap_or_else(|| std::path::Path::new(".")).join("hints").to_string_lossy(),
                "discord_rpc": "See connection events below; preview is the last successfully sent card",
                "last_presence": *preview.lock().unwrap_or_else(|e| e.into_inner()),
                "inspection": *inspection.lock().unwrap_or_else(|e| e.into_inner()),
            });
            json_response(response.to_string())
        }
        (&Method::Get, "/api/live") => {
            let config = config.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let statuses: Vec<_> = config
                .apps
                .keys()
                .map(|name| crate::control::probe(&config, name))
                .collect();
            let restart = restart_needed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let body = serde_json::json!({"apps": statuses, "restart_needed": restart});
            json_response(body.to_string())
        }
        (&Method::Post, "/api/live/enable") => {
            let config = config.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let names: Vec<&str> = config.apps.keys().map(String::as_str).collect();
            let (applied, persisted, lines) = crate::control::enable_environment(&config, &names);
            json_response(serde_json::json!({"applied": applied, "persisted": persisted, "lines": lines, "platform": std::env::consts::OS}).to_string())
        }
        (&Method::Get, "/api/pause") => json_response(
            serde_json::json!({"paused": crate::pause::is_paused(), "until": crate::pause::until(), "auto": crate::pause::auto()}).to_string(),
        ),
        (&Method::Get, "/api/history") => json_response(
            serde_json::to_string(&crate::history::entries()).unwrap_or_else(|_| "[]".to_string()),
        ),
        (&Method::Post, "/api/hide-current") => {
            let Some((_, document)) = crate::history::current() else {
                return json_response(serde_json::json!({"hidden": null}).to_string());
            };
            let pattern = format!("^{}$", regex::escape(&document));
            let mut config = config.lock().unwrap_or_else(|e| e.into_inner());
            if !config.hide_patterns.contains(&pattern) {
                config.hide_patterns.push(pattern);
            }
            if let Err(error) = config.save() {
                return Response::from_string(error).with_status_code(StatusCode(500));
            }
            json_response(serde_json::json!({"hidden": document}).to_string())
        }
        (&Method::Post, "/api/pause") => {
            let mut body = String::new();
            let _ = request.as_reader().take(1024).read_to_string(&mut body);
            let minutes = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("minutes").and_then(|m| m.as_i64()));
            match minutes {
                Some(0) => crate::pause::resume(),
                Some(minutes) if minutes > 0 => crate::pause::pause_for_minutes(minutes),
                _ => crate::pause::pause_until_resumed(),
            }
            json_response(
                serde_json::json!({"paused": crate::pause::is_paused(), "until": crate::pause::until()}).to_string(),
            )
        }
        (&Method::Get, "/api/stats") => {
            let today = crate::stats::day_of(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            );
            let (today_totals, week_totals) = crate::stats::summary(today);
            json_response(serde_json::json!({"today": today_totals, "week": week_totals}).to_string())
        }
        (&Method::Post, "/api/stats/clear") => {
            crate::stats::clear();
            json_response("{}".to_string())
        }
        (&Method::Get, "/api/update") => json_response(
            serde_json::json!({
                "current": crate::update::current_version(),
                "available": crate::update::available(),
            })
            .to_string(),
        ),
        (&Method::Post, "/api/update/open") => {
            // Only ever opens the release page found by the update checker.
            if let Some(found) = crate::update::available() {
                let _ = open::that(found.url);
            }
            json_response("{}".to_string())
        }
        (&Method::Get, "/api/log") => {
            let entries = log.lock().unwrap_or_else(|e| e.into_inner()).clone();
            json_response(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string()))
        }
        (&Method::Post, "/api/test-action") => {
            let mut body = String::new();
            let _ = request.as_reader().take(4096).read_to_string(&mut body);
            let action = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(str::to_owned))
                .unwrap_or_else(|| "Preview action".to_string());
            let card = PresenceCard {
                app_name: "PhotoCraft".to_string(),
                document: "Your project".to_string(),
                state: format!(
                    "in PhotoCraft — {}",
                    action.chars().take(100).collect::<String>()
                ),
                large_image_key: "photocraft".to_string(),
                small_image_key: "artcraft".to_string(),
                started_at: None,
                buttons: Vec::new(),
            };
            *preview.lock().unwrap_or_else(|e| e.into_inner()) = Some(card.clone());
            *test_request.lock().unwrap_or_else(|e| e.into_inner()) = Some(card.clone());
            json_response(serde_json::to_string(&card).unwrap_or_else(|_| "{}".to_string()))
        }
        _ => Response::from_string("Not found").with_status_code(StatusCode(404)),
    }
}

fn make_token() -> String {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).expect("operating system random source unavailable");
    random.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}

fn html_response(body: &str, content_type: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(Header::from_bytes("Content-Type", content_type).unwrap())
}

fn safe_session(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| {
        value.len() <= 80
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.'))
    })
}

#[derive(Deserialize)]
struct RuleTest {
    app: String,
    controls: Vec<String>,
    rules: Vec<ActivityRule>,
    mode: crate::rules::MatchMode,
}

fn test_rules(request: &mut tiny_http::Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = String::new();
    if request
        .as_reader()
        .take(65537)
        .read_to_string(&mut body)
        .is_err()
        || body.len() > 65536
    {
        return Response::from_string("Rule test is too large").with_status_code(StatusCode(400));
    }
    let test: RuleTest = match serde_json::from_str(&body) {
        Ok(test) => test,
        Err(_) => {
            return Response::from_string("Invalid rule test").with_status_code(StatusCode(400))
        }
    };
    if !crate::APP_NAMES
        .iter()
        .any(|app| app.eq_ignore_ascii_case(&test.app))
        || test.rules.len() > 100
        || test.controls.len() > 32
        || test
            .controls
            .iter()
            .any(|control| control.chars().count() > 200)
        || test.rules.iter().any(|rule| {
            rule.element.chars().count() > 100
                || rule.activity.chars().count() > 100
                || rule.app.len() > 40
        })
    {
        return Response::from_string("Use a supported app, up to 32 controls and up to 100 rules")
            .with_status_code(StatusCode(400));
    }
    let controls: Vec<&str> = test.controls.iter().map(String::as_str).collect();
    let matched = crate::rules::matching_rule(&test.app, &controls, &test.rules, test.mode);
    json_response(
        serde_json::json!({
            "matched_rule": matched.map(|index| index + 1),
            "activity": matched.map(|index| &test.rules[index].activity),
        })
        .to_string(),
    )
}

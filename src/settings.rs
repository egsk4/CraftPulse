use crate::{
    config::{AppConfig, Config, SharedConfig},
    presence::PresenceCard,
    startup,
};
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
    hide_project_name: Option<bool>,
    hide_patterns: Option<Vec<String>>,
    action: Option<String>,
    launch_at_login: Option<bool>,
    apps: Option<std::collections::BTreeMap<String, AppConfig>>,
}

#[derive(Debug, Clone, Serialize)]
struct SettingsResponse {
    enabled: bool,
    hide_project_name: bool,
    hide_patterns: Vec<String>,
    action: String,
    launch_at_login: bool,
    apps: std::collections::BTreeMap<String, AppConfig>,
}

pub struct SettingsServer {
    address: SocketAddr,
    token: String,
}

impl SettingsServer {
    pub fn start(
        config: SharedConfig,
        log: Arc<Mutex<Vec<String>>>,
        preview: Arc<Mutex<Option<PresenceCard>>>,
        test_request: Arc<Mutex<Option<PresenceCard>>>,
    ) -> Self {
        let server = Server::http(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .expect("could not start local settings server");
        let address = server.server_addr().to_ip().expect("local server address");
        let token = make_token();
        let thread_token = token.clone();
        thread::Builder::new()
            .name("craftpulse-settings".to_string())
            .spawn(move || {
                for mut request in server.incoming_requests() {
                    let response = handle_request(
                        &mut request,
                        &thread_token,
                        &config,
                        &log,
                        &preview,
                        &test_request,
                    );
                    let _ = request.respond(response);
                }
            })
            .expect("could not start settings handler");
        Self { address, token }
    }

    pub fn url(&self) -> String {
        format!("http://{}/?token={}", self.address, self.token)
    }
}

fn handle_request(
    request: &mut tiny_http::Request,
    token: &str,
    config: &SharedConfig,
    log: &Arc<Mutex<Vec<String>>>,
    preview: &Arc<Mutex<Option<PresenceCard>>>,
    test_request: &Arc<Mutex<Option<PresenceCard>>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
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
                hide_project_name: config.hide_project_name,
                hide_patterns: config.hide_patterns.clone(),
                action: config.action.clone(),
                launch_at_login: config.launch_at_login,
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
            let mut config = config.lock().unwrap_or_else(|e| e.into_inner());
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
            drop(config);
            if let Err(error) = startup::set_enabled(launch_at_login) {
                return Response::from_string(error).with_status_code(StatusCode(500));
            }
            json_response(r#"{"saved":true}"#.to_string())
        }
        (&Method::Get, "/api/diagnostics") => {
            let response = serde_json::json!({
                "platform": std::env::consts::OS,
                "config_path": Config::path().to_string_lossy(),
                "hints_path": Config::path().parent().unwrap_or_else(|| std::path::Path::new(".")).join("hints").to_string_lossy(),
                "discord_rpc": "Application ID 1558510318513033329",
                "last_presence": *preview.lock().unwrap_or_else(|e| e.into_inner()),
            });
            json_response(response.to_string())
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

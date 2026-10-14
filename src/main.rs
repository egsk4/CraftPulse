#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod control;
mod detection;
mod hints;
mod notify;
mod popup;
mod presence;
mod privacy;
mod rate_limit;
mod settings;
mod startup;
#[cfg(test)]
mod tests;
mod tray;

use config::Config;
use control::ControlHub;
use detection::ProcessDetector;
use hints::read_hint;
use presence::{PresenceCard, RpcWorker};
use rate_limit::RpcRateLimiter;
use settings::SettingsServer;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const APP_NAMES: [&str; 7] = [
    "PhotoCraft",
    "VectorCraft",
    "FilmCraft",
    "LightCraft",
    "PdfCraft",
    "EffectCraft",
    "DesignCraft",
];
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How often to resend an unchanged card while connected. A refreshed or
/// restarted Discord drops the activity (or the pipe), and this puts it back.
const RPC_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// How often to try reaching Discord while it is closed. Failed connection
/// attempts send nothing to Discord, so they don't count against the rate limit.
const RPC_RETRY_INTERVAL: Duration = Duration::from_secs(3);

fn main() {
    let shared_config = Arc::new(Mutex::new(Config::load_or_create()));
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let preview = Arc::new(Mutex::new(None));
    let test_request = Arc::new(Mutex::new(None));
    let restart_needed = Arc::new(Mutex::new(Vec::<String>::new()));
    let settings = SettingsServer::start(
        Arc::clone(&shared_config),
        Arc::clone(&log),
        Arc::clone(&preview),
        Arc::clone(&test_request),
        Arc::clone(&restart_needed),
    );
    let settings_url = settings.url();
    // First run: open the settings page so the live-detection question is seen.
    if !shared_config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .live_prompted
    {
        let _ = open::that(&settings_url);
    }
    let _server = settings;

    let quit = tray::start(settings_url);
    let mut detector = ProcessDetector::default();
    let hub = ControlHub::start();
    let mut rpc = RpcWorker::new();
    let mut last_presence: Option<PresenceCard> = None;
    let mut last_rpc_attempt = Instant::now() - RPC_RETRY_INTERVAL;
    let mut test_override: Option<(PresenceCard, Instant)> = None;
    let mut rate_limiter = RpcRateLimiter::default();
    let mut last_poll = Instant::now();
    let mut last_titles: Vec<String> = Vec::new();
    let mut last_unknown: Vec<String> = Vec::new();
    let mut discord_unavailable_logged = false;
    // Apps already reminded about this session (one toast each).
    let mut toasted: Vec<String> = Vec::new();

    loop {
        if quit.load(std::sync::atomic::Ordering::Relaxed) {
            rpc.quit();
            break;
        }
        let elapsed = last_poll.elapsed();
        if elapsed < POLL_INTERVAL {
            thread::sleep(POLL_INTERVAL - elapsed);
        }
        last_poll = Instant::now();

        let current_config = {
            let mut config = shared_config.lock().unwrap_or_else(|e| e.into_inner());
            config.reload_if_changed();
            config.clone()
        };
        detector.set_rules(&current_config.rules);
        let detected = detector.detect(&APP_NAMES);
        let auto_activity = detector.activity.clone();
        let idle = detector.idle;
        if detector.unknown != last_unknown {
            last_unknown = detector.unknown.clone();
            if !last_unknown.is_empty() {
                push_log(
                    &log,
                    &format!("Unrecognised ArtCraft-like processes: {last_unknown:?}"),
                );
            }
        }
        if detector.titles != last_titles {
            last_titles = detector.titles.clone();
            if !last_titles.is_empty() {
                push_log(&log, &format!("Window titles: {last_titles:?}"));
            }
        }
        // Point the control-channel reader at the app we are showing.
        hub.set_target(detected.as_ref().and_then(|app| {
            let app_config = current_config.app_config(&app.name);
            (app_config.enabled && app_config.live).then(|| {
                (
                    app.name.clone(),
                    current_config.control_port(&app.name),
                    current_config.control_token_file(&app.name),
                )
            })
        }));
        {
            let needs: Vec<String> = detected
                .as_ref()
                .filter(|app| current_config.live_prompted && hub.needs_restart(&app.name))
                .map(|app| vec![app.name.clone()])
                .unwrap_or_default();
            let mut shared = restart_needed.lock().unwrap_or_else(|e| e.into_inner());
            if *shared != needs {
                if let Some(name) = needs.first() {
                    if !toasted.contains(name) {
                        toasted.push(name.clone());
                        notify::toast(
                            "Restart needed",
                            &format!("Close and reopen {name} once so CraftPulse can show what you're doing."),
                        );
                    }
                    push_log(
                        &log,
                        &format!("{name} is running without live detection; restart it once to turn it on"),
                    );
                }
                *shared = needs;
            }
        }
        for message in hub.take_logs() {
            push_log(&log, &message);
        }
        let next_presence = detected.and_then(|app| {
            let app_config = current_config.app_config(&app.name);
            if !app_config.enabled || !current_config.enabled {
                return None;
            }
            let hint = read_hint(&current_config.hints_dir(), &app.name);
            // What the app itself says (control channel) beats anything guessed
            // from its windows.
            let live = hub.state_for(&app.name);
            let document = live
                .as_ref()
                .and_then(|state| state.document.clone())
                .or_else(|| hint.as_ref().and_then(|h| h.document.clone()))
                .or(app.document.clone());
            let busy = live.as_ref().and_then(|state| state.busy.clone());
            let live_activity = live.as_ref().and_then(|state| state.activity(&app.name));
            let action = hint
                .as_ref()
                .and_then(|h| h.action.clone())
                .or_else(|| app_config.action.clone())
                .or(busy)
                .or_else(|| idle.then(|| "Idle".to_string()))
                .or(live_activity)
                .or_else(|| auto_activity.clone())
                .unwrap_or_else(|| default_activity(&app.name, &current_config.action));
            Some(privacy::make_card(
                &app,
                document.as_deref(),
                &action,
                &current_config,
                &app_config,
            ))
        });

        let pending_test = test_request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(test_card) = pending_test {
            test_override = Some((test_card, Instant::now() + Duration::from_secs(10)));
        }
        if test_override
            .as_ref()
            .is_some_and(|(_, expires)| Instant::now() >= *expires)
        {
            test_override = None;
        }

        let active_card = test_override
            .as_ref()
            .map(|(card, _)| card.clone())
            .or(next_presence);
        if active_card.is_none() {
            if last_presence.is_some() && rate_limiter.try_acquire(Instant::now()) {
                rpc.clear();
                last_presence = None;
            }
            // Allow an immediate send when presence comes back (re-enabled, or app returns).
            last_rpc_attempt = Instant::now() - RPC_RETRY_INTERVAL;
        } else if let Some(card) = active_card {
            let changed = last_presence.as_ref() != Some(&card);
            let connected = last_presence.is_some();
            let waited = last_rpc_attempt.elapsed();
            let should_send = if connected {
                (changed && waited >= Duration::from_secs(5)) || waited >= RPC_KEEPALIVE_INTERVAL
            } else {
                // Not connected (Discord closed, restarted or refreshed): keep trying,
                // quickly and without giving up, so presence returns by itself.
                waited >= RPC_RETRY_INTERVAL
            };
            let now = Instant::now();
            // Sending a card counts against Discord's rate limit; failing to
            // connect sends nothing, so it never needs a slot.
            if should_send && (!connected || rate_limiter.try_acquire(now)) {
                last_rpc_attempt = now;
                let (sent, timed_out) = rpc.set_activity(card.clone());
                if sent {
                    if !connected {
                        push_log(&log, "Connected to Discord");
                    }
                    last_presence = Some(card.clone());
                    *preview.lock().unwrap_or_else(|e| e.into_inner()) = Some(card);
                    if test_override.is_some() {
                        push_log(&log, "Test activity sent to Discord");
                    } else if changed || !connected {
                        push_log(&log, "Rich Presence updated");
                    }
                } else {
                    if connected {
                        push_log(&log, "Lost the connection to Discord; reconnecting");
                    } else if !discord_unavailable_logged {
                        push_log(&log, "Discord is unavailable; retrying every few seconds");
                    }
                    discord_unavailable_logged = true;
                    last_presence = None;
                    if timed_out {
                        push_log(&log, "Discord RPC timed out after 4 seconds");
                        rpc = RpcWorker::new();
                    }
                }
                if sent {
                    discord_unavailable_logged = false;
                }
            }
        }
    }
}

fn push_log(log: &Arc<Mutex<Vec<String>>>, message: &str) {
    let mut entries = log.lock().unwrap_or_else(|e| e.into_inner());
    entries.push(format!("{}  {message}", chrono_free_timestamp()));
    if entries.len() > 200 {
        let excess = entries.len() - 200;
        entries.drain(0..excess);
    }
}

fn chrono_free_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("+{seconds}")
}

/// Per-app default shown when nothing more specific is known. If you change the
/// default activity in settings away from "Creating", your text is used instead.
fn default_activity(app_name: &str, configured: &str) -> String {
    if configured != "Creating" {
        return configured.to_string();
    }
    match app_name.to_ascii_lowercase().as_str() {
        "photocraft" => "Editing an image",
        "vectorcraft" => "Drawing vectors",
        "filmcraft" => "Editing a video",
        "lightcraft" => "Developing photos",
        "pdfcraft" => "Working on a PDF",
        "effectcraft" => "Making motion graphics",
        "designcraft" => "Designing a layout",
        _ => "Creating",
    }
    .to_string()
}

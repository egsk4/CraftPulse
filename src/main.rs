#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod control;
mod detection;
mod hints;
mod history;
mod instance;
#[cfg(target_os = "linux")]
mod linux;
mod milestones;
mod monitor;
mod notify;
mod pause;
mod popup;
mod presence;
mod privacy;
mod quiet;
mod rate_limit;
mod rules;
mod settings;
mod startup;
mod stats;
mod storage;
#[cfg(test)]
mod tests;
mod tray;
mod update;

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

const APP_NAMES: [&str; 13] = [
    "PhotoCraft",
    "VectorCraft",
    "FilmCraft",
    "LightCraft",
    "PdfCraft",
    "EffectCraft",
    "DesignCraft",
    "FigCraft",
    "WordCraft",
    "SoundCraft",
    "DeckCraft",
    "CADCraft",
    "GridCraft",
];
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How often to resend an unchanged card while connected. A refreshed or
/// restarted Discord drops the activity (or the pipe), and this puts it back.
const RPC_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// How often to try reaching Discord while it is closed. Failed connection
/// attempts send nothing to Discord, so they don't count against the rate limit.
const RPC_RETRY_INTERVAL: Duration = Duration::from_secs(3);

fn main() {
    let _instance = match instance::acquire() {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("Could not lock CraftPulse instance: {error}");
            return;
        }
    };
    monitor::start();
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
    // Keep the "launch at login" entry pointing at this exe, in case it was moved,
    // renamed or replaced by a new version.
    if shared_config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .launch_at_login
    {
        if let Err(error) = startup::set_enabled(true) {
            push_log(&log, &format!("Could not refresh launch at login: {error}"));
        }
    }
    update::start(Arc::clone(&shared_config));
    // First run: open the settings page so the live-detection question is seen.
    if !shared_config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .live_prompted
    {
        let _ = open::that(&settings_url);
    }
    let server = settings;

    let quit = tray::start(settings_url);
    let mut detector = ProcessDetector::default();
    let hub = ControlHub::start();
    // The Discord application ID lives in config.toml; the environment overrides it.
    let application_id = std::env::var("CRAFTPULSE_APPLICATION_ID").unwrap_or_else(|_| {
        shared_config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .application_id
            .clone()
    });
    let mut rpc = RpcWorker::new(application_id.clone());
    let mut last_presence: Option<PresenceCard> = None;
    let mut last_rpc_attempt = Instant::now() - RPC_RETRY_INTERVAL;
    let mut test_override: Option<(PresenceCard, Instant)> = None;
    let mut rate_limiter = RpcRateLimiter::default();
    let mut last_poll = Instant::now();
    let mut poll_interval = POLL_INTERVAL;
    let mut was_idle = false;
    let mut last_titles: Vec<String> = Vec::new();
    let mut last_unknown: Vec<String> = Vec::new();
    let mut discord_unavailable_logged = false;
    let mut milestones = milestones::Milestones::default();
    let mut last_auto_pause: Option<&'static str> = None;
    let mut stat_carry = 0.0_f64;
    let mut last_stats_save = Instant::now();
    // Elapsed-timer starting points (unix seconds), per app session and per document.
    let mut session_timer: Option<((String, u32), i64)> = None;
    type DocumentTimer = ((String, u32, Option<String>), i64);
    let mut document_timer: Option<DocumentTimer> = None;
    // Apps already reminded about this session (one toast each).
    let mut toasted: Vec<String> = Vec::new();

    loop {
        if quit.load(std::sync::atomic::Ordering::Relaxed) {
            rpc.quit();
            stats::save();
            break;
        }
        let elapsed = last_poll.elapsed();
        if elapsed < poll_interval {
            thread::sleep(poll_interval - elapsed);
        }
        let loop_seconds = last_poll.elapsed().as_secs_f64().min(2.0);
        last_poll = Instant::now();

        let current_config = {
            let mut config = shared_config.lock().unwrap_or_else(|e| e.into_inner());
            config.reload_if_changed();
            config.clone()
        };
        detector.set_rules(&current_config.rules);
        detector.idle_after_secs = current_config.idle_after_secs;
        detector.idle_overrides = current_config
            .apps
            .iter()
            .filter_map(|(name, app)| {
                app.idle_after_secs
                    .map(|seconds| (name.to_ascii_lowercase(), seconds))
            })
            .collect();
        detector.preferred_app = current_config.preferred_app.clone();
        let supported: Vec<&str> = APP_NAMES
            .iter()
            .copied()
            .filter(|name| current_config.app_config(name).enabled)
            .collect();
        let detected = detector.detect(&supported);
        poll_interval = if current_config.enabled && detected.is_some() {
            POLL_INTERVAL
        } else {
            Duration::from_secs(2)
        };
        let auto_activity = detector.activity.clone();
        let idle = detector.idle;
        if idle != was_idle {
            push_log(
                &log,
                if idle {
                    "Idle threshold reached"
                } else {
                    "Input resumed; idle status cleared"
                },
            );
            was_idle = idle;
        }
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
            let port = current_config.control_port(&app.name);
            let control_available = port != 0 || app.name == "PdfCraft";
            (app_config.enabled && app_config.live && control_available).then(|| {
                (
                    app.name.clone(),
                    port,
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
        let app_present = detected.is_some();
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let in_use_app = detected.as_ref().map(|app| app.name.clone());
        let mut inspection = settings::DetectionInspection {
            app: in_use_app.clone(),
            selection: detector.selection_reason,
            activity_source: "No activity selected",
            ..settings::DetectionInspection::default()
        };
        let next_presence = detected.and_then(|app| {
            let app_config = current_config.app_config(&app.name);
            if !app_config.enabled || !current_config.enabled {
                return None;
            }
            let hint = read_hint(&current_config.hints_dir(), &app.name);
            // What the app itself says (control channel) beats anything guessed
            // from its windows.
            let live = hub.state_for(&app.name);
            inspection.live_status = if !app_config.live {
                "Window detection selected"
            } else if current_config.control_port(&app.name) == 0 && app.name != "PdfCraft" {
                "No control port configured"
            } else if live.is_some() {
                "Live API state available"
            } else {
                "Waiting for live API"
            };
            if let Some(state) = &live {
                inspection.live_controls = [&state.tool, &state.panel, &state.view]
                    .into_iter()
                    .filter_map(|value| value.clone())
                    .collect();
            }
            let document = live
                .as_ref()
                .and_then(|state| state.document.clone())
                .or_else(|| hint.as_ref().and_then(|h| h.document.clone()))
                .or(app.document.clone());
            let busy = live.as_ref().and_then(|state| state.busy.clone());
            let live_activity = live.as_ref().and_then(|state| state.activity(&app.name));
            let (action, source) = hint
                .as_ref()
                .and_then(|hint| hint.action.clone())
                .map(|action| (action, "Integration hint"))
                .or_else(|| {
                    app_config
                        .action
                        .clone()
                        .map(|action| (action, "Per-app override"))
                })
                .or_else(|| busy.map(|action| (action, "Live background work")))
                .or_else(|| {
                    live.as_ref()
                        .and_then(|state| state.dialog_activity())
                        .map(|action| (action, "Live dialog"))
                })
                .or_else(|| idle.then(|| ("Idle".to_string(), "Idle timeout")))
                .or_else(|| {
                    live.as_ref()
                        .and_then(|state| state.rule_activity(&app.name, &current_config.rules))
                        .map(|action| (action, "Live control rule"))
                })
                .or_else(|| live_activity.map(|action| (action, "Live app state")))
                .or_else(|| {
                    auto_activity
                        .clone()
                        .map(|action| (action, "Window or accessibility detection"))
                })
                .unwrap_or_else(|| {
                    (
                        default_activity(&app.name, &current_config.action),
                        "Fallback activity",
                    )
                });
            inspection.activity = Some(action.clone());
            inspection.activity_source = source;
            let started_at = match current_config.timer_mode.as_str() {
                "session" => {
                    let key = (app.name.clone(), app.process_id);
                    if session_timer.as_ref().map(|(k, _)| k) != Some(&key) {
                        session_timer = Some((key, now_unix));
                    }
                    session_timer.as_ref().map(|(_, start)| *start)
                }
                "document" => {
                    let key = (app.name.clone(), app.process_id, document.clone());
                    if document_timer.as_ref().map(|(k, _)| k) != Some(&key) {
                        document_timer = Some((key, now_unix));
                    }
                    document_timer.as_ref().map(|(_, start)| *start)
                }
                _ => None,
            };
            let mut card = privacy::make_card(
                &app,
                document.as_deref(),
                &action,
                &current_config,
                &app_config,
            );
            card.started_at = started_at;
            let detail = live.as_ref().and_then(|state| state.detail.clone());
            let private = card.document == "Private project" || card.document == "Working";
            let shown_detail = if current_config.show_details && !private {
                detail.unwrap_or_default()
            } else {
                String::new()
            };
            let document_name = card.document.clone();
            let values = [
                ("app", app.name.as_str()),
                ("activity", action.as_str()),
                ("document", document_name.as_str()),
                ("detail", shown_detail.as_str()),
            ];
            let mut details_text =
                privacy::render_template(&current_config.details_template, &values);
            // Without {detail} in the template, extras are appended to the document line.
            if !shown_detail.is_empty() && !current_config.details_template.contains("{detail}") {
                details_text = format!("{details_text} · {shown_detail}");
            }
            if !details_text.is_empty() {
                card.document = hints::truncate_chars(&details_text, 128);
            }
            let state_text = privacy::render_template(&current_config.state_template, &values);
            if !state_text.is_empty() {
                card.state = hints::truncate_chars(&state_text, 128);
            }
            history::record(now_unix, &app.name, &action, &document_name);
            history::set_current(document.clone().map(|name| (app.name.clone(), name)));
            if current_config.milestones {
                let key = (app.name.clone(), app.process_id);
                if let Some(note) = milestones.update(&key, now_unix, &action, Instant::now()) {
                    card.state = hints::truncate_chars(&format!("{} · {note}", card.state), 128);
                }
            }
            card.buttons = current_config
                .buttons
                .iter()
                .take(2)
                .map(|button| (button.label.clone(), button.url.clone()))
                .collect();
            Some(card)
        });
        // Count time in each app (only while you're active and sharing isn't paused).
        let auto_pause = if quiet::is_quiet(&current_config) {
            Some("quiet hours")
        } else if current_config.pause_while_streaming && detector.streaming {
            Some("a streaming or recording app is running")
        } else {
            None
        };
        if let Some(name) = in_use_app.as_ref().filter(|name| {
            stats::should_count(
                current_config.enabled,
                current_config.app_config(name).enabled,
                idle,
                pause::is_paused(),
                auto_pause.is_some(),
            )
        }) {
            stat_carry += loop_seconds;
            if stat_carry >= 1.0 {
                let whole = stat_carry as u64;
                stat_carry -= whole as f64;
                stats::record(name, stats::day_of(now_unix), whole);
            }
        } else {
            stat_carry = 0.0;
        }
        if last_stats_save.elapsed() >= Duration::from_secs(60) {
            last_stats_save = Instant::now();
            stats::save();
        }
        // Paused from the tray or settings: show nothing.
        pause::set_auto(auto_pause);
        if auto_pause != last_auto_pause {
            last_auto_pause = auto_pause;
            match auto_pause {
                Some(reason) => push_log(&log, &format!("Sharing paused: {reason}")),
                None => push_log(&log, "Sharing resumed"),
            }
        }
        let next_presence = if pause::is_paused() || auto_pause.is_some() {
            None
        } else {
            next_presence
        };
        if !app_present {
            milestones.reset();
            history::set_current(None);
            session_timer = None;
            document_timer = None;
        }

        let pending_test = test_request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut test_card) = pending_test {
            test_card.buttons = current_config
                .buttons
                .iter()
                .take(2)
                .map(|button| (button.label.clone(), button.url.clone()))
                .collect();
            test_override = Some((test_card, Instant::now() + Duration::from_secs(10)));
        }
        if test_override
            .as_ref()
            .is_some_and(|(_, expires)| Instant::now() >= *expires)
        {
            test_override = None;
        }

        let active_card = if !current_config.enabled || pause::is_paused() || auto_pause.is_some() {
            test_override = None;
            None
        } else {
            test_override
                .as_ref()
                .map(|(card, _)| card.clone())
                .or(next_presence)
        };
        if active_card.is_none() {
            *preview.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
                    *preview.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    if timed_out {
                        push_log(&log, "Discord RPC timed out after 4 seconds");
                        rpc = RpcWorker::new(application_id.clone());
                    }
                }
                if sent {
                    discord_unavailable_logged = false;
                }
            }
        }
        inspection.sharing = if !current_config.enabled {
            "Sharing disabled"
        } else if pause::is_paused() {
            "Paused manually"
        } else if auto_pause.is_some() {
            "Paused automatically"
        } else if test_override.is_some() {
            "Test card active"
        } else if inspection.app.is_none() {
            "Waiting for an app"
        } else {
            "Ready to share"
        };
        inspection.rpc = if last_presence.is_some() {
            "Last update written to Discord"
        } else {
            "No active card written"
        };
        server.set_inspection(inspection);
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
        "figcraft" => "Designing an interface",
        "wordcraft" => "Writing a document",
        "soundcraft" => "Editing audio",
        "deckcraft" => "Editing a presentation",
        "cadcraft" => "Drafting",
        "gridcraft" => "Editing a spreadsheet",
        _ => "Creating",
    }
    .to_string()
}

//! Checks GitHub for a newer CraftPulse release. It only tells the user;
//! nothing is downloaded or installed automatically.

use crate::config::SharedConfig;
use serde::Serialize;
use std::{
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const RELEASES_URL: &str = "https://api.github.com/repos/egsk4/CraftPulse/releases?per_page=10";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

static AVAILABLE: Mutex<Option<UpdateInfo>> = Mutex::new(None);

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn available() -> Option<UpdateInfo> {
    AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// "CraftPulse-v0.3.1-alpha" -> (0, 3, 1). Anything after the numbers is ignored.
pub fn parse_version(tag: &str) -> Option<(u32, u32, u32)> {
    let start = tag.find(|ch: char| ch.is_ascii_digit())?;
    let mut parts = tag[start..]
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().ok());
    Some((
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
        parts.next().flatten().unwrap_or(0),
    ))
}

/// The newest non-draft release in a GitHub `releases` reply, if it is newer than `current`.
pub fn newer_release(body: &str, current: &str) -> Option<UpdateInfo> {
    let releases: serde_json::Value = serde_json::from_str(body).ok()?;
    let current = parse_version(current)?;
    releases
        .as_array()?
        .iter()
        .filter(|release| release["draft"].as_bool() != Some(true))
        .filter_map(|release| {
            let tag = release["tag_name"].as_str()?;
            let url = release["html_url"].as_str()?;
            Some((parse_version(tag)?, tag.to_string(), url.to_string()))
        })
        .max_by_key(|(version, _, _)| *version)
        .filter(|(version, _, _)| *version > current)
        .filter(|(_, _, url)| url.starts_with("https://github.com/"))
        .map(|(_, tag, url)| UpdateInfo { version: tag, url })
}

fn fetch() -> Option<String> {
    ureq::get(RELEASES_URL)
        .set("User-Agent", "CraftPulse")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .ok()?
        .into_string()
        .ok()
}

/// Starts the background checker. It does nothing while `check_updates` is off.
pub fn start(config: SharedConfig) {
    let _ = thread::Builder::new()
        .name("craftpulse-update".to_string())
        .spawn(move || {
            let mut last: Option<Instant> = None;
            loop {
                let enabled = config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .check_updates;
                let due = last.is_none_or(|at| at.elapsed() >= CHECK_EVERY);
                if enabled && due {
                    last = Some(Instant::now());
                    if let Some(found) =
                        fetch().and_then(|body| newer_release(&body, current_version()))
                    {
                        let first_time = available().as_ref() != Some(&found);
                        *AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
                        if first_time {
                            crate::notify::toast(
                                "CraftPulse update available",
                                &format!(
                                    "{} is out. Open CraftPulse's settings to get it.",
                                    found.version
                                ),
                            );
                        }
                    }
                }
                thread::sleep(Duration::from_secs(30));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_tags() {
        assert_eq!(parse_version("CraftPulse-v0.3.1-alpha"), Some((0, 3, 1)));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn picks_a_newer_release_only() {
        let body = r#"[
          {"tag_name":"CraftPulse-v0.2.0","html_url":"https://github.com/egsk4/CraftPulse/releases/tag/CraftPulse-v0.2.0","draft":false},
          {"tag_name":"CraftPulse-v0.4.0","html_url":"https://github.com/egsk4/CraftPulse/releases/tag/CraftPulse-v0.4.0","draft":false},
          {"tag_name":"CraftPulse-v0.9.0","html_url":"https://github.com/egsk4/CraftPulse/releases/tag/x","draft":true}
        ]"#;
        let found = newer_release(body, "0.3.0").unwrap();
        assert_eq!(found.version, "CraftPulse-v0.4.0");
        assert_eq!(newer_release(body, "0.4.0"), None);
        assert_eq!(newer_release("not json", "0.3.0"), None);
    }

    #[test]
    fn ignores_links_that_are_not_github() {
        let body = r#"[{"tag_name":"v9.0.0","html_url":"https://evil.example/x","draft":false}]"#;
        assert_eq!(newer_release(body, "0.3.0"), None);
    }
}

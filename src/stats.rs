//! Local usage statistics: how long each Craft app was in use. Stored only in
//! `stats.json` next to config.toml and never sent anywhere.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

/// Days are whole UTC days since 1970 (kept as the map key).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stats {
    /// day number -> app name -> seconds
    pub days: BTreeMap<i64, BTreeMap<String, u64>>,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

fn path() -> PathBuf {
    Config::path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("stats.json")
}

pub fn day_of(unix_seconds: i64) -> i64 {
    unix_seconds.div_euclid(86_400)
}

impl Stats {
    pub fn add(&mut self, app: &str, day: i64, seconds: u64) {
        *self
            .days
            .entry(day)
            .or_default()
            .entry(app.to_string())
            .or_insert(0) += seconds;
        // Keep about two months of history.
        let oldest = day - 60;
        self.days.retain(|d, _| *d >= oldest);
    }

    /// Seconds per app for days in `from..=to`.
    pub fn totals(&self, from: i64, to: i64) -> BTreeMap<String, u64> {
        let mut totals = BTreeMap::new();
        for (_, apps) in self.days.range(from..=to) {
            for (app, seconds) in apps {
                *totals.entry(app.clone()).or_insert(0) += seconds;
            }
        }
        totals
    }
}

fn with<T>(f: impl FnOnce(&mut Stats) -> T) -> T {
    let mut guard = STATS.lock().unwrap_or_else(|e| e.into_inner());
    let stats = guard.get_or_insert_with(|| {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    });
    f(stats)
}

pub fn record(app: &str, day: i64, seconds: u64) {
    with(|stats| stats.add(app, day, seconds));
}

pub fn save() {
    let text = with(|stats| serde_json::to_string(stats).ok());
    if let Some(text) = text {
        if let Some(parent) = path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::storage::atomic_write(&path(), text.as_bytes());
    }
}

pub fn clear() {
    with(|stats| stats.days.clear());
    save();
}

/// (today, last 7 days including today), seconds per app.
pub fn summary(today: i64) -> (BTreeMap<String, u64>, BTreeMap<String, u64>) {
    with(|stats| (stats.totals(today, today), stats.totals(today - 6, today)))
}

pub fn should_count(
    enabled: bool,
    app_enabled: bool,
    idle: bool,
    paused: bool,
    auto_paused: bool,
) -> bool {
    enabled && app_enabled && !idle && !paused && !auto_paused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_counts_enabled_active_unpaused_sharing() {
        for mask in 0..32 {
            let enabled = mask & 1 != 0;
            let app = mask & 2 != 0;
            let idle = mask & 4 != 0;
            let paused = mask & 8 != 0;
            let auto = mask & 16 != 0;
            assert_eq!(should_count(enabled, app, idle, paused, auto), mask == 3);
        }
    }
    #[test]
    fn totals_cover_the_requested_days_only() {
        let mut stats = Stats::default();
        stats.add("PhotoCraft", 100, 60);
        stats.add("PhotoCraft", 100, 30);
        stats.add("LightCraft", 99, 10);
        stats.add("LightCraft", 80, 500);
        assert_eq!(stats.totals(100, 100).get("PhotoCraft"), Some(&90));
        let week = stats.totals(94, 100);
        assert_eq!(week.get("LightCraft"), Some(&10));
        assert_eq!(week.get("PhotoCraft"), Some(&90));
    }

    #[test]
    fn old_days_are_dropped() {
        let mut stats = Stats::default();
        stats.add("PdfCraft", 1, 5);
        stats.add("PdfCraft", 100, 5);
        assert!(!stats.days.contains_key(&1));
    }

    #[test]
    fn day_numbers_use_utc_days() {
        assert_eq!(day_of(0), 0);
        assert_eq!(day_of(86_399), 0);
        assert_eq!(day_of(86_400), 1);
    }
}

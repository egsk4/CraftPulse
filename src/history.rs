//! Short in-memory list of what CraftPulse showed recently, plus the document
//! currently open (so the settings page can offer "Hide this document").
//! Nothing here is written to disk.

use serde::Serialize;
use std::{collections::VecDeque, sync::Mutex};

const MAX_ENTRIES: usize = 40;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    /// Unix seconds.
    pub at: i64,
    pub app: String,
    pub activity: String,
    /// As shown on Discord (already masked if the name is hidden).
    pub document: String,
}

static HISTORY: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());
static CURRENT: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Adds an entry unless it repeats the newest one.
pub fn record(at: i64, app: &str, activity: &str, document: &str) {
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    let same = history.back().is_some_and(|last| {
        last.app == app && last.activity == activity && last.document == document
    });
    if same {
        return;
    }
    history.push_back(Entry {
        at,
        app: app.to_string(),
        activity: activity.to_string(),
        document: document.to_string(),
    });
    while history.len() > MAX_ENTRIES {
        history.pop_front();
    }
}

/// Newest first.
pub fn entries() -> Vec<Entry> {
    HISTORY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .rev()
        .cloned()
        .collect()
}

/// The app and the real (unmasked) name of its open document.
pub fn set_current(current: Option<(String, String)>) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = current;
}

pub fn current() -> Option<(String, String)> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_skipped_and_history_is_capped() {
        HISTORY.lock().unwrap().clear();
        record(1, "LightCraft", "Editing a photo", "a.cr3");
        record(2, "LightCraft", "Editing a photo", "a.cr3");
        record(3, "LightCraft", "Exporting", "a.cr3");
        assert_eq!(entries().len(), 2);
        assert_eq!(entries()[0].activity, "Exporting");
        for i in 0..100 {
            record(10 + i, "PdfCraft", &format!("step {i}"), "x.pdf");
        }
        assert_eq!(entries().len(), MAX_ENTRIES);
        HISTORY.lock().unwrap().clear();
    }
}

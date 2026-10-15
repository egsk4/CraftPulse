//! "Pause sharing" state, shared by the tray menu, the settings page and the main loop.

use std::sync::{
    atomic::{AtomicI64, Ordering},
    Mutex,
};

/// Unix seconds until which sharing is paused. 0: not paused. `i64::MAX`: until resumed.
static PAUSED_UNTIL: AtomicI64 = AtomicI64::new(0);

/// Why sharing is paused automatically right now (quiet hours, streaming).
static AUTO: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn set_auto(reason: Option<&'static str>) {
    *AUTO.lock().unwrap_or_else(|e| e.into_inner()) = reason;
}

pub fn auto() -> Option<&'static str> {
    *AUTO.lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn pause_for_minutes(minutes: i64) {
    PAUSED_UNTIL.store(
        now().saturating_add(minutes.clamp(1, 24 * 60) * 60),
        Ordering::Relaxed,
    );
}

pub fn pause_until_resumed() {
    PAUSED_UNTIL.store(i64::MAX, Ordering::Relaxed);
}

pub fn resume() {
    PAUSED_UNTIL.store(0, Ordering::Relaxed);
}

pub fn is_paused() -> bool {
    PAUSED_UNTIL.load(Ordering::Relaxed) > now()
}

/// 0 when not paused, `i64::MAX` when paused until resumed, else the resume time.
pub fn until() -> i64 {
    if is_paused() {
        PAUSED_UNTIL.load(Ordering::Relaxed)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_and_resume() {
        resume();
        assert!(!is_paused());
        pause_for_minutes(30);
        assert!(is_paused());
        assert!(until() > now());
        pause_until_resumed();
        assert_eq!(until(), i64::MAX);
        resume();
        assert!(!is_paused());
        assert_eq!(until(), 0);
    }
}

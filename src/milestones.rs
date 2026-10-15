//! Short celebratory notes on the card: "2h in", "export #5".

use std::time::{Duration, Instant};

const NOTE_LASTS: Duration = Duration::from_secs(5 * 60);
const EXPORT_MILESTONES: [u32; 6] = [1, 5, 10, 25, 50, 100];

#[derive(Default)]
pub struct Milestones {
    app: Option<(String, u32)>,
    started: i64,
    last_hour: i64,
    exports: u32,
    exporting: bool,
    note: Option<(String, Instant)>,
}

impl Milestones {
    /// Call every loop with the app in use and its current activity text.
    /// Returns the note to show right now, if any.
    pub fn update(
        &mut self,
        app: &(String, u32),
        now_unix: i64,
        activity: &str,
        now: Instant,
    ) -> Option<String> {
        if self.app.as_ref() != Some(app) {
            *self = Self::default();
            self.app = Some(app.clone());
            self.started = now_unix;
        }
        let hours = (now_unix - self.started) / 3600;
        if hours > self.last_hour {
            self.last_hour = hours;
            self.note = Some((format!("{hours}h in"), now));
        }
        let exporting = activity.to_ascii_lowercase().starts_with("export");
        if exporting && !self.exporting {
            self.exports += 1;
            if EXPORT_MILESTONES.contains(&self.exports) {
                self.note = Some((format!("export #{}", self.exports), now));
            }
        }
        self.exporting = exporting;
        if self
            .note
            .as_ref()
            .is_some_and(|(_, at)| now.duration_since(*at) >= NOTE_LASTS)
        {
            self.note = None;
        }
        self.note.as_ref().map(|(text, _)| text.clone())
    }

    /// Forget everything (app closed).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_marks_and_export_counts() {
        let key = ("PhotoCraft".to_string(), 1);
        let mut m = Milestones::default();
        let t0 = Instant::now();
        assert_eq!(m.update(&key, 1000, "Editing", t0), None);
        assert_eq!(
            m.update(&key, 1000 + 3600, "Editing", t0).as_deref(),
            Some("1h in")
        );
        // The note fades after five minutes.
        assert_eq!(
            m.update(&key, 1000 + 3700, "Editing", t0 + Duration::from_secs(301)),
            None
        );
        assert_eq!(
            m.update(&key, 5000, "Exporting", t0 + Duration::from_secs(400))
                .as_deref(),
            Some("export #1")
        );
        // Staying in the same export doesn't count again.
        m.update(&key, 5001, "Exporting", t0 + Duration::from_secs(401));
        assert_eq!(m.exports, 1);
        m.update(&key, 5002, "Editing", t0 + Duration::from_secs(402));
        m.update(&key, 5003, "Exporting", t0 + Duration::from_secs(403));
        assert_eq!(m.exports, 2);
    }

    #[test]
    fn a_new_app_session_starts_over() {
        let mut m = Milestones::default();
        let t0 = Instant::now();
        m.update(&("A".into(), 1), 0, "x", t0);
        m.update(&("A".into(), 1), 7300, "x", t0);
        m.update(&("A".into(), 2), 7400, "x", t0);
        assert_eq!(m.last_hour, 0);
    }
}

//! Quiet hours: automatically stop sharing during a daily time window.

use crate::config::Config;
use chrono::Timelike;

/// "22:30" -> minutes since midnight.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// Whether `now` (minutes since midnight) falls in `start..end`. The window
/// may cross midnight (22:00 to 07:00). Equal start and end means no window.
pub fn in_window(now: u32, start: u32, end: u32) -> bool {
    if start == end {
        false
    } else if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

pub fn is_quiet(config: &Config) -> bool {
    if !config.quiet_hours {
        return false;
    }
    let (Some(start), Some(end)) = (
        parse_hhmm(&config.quiet_start),
        parse_hhmm(&config.quiet_end),
    ) else {
        return false;
    };
    let now = chrono::Local::now();
    in_window(now.hour() * 60 + now.minute(), start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_hhmm("07:30"), Some(450));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("7"), None);
    }

    #[test]
    fn windows_including_overnight() {
        let (start, end) = (parse_hhmm("22:00").unwrap(), parse_hhmm("07:00").unwrap());
        assert!(in_window(parse_hhmm("23:30").unwrap(), start, end));
        assert!(in_window(parse_hhmm("03:00").unwrap(), start, end));
        assert!(!in_window(parse_hhmm("12:00").unwrap(), start, end));
        assert!(in_window(600, 540, 1020));
        assert!(!in_window(1100, 540, 1020));
        assert!(!in_window(600, 600, 600));
    }
}

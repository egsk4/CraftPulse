//! Quiet hours: automatically stop sharing during a daily time window.

use crate::config::Config;
use chrono::NaiveTime;

/// Serde format for a time of day written as "22:30" (config.toml and the settings API).
pub mod hh_mm {
    use chrono::NaiveTime;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%H:%M";

    pub fn serialize<S: Serializer>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&time.format(FORMAT))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NaiveTime, D::Error> {
        let text = String::deserialize(deserializer)?;
        NaiveTime::parse_from_str(text.trim(), FORMAT).map_err(Error::custom)
    }

    pub fn deserialize_opt<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<NaiveTime>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| NaiveTime::parse_from_str(text.trim(), FORMAT).map_err(Error::custom))
            .transpose()
    }
}

/// Whether `now` falls in `start..end`. The window may cross midnight
/// (22:00 to 07:00). Equal start and end means no window.
pub fn in_window(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start == end {
        false
    } else if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

pub fn is_quiet(config: &Config) -> bool {
    config.quiet_hours
        && in_window(
            chrono::Local::now().time(),
            config.quiet_start,
            config.quiet_end,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveTime {
        NaiveTime::parse_from_str(text, "%H:%M").unwrap()
    }

    #[test]
    fn parses_times() {
        assert!(NaiveTime::parse_from_str("24:00", "%H:%M").is_err());
        assert!(NaiveTime::parse_from_str("7", "%H:%M").is_err());
        let config: Config = toml_edit::de::from_str("quiet_start = \"07:30\"").unwrap();
        assert_eq!(config.quiet_start, at("07:30"));
        assert!(toml_edit::de::from_str::<Config>("quiet_start = \"25:00\"").is_err());
    }

    #[test]
    fn windows_including_overnight() {
        let (start, end) = (at("22:00"), at("07:00"));
        assert!(in_window(at("23:30"), start, end));
        assert!(in_window(at("03:00"), start, end));
        assert!(!in_window(at("12:00"), start, end));
        assert!(in_window(at("10:00"), at("09:00"), at("17:00")));
        assert!(!in_window(at("18:20"), at("09:00"), at("17:00")));
        assert!(!in_window(at("10:00"), at("10:00"), at("10:00")));
    }
}

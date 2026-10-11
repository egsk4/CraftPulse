use serde::Deserialize;
use std::{fs, path::Path, time::SystemTime};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Hint {
    pub action: Option<String>,
    pub document: Option<String>,
}

pub fn read_hint(hints_dir: &Path, app_name: &str) -> Option<Hint> {
    let canonical_path = hints_dir.join(format!("{app_name}.json"));
    let lowercase_path = hints_dir.join(format!("{}.json", app_name.to_ascii_lowercase()));
    let path = if canonical_path.exists() {
        canonical_path
    } else {
        lowercase_path
    };
    let metadata = fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    if SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        .as_secs()
        > 120
    {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let mut hint: Hint = serde_json::from_str(&text).ok()?;
    hint.action = hint.action.map(|s| truncate_chars(&s, 100));
    hint.document = hint.document.map(|s| truncate_chars(&s, 100));
    Some(hint)
}

pub fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncates_by_unicode_characters_not_bytes() {
        assert_eq!(
            truncate_chars("界".repeat(101).as_str(), 100),
            "界".repeat(100)
        );
    }
    #[test]
    fn parses_hint_values() {
        let value: Hint =
            serde_json::from_str(r#"{"action":"Exporting","document":"Cover"}"#).unwrap();
        assert_eq!(value.action.as_deref(), Some("Exporting"));
    }

    #[test]
    fn rejects_stale_hints() {
        let temp = std::env::temp_dir().join(format!("craftpulse-hints-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp);
        let path = temp.join("PhotoCraft.json");
        fs::write(&path, r#"{"action":"Editing","document":"Old"}"#).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(180);
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(old)).unwrap();
        assert_eq!(read_hint(&temp, "PhotoCraft"), None);
        let _ = fs::remove_dir_all(temp);
    }
}

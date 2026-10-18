use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use toml_edit::{de, value, DocumentMut};

pub const DEFAULT_DETAILS_TEMPLATE: &str = "{document}";
pub const DEFAULT_STATE_TEMPLATE: &str = "in {app} — {activity}";

pub type SharedConfig = Arc<Mutex<Config>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub preferred_app: Option<String>,
    /// Seconds without input before Idle; zero disables idle status.
    pub idle_after_secs: u64,
    pub hide_project_name: bool,
    pub hide_patterns: Vec<String>,
    pub action: String,
    pub application_id: String,
    pub launch_at_login: bool,
    /// The first-run question about live detection has been answered.
    pub live_prompted: bool,
    /// Elapsed-time display on the Discord card: "off", "session" or "document".
    pub timer_mode: String,
    /// Look for newer releases on GitHub.
    pub check_updates: bool,
    pub include_prereleases: bool,
    /// Add extras like "Page 3 of 12" or image size to the document line.
    pub show_details: bool,
    /// Up to two link buttons on the card.
    #[serde(deserialize_with = "lenient_buttons")]
    pub buttons: Vec<CardButton>,
    /// Stop sharing between `quiet_start` and `quiet_end` (24h "HH:MM", may cross midnight).
    pub quiet_hours: bool,
    #[serde(with = "crate::quiet::hh_mm")]
    pub quiet_start: NaiveTime,
    #[serde(with = "crate::quiet::hh_mm")]
    pub quiet_end: NaiveTime,
    /// Stop sharing while a streaming or recording app is running.
    pub pause_while_streaming: bool,
    /// Occasional notes on the card ("2h in", "export #5").
    pub milestones: bool,
    /// Text of the top line. Placeholders: {app} {activity} {document} {detail}.
    pub details_template: String,
    /// Text of the second line. Same placeholders.
    pub state_template: String,
    /// Settings page look: "system", "dark", "light" or "mono".
    pub theme: String,
    pub apps: BTreeMap<String, AppConfig>,
    pub rules: Vec<ActivityRule>,
    #[serde(skip)]
    last_modified: Option<SystemTime>,
    #[serde(skip)]
    pub load_error: Option<String>,
}

/// Shows `activity` while the button/tab whose name starts with `element`
/// is selected in `app`. Add these under `[[rules]]` in config.toml.
/// A button as typed or read from a file, before it is checked.
#[derive(Debug, Clone, Deserialize)]
pub struct RawCardButton {
    pub label: String,
    pub url: String,
}

/// A link button on the Discord card (Discord allows two, https only).
/// The only way to get one from text is `TryFrom<RawCardButton>`, so every
/// `CardButton` already meets Discord's limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "RawCardButton")]
pub struct CardButton {
    pub label: String,
    pub url: String,
}

impl TryFrom<RawCardButton> for CardButton {
    type Error = String;

    fn try_from(raw: RawCardButton) -> Result<Self, String> {
        let (label, url) = (raw.label.trim().to_string(), raw.url.trim().to_string());
        if label.is_empty()
            || label.chars().count() > 32
            || url.len() > 512
            || !url.starts_with("https://")
            || url.contains(char::is_whitespace)
        {
            return Err(format!(
                "Button “{label}” needs a label of up to 32 characters and a link starting with https://"
            ));
        }
        Ok(Self { label, url })
    }
}

/// Reads the button list from config.toml, skipping a hand-edited bad entry
/// instead of failing the whole file (which would reset every setting).
fn lenient_buttons<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<CardButton>, D::Error> {
    let raw = Vec::<RawCardButton>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|b| b.try_into().ok()).collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityRule {
    pub app: String,
    pub element: String,
    pub activity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub enabled: bool,
    pub idle_after_secs: Option<u64>,
    pub hide_project_name: Option<bool>,
    pub action: Option<String>,
    pub large_image_key: Option<String>,
    /// Read the app's own state over its JSON control channel (see control.rs).
    pub live: bool,
    /// Port the app's control channel listens on. Default: see `default_control_port`.
    pub control_port: Option<u16>,
    /// Token file shared with the app. Default: `control/<app>.token` next to config.toml.
    pub control_token_file: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let apps = crate::APP_NAMES
            .into_iter()
            .map(|name| {
                let live = default_control_port(name) != 0 || name == "PdfCraft";
                let slug = name.to_ascii_lowercase();
                let image_key = if live {
                    slug
                } else {
                    format!("https://raw.githubusercontent.com/storytold/{slug}/main/assets/app-icon/{slug}-1024.png")
                };
                let settings = AppConfig {
                    large_image_key: Some(image_key),
                    live,
                    ..AppConfig::default()
                };
                (name.to_string(), settings)
            })
            .collect();
        Self {
            enabled: true,
            preferred_app: None,
            idle_after_secs: 300,
            hide_project_name: false,
            hide_patterns: Vec::new(),
            action: "Creating".to_string(),
            application_id: "1558510318513033329".to_string(),
            launch_at_login: false,
            live_prompted: false,
            timer_mode: "session".to_string(),
            check_updates: true,
            include_prereleases: false,
            show_details: true,
            buttons: Vec::new(),
            quiet_hours: false,
            quiet_start: NaiveTime::from_hms_opt(22, 0, 0).expect("valid time"),
            quiet_end: NaiveTime::from_hms_opt(7, 0, 0).expect("valid time"),
            pause_while_streaming: false,
            milestones: false,
            details_template: DEFAULT_DETAILS_TEMPLATE.to_string(),
            state_template: DEFAULT_STATE_TEMPLATE.to_string(),
            theme: "system".to_string(),
            apps,
            rules: Vec::new(),
            last_modified: None,
            load_error: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_after_secs: None,
            hide_project_name: None,
            action: None,
            large_image_key: None,
            live: true,
            control_port: None,
            control_token_file: None,
        }
    }
}

/// Default control-channel port per app: the ports the apps' own docs use.
/// (DesignCraft's docs also say 7979, which VectorCraft uses, so it gets 7981.)
/// PdfCraft picks a random port itself; see `Config::control_token_file`.
pub fn default_control_port(name: &str) -> u16 {
    match name.to_ascii_lowercase().as_str() {
        "photocraft" => 7878,
        "vectorcraft" => 7979,
        "lightcraft" => 7980,
        "designcraft" => 7981,
        "filmcraft" => 9876,
        "effectcraft" => 9877,
        _ => 0,
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("CraftPulse")
            .join("config.toml")
    }

    pub fn hints_dir(&self) -> PathBuf {
        Self::path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("hints")
    }

    pub fn load_or_create() -> Self {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !path.exists() {
            let mut config = Self::default();
            if config.save_to(path).is_err() {
                config.enabled = false;
                config.hide_project_name = true;
                config.load_error = Some(
                    "Could not create config.toml. Sharing is off; check directory permissions."
                        .into(),
                );
            }
            return config;
        }
        let mut config = match fs::read_to_string(path).map_err(|e| e.to_string())
            .and_then(|text| de::from_str::<Self>(&text).map_err(|_| "Invalid config.toml".to_string())) {
            Ok(config) => config,
            Err(_) => Self { enabled: false, hide_project_name: true,
                load_error: Some("Configuration could not be loaded. Sharing is off. Repair config.toml or restore its backup in settings.".into()),
                ..Self::default() },
        };
        config.ensure_builtin_apps();
        config.last_modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        config
    }

    fn ensure_builtin_apps(&mut self) {
        for (name, app) in Self::default().apps {
            if !self.apps.keys().any(|key| key.eq_ignore_ascii_case(&name)) {
                self.apps.insert(name, app);
            }
        }
    }

    pub fn reload_if_changed(&mut self) {
        let path = Self::path();
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        if modified.is_none() || modified == self.last_modified {
            return;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(mut loaded) = de::from_str::<Self>(&text) {
                loaded.ensure_builtin_apps();
                loaded.last_modified = modified;
                *self = loaded;
            }
        }
    }

    /// Folder holding the bearer-token files shared with the apps.
    pub fn control_dir() -> PathBuf {
        Self::path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("control")
    }

    pub fn control_port(&self, name: &str) -> u16 {
        self.app_config(name)
            .control_port
            .unwrap_or_else(|| default_control_port(name))
    }

    /// PhotoCraft: the bearer-token file. PdfCraft: the control file the app
    /// writes (`pdfcraft --control <file>`). Other apps need neither.
    pub fn control_token_file(&self, name: &str) -> PathBuf {
        self.app_config(name)
            .control_token_file
            .filter(|file| !file.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if name.eq_ignore_ascii_case("pdfcraft") {
                    return dirs::home_dir()
                        .unwrap_or_else(std::env::temp_dir)
                        .join(".pdfcraft-control.json");
                }
                Self::control_dir().join(format!("{}.token", name.to_ascii_lowercase()))
            })
    }

    pub fn app_config(&self, name: &str) -> AppConfig {
        self.apps
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    pub fn restore_backup() -> Result<(), String> {
        let path = Self::path();
        let text = fs::read_to_string(path.with_extension("toml.bak"))
            .map_err(|_| "No readable config.toml.bak found".to_string())?;
        let mut restored: Self =
            de::from_str(&text).map_err(|_| "Backup is invalid".to_string())?;
        restored.enabled = false;
        if let Ok(old) = fs::read(&path) {
            let mut preserved = tempfile::Builder::new()
                .prefix("config-recovered-")
                .suffix(".toml")
                .tempfile_in(path.parent().ok_or("Missing config directory")?)
                .map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut preserved, &old).map_err(|e| e.to_string())?;
            preserved.keep().map_err(|e| e.to_string())?;
        }
        crate::storage::atomic_write(&path, text.as_bytes())?;
        restored.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        fs::create_dir_all(path.parent().ok_or("Missing config directory")?)
            .map_err(|e| e.to_string())?;
        let mut document = if path.exists() {
            let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
            de::from_str::<Self>(&text).map_err(|_| {
                "Config is invalid. Repair it or restore the backup before saving.".to_string()
            })?;
            text.parse::<DocumentMut>().map_err(|e| e.to_string())?
        } else {
            default_document()
        };
        set_value(&mut document["enabled"], value(self.enabled));
        if let Some(name) = &self.preferred_app {
            set_value(&mut document["preferred_app"], value(name.clone()));
        } else {
            document.remove("preferred_app");
        }
        set_value(
            &mut document["hide_project_name"],
            value(self.hide_project_name),
        );
        set_value(&mut document["action"], value(self.action.clone()));
        set_value(
            &mut document["application_id"],
            value(self.application_id.clone()),
        );
        set_value(
            &mut document["launch_at_login"],
            value(self.launch_at_login),
        );
        set_value(&mut document["live_prompted"], value(self.live_prompted));
        set_value(&mut document["timer_mode"], value(self.timer_mode.clone()));
        set_value(&mut document["theme"], value(self.theme.clone()));
        set_value(&mut document["check_updates"], value(self.check_updates));
        set_value(
            &mut document["include_prereleases"],
            value(self.include_prereleases),
        );
        let mut rules = toml_edit::ArrayOfTables::new();
        for rule in &self.rules {
            let mut table = toml_edit::Table::new();
            table["app"] = value(rule.app.clone());
            table["element"] = value(rule.element.clone());
            table["activity"] = value(rule.activity.clone());
            rules.push(table);
        }
        document["rules"] = toml_edit::Item::ArrayOfTables(rules);
        set_value(&mut document["show_details"], value(self.show_details));
        set_value(
            &mut document["details_template"],
            value(self.details_template.clone()),
        );
        set_value(
            &mut document["state_template"],
            value(self.state_template.clone()),
        );
        set_value(
            &mut document["idle_after_secs"],
            value(self.idle_after_secs as i64),
        );
        set_value(&mut document["quiet_hours"], value(self.quiet_hours));
        set_value(
            &mut document["quiet_start"],
            value(self.quiet_start.format("%H:%M").to_string()),
        );
        set_value(
            &mut document["quiet_end"],
            value(self.quiet_end.format("%H:%M").to_string()),
        );
        set_value(
            &mut document["pause_while_streaming"],
            value(self.pause_while_streaming),
        );
        set_value(&mut document["milestones"], value(self.milestones));
        let mut buttons = toml_edit::Array::new();
        for button in &self.buttons {
            let mut entry = toml_edit::InlineTable::new();
            entry.insert("label", button.label.as_str().into());
            entry.insert("url", button.url.as_str().into());
            buttons.push(entry);
        }
        set_value(&mut document["buttons"], value(buttons));
        let mut patterns = toml_edit::Array::new();
        for pattern in &self.hide_patterns {
            patterns.push(pattern.as_str());
        }
        set_value(&mut document["hide_patterns"], value(patterns));
        if document
            .get("apps")
            .and_then(toml_edit::Item::as_table)
            .is_none()
        {
            document["apps"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        for (name, app) in &self.apps {
            if document["apps"].as_table().unwrap().get(name).is_none() {
                document["apps"][name] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let table = &mut document["apps"][name];
            set_value(&mut table["enabled"], value(app.enabled));
            if let Some(seconds) = app.idle_after_secs {
                set_value(&mut table["idle_after_secs"], value(seconds as i64));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("idle_after_secs");
            }
            if let Some(hidden) = app.hide_project_name {
                set_value(&mut table["hide_project_name"], value(hidden));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("hide_project_name");
            }
            if let Some(action) = &app.action {
                set_value(&mut table["action"], value(action.clone()));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("action");
            }
            if let Some(key) = &app.large_image_key {
                set_value(&mut table["large_image_key"], value(key.clone()));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("large_image_key");
            }
            set_value(&mut table["live"], value(app.live));
            if let Some(port) = app.control_port {
                set_value(&mut table["control_port"], value(i64::from(port)));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("control_port");
            }
            if let Some(file) = &app.control_token_file {
                set_value(&mut table["control_token_file"], value(file.clone()));
            } else if let Some(table) = table.as_table_mut() {
                table.remove("control_token_file");
            }
        }
        if path.exists() {
            let old = fs::read(path).map_err(|e| e.to_string())?;
            crate::storage::atomic_write(&path.with_extension("toml.bak"), &old)?;
        }
        crate::storage::atomic_write(path, document.to_string().as_bytes())?;
        Ok(())
    }
}

fn set_value(item: &mut toml_edit::Item, replacement: toml_edit::Item) {
    if let (Some(previous), Some(next)) = (item.as_value_mut(), replacement.as_value()) {
        let mut next = next.clone();
        *next.decor_mut() = previous.decor().clone();
        *previous = next;
    } else {
        *item = replacement;
    }
}

fn default_document() -> DocumentMut {
    r#"# CraftPulse settings — changes are picked up automatically.
enabled = true
hide_project_name = false
hide_patterns = []
action = "Creating"
application_id = "1558510318513033329"
launch_at_login = false

[apps.PhotoCraft]
enabled = true
large_image_key = "photocraft"

[apps.VectorCraft]
enabled = true
large_image_key = "vectorcraft"

[apps.FilmCraft]
enabled = true
large_image_key = "filmcraft"

[apps.LightCraft]
enabled = true
large_image_key = "lightcraft"

[apps.PdfCraft]
enabled = true
large_image_key = "pdfcraft"

[apps.EffectCraft]
enabled = true
large_image_key = "effectcraft"

[apps.DesignCraft]
enabled = true
large_image_key = "designcraft"

[apps.FigCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/figcraft/main/assets/app-icon/figcraft-1024.png"

[apps.WordCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/wordcraft/main/assets/app-icon/wordcraft-1024.png"

[apps.SoundCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/soundcraft/main/assets/app-icon/soundcraft-1024.png"

[apps.DeckCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/deckcraft/main/assets/app-icon/deckcraft-1024.png"

[apps.CADCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/cadcraft/main/assets/app-icon/cadcraft-1024.png"

[apps.GridCraft]
enabled = true
live = false
large_image_key = "https://raw.githubusercontent.com/storytold/gridcraft/main/assets/app-icon/gridcraft-1024.png"

# Optional: show an activity while a button or tab is selected in an app.
# Find element names by running CraftPulse with CRAFTPULSE_DUMP_UIA=1
# (it writes uia-<app>.txt next to this file).
# [[rules]]
# app = "LightCraft"
# element = "Info (I)"
# activity = "Viewing photo info"
"#
    .parse()
    .expect("static default config is valid TOML")
}

#[cfg(test)]
mod tests {
    #[test]
    fn older_configs_gain_missing_apps_without_overwriting_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[apps.photocraft]\nenabled = false\nhide_project_name = true\naction = 'Painting'\n",
        )
        .unwrap();
        let config = super::Config::load_from(&path);
        assert_eq!(config.apps.len(), crate::APP_NAMES.len());
        let photo = config.app_config("PhotoCraft");
        assert!(!photo.enabled);
        assert_eq!(photo.hide_project_name, Some(true));
        assert_eq!(photo.action.as_deref(), Some("Painting"));
        for name in [
            "FigCraft",
            "WordCraft",
            "SoundCraft",
            "DeckCraft",
            "CADCraft",
            "GridCraft",
        ] {
            let app = &config.apps[name];
            assert!(app.enabled);
            assert!(!app.live);
            assert_eq!(config.control_port(name), 0);
            assert!(app
                .large_image_key
                .as_ref()
                .unwrap()
                .starts_with("https://raw.githubusercontent.com/storytold/"));
        }
    }

    #[test]
    fn pinned_app_round_trips_and_can_be_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = super::Config {
            preferred_app: Some("WordCraft".into()),
            ..super::Config::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(
            super::Config::load_from(&path).preferred_app.as_deref(),
            Some("WordCraft")
        );
        config.preferred_app = None;
        config.save_to(&path).unwrap();
        assert_eq!(super::Config::load_from(&path).preferred_app, None);
    }

    #[test]
    fn broken_startup_config_disables_sharing_without_replacing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "broken = [").unwrap();
        let config = super::Config::load_from(&path);
        assert!(!config.enabled);
        assert!(config.hide_project_name);
        assert!(config.load_error.is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken = [");
    }
    #[test]
    fn backup_preserves_previous_config_and_invalid_config_cannot_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = super::Config {
            hide_project_name: true,
            ..super::Config::default()
        };
        config.rules.push(super::ActivityRule {
            app: "PhotoCraft".into(),
            element: "Crop".into(),
            activity: "Cropping".into(),
        });
        config.apps.get_mut("PhotoCraft").unwrap().idle_after_secs = Some(60);
        config.save_to(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        config.idle_after_secs = 42;
        config.save_to(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(path.with_extension("toml.bak")).unwrap(),
            first
        );
        let loaded: super::Config =
            toml_edit::de::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.rules, config.rules);
        assert_eq!(loaded.apps["PhotoCraft"].idle_after_secs, Some(60));
        std::fs::write(&path, "broken = [").unwrap();
        assert!(config.save_to(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken = [");
    }
    #[test]
    fn idle_setting_round_trips_and_old_configs_keep_five_minutes() {
        let original: super::Config = toml_edit::de::from_str("enabled = true").unwrap();
        assert_eq!(original.idle_after_secs, 300);
        let path =
            std::env::temp_dir().join(format!("craftpulse-idle-{}.toml", std::process::id()));
        let mut config = original;
        config.idle_after_secs = 1;
        std::fs::write(&path, "# Older minimal config\nenabled = true\n").unwrap();
        config.save_to(&path).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("# Older minimal config"));
        let loaded: super::Config =
            toml_edit::de::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.idle_after_secs, 1);
        std::fs::remove_file(path).unwrap();
    }
    use super::*;

    #[test]
    fn default_config_has_all_apps() {
        let config = Config::default();
        assert_eq!(config.apps.len(), crate::APP_NAMES.len());
        assert_eq!(config.application_id, "1558510318513033329");
    }

    #[test]
    fn app_lookup_is_case_insensitive() {
        let config = Config::default();
        assert_eq!(
            config.app_config("photocraft"),
            config.app_config("PhotoCraft")
        );
    }

    #[test]
    fn control_ports_are_distinct_and_overridable() {
        let mut config = Config::default();
        let ports: std::collections::BTreeSet<u16> = config
            .apps
            .keys()
            .map(|name| config.control_port(name))
            .collect();
        // PdfCraft has no fixed port (0), the other six are distinct.
        assert_eq!(ports.len(), 7);
        config.apps.get_mut("PhotoCraft").unwrap().control_port = Some(9000);
        assert_eq!(config.control_port("photocraft"), 9000);
        assert!(config.app_config("PhotoCraft").live);
        assert!(config
            .control_token_file("PhotoCraft")
            .ends_with("photocraft.token"));
    }

    #[test]
    fn rules_parse_from_toml() {
        let parsed: Config = de::from_str(
            "[[rules]]\napp = \"LightCraft\"\nelement = \"Info\"\nactivity = \"Looking\"\n",
        )
        .unwrap();
        assert_eq!(parsed.rules.len(), 1);
    }

    #[test]
    fn timer_and_theme_have_defaults_and_old_files_still_load() {
        let config = Config::default();
        assert_eq!(config.timer_mode, "session");
        assert_eq!(config.theme, "system");
        let parsed: Config = de::from_str("enabled = true\n").unwrap();
        assert_eq!(parsed.timer_mode, "session");
    }

    #[test]
    fn buttons_round_trip_and_validate() {
        let path =
            std::env::temp_dir().join(format!("craftpulse-buttons-{}.toml", std::process::id()));
        let config = Config {
            buttons: vec![CardButton {
                label: "Portfolio".into(),
                url: "https://example.com/me".into(),
            }],
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        let loaded: Config = de::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.buttons, config.buttons);
        let _ = fs::remove_file(path);
        let raw = |label: &str, url: &str| RawCardButton {
            label: label.into(),
            url: url.into(),
        };
        assert!(CardButton::try_from(raw(" Site ", " https://example.com ")).is_ok());
        assert!(CardButton::try_from(raw("x", "http://example.com")).is_err());
        assert!(CardButton::try_from(raw("", "https://example.com")).is_err());
        // One bad button in the file is dropped, the rest of the file still loads.
        let parsed: Config = de::from_str(
            "buttons = [{label = \"Ok\", url = \"https://a.b\"}, {label = \"Bad\", url = \"ftp://a\"}]",
        )
        .unwrap();
        assert_eq!(parsed.buttons.len(), 1);
    }

    #[test]
    fn default_document_parses() {
        let parsed: Config = de::from_str(&default_document().to_string()).unwrap();
        assert_eq!(parsed.apps.len(), crate::APP_NAMES.len());
    }

    #[test]
    fn save_preserves_existing_comments() {
        let path =
            std::env::temp_dir().join(format!("craftpulse-config-{}.toml", std::process::id()));
        fs::write(
            &path,
            "# Keep this note\n\
enabled = true\n\
hide_project_name = false\n\
hide_patterns = []\n\
action = \"Creating\"\n\
application_id = \"1558510318513033329\"\n\
launch_at_login = false\n\
\n\
# Preserve app-specific guidance\n\
[apps.PhotoCraft]\n\
enabled = true # preserves trailing comment\n\
large_image_key = \"photocraft\"\n",
        )
        .unwrap();
        let config = Config {
            action: "Exporting".to_string(),
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# Keep this note"));
        assert!(saved.contains("# Preserve app-specific guidance"));
        assert!(saved.contains("enabled = true # preserves trailing comment"));
        assert!(saved.contains("action = \"Exporting\""));
        let _ = fs::remove_file(path);
    }
}

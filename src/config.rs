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
    /// Add extras like "Page 3 of 12" or image size to the document line.
    pub show_details: bool,
    /// Up to two link buttons on the card.
    pub buttons: Vec<CardButton>,
    /// Stop sharing between `quiet_start` and `quiet_end` (24h "HH:MM", may cross midnight).
    pub quiet_hours: bool,
    pub quiet_start: String,
    pub quiet_end: String,
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
}

/// Shows `activity` while the button/tab whose name starts with `element`
/// is selected in `app`. Add these under `[[rules]]` in config.toml.
/// A link button on the Discord card (Discord allows two, https only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardButton {
    pub label: String,
    pub url: String,
}

impl CardButton {
    /// Discord's limits: a short label and an https link.
    pub fn is_valid(&self) -> bool {
        let label = self.label.trim();
        !label.is_empty()
            && label.chars().count() <= 32
            && self.url.len() <= 512
            && self.url.starts_with("https://")
            && !self.url.contains(char::is_whitespace)
    }
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
        let apps = [
            ("PhotoCraft", "photocraft"),
            ("VectorCraft", "vectorcraft"),
            ("FilmCraft", "filmcraft"),
            ("LightCraft", "lightcraft"),
            ("PdfCraft", "pdfcraft"),
            ("EffectCraft", "effectcraft"),
            ("DesignCraft", "designcraft"),
            ("FigCraft", "figcraft"),
            ("WordCraft", "wordcraft"),
            ("SoundCraft", "soundcraft"),
            ("DeckCraft", "deckcraft"),
            ("CADCraft", "cadcraft"),
            ("GridCraft", "gridcraft"),
        ]
        .into_iter()
        .map(|(name, key)| {
            (
                name.to_string(),
                AppConfig {
                    large_image_key: Some(key.to_string()),
                    ..AppConfig::default()
                },
            )
        })
        .collect();
        Self {
            enabled: true,
            hide_project_name: false,
            hide_patterns: Vec::new(),
            action: "Creating".to_string(),
            application_id: "1558510318513033329".to_string(),
            launch_at_login: false,
            live_prompted: false,
            timer_mode: "session".to_string(),
            check_updates: true,
            show_details: true,
            buttons: Vec::new(),
            quiet_hours: false,
            quiet_start: "22:00".to_string(),
            quiet_end: "07:00".to_string(),
            pause_while_streaming: false,
            milestones: false,
            details_template: DEFAULT_DETAILS_TEMPLATE.to_string(),
            state_template: DEFAULT_STATE_TEMPLATE.to_string(),
            theme: "system".to_string(),
            apps,
            rules: Vec::new(),
            last_modified: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !path.exists() {
            let config = Self::default();
            let _ = config.save();
            return config;
        }
        let mut config = fs::read_to_string(&path)
            .ok()
            .and_then(|text| de::from_str::<Self>(&text).ok())
            .unwrap_or_default();
        config.ensure_builtin_apps();
        config.last_modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        config
    }

    /// Older config files predate some apps. Fill those in so they can be
    /// toggled, without changing apps the user already has.
    fn ensure_builtin_apps(&mut self) {
        for (name, app) in Self::default().apps {
            self.apps.entry(name).or_insert(app);
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

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        fs::create_dir_all(path.parent().ok_or("Missing config directory")?)
            .map_err(|e| e.to_string())?;
        let mut document = if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|text| text.parse::<DocumentMut>().ok())
                .unwrap_or_else(default_document)
        } else {
            default_document()
        };
        set_value(&mut document["enabled"], value(self.enabled));
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
        set_value(&mut document["show_details"], value(self.show_details));
        set_value(
            &mut document["details_template"],
            value(self.details_template.clone()),
        );
        set_value(
            &mut document["state_template"],
            value(self.state_template.clone()),
        );
        set_value(&mut document["quiet_hours"], value(self.quiet_hours));
        set_value(
            &mut document["quiet_start"],
            value(self.quiet_start.clone()),
        );
        set_value(&mut document["quiet_end"], value(self.quiet_end.clone()));
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
        if document["apps"].as_table().is_none() {
            document["apps"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        for (name, app) in &self.apps {
            if document["apps"].as_table().unwrap().get(name).is_none() {
                document["apps"][name] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let table = &mut document["apps"][name];
            set_value(&mut table["enabled"], value(app.enabled));
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
        fs::write(path, document.to_string()).map_err(|e| e.to_string())?;
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
large_image_key = "figcraft"

[apps.WordCraft]
enabled = true
large_image_key = "wordcraft"

[apps.SoundCraft]
enabled = true
large_image_key = "soundcraft"

[apps.DeckCraft]
enabled = true
large_image_key = "deckcraft"

[apps.CADCraft]
enabled = true
large_image_key = "cadcraft"

[apps.GridCraft]
enabled = true
large_image_key = "gridcraft"

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
    use super::*;

    #[test]
    fn default_config_has_all_apps() {
        let config = Config::default();
        assert_eq!(config.apps.len(), 13);
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
        // Apps without their own port share 0. The six fixed ports stay distinct.
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
        assert!(config.buttons[0].is_valid());
        assert!(!CardButton {
            label: "x".into(),
            url: "http://example.com".into()
        }
        .is_valid());
        assert!(!CardButton {
            label: "".into(),
            url: "https://example.com".into()
        }
        .is_valid());
    }

    #[test]
    fn default_document_parses() {
        let parsed: Config = de::from_str(&default_document().to_string()).unwrap();
        assert_eq!(parsed.apps.len(), 13);
    }

    #[test]
    fn older_config_gains_apps_added_later() {
        let mut parsed: Config = de::from_str("[apps.PhotoCraft]\nenabled = true\n").unwrap();
        assert!(!parsed.apps.contains_key("WordCraft"));
        parsed.ensure_builtin_apps();
        assert!(parsed.apps.contains_key("WordCraft"));
        assert!(parsed.apps.contains_key("GridCraft"));
        assert!(parsed.apps.contains_key("DeckCraft"));
        assert!(parsed.apps.contains_key("FigCraft"));
        assert!(parsed.apps.contains_key("SoundCraft"));
        assert!(parsed.apps.contains_key("CADCraft"));
        assert!(parsed.apps.contains_key("LightCraft"));
        assert!(parsed.app_config("PhotoCraft").enabled);
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

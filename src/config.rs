use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use toml_edit::{de, value, DocumentMut};

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
    pub apps: BTreeMap<String, AppConfig>,
    pub rules: Vec<ActivityRule>,
    #[serde(skip)]
    last_modified: Option<SystemTime>,
}

/// Shows `activity` while the button/tab whose name starts with `element`
/// is selected in `app`. Add these under `[[rules]]` in config.toml.
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
        }
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
        config.last_modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        config
    }

    pub fn reload_if_changed(&mut self) {
        let path = Self::path();
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        if modified.is_none() || modified == self.last_modified {
            return;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(mut loaded) = de::from_str::<Self>(&text) {
                loaded.last_modified = modified;
                *self = loaded;
            }
        }
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
        assert_eq!(config.apps.len(), 7);
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
    fn rules_parse_from_toml() {
        let parsed: Config = de::from_str(
            "[[rules]]\napp = \"LightCraft\"\nelement = \"Info\"\nactivity = \"Looking\"\n",
        )
        .unwrap();
        assert_eq!(parsed.rules.len(), 1);
    }

    #[test]
    fn default_document_parses() {
        let parsed: Config = de::from_str(&default_document().to_string()).unwrap();
        assert_eq!(parsed.apps.len(), 7);
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

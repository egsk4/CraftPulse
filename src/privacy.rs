use crate::{
    config::{AppConfig, Config},
    detection::DetectedApp,
    hints::truncate_chars,
    presence::PresenceCard,
};
use regex::RegexBuilder;

pub fn make_card(
    app: &DetectedApp,
    document: Option<&str>,
    action: &str,
    config: &Config,
    app_config: &AppConfig,
) -> PresenceCard {
    let hidden = app_config
        .hide_project_name
        .unwrap_or(config.hide_project_name)
        || config.hide_patterns.iter().any(|pattern| {
            RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map(|regex| document.map(|name| regex.is_match(name)).unwrap_or(false))
                .unwrap_or(false)
        });
    let document = if hidden {
        "Private project".to_string()
    } else {
        document
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Working")
            .to_string()
    };
    PresenceCard {
        app_name: app.name.clone(),
        document: truncate_chars(&document, 128),
        state: format!("in {} — {}", app.name, truncate_chars(action, 100)),
        large_image_key: app_config
            .large_image_key
            .clone()
            .unwrap_or_else(|| app.name.to_lowercase()),
        small_image_key: "artcraft".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hide_project_name_overrides_document() {
        let mut config = Config::default();
        config.hide_project_name = true;
        let card = make_card(
            &DetectedApp {
                name: "PhotoCraft".to_string(),
                process_id: 5,
                document: Some("Secret".to_string()),
            },
            Some("Secret"),
            "Editing",
            &config,
            &config.app_config("PhotoCraft"),
        );
        assert_eq!(card.document, "Private project");
    }

    #[test]
    fn regex_hides_matching_document() {
        let mut config = Config::default();
        config.hide_patterns = vec!["secret".to_string()];
        let app = config.app_config("PhotoCraft");
        let card = make_card(
            &DetectedApp {
                name: "PhotoCraft".to_string(),
                process_id: 1,
                document: None,
            },
            Some("Very Secret Work"),
            "Editing",
            &config,
            &app,
        );
        assert_eq!(card.document, "Private project");
    }
}

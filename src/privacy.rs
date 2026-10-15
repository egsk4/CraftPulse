use crate::{
    config::{AppConfig, Config},
    detection::DetectedApp,
    hints::truncate_chars,
    presence::PresenceCard,
};
use regex::RegexBuilder;

/// Fills `{app}`, `{activity}`, `{document}` and `{detail}` in a template.
/// Separators left dangling by an empty value ("poster.psd · ") are trimmed.
pub fn render_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text.trim_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '·' | '-' | '—' | '–' | '|' | ':')
    })
    .to_string()
}

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
        started_at: None,
        buttons: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_fill_placeholders_and_trim_empty_ones() {
        let values = [
            ("app", "LightCraft"),
            ("activity", "Masking"),
            ("document", "a.cr3"),
            ("detail", ""),
        ];
        assert_eq!(
            render_template("in {app} — {activity}", &values),
            "in LightCraft — Masking"
        );
        assert_eq!(render_template("{document} · {detail}", &values), "a.cr3");
        assert_eq!(
            render_template("{activity} on {document}", &values),
            "Masking on a.cr3"
        );
    }

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

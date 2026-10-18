use crate::config::ActivityRule;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchMode {
    Window,
    Live,
}

fn control_key(text: &str, mode: MatchMode) -> String {
    match mode {
        MatchMode::Window => text.to_lowercase(),
        MatchMode::Live => text
            .split('(')
            .next()
            .unwrap_or(text)
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect(),
    }
}

pub fn matching_rule(
    app: &str,
    controls: &[&str],
    rules: &[ActivityRule],
    mode: MatchMode,
) -> Option<usize> {
    if controls.is_empty() || rules.is_empty() {
        return None;
    }
    let controls: Vec<String> = controls
        .iter()
        .map(|text| control_key(text, mode))
        .collect();
    rules.iter().position(|rule| {
        if !rule.app.eq_ignore_ascii_case(app) {
            return false;
        }
        let prefix = control_key(&rule.element, mode);
        !prefix.is_empty() && controls.iter().any(|control| control.starts_with(&prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app: &str, element: &str) -> ActivityRule {
        ActivityRule {
            app: app.into(),
            element: element.into(),
            activity: "Matched".into(),
        }
    }

    #[test]
    fn first_rule_wins_across_multiple_controls() {
        let rules = [rule("PhotoCraft", "Crop"), rule("PhotoCraft", "Brush")];
        assert_eq!(
            matching_rule(
                "photocraft",
                &["Brush", "Crop & Rotate"],
                &rules,
                MatchMode::Window
            ),
            Some(0)
        );
        assert_eq!(
            matching_rule("GridCraft", &["Crop"], &rules, MatchMode::Window),
            None
        );
    }

    #[test]
    fn live_rules_normalize_shortcuts_and_punctuation_but_window_rules_do_not() {
        let rules = [rule("LightCraft", "Crop & Rotate (R)")];
        assert_eq!(
            matching_rule("LightCraft", &["cropRotate"], &rules, MatchMode::Live),
            Some(0)
        );
        assert_eq!(
            matching_rule("LightCraft", &["cropRotate"], &rules, MatchMode::Window),
            None
        );
        assert_eq!(
            matching_rule(
                "LightCraft",
                &["CROP & ROTATE (R) panel"],
                &rules,
                MatchMode::Window
            ),
            Some(0)
        );
    }

    #[test]
    fn empty_normalized_prefixes_do_not_match_everything() {
        assert_eq!(
            matching_rule(
                "PhotoCraft",
                &["Brush"],
                &[rule("PhotoCraft", "()")],
                MatchMode::Live
            ),
            None
        );
        assert_eq!(
            matching_rule(
                "PhotoCraft",
                &[],
                &[rule("PhotoCraft", "Brush")],
                MatchMode::Live
            ),
            None
        );
    }
}

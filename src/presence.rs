use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde::{Deserialize, Serialize};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

const APPLICATION_ID: &str = "1558510318513033329";

/// Asset keys that exist on the CraftPulse Discord application. Any other key
/// is dropped by Discord, which then shows the plain ArtCraft icon and the
/// application name instead of the Craft app.
const UPLOADED_ASSETS: &[&str] = &[
    "artcraft",
    "photocraft",
    "vectorcraft",
    "filmcraft",
    "lightcraft",
    "pdfcraft",
    "effectcraft",
    "designcraft",
];

/// Image Discord should show for this app. Uploaded keys are used as-is.
/// The other Craft apps use the icon published in each app's repository,
/// which Discord accepts as an external image.
pub fn discord_large_image(app_name: &str, key: &str) -> String {
    let key = key.trim();
    if key.starts_with("https://") || key.starts_with("http://") || key.starts_with("mp:") {
        return key.to_string();
    }
    if UPLOADED_ASSETS
        .iter()
        .any(|asset| asset.eq_ignore_ascii_case(key))
    {
        return key.to_ascii_lowercase();
    }
    let slug: String = app_name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if key.is_empty() || key.eq_ignore_ascii_case(&slug) {
        format!("https://raw.githubusercontent.com/storytold/{slug}/main/assets/app-icon/{slug}-1024.png")
    } else {
        key.to_string()
    }
}

fn activity_message(card: &PresenceCard) -> serde_json::Value {
    let mut activity = serde_json::json!({
        "name": card.app_name,
        "type": 0,
        "details": card.document,
        "state": card.state,
        "assets": {
            "large_image": discord_large_image(&card.app_name, &card.large_image_key),
            "large_text": card.app_name,
            "small_image": card.small_image_key,
            "small_text": "ArtCraft",
        }
    });
    if let Some(start) = card.started_at {
        activity["timestamps"] = serde_json::json!({ "start": start });
    }
    if !card.buttons.is_empty() {
        activity["buttons"] = serde_json::Value::Array(
            card.buttons
                .iter()
                .take(2)
                .map(|(label, url)| serde_json::json!({ "label": label, "url": url }))
                .collect(),
        );
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity
        },
        "nonce": format!("craftpulse-{nonce}")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresenceCard {
    pub app_name: String,
    pub document: String,
    pub state: String,
    pub large_image_key: String,
    pub small_image_key: String,
    /// Unix seconds the elapsed timer counts from (`None`: no timer).
    #[serde(default)]
    pub started_at: Option<i64>,
    /// (label, https url) link buttons, at most two.
    #[serde(default)]
    pub buttons: Vec<(String, String)>,
}

enum Command {
    Set(PresenceCard, Sender<bool>),
    Clear(Sender<bool>),
    Quit,
}

pub struct RpcWorker {
    sender: Sender<Command>,
}

impl RpcWorker {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("craftpulse-discord-rpc".to_string())
            .spawn(move || run_rpc(receiver))
            .expect("could not start Discord RPC worker");
        Self { sender }
    }

    pub fn set_activity(&self, card: PresenceCard) -> (bool, bool) {
        self.request(|tx| Command::Set(card, tx))
    }

    pub fn clear(&self) {
        let _ = self.request(Command::Clear);
    }

    pub fn quit(&self) {
        let _ = self.sender.send(Command::Quit);
    }

    fn request(&self, make: impl FnOnce(Sender<bool>) -> Command) -> (bool, bool) {
        let (tx, rx) = mpsc::channel();
        if self.sender.send(make(tx)).is_err() {
            return (false, false);
        }
        match rx.recv_timeout(Duration::from_secs(4)) {
            Ok(result) => (result, false),
            Err(mpsc::RecvTimeoutError::Timeout) => (false, true),
            Err(mpsc::RecvTimeoutError::Disconnected) => (false, false),
        }
    }
}

/// Opens a fresh connection to the Discord desktop app, if it is running.
fn connect() -> Option<DiscordIpcClient> {
    let mut client = DiscordIpcClient::new(APPLICATION_ID).ok()?;
    client.connect().ok()?;
    Some(client)
}

fn run_rpc(receiver: Receiver<Command>) {
    let mut client: Option<DiscordIpcClient> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Set(card, response) => {
                let payload = activity_message(&card);
                // A refreshed or restarted Discord leaves us holding a dead
                // pipe. Try the existing connection first, then reconnect once.
                let mut ok = client
                    .as_mut()
                    .is_some_and(|rpc| rpc.send(payload.clone(), 1).is_ok());
                if !ok {
                    if let Some(mut dead) = client.take() {
                        let _ = dead.close();
                    }
                    if let Some(fresh) = connect() {
                        client = Some(fresh);
                        ok = client
                            .as_mut()
                            .is_some_and(|rpc| rpc.send(payload, 1).is_ok());
                        if !ok {
                            client = None;
                        }
                    }
                }
                let _ = response.send(ok);
            }
            Command::Clear(response) => {
                let ok = client
                    .as_mut()
                    .is_some_and(|rpc| rpc.clear_activity().is_ok());
                let _ = response.send(ok);
            }
            Command::Quit => {
                if let Some(mut client) = client {
                    let _ = client.close();
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_contains_discord_display_fields() {
        let card = PresenceCard {
            app_name: "PhotoCraft".into(),
            document: "Poster".into(),
            state: "in PhotoCraft — Editing".into(),
            large_image_key: "photocraft".into(),
            small_image_key: "artcraft".into(),
            started_at: None,
            buttons: Vec::new(),
        };
        assert!(card.state.contains("Editing"));
        assert_eq!(card.app_name, "PhotoCraft");
    }

    #[test]
    fn uploaded_icons_stay_asset_keys_and_the_rest_use_the_published_png() {
        assert_eq!(
            discord_large_image("PhotoCraft", "photocraft"),
            "photocraft"
        );
        assert_eq!(
            discord_large_image("LightCraft", "lightcraft"),
            "lightcraft"
        );
        assert_eq!(discord_large_image("PdfCraft", "pdfcraft"), "pdfcraft");
        assert_eq!(
            discord_large_image("VectorCraft", "vectorcraft"),
            "vectorcraft"
        );
        for (app, slug) in [
            ("WordCraft", "wordcraft"),
            ("GridCraft", "gridcraft"),
            ("DeckCraft", "deckcraft"),
            ("FigCraft", "figcraft"),
            ("SoundCraft", "soundcraft"),
            ("CADCraft", "cadcraft"),
        ] {
            assert_eq!(
                discord_large_image(app, slug),
                format!("https://raw.githubusercontent.com/storytold/{slug}/main/assets/app-icon/{slug}-1024.png")
            );
        }
        assert_eq!(
            discord_large_image("GridCraft", "https://example.com/grid.png"),
            "https://example.com/grid.png"
        );
        assert_eq!(discord_large_image("GridCraft", "custom-key"), "custom-key");
    }

    #[test]
    fn activity_names_the_craft_app_and_points_at_its_icon() {
        let card = PresenceCard {
            app_name: "GridCraft".into(),
            document: "Budget".into(),
            state: "in GridCraft — Editing a spreadsheet".into(),
            large_image_key: "gridcraft".into(),
            small_image_key: "artcraft".into(),
            started_at: Some(20),
            buttons: Vec::new(),
        };
        let message = activity_message(&card);
        let activity = &message["args"]["activity"];
        assert_eq!(activity["name"], "GridCraft");
        assert_eq!(activity["type"], 0);
        assert_eq!(activity["details"], "Budget");
        assert!(activity["assets"]["large_image"]
            .as_str()
            .unwrap()
            .contains("gridcraft-1024.png"));
        assert_eq!(activity["assets"]["small_image"], "artcraft");
    }

    /// Talks to the Discord desktop app and checks that apps without an
    /// uploaded asset still keep an icon and their own name.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires the Discord desktop app"]
    fn live_discord_keeps_icons_for_apps_without_uploaded_art() {
        use discord_rich_presence::DiscordIpcClient;
        let mut client = DiscordIpcClient::new(APPLICATION_ID).expect("discord client");
        client
            .connect()
            .expect("Discord desktop app is not running");
        for (app, slug) in [
            ("WordCraft", "wordcraft"),
            ("GridCraft", "gridcraft"),
            ("DeckCraft", "deckcraft"),
            ("FigCraft", "figcraft"),
            ("SoundCraft", "soundcraft"),
            ("CADCraft", "cadcraft"),
            ("PhotoCraft", "photocraft"),
        ] {
            let card = PresenceCard {
                app_name: app.into(),
                document: "Working".into(),
                state: format!("in {app} — testing"),
                large_image_key: slug.into(),
                small_image_key: "artcraft".into(),
                started_at: None,
                buttons: Vec::new(),
            };
            client
                .send(activity_message(&card), 1)
                .expect("send activity");
            let (_opcode, response) = client.recv().expect("discord reply");
            assert!(
                response.get("evt").is_none() || response["evt"].is_null(),
                "{app} rejected: {response}"
            );
            assert_eq!(response["data"]["name"], app, "{response}");
            let image = response["data"]["assets"]["large_image"]
                .as_str()
                .unwrap_or("");
            assert!(
                image.starts_with("mp:external/") || image.chars().all(|ch| ch.is_ascii_digit()),
                "{app} lost its icon: {response}"
            );
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let _ = client.clear_activity();
        let _ = client.recv();
        let _ = client.close();
    }
}

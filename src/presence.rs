use discord_rich_presence::{
    activity::{Activity, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

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
    pub fn new(application_id: String) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("craftpulse-discord-rpc".to_string())
            .spawn(move || run_rpc(receiver, application_id))
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
fn connect(application_id: &str) -> Option<DiscordIpcClient> {
    let mut client = DiscordIpcClient::new(application_id).ok()?;
    client.connect().ok()?;
    Some(client)
}

fn run_rpc(receiver: Receiver<Command>, application_id: String) {
    let mut client: Option<DiscordIpcClient> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Set(card, response) => {
                let activity = || {
                    let activity = Activity::new()
                        .details(&card.document)
                        .state(&card.state)
                        .assets(
                            Assets::new()
                                .large_image(&card.large_image_key)
                                .large_text(&card.app_name)
                                .small_image(&card.small_image_key),
                        );
                    let activity = match card.started_at {
                        Some(start) => activity.timestamps(Timestamps::new().start(start)),
                        None => activity,
                    };
                    if card.buttons.is_empty() {
                        activity
                    } else {
                        activity.buttons(
                            card.buttons
                                .iter()
                                .take(2)
                                .map(|(label, url)| Button::new(label, url))
                                .collect(),
                        )
                    }
                };
                // A refreshed or restarted Discord leaves us holding a dead
                // pipe. Try the existing connection first, then reconnect once.
                let mut ok = client
                    .as_mut()
                    .is_some_and(|rpc| rpc.set_activity(activity()).is_ok());
                if !ok {
                    if let Some(mut dead) = client.take() {
                        let _ = dead.close();
                    }
                    if let Some(fresh) = connect(&application_id) {
                        client = Some(fresh);
                        ok = client
                            .as_mut()
                            .is_some_and(|rpc| rpc.set_activity(activity()).is_ok());
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
}

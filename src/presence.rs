use discord_rich_presence::{
    activity::{Activity, Assets},
    DiscordIpc, DiscordIpcClient,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

const APPLICATION_ID: &str = "1558510318513033329";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresenceCard {
    pub app_name: String,
    pub document: String,
    pub state: String,
    pub large_image_key: String,
    pub small_image_key: String,
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

fn run_rpc(receiver: Receiver<Command>) {
    let mut client: Option<DiscordIpcClient> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Set(card, response) => {
                if client.is_none() {
                    let mut next = match DiscordIpcClient::new(APPLICATION_ID) {
                        Ok(client) => client,
                        Err(_) => {
                            let _ = response.send(false);
                            continue;
                        }
                    };
                    if next.connect().is_err() {
                        let _ = response.send(false);
                        continue;
                    }
                    client = Some(next);
                }
                let activity = Activity::new()
                    .details(&card.document)
                    .state(&card.state)
                    .assets(
                        Assets::new()
                            .large_image(&card.large_image_key)
                            .large_text(&card.app_name)
                            .small_image(&card.small_image_key)
                            .small_text("ArtCraft"),
                    );
                let ok = client
                    .as_mut()
                    .is_some_and(|rpc| rpc.set_activity(activity).is_ok());
                if !ok {
                    client = None;
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
        };
        assert!(card.state.contains("Editing"));
        assert_eq!(card.app_name, "PhotoCraft");
    }
}

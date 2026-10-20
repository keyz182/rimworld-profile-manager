//! The real [`Backend`]: the running Steam client, through Valve's Steamworks library, as the
//! game's own app id. Steam shows the player as playing RimWorld while this exists.

use std::sync::{Arc, Mutex};

use steamworks::{AppId, Client, ItemState, PublishedFileId};

use crate::args::Op;
use crate::drive::{Backend, Item};

const RIMWORLD: u32 = 294_100;

type Answers = Arc<Mutex<Vec<(u64, Result<(), String>)>>>;

pub struct Real {
    client: Client,
    answers: Answers,
}

impl Real {
    /// Fails in plain words when Steam is not running or does not know the game.
    pub fn connect() -> Result<Self, String> {
        let client = Client::init_app(AppId(RIMWORLD)).map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            answers: Answers::default(),
        })
    }
}

impl Backend for Real {
    fn item(&self, id: u64) -> Item {
        let ugc = self.client.ugc();
        let id = PublishedFileId(id);
        let state = ugc.item_state(id);
        let working =
            ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING | ItemState::NEEDS_UPDATE;
        Item {
            subscribed: state.contains(ItemState::SUBSCRIBED),
            installed: state.contains(ItemState::INSTALLED),
            busy: state.intersects(working),
            progress: ugc.item_download_info(id).filter(|(_, total)| *total > 0),
        }
    }

    fn start(&mut self, op: Op, id: u64) -> Result<(), String> {
        let ugc = self.client.ugc();
        let file = PublishedFileId(id);
        let answers = Arc::clone(&self.answers);
        let answer = move |result: Result<(), steamworks::SteamError>| {
            if let Ok(mut list) = answers.lock() {
                list.push((id, result.map_err(|e| e.to_string())));
            }
        };
        match op {
            Op::Download if ugc.download_item(file, true) => Ok(()),
            Op::Download => Err("Steam would not start the download".into()),
            Op::Subscribe => {
                ugc.subscribe_item(file, answer);
                Ok(())
            }
            Op::Unsubscribe => {
                ugc.unsubscribe_item(file, answer);
                Ok(())
            }
        }
    }

    fn poll(&mut self) -> Vec<(u64, Result<(), String>)> {
        self.client.run_callbacks();
        self.answers
            .lock()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default()
    }
}

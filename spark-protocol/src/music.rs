use serde::{Deserialize, Serialize};
use std::borrow::Cow;

type PlayerIdx = usize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MusicCmd<'s> {
    pub index: Option<PlayerIdx>,
    pub command: MusicCmdKind<'s>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum MusicCmdKind<'s> {
    Frwd,
    Back,
    CyclePause,
    ChangeVolume { amount: i32 },
    Current,
    Queue { query: Cow<'s, str>, search: bool },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum Response {
    Title {
        title: String,
    },
    PlayState {
        paused: bool,
    },
    Volume {
        volume: u32,
    },
    Current {
        title: String,
        chapter: Option<Chapter>,
        volume: f32,
        progress: f32,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub index: u32,
}

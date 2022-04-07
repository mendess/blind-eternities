use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MusicCmd<'s> {
    pub index: u8,
    pub command: MusicCmdKind<'s>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum MusicCmdKind<'s> {
    Fire(Cow<'s, [u8]>),
    Compute(serde_json::Value),
    Execute(Cow<'s, [u8]>),
    Observe(Cow<'s, [u8]>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlayerRef<'s> {
    pub machine: Cow<'s, str>,
    pub index: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MpvMeta<'s> {
    /// fetch the last queue position
    LastFetch(PlayerRef<'s>),
    /// reset the last queue position
    LastReset(PlayerRef<'s>),
    /// set the last queue position
    LastSet(usize, PlayerRef<'s>),
    /// create a new player
    CreatePlayer(u8),
    /// delete a player
    DeletePlayer(u8),
    /// get current player
    GetCurrentPlayer,
    /// set default player
    SetCurrentPlayer(u8),
    /// list all players
    ListPlayers,
}

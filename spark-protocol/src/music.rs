use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use common::domain::music::PlayerIdx;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MusicCmd<'s> {
    pub index: PlayerIdx,
    pub command: MusicCmdKind<'s>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum MusicCmdKind<'s> {
    Fire(Cow<'s, [u8]>),
    Compute(serde_json::Value),
    Execute(Cow<'s, [u8]>),
    Observe(Cow<'s, [u8]>),
    Meta(LocalMetadata),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalMetadata {
    /// fetch the last queue position
    LastFetch,
    /// reset the last queue position
    LastReset,
    /// set the last queue position
    LastSet(usize),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MpvMeta<'s> {
    /// create a new player
    CreatePlayer(PlayerIdx),
    /// delete a player
    DeletePlayer(PlayerIdx),
    /// get current player
    GetCurrentPlayer,
    /// set default player
    SetCurrentPlayer(PlayerIdx),
    /// list all players
    ListPlayers,
    _Unused([&'s str; 0]),
}

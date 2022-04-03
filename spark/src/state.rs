use std::sync::RwLock;

use common::domain::music::Player;
use once_cell::sync::Lazy;

#[derive(Debug, Default)]
pub struct State {
    pub backend_players: Vec<Player>,
}

pub static STATE: Lazy<RwLock<State>> = Lazy::new(Default::default);

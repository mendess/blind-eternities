use serde::{Deserialize, Serialize};

use super::Hostname;

pub type PlayerIdx = u8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub hostname: Hostname,
    pub player: PlayerIdx,
}

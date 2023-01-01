use serde::{Deserialize, Serialize};

type PlayerIdx = usize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "structopt", derive(structopt::StructOpt))]
pub struct MusicCmd {
    pub index: Option<PlayerIdx>,
    #[cfg_attr(feature = "structopt", structopt(subcommand))]
    pub command: MusicCmdKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "structopt", derive(structopt::StructOpt))]
pub enum MusicCmdKind {
    Frwd,
    Back,
    CyclePause,
    ChangeVolume {
        amount: i32,
    },
    Current,
    Queue {
        query: String,
        #[cfg_attr(feature = "structopt", structopt(short, long))]
        search: bool,
    },
}

impl MusicCmdKind {
    pub fn to_route(&self) -> &str {
        match self {
            MusicCmdKind::Frwd => "frwd",
            MusicCmdKind::Back => "back",
            MusicCmdKind::CyclePause => "cycle-pause",
            MusicCmdKind::ChangeVolume { .. } => "change-volume",
            MusicCmdKind::Current => "current",
            MusicCmdKind::Queue { .. } => "queue",
        }
    }
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
        volume: f64,
    },
    Current {
        title: String,
        chapter: Option<Chapter>,
        volume: f64,
        progress: f64,
    },
    QueueSummary {
        from: usize,
        moved_to: usize,
        current: usize,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub index: u32,
}

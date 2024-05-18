use serde::{Deserialize, Serialize};

type PlayerIdx = usize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct MusicCmd {
    #[cfg_attr(feature = "clap", command(subcommand))]
    pub command: MusicCmdKind,
    #[cfg_attr(feature = "clap", arg(short, long))]
    pub index: Option<PlayerIdx>,
    #[cfg_attr(feature = "clap", arg(short, long))]
    pub username: Option<String>,
}

impl From<MusicCmd> for super::Command {
    fn from(cmd: MusicCmd) -> Self {
        Self::Music(cmd)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
pub enum MusicCmdKind {
    Frwd,
    Back,
    CyclePause,
    ChangeVolume {
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        amount: i32,
    },
    Current,
    Queue {
        query: String,
        #[cfg_attr(feature = "clap", clap(short, long))]
        search: bool,
    },
    Now {
        amount: Option<usize>,
    },
}

impl From<MusicCmdKind> for super::Command {
    fn from(command: MusicCmdKind) -> Self {
        Self::Music(MusicCmd {
            command,
            index: None,
            username: None,
        })
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
        #[serde(flatten)]
        current: Current,
    },
    QueueSummary {
        from: usize,
        moved_to: usize,
        current: usize,
    },
    Now {
        before: Vec<String>,
        current: String,
        after: Vec<String>,
    },
}

pub use mlib::queue::Current;

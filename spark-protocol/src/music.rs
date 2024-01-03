use serde::{Deserialize, Serialize};

type PlayerIdx = usize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct MusicCmd {
    pub index: Option<PlayerIdx>,
    pub username: Option<String>,
    #[cfg_attr(feature = "clap", command(subcommand))]
    pub command: MusicCmdKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub enum MusicCmdKind {
    Frwd,
    Back,
    CyclePause,
    ChangeVolume {
        #[arg(allow_hyphen_values = true)]
        amount: i32,
    },
    Current,
    Queue {
        query: String,
        #[cfg_attr(feature = "clap", clap(short, long))]
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

    pub fn to_query_string<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[(&str, &str)]) -> R,
    {
        match self {
            MusicCmdKind::ChangeVolume { amount } => f(&[("a", &amount.to_string())]),
            MusicCmdKind::Queue { query, search } => f(&[("q", query), ("s", &search.to_string())]),
            MusicCmdKind::Frwd
            | MusicCmdKind::Back
            | MusicCmdKind::Current
            | MusicCmdKind::CyclePause => f(&[]),
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Chapter {
    pub title: String,
    pub index: u32,
}

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
        paused: bool,
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

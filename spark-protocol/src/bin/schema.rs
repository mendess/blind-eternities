use core::fmt;
use std::time::Duration;

use mlib::queue::Current;
use serde::Serialize;
use spark_protocol::{
    Command, ErrorResponse, SuccessfulResponse,
    music::{self, MusicCmdKind},
};

fn display<T>(i: impl IntoIterator<Item = T>)
where
    T: Serialize,
    T: fmt::Debug,
{
    i.into_iter()
        .map(|command| (serde_json::to_string_pretty(&command).unwrap(), command))
        .for_each(|(string, _command)| println!("{string}"));
}

fn main() {
    let current = Current {
        title: "title".into(),
        artist: None,
        chapter: Some((2, "chapter".into())),
        playing: true,
        volume: 54.,
        progress: Some(50.),
        playback_time: Some(Duration::from_secs(2)),
        duration: Duration::from_secs(60),
        categories: vec!["category".into()],
        index: 1,
        next: None,
    };
    display(
        [Command::Reload, Command::Version, Command::Heartbeat]
            .into_iter()
            .chain(
                [
                    MusicCmdKind::Frwd,
                    MusicCmdKind::Back,
                    MusicCmdKind::CyclePause,
                    MusicCmdKind::Current,
                    MusicCmdKind::ChangeVolume { amount: 4 },
                    MusicCmdKind::Queue {
                        query: "http://link".into(),
                        search: false,
                    },
                    MusicCmdKind::Now { amount: Some(10) },
                    MusicCmdKind::Now { amount: None },
                ]
                .into_iter()
                .flat_map(|command| {
                    [
                        Command::Music(spark_protocol::music::MusicCmd {
                            command: command.clone(),
                            index: None,
                            username: Some("username".into()),
                        }),
                        Command::Music(spark_protocol::music::MusicCmd {
                            command: command.clone(),
                            index: Some(1),
                            username: None,
                        }),
                        Command::Music(spark_protocol::music::MusicCmd {
                            command: command.clone(),
                            index: None,
                            username: None,
                        }),
                    ]
                }),
            ),
    );

    display(
        [
            SuccessfulResponse::Unit,
            SuccessfulResponse::Version("1.1.1".into()),
        ]
        .into_iter()
        .chain(
            [
                music::Response::Title {
                    title: "title".into(),
                },
                music::Response::Volume { volume: 54. },
                music::Response::PlayState { paused: true },
                music::Response::Current { current },
                music::Response::QueueSummary {
                    from: 7,
                    moved_to: 4,
                    current: 3,
                },
                music::Response::Now {
                    before: vec!["before".into()],
                    current: "current".into(),
                    after: vec!["after".into()],
                },
            ]
            .map(SuccessfulResponse::MusicResponse),
        )
        .map(Ok::<_, ErrorResponse>),
    );
    display(
        [
            ErrorResponse::IoError("error".into()),
            ErrorResponse::DeserializingCommand("error".into()),
            ErrorResponse::ForwardedError("error".into()),
            ErrorResponse::RelayError("error".into()),
            ErrorResponse::RequestFailed("error".into()),
        ]
        .map(Err::<SuccessfulResponse, _>),
    )
}

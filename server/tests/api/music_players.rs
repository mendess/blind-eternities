use std::time::Duration;

use anyhow::Context;
use blind_eternities::auth::music_session::MusicSession;
use common::domain::Hostname;
use fake::{Fake, Faker};
use spark_protocol::SuccessfulResponse;
use spark_protocol::music::{self, Current, MusicCmdKind};

use crate::helpers::{Simulation, TestApp, fake_hostname};
use crate::timeout;

impl TestApp {
    async fn create_session(&self, hostname: &Hostname) -> MusicSession {
        self.get_authed(&format!("admin/music-session/{hostname}"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .context("request to create session errored")
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn send_session_cmd(
        &self,
        session: &MusicSession,
        command: MusicCmdKind,
    ) -> spark_protocol::Response {
        self.post(&format!("music/{session}"))
            .json(&command)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

/// [`spark_protocol::Command::Frwd`]
/// [`spark_protocol::Command::Back`]
/// ```
///     { title: String }
/// ```
/// [`spark_protocol::Command::CyclePause`]
/// ```
///     { paused: bool }
/// ```
/// [`spark_protocol::Command::ChangeVolume`]
/// ```
///     { volume: f32 }
/// ```
/// [`spark_protocol::Command::Current`]
/// ```
/// {
///     title: String,
///     chapter: {
///         title: String,
///         index: i32,
///     },
///     volume: f32,
///     progress: f32,
/// }
/// ```
/// [`spark_protocol::Command::Queue`]
/// ```
/// // =>
/// { index: usize? } & (
///     | { name_or_link: String }
///     | { search: String }
/// )
/// ```
/// ```
/// // <=
/// Result<(), String>
/// ```

#[tokio::test]
async fn requesting_to_skip_a_song_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let title = "title";
    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: MusicCmdKind::Frwd,
            respond_with: Ok(SuccessfulResponse::MusicResponse(music::Response::Title {
                title: title.into(),
            })),
        })
        .await;

    let response = timeout!(app.send_session_cmd(&session, MusicCmdKind::Frwd));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Title { title }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(title.into()), last);
}

#[tokio::test]
async fn requesting_to_skip_back_a_song_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let title = "title";
    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: MusicCmdKind::Back,
            respond_with: Ok(SuccessfulResponse::MusicResponse(music::Response::Title {
                title: title.into(),
            })),
        })
        .await;

    let response = timeout!(app.send_session_cmd(&session, MusicCmdKind::Back));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Title { title }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(title.into()), last);
}

#[tokio::test]
async fn requesting_to_cycle_pause_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: MusicCmdKind::CyclePause,
            respond_with: Ok(SuccessfulResponse::MusicResponse(
                music::Response::PlayState { paused: true },
            )),
        })
        .await;

    let response = timeout!(app.send_session_cmd(&session, MusicCmdKind::CyclePause));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::PlayState { paused }) => paused,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(true), last);
}

#[tokio::test]
async fn requesting_to_change_volume_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: MusicCmdKind::ChangeVolume { amount: 2 },
            respond_with: Ok(SuccessfulResponse::MusicResponse(music::Response::Volume {
                volume: 2.0,
            })),
        })
        .await;

    let response =
        timeout!(app.send_session_cmd(&session, MusicCmdKind::ChangeVolume { amount: 2 }));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Volume { volume }) => volume,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(2.0), last);
}

#[tokio::test]
async fn requesting_current_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: MusicCmdKind::Current,
            respond_with: Ok(SuccessfulResponse::MusicResponse(
                music::Response::Current {
                    current: Current {
                        title: "title".into(),
                        chapter: Faker.fake(),
                        playing: Faker.fake(),
                        volume: Faker.fake(),
                        progress: Faker.fake(),
                        playback_time: None,
                        duration: Duration::from_secs(Faker.fake()),
                        categories: Faker.fake(),
                        index: Faker.fake(),
                        next: Faker.fake(),
                    },
                },
            )),
        })
        .await;

    let response = timeout!(app.send_session_cmd(&session, MusicCmdKind::Current));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Current {
            current: Current { title, .. },
        }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(last, Ok("title".into()));
}

#[tokio::test]
async fn requesting_to_queue_a_song_is_delivered() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session = app.create_session(&hostname).await;

    let command_to_send = MusicCmdKind::Queue {
        query: "nice song :)".into(),
        search: false,
    };

    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: command_to_send.clone(),
            respond_with: Ok(SuccessfulResponse::Unit),
        })
        .await;

    let response = timeout!(app.send_session_cmd(&session, command_to_send));

    device.await.expect("device task failed");

    assert_eq!(Ok(SuccessfulResponse::Unit), response);
}

#[tokio::test]
async fn creating_two_tokens_to_the_same_hostname_returns_the_same_token() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let session0 = app.create_session(&hostname).await;
    let session1 = app.create_session(&hostname).await;

    assert_eq!(session0, session1);
}

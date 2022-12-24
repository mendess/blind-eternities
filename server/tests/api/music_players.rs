use common::domain::Hostname;
use reqwest::StatusCode;
use serde::Serialize;
use spark_protocol::music::{self, MusicCmd, MusicCmdKind};
use spark_protocol::{Local, Response, SuccessfulResponse};

use crate::helpers::{fake_hostname, TestApp};
use crate::{assert_status, timeout};

impl TestApp<false> {
    async fn request_cmd(&self, hostname: &Hostname, cmd: &str) -> Response {
        let resp = self
            .get_authed(&format!("music/players/{hostname}/{cmd}"))
            .send()
            .await
            .expect("success");
        assert_status!(StatusCode::OK, resp.status());
        resp.json().await.expect("deserialized successfully")
    }

    async fn simulate_device<R>(
        &self,
        hostname: &Hostname,
        expect_receive: MusicCmdKind<'static>,
        respond_with: R,
    ) -> tokio::task::JoinHandle<()>
    where
        R: Into<SuccessfulResponse>,
    {
        let mut device = timeout!(self.connect_device(hostname));

        let respond_with = respond_with.into();
        tokio::spawn(async move {
            let req = timeout!(device.recv()).expect("success recv").expect("eof");
            assert_eq!(
                Local::Music(MusicCmd {
                    index: None,
                    command: expect_receive,
                }),
                req
            );
            timeout!(device.send(Ok(respond_with))).expect("success send");
        })
    }
}

/// GET  /{hostname}/frwd
/// GET  /{hostname}/back
/// ```
///     { title: String }
/// ```
/// GET  /{hostname}/cycle-pause
/// ```
///     { paused: bool }
/// ```
/// GET  /{hostname}/change-volume
/// GET  /{hostname}/current
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
/// POST /{hostname}/queue
/// ```
/// // =>
/// { index: usize? } & (
///     { name_or_link: String } | { search: String }
/// )
/// ```
/// ```
/// // <=
/// Result<(), String>
/// ```

#[actix_rt::test]
async fn requesting_to_skip_a_song_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let title = "title";
    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::Frwd,
            music::Response::Title {
                title: title.into(),
            },
        )
        .await;

    let response = timeout!(app.request_cmd(&hostname, "frwd"));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Title { title }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(title.into()), last);
}

#[actix_rt::test]
async fn requesting_to_skip_back_a_song_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let title = "title";
    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::Back,
            music::Response::Title {
                title: title.into(),
            },
        )
        .await;

    let response = timeout!(app.request_cmd(&hostname, "back"));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Title { title }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(title.into()), last);
}

#[actix_web::test]
async fn requesting_to_cycle_pause_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::CyclePause,
            music::Response::PlayState { paused: true },
        )
        .await;

    let response = timeout!(app.request_cmd(&hostname, "cycle-pause"));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::PlayState { paused }) => paused,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(true), last);
}

#[actix_web::test]
async fn requesting_to_change_volume_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::ChangeVolume { amount: 2 },
            music::Response::Volume { volume: 2 },
        )
        .await;

    let response = timeout!(app.request_cmd(&hostname, "change-volume?a=2"));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Volume { volume }) => volume,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(2), last);
}

#[actix_web::test]
async fn requesting_current_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::Current,
            music::Response::Current {
                title: "title".into(),
                chapter: None,
                volume: 100.,
                progress: 53.,
            },
        )
        .await;

    let response = timeout!(app.request_cmd(&hostname, "current"));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Current { title, .. }) => title,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok("title".into()), last);
}

#[actix_web::test]
async fn requesting_to_queue_a_song_is_delivered() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let device = app
        .simulate_device(
            &hostname,
            MusicCmdKind::Queue {
                query: "nice song :)".into(),
                search: false,
            },
            SuccessfulResponse::Unit,
        )
        .await;

    #[derive(Serialize)]
    struct QueueRequest {
        query: String,
        search: bool,
    }

    let response = timeout!(async {
        app.post_authed(&format!("music/players/{hostname}/queue"))
            .json(&QueueRequest {
                query: "nice song :)".into(),
                search: false,
            })
            .send()
            .await
            .expect("failed to send request")
            .json::<Response>()
            .await
            .expect("failed to deserialize")
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(SuccessfulResponse::Unit), response);
}

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
        expect_receive: MusicCmdKind,
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
                    username: None,
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

    let response = timeout!(app.request_cmd(&hostname, MusicCmdKind::Frwd.to_route()));

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

    let response = timeout!(app.request_cmd(&hostname, MusicCmdKind::Back.to_route()));

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

    let response = timeout!(app.request_cmd(&hostname, MusicCmdKind::CyclePause.to_route()));

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
            music::Response::Volume { volume: 2.0 },
        )
        .await;

    let response = timeout!(app.request_cmd(
        &hostname,
        &format!(
            "{}?a=2",
            MusicCmdKind::ChangeVolume { amount: 0 }.to_route()
        )
    ));

    let last = response.map(|e| match e {
        SuccessfulResponse::MusicResponse(music::Response::Volume { volume }) => volume,
        _ => panic!("unexpected response variant: {e:?}"),
    });

    device.await.expect("device task failed");

    assert_eq!(Ok(2.0), last);
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

    let response = timeout!(app.request_cmd(&hostname, MusicCmdKind::Current.to_route()));

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
        app.post_authed(&format!(
            "music/players/{hostname}/{}",
            MusicCmdKind::Queue {
                query: "".into(),
                search: false
            }
            .to_route()
        ))
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

#[actix_web::test]
async fn username_can_be_overridden() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let username = fake_hostname().into_string();

    let mut device = timeout!(app.connect_device(&hostname));

    let device_task = tokio::spawn({
        let username = username.clone();
        async move {
            let req = timeout!(device.recv()).expect("success recv").expect("eof");
            assert_eq!(
                Local::Music(MusicCmd {
                    index: None,
                    username: Some(username),
                    command: MusicCmdKind::Frwd,
                }),
                req
            );
            timeout!(device.send(Ok(SuccessfulResponse::Unit))).expect("success send");
        }
    });

    let response = timeout!(async {
        let resp = app
            .get_authed(&format!(
                "music/players/{hostname}/{}",
                MusicCmdKind::Frwd.to_route()
            ))
            .query(&[("u", &username)])
            .send()
            .await
            .expect("success");
        assert_status!(StatusCode::OK, resp.status());
        resp.json::<Response>().await.expect("deserialized successfully")
    });

    match response {
        Ok(SuccessfulResponse::Unit) => {}
        r => panic!("unexpected response variant: {r:?}"),
    }

    device_task.await.expect("device task failed");
}

use common::domain::Hostname;
use fake::{faker::internet::en::IP, Fake};
use reqwest::StatusCode;
use serde_json::json;
use spark_protocol::{
    music::{LocalMetadata, MusicCmd, MusicCmdKind},
    ErrorResponse, Local, Response,
};

use crate::{
    assert_status,
    helpers::{fake_hostname, TestApp},
    timeout,
};

impl TestApp {
    async fn get_music_players(&self) -> reqwest::Response {
        self.get_authed("music/player")
            .send()
            .await
            .expect("failed to execute request")
    }

    async fn get_current_player(&self) -> reqwest::Response {
        self.get_authed("music/player/current")
            .send()
            .await
            .expect("failed to execute request")
    }

    async fn reprioritize(&self, host: &str, index: usize) -> reqwest::Response {
        self.patch_authed(&format!("music/player/{host}/{index}"))
            .send()
            .await
            .expect("Failed to execute request")
    }

    async fn populate_statuses<const N: usize>(&self) -> [Hostname; N] {
        let hostnames = (0..N).map(|_| fake_hostname()).collect::<Vec<_>>();
        for hostname in &hostnames {
            sqlx::query!(
                r#"INSERT INTO machine_status (hostname, external_ip, last_heartbeat)
                VALUES ($1, $2, NOW())"#,
                hostname.as_ref(),
                IP().fake::<std::net::IpAddr>().to_string(),
            )
            .execute(&self.db_pool)
            .await
            .expect("failed to set machine status");
        }
        hostnames.try_into().unwrap()
    }
}

#[actix_rt::test]
async fn music_players_return_empty_when_there_are_no_players() {
    let app = TestApp::spawn().await;

    let response = app.get_music_players().await;

    assert_eq!(StatusCode::OK, response.status());

    let json = response.json::<serde_json::Value>().await.expect("json");

    assert!(json.as_array().expect("array").is_empty());
}

#[actix_rt::test]
async fn music_players_in_db_are_returned_in_order() {
    let app = TestApp::spawn().await;

    let [hostname1, hostname2] = app.populate_statuses().await;

    sqlx::query!(
        r#"INSERT INTO music_player (hostname, player) VALUES
            ($1, 0),
            ($2, 0)
        "#,
        hostname1.as_ref(),
        hostname2.as_ref(),
    )
    .execute(&app.db_pool)
    .await
    .expect("failed to set db");

    let response = app.get_music_players().await;
    assert_eq!(StatusCode::OK, response.status());
    let json = response
        .json::<serde_json::Value>()
        .await
        .expect("json response");

    assert_eq!(
        json!([
            {
                "hostname": hostname1,
                "player": 0
            },
            {
                "hostname": hostname2,
                "player": 0
            }
        ]),
        json,
    );
}

#[actix_rt::test]
async fn new_players_become_the_new_default() {
    let app = TestApp::spawn().await;

    let [hostname1, hostname2] = app.populate_statuses().await;

    sqlx::query!(
        r#"INSERT INTO music_player (hostname, player) VALUES
        ($1, $2), ($3, $4), ($1, $5)"#,
        hostname1.as_ref(),
        0,
        hostname2.as_ref(),
        0,
        1
    )
    .execute(&app.db_pool)
    .await
    .expect("to insert players");

    assert_eq!(
        json!({ "hostname": hostname1, "player": 1 }),
        app.get_current_player()
            .await
            .json::<serde_json::Value>()
            .await
            .expect("json")
    );
    let response = app
        .post_authed(&format!("music/player/{hostname2}/1"))
        .send()
        .await
        .expect("failed to send request");
    assert_eq!(
        StatusCode::CREATED,
        response.status(),
        "{:?}",
        response.text().await
    );
    assert_eq!(
        json!({ "hostname": hostname2, "player": 1 }),
        app.get_current_player()
            .await
            .json::<serde_json::Value>()
            .await
            .expect("json")
    );
}

#[actix_rt::test]
async fn music_players_can_be_reprioritized() {
    let app = TestApp::spawn().await;

    let [hostname1, hostname2] = app.populate_statuses().await;
    sqlx::query!(
        r#"INSERT INTO music_player (hostname, player) VALUES
            ($1, 0),
            ($2, 0)
        "#,
        hostname1.as_ref(),
        hostname2.as_ref(),
    )
    .execute(&app.db_pool)
    .await
    .expect("failed to set db");

    assert_eq!(
        json!({"hostname": hostname2, "player": 0}),
        app.get_current_player()
            .await
            .json::<serde_json::Value>()
            .await
            .unwrap()
    );
    let response = app.reprioritize(hostname1.as_ref(), 0).await;
    assert_eq!(
        StatusCode::OK,
        response.status(),
        "{:?}",
        response.text().await
    );
    assert_eq!(
        json!({"hostname": hostname1, "player": 0}),
        app.get_current_player()
            .await
            .json::<serde_json::Value>()
            .await
            .unwrap()
    );
}

#[actix_rt::test]
async fn deleting_a_non_existent_player_returns_404() {
    let app = TestApp::spawn().await;

    let response = app
        .delete_authed(&format!("music/player/{}/0", fake_hostname()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        StatusCode::NOT_FOUND,
        response.status(),
        "{:?}",
        response.text().await
    );
}

#[actix_rt::test]
async fn can_delete_a_player() {
    let app = TestApp::spawn().await;

    let [hostname] = app.populate_statuses().await;
    sqlx::query!(
        "INSERT INTO music_player (hostname, player) VALUES ($1, $2)",
        hostname.as_ref(),
        0
    )
    .execute(&app.db_pool)
    .await
    .expect("to insert a player");

    let response = app
        .delete_authed(&format!("music/player/{hostname}/0"))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        StatusCode::OK,
        response.status(),
        "{:?}",
        response.text().await
    );
}

#[actix_rt::test]
async fn get_last_queue_from_existing_device_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let join = tokio::spawn(async move {
        timeout!(20 => async {
            let resp = app.get_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("success");
            assert_status!(StatusCode::OK, resp.status());
            resp
                .json::<Result<Response, ErrorResponse>>()
                .await
        })
    });

    let req = timeout!(device.recv()).expect("success recv");
    assert_eq!(
        Local::Music(MusicCmd {
            index: 0,
            command: MusicCmdKind::Meta(LocalMetadata::LastFetch),
        }),
        req,
    );
    timeout!(device.send(Ok(Response::ForwardValue(json!(0i64))))).expect("success send");

    let last = join.await.expect("join success").expect("request success");
    let last = last.map(|e| match e {
        Response::ForwardValue(v) => serde_json::from_value(v).expect("deserialization"),
        _ => panic!("unexpected response variant: {e:?}"),
    });

    assert_eq!(Ok(Some(0)), last);
}

#[actix_rt::test]
async fn get_null_last_queue_from_existing_device_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let join = tokio::spawn(async move {
        timeout!(20 => async {
            let resp = app.get_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("success");
            assert_status!(StatusCode::OK, resp.status());
            resp
                .json::<Result<Response, ErrorResponse>>()
                .await
        })
    });

    let req = timeout!(device.recv()).expect("success recv");
    assert_eq!(
        Local::Music(MusicCmd {
            index: 0,
            command: MusicCmdKind::Meta(LocalMetadata::LastFetch),
        }),
        req,
    );
    timeout!(device.send(Ok(Response::ForwardValue(json!(null))))).expect("success send");

    let last = join.await.expect("join success").expect("request success");
    let last = last.map(|e| match e {
        Response::ForwardValue(v) => serde_json::from_value::<Option<usize>>(v).expect("deserialization"),
        _ => panic!("unexpected response variant: {e:?}"),
    });
    assert_eq!(Ok(None), last);
}

#[actix_rt::test]
async fn get_last_queue_from_non_existent_device_404s() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.get_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status())
}

#[actix_rt::test]
async fn get_last_queue_from_non_responsive_device_timesout() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.get_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status())
}

#[actix_rt::test]
async fn reset_last_queue_on_existing_device_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let resp = tokio::spawn(async move {
        timeout!(20 => async {
            app.delete_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("request to succeed")
        })
    });

    let req = timeout!(device.recv()).expect("success receive");
    assert_eq!(
        Local::Music(MusicCmd {
            index: 0,
            command: MusicCmdKind::Meta(LocalMetadata::LastReset)
        }),
        req
    );
    timeout!(device.send(Ok(Response::Unit))).expect("successful send");

    assert_status!(StatusCode::OK, resp.await.expect("join success").status());
}

#[actix_rt::test]
async fn reset_last_queue_of_non_existent_device_404s() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.delete_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status());
}

#[actix_rt::test]
async fn reset_last_queue_of_non_responsive_device_timesout() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.delete_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_on_existing_device_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let resp = tokio::spawn(async move {
        timeout!(20 => async {
            app.post_authed(&format!("music/player/{hostname}/0/last"))
                .json(&json!(0i64))
                .send()
                .await
                .expect("request to succeed")
        })
    });

    let req = timeout!(device.recv()).expect("success receive");
    assert_eq!(
        Local::Music(MusicCmd {
            index: 0,
            command: MusicCmdKind::Meta(LocalMetadata::LastSet(0))
        }),
        req
    );
    timeout!(device.send(Ok(Response::Unit))).expect("successful send");

    assert_status!(StatusCode::OK, resp.await.expect("join success").status());
}

#[actix_rt::test]
async fn set_null_last_queue_on_existing_device_returns_bad_request() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::BAD_REQUEST, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_of_non_existent_device_404s() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .json(&json!(0i64))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_of_non_responsive_device_timesout() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .json(&json!(0i64))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status());
}

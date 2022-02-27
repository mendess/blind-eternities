use fake::{faker::internet::en::IP, Fake};
use reqwest::StatusCode;
use serde_json::json;

use crate::helpers::{fake_hostname, TestApp};

impl TestApp {
    async fn get_music_players(&self) -> reqwest::Response {
        self.get_authed("music/players")
            .send()
            .await
            .expect("failed to execute request")
    }

    async fn get_current_player(&self) -> reqwest::Response {
        self.get_authed("music/players/current")
            .send()
            .await
            .expect("failed to execute request")
    }

    async fn reprioritize(&self, host: &str, index: usize) -> reqwest::Response {
        self.patch_authed("music/players")
            .header("Content-Type", "application/json")
            .body(json!({"hostname": host, "player": index}).to_string())
            .send()
            .await
            .expect("Failed to execute request")
    }

    async fn populate_statuses<const N: usize>(&self) -> [String; N] {
        let faker = fake_hostname();
        let hostnames = (0..N).map(|_| faker.fake::<String>()).collect::<Vec<_>>();
        for hostname in &hostnames {
            sqlx::query!(
                r#"INSERT INTO machine_status (hostname, external_ip, last_heartbeat)
                VALUES ($1, $2, NOW())"#,
                hostname,
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
        hostname1,
        hostname2,
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
        hostname1,
        0,
        hostname2,
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
        .post_authed("music/players")
        .json(&json!({
            "hostname": hostname2,
            "player": 1
        }))
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
async fn music_players_can_reprioritized() {
    let app = TestApp::spawn().await;

    let [hostname1, hostname2] = app.populate_statuses().await;
    sqlx::query!(
        r#"INSERT INTO music_player (hostname, player) VALUES
            ($1, 0),
            ($2, 0)
        "#,
        hostname1,
        hostname2,
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
    let response = app.reprioritize(&hostname1, 0).await;
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
        .delete_authed("music/players")
        .json(&json!({
            "hostname": fake_hostname().fake::<String>(),
            "player": 0
        }))
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
        hostname,
        0
    )
    .execute(&app.db_pool)
    .await
    .expect("to insert a player");

    let response = app
        .delete_authed("music/players")
        .json(&json!({
            "hostname": hostname,
            "player": 0
        }))
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

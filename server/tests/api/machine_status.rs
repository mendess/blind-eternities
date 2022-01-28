use crate::helpers::{TestApp, fake_hostname};
use chrono::{NaiveDateTime, Utc};
use fake::{
    faker::internet::en::{MACAddress, IP},
    Fake,
};
use reqwest::StatusCode;
use serde_json::json;

fn well_formed_json() -> serde_json::Value {
    let fake_hostname = fake_hostname();
    json!({
        "hostname": fake_hostname.fake::<String>(),
        "ip_connections": [{
            "local_ip": IP().fake::<std::net::IpAddr>(),
            "gateway_ip": IP().fake::<std::net::IpAddr>(),
            "gateway_mac": MACAddress().fake::<String>().to_lowercase(),
        }],
        "external_ip": IP().fake::<std::net::IpAddr>(),
        "ssh": null,
    })
}

#[actix_rt::test]
async fn machine_status() {
    let app = TestApp::spawn().await;
    let body = well_formed_json();

    let response = app.post_machine_status(&body).await;

    assert_eq!(StatusCode::OK, response.status());

    let saved = sqlx::query!("SELECT hostname, external_ip FROM machine_status")
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch machine status");

    assert_eq!(saved.hostname, body["hostname"].as_str().unwrap());
    assert_eq!(saved.external_ip, body["external_ip"].as_str().unwrap());
}

#[actix_rt::test]
async fn machine_status_returns_400_when_data_is_missing() {
    let app = TestApp::spawn().await;

    let test_cases = vec![
        (
            json!({"hostname": "tolaria"}),
            "missing local_ip, external_ip, gateway_ip, gateway_mac",
        ),
        (
            json!({
                "hostname": "tolaria",
                "local_ip": "192.168.1.204",
                "gateway_ip": "192.168.1.255",
            }),
            "missing external_ip",
        ),
    ];

    for (invalid_body, error_msg) in test_cases {
        let response = app.post_machine_status(invalid_body).await;
        assert_eq!(
            StatusCode::BAD_REQUEST,
            response.status(),
            "The API did not fail with 400 Bad Request when the payload was {}",
            error_msg
        );
    }
}

#[actix_rt::test]
async fn machine_status_returns_400_when_hostname_is_malformed() {
    let app = TestApp::spawn().await;
    let base_json = well_formed_json();

    fn update(json: &serde_json::Value, hostname: &str) -> serde_json::Value {
        let mut clone = json.clone();
        clone["hostname"] = serde_json::Value::String(hostname.into());
        clone
    }

    let test_cases = vec![
        (update(&base_json, "."), "was just a dot"),
        (
            update(&base_json, "name_with_underscore"),
            "has an underscore",
        ),
        (
            update(&base_json, &"really.long.name.with.valid.labels".repeat(50)),
            "was too long",
        ),
    ];

    for (invalid_body, error_msg) in test_cases {
        let response = app.post_machine_status(invalid_body).await;
        assert_eq!(
            StatusCode::BAD_REQUEST,
            response.status(),
            "The API did not fail with 400 Bad Request when the payload {}",
            error_msg
        );
    }
}

#[actix_rt::test]
async fn machine_status_returns_posted_status() {
    // setup
    let app = TestApp::spawn().await;
    let base_json = well_formed_json();

    assert_eq!(
        app.post_machine_status(&base_json).await.status(),
        StatusCode::OK
    );

    {
        // record without ip connections
        let mut j = well_formed_json();
        j.as_object_mut()
            .expect("object")
            .get_mut("ip_connections")
            .expect("ip connections field")
            .as_array_mut()
            .expect("an array")
            .clear();
        assert_eq!(app.post_machine_status(&j).await.status(), StatusCode::OK);
    }

    // act
    let response = app
        .get("machine/status")
        .bearer_auth(app.token)
        .send()
        .await
        .expect("failed to execute request");

    // test
    assert_eq!(response.status(), StatusCode::OK);
    let mut returned_json = response
        .json::<serde_json::Value>()
        .await
        .expect("didn't parse");
    let jsons = returned_json.as_object_mut().expect("should be an object");
    assert_eq!(jsons.len(), 2);

    let mut json = jsons
        .remove(base_json["hostname"].as_str().unwrap())
        .expect("an object");

    fn json_value_to_ndt(j: &serde_json::Value) -> NaiveDateTime {
        j.as_str()
            .expect("is a string")
            .parse::<NaiveDateTime>()
            .expect("naive date time to be parsed from last heartbeat")
    }

    let oldest_hb = {
        let hb = json_value_to_ndt(
            &json
                .as_object_mut()
                .expect("should be an object")
                .remove("last_heartbeat")
                .expect("a last heartbeat"),
        );
        assert!(hb < Utc::now().naive_utc());
        hb
    };

    assert_eq!(json, base_json);

    for o in jsons.values() {
        let o = o.as_object().expect("an object");
        assert!(json_value_to_ndt(&o["last_heartbeat"]) > oldest_hb);
        assert!(o["ip_connections"].as_array().expect("array").is_empty());
    }
}

use crate::helpers::TestApp;
use serde_json::json;

#[actix_rt::test]
async fn machine_status() {
    let app = TestApp::spawn().await;
    let body = json!({
        "hostname": "tolaria",
        "local_ip": "192.168.1.204",
        "external_ip": "89.154.165.141",
        "gateway_ip": "192.168.1.255",
        "gateway_mac": "08:B0:55:06:B3:A6"
    });

    let response = app.post_machine_status(body).await;

    assert_eq!(200, response.status().as_u16());

    let saved = sqlx::query!("SELECT hostname, local_ip FROM machine_status")
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch machine status");

    assert_eq!(saved.hostname, "tolaria");
    assert_eq!(saved.local_ip, "192.168.1.204");
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
            400,
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request when the payload was {}",
            error_msg
        );
    }
}

#[actix_rt::test]
async fn machine_status_returns_400_when_hostname_is_malformed() {
    let app = TestApp::spawn().await;
    let base_json = json!({
        "hostname": "tolaria",
        "local_ip": "192.168.1.204",
        "external_ip": "89.154.165.141",
        "gateway_ip": "192.168.1.255",
        "gateway_mac": "08:B0:55:06:B3:A6"
    });

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
            400,
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request when the payload {}",
            error_msg
        );
    }
}

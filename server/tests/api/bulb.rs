use fake::{
    faker::internet::en::{MACAddress, IP},
    Fake,
};
use reqwest::StatusCode;
use uuid::Uuid;

use crate::helpers::{fake_hostname, TestApp};

async fn prepopulate_with_hosts_and_bulbs(app: &TestApp) {
    let fake_hostname = fake_hostname();
    let host1 = fake_hostname.fake::<String>();
    let host2 = fake_hostname.fake::<String>();
    sqlx::query!(
        "INSERT INTO machine_status (hostname, last_heartbeat, external_ip)
        VALUES ($1, NOW(), $2), ($3, NOW(), $4);",
        host1,
        IP().fake::<std::net::IpAddr>().to_string(),
        host2,
        IP().fake::<std::net::IpAddr>().to_string(),
    )
    .execute(&app.db_pool)
    .await
    .expect("failed to prepoluate db with hosts");

    sqlx::query!(
        "INSERT INTO bulb (id, ip, mac, owner) VALUES
        ($1, $2, $3, $4),
        ($5, $6, $7, $8),
        ($9, $10, $11, $12);",
        Uuid::new_v4(),
        IP().fake::<std::net::IpAddr>().to_string(),
        MACAddress().fake::<String>(),
        host1,
        Uuid::new_v4(),
        IP().fake::<std::net::IpAddr>().to_string(),
        MACAddress().fake::<String>(),
        host1,
        Uuid::new_v4(),
        IP().fake::<std::net::IpAddr>().to_string(),
        MACAddress().fake::<String>(),
        host2,
    )
    .execute(&app.db_pool)
    .await
    .expect("failed to prepoluate db with bulbs");
}

#[actix_rt::test]
async fn status() {
    let app = TestApp::spawn().await;
    prepopulate_with_hosts_and_bulbs(&app).await;

    let response = app
        .get_authed("bulb")
        .send()
        .await
        .expect("bulb status failed");

    assert_eq!(StatusCode::OK, response.status());

    let json = response
        .json::<serde_json::Value>()
        .await
        .expect("failed to deser to json");

    let list = json.as_array().expect("top level array");
    for v in list {
        let obj = v.as_object().expect("objects in array");
        assert!(obj.contains_key("color"), "obj contains color");
    }
}

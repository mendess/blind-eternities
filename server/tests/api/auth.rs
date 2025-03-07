use blind_eternities::auth;
use reqwest::StatusCode;

use crate::helpers::{TestApp, fake_hostname};

#[tokio::test]
async fn auth_is_required() {
    let app = TestApp::spawn().await;
    let response = app
        .get("admin/health_check")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn non_existent_token_is_regected() {
    let app = TestApp::spawn().await;
    let response = app
        .get("admin/health_check")
        .bearer_auth(uuid::Uuid::new_v4())
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_format_token_is_rejected() {
    let app = TestApp::spawn().await;
    let response = app
        .get("admin/health_check")
        .bearer_auth("I'm a very naughty token hehehehe")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn music_auth_cant_access_admin_routes() {
    let app = TestApp::spawn().await.downgrade_to::<auth::Music>().await;

    let response = app
        .get_authed("admin/health_check")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_auth_can_access_music_routes() {
    let app = TestApp::spawn().await;

    let response = app
        .get_authed(&format!("music/players/{}/current", fake_hostname()))
        .send()
        .await
        .expect("Failed to send request");

    assert_ne!(response.status(), StatusCode::OK);
}

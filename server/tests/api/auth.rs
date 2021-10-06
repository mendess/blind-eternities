use reqwest::StatusCode;

use crate::helpers::TestApp;

#[actix_rt::test]
async fn auth_is_required() {
    let app = TestApp::spawn().await;
    let response = app
        .get("/health_check")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status().as_u16(), StatusCode::UNAUTHORIZED,);
}

#[actix_rt::test]
async fn non_existent_token_is_regected() {
    let app = TestApp::spawn().await;
    let response = app
        .get("/health_check")
        .bearer_auth(uuid::Uuid::new_v4())
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED,);
}

#[actix_rt::test]
async fn invalid_format_token_is_rejected() {
    let app = TestApp::spawn().await;
    let response = app
        .get("/health_check")
        .bearer_auth("I'm a very naughty token hehehehe")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST,);
}

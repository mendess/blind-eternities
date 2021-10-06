use crate::helpers::TestApp;

#[actix_rt::test]
async fn health_check_works() {
    let TestApp {
        address: addr,
        token,
        ..
    } = &TestApp::spawn().await;

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/health_check", addr))
        .bearer_auth(token)
        .send()
        .await
        .expect("Failed to execute request.");

    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

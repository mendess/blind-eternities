use crate::helpers::TestApp;

#[actix_rt::test]
async fn health_check_works() {
    let test_app = TestApp::spawn().await;
    let response = test_app
        .get_authed("admin/health_check")
        .send()
        .await
        .expect("Failed to execute request.");

    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

use common::domain::Hostname;
use common::net::PERSISTENT_CONN_RECV_TIMEOUT;
use reqwest::StatusCode;
use spark_protocol::{Command, SuccessfulResponse};

use crate::helpers::{Simulation, TestApp, fake_hostname};
use crate::{assert_status, timeout};

#[tokio::test]
async fn requesting_version_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();

    let command = Command::Version;
    let expected_response = Ok(SuccessfulResponse::Version(
        env!("CARGO_PKG_VERSION").into(),
    ));
    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: command.clone(),
            respond_with: expected_response.clone(),
        })
        .await;

    let response = timeout!(app.send_cmd(hostname, command));

    device.await.expect("device task failed");

    assert_eq!(response, expected_response);
}

#[tokio::test]
async fn list_connections_works() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: Command::Heartbeat,
            respond_with: Ok(SuccessfulResponse::Unit),
        })
        .await;

    let response = app
        .get_authed("persistent-connections")
        .send()
        .await
        .unwrap();

    assert_status!(StatusCode::OK, response.status());

    let list = response.json::<Vec<Hostname>>().await.unwrap();

    assert_eq!(
        app.send_cmd(hostname.clone(), Command::Heartbeat).await,
        Ok(SuccessfulResponse::Unit)
    );

    device.await.expect("device task failed");

    assert_eq!(vec![hostname], list)
}

#[tokio::test]
async fn list_connections_is_empty_after_device_quits() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname();
    let device = app
        .simulate_device(Simulation {
            hostname: &hostname,
            expect_to_receive: Command::Heartbeat,
            respond_with: Ok(SuccessfulResponse::Unit),
        })
        .await;

    assert_eq!(
        app.send_cmd(hostname.clone(), Command::Heartbeat).await,
        Ok(SuccessfulResponse::Unit)
    );

    device.await.expect("device task failed");

    tokio::time::sleep(PERSISTENT_CONN_RECV_TIMEOUT / 5).await;

    let response = app
        .get_authed("persistent-connections")
        .send()
        .await
        .unwrap();

    assert_status!(response.status(), StatusCode::OK);

    let list = response.json::<Vec<Hostname>>().await.unwrap();

    assert!(list.is_empty(), "list: {list:?}");
}

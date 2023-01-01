use crate::{
    helpers::{fake_hostname, TestApp},
    timeout,
};
use common::domain::Hostname;
use reqwest::StatusCode;
use spark_protocol::{ErrorResponse, Local, SuccessfulResponse};

impl TestApp<false> {
    async fn send(&self, hostname: Hostname, cmd: Local) -> reqwest::Response {
        tracing::debug!("sending command {cmd:?} to {hostname}");
        self.post_authed(&format!("remote-spark/{hostname}"))
            .json(&cmd)
            .send()
            .await
            .expect("success")
    }
}

#[actix_rt::test]
async fn sending_a_valid_cmd_to_an_existing_conn_forwards_the_request() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect_device(&hostname));

    let join = tokio::spawn(async move {
        timeout!(20 => async {
            app.send(hostname, Local::Reload)
                .await
                .json::<Result<SuccessfulResponse, ErrorResponse>>()
                .await
        })
    });

    let req = timeout!(device.recv())
        .expect("failed to receive")
        .expect("eof");
    assert_eq!(Local::Reload, req);
    timeout!(device.send(Ok(SuccessfulResponse::Unit))).expect("to send");

    let resp = timeout!(join).expect("failed to join").expect("deser");
    assert_eq!(resp, Ok(SuccessfulResponse::Unit));
}

#[actix_rt::test]
async fn sending_a_command_to_a_non_existent_machine_404s() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let response = timeout!(20 => app.send(hostname, Local::Reload));

    assert_eq!(StatusCode::NOT_FOUND, response.status(),)
}

#[actix_rt::test]
async fn sending_a_command_to_an_existing_but_unresponsive_machine_times_out() {
    let app = TestApp::spawn_without_db().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect_device(&hostname));

    let response = timeout!(20 => app.send(hostname, Local::Reload));

    assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status(),)
}

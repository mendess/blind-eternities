use reqwest::StatusCode;
use serde_json::json;
use spark_protocol::{Local, ProtocolMsg, Response};

use crate::helpers::{fake_hostname, TestApp};
use crate::{assert_status, timeout};

#[actix_rt::test]
async fn get_last_queue_from_existing_device_works() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    eprintln!("connecting to app");
    let mut device = timeout!(app.connect(&hostname));

    eprintln!("starting task");
    let join = tokio::spawn(async move {
        timeout!(20 => async {
            let resp = app.get_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("success");
            assert_status!(StatusCode::OK, resp.status());
            resp
                .json::<Response>()
                .await
        })
    });

    eprintln!("requesting data");
    let req = timeout!(device.recv()).expect("success recv").expect("eof");
    assert_eq!(
        Local::Music("m status".into()),
        req,
    );
    eprintln!("sending data");
    timeout!(device.send(Ok(ProtocolMsg::ForwardValue(json!(0i64))))).expect("success send");

    eprintln!("joining task");
    let last = join.await.expect("join success").expect("request success");
    let last = last.map(|e| match e {
        ProtocolMsg::ForwardValue(v) => serde_json::from_value(v).expect("deserialization"),
        _ => panic!("unexpected response variant: {e:?}"),
    });

    assert_eq!(Ok(Some(0)), last);
}

#[actix_rt::test]
async fn get_null_last_queue_from_existing_device_works() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let join = tokio::spawn(async move {
        timeout!(20 => async {
            let resp = app.get_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("success");
            assert_status!(StatusCode::OK, resp.status());
            resp
                .json::<Response>()
                .await
        })
    });

    let req = timeout!(device.recv()).expect("success recv").expect("eof");
    assert_eq!(
        Local::Music("m status".into()),
        req,
    );
    timeout!(device.send(Ok(ProtocolMsg::ForwardValue(json!(null))))).expect("success send");

    let last = join.await.expect("join success").expect("request success");
    let last = last.map(|e| match e {
        ProtocolMsg::ForwardValue(v) => {
            serde_json::from_value::<Option<usize>>(v).expect("deserialization")
        }
        _ => panic!("unexpected response variant: {e:?}"),
    });
    assert_eq!(Ok(None), last);
}

#[actix_rt::test]
async fn get_last_queue_from_non_existent_device_404s() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.get_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status())
}

#[actix_rt::test]
async fn get_last_queue_from_non_responsive_device_timesout() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.get_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status())
}

#[actix_rt::test]
async fn reset_last_queue_on_existing_device_works() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let resp = tokio::spawn(async move {
        timeout!(20 => async {
            app.delete_authed(&format!("music/player/{hostname}/0/last"))
                .send()
                .await
                .expect("request to succeed")
        })
    });

    let req = timeout!(device.recv())
        .expect("success receive")
        .expect("eof");
    assert_eq!(
        Local::Music("m status".into()),
        req
    );
    timeout!(device.send(Ok(ProtocolMsg::Unit))).expect("successful send");

    assert_status!(StatusCode::OK, resp.await.expect("join success").status());
}

#[actix_rt::test]
async fn reset_last_queue_of_non_existent_device_404s() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.delete_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status());
}

#[actix_rt::test]
async fn reset_last_queue_of_non_responsive_device_timesout() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.delete_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_on_existing_device_works() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let mut device = timeout!(app.connect(&hostname));

    let resp = tokio::spawn(async move {
        timeout!(20 => async {
            app.post_authed(&format!("music/player/{hostname}/0/last"))
                .json(&json!(0i64))
                .send()
                .await
                .expect("request to succeed")
        })
    });

    let req = timeout!(device.recv())
        .expect("success receive")
        .expect("eof");
    assert_eq!(
        Local::Music("m status".into()),
        req
    );
    timeout!(device.send(Ok(ProtocolMsg::Unit))).expect("successful send");

    assert_status!(StatusCode::OK, resp.await.expect("join success").status());
}

#[actix_rt::test]
async fn set_null_last_queue_on_existing_device_returns_bad_request() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::BAD_REQUEST, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_of_non_existent_device_404s() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .json(&json!(0i64))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::NOT_FOUND, resp.status());
}

#[actix_rt::test]
async fn set_last_queue_of_non_responsive_device_timesout() {
    let app = TestApp::<false>::spawn().await;

    let hostname = fake_hostname();

    let _device = timeout!(app.connect(&hostname));

    let resp = timeout!(20 => async {
        app.post_authed(&format!("music/player/{hostname}/0/last"))
            .json(&json!(0i64))
            .send()
            .await
            .expect("request to succeed")
    });

    assert_status!(StatusCode::REQUEST_TIMEOUT, resp.status());
}

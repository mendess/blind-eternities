use std::future::Future;
use std::os::unix::prelude::CommandExt;

pub async fn reload<Fut, R>(respond_to: impl FnOnce(spark_protocol::Response) -> Fut) -> R
where
    Fut: Future<Output = R>,
{
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            return respond_to(Err(spark_protocol::ErrorResponse::RequestFailed(
                e.to_string(),
            )))
            .await
        }
    };
    tracing::info!("realoading spark daemon");
    let r = respond_to(Ok(spark_protocol::SuccessfulResponse::Unit)).await;
    let e = std::process::Command::new(exe).arg("daemon").exec();
    tracing::error!(?e, "exec self failed");
    if let Some(arg0) = std::env::args().next() {
        let e = std::process::Command::new(arg0).arg("daemon").exec();
        tracing::error!(?e, "exec arg0 failed");
    }
    r
}

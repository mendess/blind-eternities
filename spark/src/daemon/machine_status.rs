use crate::{config::Config, util::get_current_status};
use common::net::{AuthenticatedClient, auth_client::UrlParseError};
use reqwest::StatusCode;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info_span, warn};

pub fn start(config: Arc<Config>) -> Result<impl Future<Output = ()>, UrlParseError> {
    let client = AuthenticatedClient::try_from(&*config)?;
    Ok(async move {
        loop {
            let _span = info_span!("post machine status");
            match get_current_status(&config).await {
                Ok(status) => {
                    debug!("posting machine status: {:#?}", status);
                    let result = timeout(
                        Duration::from_secs(10),
                        client
                            .post("/machine/status")
                            .expect("building a request")
                            .json(&status)
                            .send(),
                    )
                    .await;
                    match result {
                        Ok(Ok(r)) if r.status() == StatusCode::OK => debug!("Post succeeded"),
                        Ok(Ok(r)) => error!("Post request failed: {}", r.status()),
                        Ok(Err(e)) => error!("Network request failed: {:?}", e),
                        Err(_elapsed) => warn!("request timed out"),
                    }
                }
                Err(e) => error!("Failed to obtain a machine status: {:?}", e),
            };

            sleep(Duration::from_secs(60)).await;
        }
    })
}

use crate::{config::Config, util::get_current_status};
use common::net::{auth_client::UrlParseError, AuthenticatedClient};
use reqwest::StatusCode;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::time::sleep;
use tracing::{debug, error, info_span};

pub fn start(config: Arc<Config>) -> Result<impl Future<Output = ()>, UrlParseError> {
    let client = AuthenticatedClient::try_from(&*config)?;
    Ok(async move {
        loop {
            let _span = info_span!("post machine status");
            match get_current_status(&config).await {
                Ok(status) => {
                    debug!("posting machine status: {:#?}", status);
                    let result = client
                        .post("/machine/status")
                        .expect("building a request")
                        .json(&status)
                        .send()
                        .await;
                    match result {
                        Ok(r) if r.status() == StatusCode::OK => debug!("Post succeeded"),
                        Ok(r) => error!("Post request failed: {}", r.status()),
                        Err(e) => error!("Network request failed: {:?}", e),
                    }
                }
                Err(e) => error!("Failed to obtain a machine status: {:?}", e),
            };

            sleep(Duration::from_secs(60)).await;
        }
    })
}

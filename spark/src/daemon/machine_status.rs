use crate::{config::Config, util::get_current_status};
use common::net::{auth_client::UrlParseError, AuthenticatedClient};
use reqwest::StatusCode;
use std::{convert::Infallible, time::Duration};
use tokio::time::sleep;
use tracing::{debug, error, info_span};

pub async fn start(config: &Config) -> Result<Infallible, UrlParseError> {
    let client = AuthenticatedClient::new(config.token.clone(), &config.backend_url)?;
    loop {
        let _span = info_span!("post machine status");
        match get_current_status(config).await {
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
}

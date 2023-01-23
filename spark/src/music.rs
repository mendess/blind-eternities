use common::net::AuthenticatedClient;
use serde::Serialize;
use spark_protocol::music::{MusicCmd, MusicCmdKind};

use crate::{config::Config, util::destination::Destination};

pub async fn handle(destination: Destination, cmd: MusicCmd, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;

    let Destination { username, hostname } = destination;

    let url = &format!("music/players/{hostname}/{}", cmd.command.to_route());

    #[derive(Serialize)]
    struct QueueRequest {
        query: String,
        search: bool,
        username: Option<String>,
    }

    let http_response = match cmd.command {
        MusicCmdKind::Queue { query, search } => {
            client
                .post(url)?
                .json(&QueueRequest {
                    query,
                    search,
                    username,
                })
                .send()
                .await?
        }
        cmd => {
            let mut request = client.get(url)?;
            if let Some(username) = username {
                request = request.query(&[("u", username)])
            };
            let request = cmd.to_query_string(|args| request.query(args));
            tracing::debug!(?request, "sending GET request");
            request.send().await?
        }
    };

    if http_response.status().is_success() {
        let response = http_response.text().await?;
        match serde_json::from_str::<spark_protocol::Response>(&response) {
            Ok(response) => println!("{response:?}"),
            Err(e) => tracing::error!(?e, "deserialization failed"),
        }
    } else {
        let status = http_response.status();
        let response = http_response.text().await?;
        tracing::error!(%status, message = %response, "request failed");
    }

    Ok(())
}

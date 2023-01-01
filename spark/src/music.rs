use common::net::AuthenticatedClient;
use serde::Serialize;
use spark_protocol::music::{MusicCmd, MusicCmdKind};

use crate::{config::Config, util::destination::Destination};

pub async fn handle(destination: Destination, cmd: MusicCmd, config: Config) -> anyhow::Result<()> {
    let client =
        AuthenticatedClient::new(config.token, &config.backend_domain, config.backend_port)?;

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
            request.send().await?
        }
    };

    // let response = http_response.text().await?;
    let response = http_response.json::<spark_protocol::Response>().await?;

    println!("{response:?}");
    Ok(())
}

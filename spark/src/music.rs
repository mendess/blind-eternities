use common::{domain::Hostname, net::AuthenticatedClient};
use serde::Serialize;
use spark_protocol::music::{MusicCmd, MusicCmdKind};

use crate::config::Config;

pub async fn handle(hostname: &Hostname, cmd: MusicCmd, config: Config) -> anyhow::Result<()> {
    let client =
        AuthenticatedClient::new(config.token, &config.backend_domain, config.backend_port)?;

    let url = &format!("music/players/{hostname}/{}", cmd.command.to_route());

    #[derive(Serialize)]
    struct QueueRequest {
        query: String,
        search: bool,
    }

    let http_response = match cmd.command {
        MusicCmdKind::Queue { query, search } => {
            client
                .post(url)?
                .json(&QueueRequest { query, search })
                .send()
                .await?
        }
        _ => client.get(url)?.send().await?,
    };

    // let response = http_response.text().await?;
    let response = http_response.json::<spark_protocol::Response>().await?;

    println!("{response:?}");
    Ok(())
}

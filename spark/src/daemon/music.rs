use std::{sync::Arc, time::Duration};

use common::{domain::music::Player, net::AuthenticatedClient};
use futures::{future::ready, TryFutureExt};
use tracing::info_span;

use crate::{config::Config, state::STATE};

pub async fn start(_config: Arc<Config>, client: Arc<AuthenticatedClient>) -> anyhow::Result<()> {
    backend_poll(client.clone()).await
}

async fn backend_poll(client: Arc<AuthenticatedClient>) -> anyhow::Result<()> {
    loop {
        let _span = info_span!("get music player");
        let players = client
            .get("music/player")
            .expect("correct url")
            .send()
            .and_then(|r| ready(r.error_for_status()))
            .and_then(|r| r.json::<Vec<Player>>())
            .await;
        match players {
            Ok(players) => {
                tracing::info!(?players);
                STATE
                    .write()
                    .map_err(|_| anyhow::anyhow!("poisoned"))?
                    .backend_players
                    .clone_from(&players);
            }
            Err(e) => tracing::error!(?e, "failed to fetch players"),
        }

        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

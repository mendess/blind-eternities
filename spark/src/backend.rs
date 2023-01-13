use common::{domain::Hostname, net::AuthenticatedClient};

use crate::config::Config;

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client =
        AuthenticatedClient::new(config.token, &config.backend_domain, config.backend_port)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
    }
    Ok(())
}

async fn display_persistent_connections(client: AuthenticatedClient) -> anyhow::Result<()> {
    let conns: Vec<Hostname> = client
        .get("/persistent-connections")?
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    println!("connected hosts are:");
    for c in conns {
        println!("- {c}");
    }

    Ok(())
}

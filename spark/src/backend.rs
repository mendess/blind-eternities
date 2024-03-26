use common::{domain::Hostname, net::AuthenticatedClient};

use crate::config::Config;

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
        crate::Backend::AddMusicToken { username } => add_music_token(client, username).await?,
        crate::Backend::DeleteMusicToken { username } => {
            delete_music_token(client, username).await?
        }
    }
    Ok(())
}

async fn display_persistent_connections(client: AuthenticatedClient) -> anyhow::Result<()> {
    let conns: Vec<Hostname> = client
        .get("/admin/persistent-connections")?
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

async fn add_music_token(client: AuthenticatedClient, username: String) -> anyhow::Result<()> {
    let token = client
        .post("/admin/music_token")?
        .body(username)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    println!("new token added: {token}");
    Ok(())
}

async fn delete_music_token(client: AuthenticatedClient, username: String) -> anyhow::Result<()> {
    client
        .delete("/admin/music_token")?
        .body(username)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    println!("token deleted");
    Ok(())
}

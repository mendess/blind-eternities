use std::time::{Duration, SystemTime};

use common::{domain::Hostname, net::AuthenticatedClient};

use crate::config::Config;

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
        crate::Backend::CreateMusicSession {
            hostname,
            expire_in,
        } => create_music_session(client, hostname, expire_in).await?,
        crate::Backend::DeleteMusicSession { session } => {
            delete_music_session(client, session).await?
        }
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

async fn create_music_session(
    client: AuthenticatedClient,
    hostname: Hostname,
    expire_in: Option<Duration>,
) -> anyhow::Result<()> {
    let mut req = client.get(&format!("/admin/music-session/{hostname}"))?;
    if let Some(expire_in) = expire_in {
        let now = SystemTime::now() + expire_in;
        req = req.query(&[
            "expire_at",
            &now.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_string(),
        ]);
    }
    let token = req
        .send()
        .await?
        .error_for_status()?
        .json::<String>()
        .await?;

    println!("session id is: {token}");
    Ok(())
}

async fn delete_music_session(client: AuthenticatedClient, session: String) -> anyhow::Result<()> {
    client
        .delete(&format!("/admin/music-session/{session}"))?
        .send()
        .await?
        .error_for_status()?;

    println!("session deleted");
    Ok(())
}

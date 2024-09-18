use std::time::Duration;

use chrono::Utc;
use common::{
    domain::{music_session::ExpiresAt, Hostname},
    net::AuthenticatedClient,
};

use crate::config::Config;

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
        crate::Backend::CreateMusicSession {
            hostname,
            expire_in,
            show_link,
        } => create_music_session(client, hostname, expire_in, show_link).await?,
        crate::Backend::DeleteMusicSession { session } => {
            delete_music_session(client, session).await?
        }
    }
    Ok(())
}

async fn display_persistent_connections(client: AuthenticatedClient) -> anyhow::Result<()> {
    let conns: Vec<Hostname> = client
        .get(
            #[cfg(feature = "ws")]
            "/persistent-connections/ws",
            #[cfg(not(feature = "ws"))]
            "/persistent-connections",
        )?
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
    show_link: bool,
) -> anyhow::Result<()> {
    let token = client
        .get(&format!("/admin/music-session/{hostname}"))?
        .query(&ExpiresAt {
            expires_at: expire_in.map(|d| Utc::now() + d),
        })
        .send()
        .await?
        .error_for_status()?
        .json::<String>()
        .await?;

    if show_link {
        println!("url: https://planar-bridge.mendess.xyz/music?session={token}");
    } else {
        println!("session id is: {token}");
    }
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

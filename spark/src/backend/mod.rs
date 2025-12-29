mod songs;

use crate::{config::Config, util::get_hostname};
use chrono::Utc;
use common::{
    domain::{Hostname, music_session::ExpiresAt},
    net::AuthenticatedClient,
};
use std::time::Duration;

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
        crate::Backend::CreateMusicSession {
            hostname,
            expire_in,
            show_link,
        } => {
            create_music_session(
                client,
                match hostname {
                    Some(h) => h,
                    None => get_hostname(&config).await?,
                },
                expire_in,
                show_link,
            )
            .await?
        }
        crate::Backend::DeleteMusicSession { session } => {
            delete_music_session(client, session).await?
        }
        crate::Backend::AddSong {
            title,
            artist,
            uri,
            thumb,
        } => {
            songs::add_song(client, title, artist, uri, thumb).await?;
        }
        crate::Backend::UpgradeSong { title, strict } => {
            songs::upgrade_song(client, title, strict).await?;
        }
    }
    Ok(())
}

async fn display_persistent_connections(client: AuthenticatedClient) -> anyhow::Result<()> {
    let conns: Vec<Hostname> = client
        .get("/persistent-connections/ws")?
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

use std::io::IsTerminal;

use common::net::AuthenticatedClient;
use serde::Serialize;
use spark_protocol::music::{
    Chapter, MusicCmd, MusicCmdKind,
    Response::{Current, PlayState, QueueSummary, Title, Volume},
};

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
        if std::io::stdout().is_terminal() {
            match http_response.json::<spark_protocol::Response>().await? {
                Ok(spark_protocol::SuccessfulResponse::MusicResponse(music)) => match music {
                    Title { title } => println!("Now playing: {title}"),
                    PlayState { paused } => {
                        println!("{}", if paused { "paused" } else { "playing" })
                    }
                    Volume { volume } => println!("volume: {volume}%"),
                    Current {
                        paused,
                        title,
                        chapter,
                        volume,
                        progress,
                    } => {
                        match chapter {
                            Some(Chapter { title, index }) => {
                                println!("Now Playing:\nVideo: {title} Song: {index} - {title}")
                            }
                            None => println!("Now Playing: {title}"),
                        };
                        println!(
                            "{} at {volume}% volume",
                            if paused { "paused" } else { "playing" }
                        );
                        println!("Progress: {progress:.2} %");
                    }
                    QueueSummary {
                        from,
                        moved_to,
                        current,
                    } => {
                        println!("Queued to position {from}.");
                        println!("--> moved to {moved_to}.");
                        println!("Currently playing {current}")
                    }
                },
                Ok(response) => {
                    tracing::error!(?response, "unexpected response")
                }
                Err(error) => {
                    tracing::error!(?error, "failed to execute music command");
                }
            }
        } else {
            let response = http_response.text().await?;
            println!("{response}");
        }
    } else {
        let status = http_response.status();
        let response = http_response.text().await?;
        tracing::error!(%status, message = %response, "request failed");
    }

    Ok(())
}

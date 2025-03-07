use std::{future::Future, io, str::FromStr, time::Duration};

use futures::{FutureExt, Stream, StreamExt, future::join3, stream};
use mlib::{
    Item, Link, Search,
    players::{
        self, SmartQueueOpts,
        event::{OwnedLibMpvEvent, PlayerEvent},
    },
    playlist::PartialSearchResult,
    queue::Queue,
};
use spark_protocol::{ErrorResponse, music::Response as MusicResponse};
use tokio::time::timeout;

fn forward<E: std::fmt::Debug>(e: E) -> ErrorResponse {
    ErrorResponse::ForwardedError(format!("{e:?}"))
}

pub async fn wait_for_next_title(player: &players::PlayerLink) -> Result<String, ErrorResponse> {
    let stream = player.subscribe().await.map_err(forward)?;
    async fn from_stream(
        stream: impl Stream<Item = io::Result<PlayerEvent>>,
    ) -> Result<Option<String>, mlib::Error> {
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            if let OwnedLibMpvEvent::PropertyChange { name, change, .. } = event?.event {
                if name == "media-title" {
                    let Ok(title) = change.into_string() else {
                        continue;
                    };
                    return Ok(Some(title));
                }
            }
        }
        Ok(None)
    }
    let title = match timeout(Duration::from_secs(2), from_stream(stream)).await {
        Ok(Err(io_error)) => return Err(ErrorResponse::IoError(io_error.to_string())),
        Ok(Ok(Some(title))) => title,
        Ok(Ok(None)) | Err(_ /*elapsed*/) => player.media_title().await.map_err(forward)?,
    };

    Ok(title)
}

pub async fn handle(cmd: spark_protocol::music::MusicCmd) -> spark_protocol::Response {
    let player = match cmd.index {
        Some(i) => &players::PlayerLink::of(i),
        None => players::PlayerLink::current(),
    };
    let player = match cmd.username {
        Some(u) => &player.linked_to(u),
        None => player,
    };

    // TODO: Io(NotFound) should be translated to "no players running for user {username}"
    let response: Result<MusicResponse, ErrorResponse> = match cmd.command {
        spark_protocol::music::MusicCmdKind::Frwd => {
            player
                .change_file(players::Direction::Next)
                .then(|_| async {
                    Ok(MusicResponse::Title {
                        title: wait_for_next_title(player).await?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::Back => {
            player
                .change_file(players::Direction::Prev)
                .then(|_| async {
                    Ok(MusicResponse::Title {
                        title: wait_for_next_title(player).await?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::CyclePause => {
            player
                .cycle_pause()
                .then(|_| async {
                    Ok(MusicResponse::PlayState {
                        paused: player.is_paused().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::ChangeVolume { amount } => {
            player
                .change_volume(amount)
                .then(|_| async {
                    Ok(MusicResponse::Volume {
                        volume: player.volume().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::Current => {
            async {
                let current = Queue::current(player, Default::default())
                    .await
                    .map_err(forward)?;
                Ok(MusicResponse::Current { current })
            }
            .await
        }
        spark_protocol::music::MusicCmdKind::Queue { query, search } => {
            async {
                let item = match Link::from_str(&query) {
                    Ok(l) => Item::Link(l),
                    Err(_) => {
                        let mut playlist =
                            mlib::playlist::Playlist::load().await.map_err(forward)?;
                        let song = handle_search_result(
                            playlist.partial_name_search_mut(query.split_whitespace()),
                        )
                        .map_err(forward)?;
                        match song {
                            Some(song) => Item::Link(song.delete().link.into()),
                            None if search => Item::Search(Search::new(query)),
                            None => return Err(forward("song not in playlist")),
                        }
                    }
                };
                let summary = player
                    .smart_queue(item, SmartQueueOpts { no_move: false })
                    .await
                    .map_err(forward)?;
                Ok(MusicResponse::QueueSummary {
                    from: summary.from,
                    moved_to: summary.moved_to,
                    current: summary.current,
                })
            }
            .await
        }
        spark_protocol::music::MusicCmdKind::Now { amount } => {
            async {
                impl<F> RustPls for F {}
                trait RustPls: Sized {
                    fn rust_pls<R>(self) -> impl Send + Future<Output = R>
                    where
                        Self: Send + Future<Output = R>,
                    {
                        self
                    }
                }
                let queue = Queue::load(player, amount.unwrap_or(20))
                    .await
                    .map_err(forward)?;
                let (before, current, after) = join3(
                    stream::iter(queue.before())
                        .then(|i| i.item.fetch_item_title().rust_pls())
                        .collect(),
                    queue.current_song().item.clone().fetch_item_title(),
                    stream::iter(queue.after())
                        .map(|i| i.item.fetch_item_title())
                        .buffered(8)
                        .collect(),
                )
                .rust_pls()
                .await;
                Ok(MusicResponse::Now {
                    before,
                    current,
                    after,
                })
            }
            .await
        }
    };
    response.map(Into::into)
}

fn handle_search_result<T>(r: PartialSearchResult<T>) -> Result<Option<T>, String> {
    use std::fmt::Write;
    match r {
        PartialSearchResult::One(t) => Ok(Some(t)),
        PartialSearchResult::None => Ok(None),
        PartialSearchResult::Many(too_many_matches) => {
            let mut buf = String::from("too many matches:\n");

            for m in too_many_matches {
                writeln!(buf, "  {m}").unwrap();
            }

            Err(buf)
        }
    }
}

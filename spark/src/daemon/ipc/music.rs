use std::future::Future;

use futures::FutureExt;
use mlib::{
    players::{self, SmartQueueOpts},
    playlist::PartialSearchResult,
    Item, Link, Search,
};
use spark_protocol::{music::Response as MusicResponse, ErrorResponse};

pub async fn handle<R, Fut>(
    cmd: spark_protocol::music::MusicCmd,
    respond_to: impl FnOnce(spark_protocol::Response) -> Fut,
) -> R
where
    Fut: Future<Output = R>,
{
    let player = match cmd.index {
        Some(i) => players::PlayerIndex::of(i),
        None => players::PlayerIndex::CURRENT,
    };
    fn forward<E: ToString>(e: E) -> ErrorResponse {
        ErrorResponse::ForwardedError(e.to_string())
    }

    let response: Result<MusicResponse, ErrorResponse> = match cmd.command {
        spark_protocol::music::MusicCmdKind::Frwd => {
            player
                .change_file(players::Direction::Next)
                .then(|_| async {
                    Ok(MusicResponse::Title {
                        title: players::media_title().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::Back => {
            player
                .change_file(players::Direction::Prev)
                .then(|_| async {
                    Ok(MusicResponse::Title {
                        title: players::media_title().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::CyclePause => {
            player
                .cycle_pause()
                .then(|_| async {
                    Ok(MusicResponse::PlayState {
                        paused: players::is_paused().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::ChangeVolume { amount } => {
            player
                .change_volume(amount)
                .then(|_| async {
                    Ok(MusicResponse::Volume {
                        volume: players::volume().await.map_err(forward)?,
                    })
                })
                .await
        }
        spark_protocol::music::MusicCmdKind::Current => {
            async {
                Ok(MusicResponse::Current {
                    title: player.media_title().await.map_err(forward)?,
                    chapter: player.chapter_metadata().await.ok().map(|m| {
                        spark_protocol::music::Chapter {
                            title: m.title,
                            index: m.index as u32,
                        }
                    }),
                    volume: player.volume().await.map_err(forward)?,
                    progress: player.percent_position().await.map_err(forward)?,
                })
            }
            .await
        }
        spark_protocol::music::MusicCmdKind::Queue { query, search } => {
            async {
                let item = if search {
                    Item::Search(Search::new(query))
                } else {
                    match Link::from_url(query) {
                        Ok(l) => Item::Link(l),
                        Err(query) => {
                            let mut playlist =
                                mlib::playlist::Playlist::load().await.map_err(forward)?;
                            let song = handle_search_result(
                                playlist.partial_name_search_mut(query.split_whitespace()),
                            )
                            .map_err(forward)?;
                            Item::Link(song.delete().link.into())
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
    };
    respond_to(response.map(Into::into)).await
}

fn handle_search_result<T>(r: PartialSearchResult<T>) -> Result<T, String> {
    use std::fmt::Write;
    match r {
        PartialSearchResult::One(t) => Ok(t),
        PartialSearchResult::None => Err(String::from("song not in playlist")),
        PartialSearchResult::Many(too_many_matches) => {
            let mut buf = String::from("too many matches:\n");

            for m in too_many_matches {
                writeln!(buf, "  {m}").unwrap();
            }

            Err(buf)
        }
    }
}

use crate::{Backend, Error, cache};
use askama::{Template, filters::urlencode};
use axum::{
    Router,
    body::Body,
    extract::Path,
    response::{Html, IntoResponse},
    routing::get,
};
use axum_extra::extract::Query;
use futures::StreamExt as _;
use http::{Response, StatusCode, header};
use mappable_rc::Marc;
use mlib::item::link::HasId as _;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io,
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;
use tokio_util::io::ReaderStream;

pub fn routes() -> Router<Backend> {
    Router::new()
        .route("/", get(index))
        .route("/playlist", get(playlist))
        .route("/audio/{id}", get(audio))
}

#[derive(Template)]
#[template(path = "playlist/index.html")]
struct Index {}

async fn index() -> Result<impl IntoResponse, Error> {
    Ok(Html(Index {}.render()?))
}

#[derive(Template)]
#[template(path = "playlist/playlist.html")]
struct Playlist {
    qstring: String,
    categories: Vec<Category>,
    songs: Vec<Song>,
    shuffled: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum CategoryFilterMode {
    MustHave,
    CantHave,
    Neutral,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Category {
    songs: u16,
    name: String,
    mode: CategoryFilterMode,
}

#[derive(Serialize, Deserialize)]
struct Song {
    id: String,
    title: String,
    artist: Option<String>,
    categories: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CategoryFilter {
    #[serde(default, rename = "s")]
    shuffle: bool,
    #[serde(default, rename = "d")]
    disabled: HashSet<String>,
    #[serde(default, rename = "m")]
    must_have: HashSet<String>,
    #[serde(default, rename = "t")]
    toggle: Option<String>,
}

async fn playlist(Query(mut query): Query<CategoryFilter>) -> Result<impl IntoResponse, Error> {
    if let Some(toggle) = std::mem::take(&mut query.toggle) {
        if query.disabled.contains(&toggle) {
            query.disabled.remove(&toggle);
        } else if query.must_have.contains(&toggle) {
            query.must_have.remove(&toggle);
            query.disabled.insert(toggle);
        } else {
            query.must_have.insert(toggle);
        }
    }
    let playlist = load_playlist().await.unwrap();
    let categories = {
        let categories = playlist.songs.iter().flat_map(|s| s.all_categories()).fold(
            HashMap::new(),
            |mut acc, cat| {
                *acc.entry(cat).or_insert(0) += 1;
                acc
            },
        );
        let mut categories = categories
            .into_iter()
            .filter(|(_, freq)| *freq > 3)
            .map(|(c, freq)| Category {
                mode: if query.disabled.contains(c) {
                    CategoryFilterMode::CantHave
                } else if query.must_have.contains(c) {
                    CategoryFilterMode::MustHave
                } else {
                    CategoryFilterMode::Neutral
                },
                name: c.into(),
                songs: freq,
            })
            .collect::<Vec<_>>();
        categories.sort_by(|s0, s1| s0.cmp(s1).reverse());
        categories
    };
    let mut songs = playlist
        .songs
        .iter()
        .filter(|s| include_song(s, &query))
        .map(song_map)
        .rev()
        .collect::<Vec<_>>();
    if query.shuffle {
        songs.shuffle(&mut rand::rng());
    }
    return Ok(Html(
        Playlist {
            shuffled: query.shuffle,
            categories,
            songs,
            qstring: set_to_qstring(
                'm',
                query.must_have,
                set_to_qstring('d', query.disabled, String::new()),
            ),
        }
        .render()?,
    ));

    fn song_map(s: &mlib::playlist::Song) -> Song {
        Song {
            id: s.link.id().to_string(),
            categories: s
                .all_categories()
                .filter(|c| Some(*c) != s.artist.as_deref())
                .map(|s| s.to_string())
                .collect(),
            title: s.name.clone(),
            artist: s.artist.clone(),
        }
    }

    fn include_song(s: &mlib::playlist::Song, query: &CategoryFilter) -> bool {
        s.all_categories().all(|c| !query.disabled.contains(c))
            && (query.must_have.is_empty()
                || s.all_categories().any(|c| query.must_have.contains(c)))
    }

    fn set_to_qstring(param: char, set: HashSet<String>, buf: String) -> String {
        set.iter().fold(buf, |mut s, c| {
            if !s.is_empty() {
                s.push('&');
            }
            s.push(param);
            s.push('=');
            s.push_str(&urlencode(c).unwrap().0.to_string());
            s
        })
    }
}

#[derive(Deserialize)]
struct AudioQuery {
    id: String,
}

async fn audio(query: Path<AudioQuery>) -> Result<impl IntoResponse, Error> {
    // Fetch the original .mka file into memory
    let mut response = reqwest::Client::new()
        .get(format!(
            "https://blind-eternities.mendess.xyz/playlist/song/audio/{}",
            query.0.id
        ))
        .send()
        .await?
        .error_for_status()?;

    // Spawn ffmpeg to transcode to mp3 (browser-friendly)
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            "pipe:0", // read input from stdin
            "-f",
            "mp3", // output format
            "-codec:a",
            "libmp3lame",
            "pipe:1", // write output to stdout
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // silence ffmpeg logs
        .spawn()?;

    // Write input bytes into ffmpeg stdin
    {
        let mut stdin = child.stdin.take().unwrap();
        tokio::spawn(async move {
            loop {
                let chunk = match response.chunk().await {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!(error = ?e, "failed to read chunk");
                        break;
                    }
                };
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(&chunk).await;
            }
        });
    }

    // Stream ffmpeg stdout back to client
    let stdout = child.stdout.take().unwrap();
    let stream = ReaderStream::new(stdout).map(|chunk| chunk.map_err(io::Error::other));

    let mut res = Response::new(Body::from_stream(stream));
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("audio/mpeg"),
    );

    Ok(res)
}

pub async fn load_playlist() -> Result<Marc<mlib::playlist::Playlist>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

    async fn init() -> Result<mlib::playlist::Playlist, Error> {
        let playlist_request = reqwest::get(
            "https://raw.githubusercontent.com/mendess/spell-book/master/runes/m/playlist.json",
        )
        .await?;

        let text = playlist_request.text().await.map_err(io::Error::other)?;

        Ok(mlib::playlist::Playlist::load_from_str(&text)?)
    }

    cache::get_or_init(Default::default(), init, ONE_HOUR).await
}

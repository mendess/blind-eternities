use crate::{Backend, Error, cache, metrics};
use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    response::{Html, IntoResponse},
    routing::get,
};
use axum_extra::extract::Query;
use base64::Engine;
use futures::StreamExt as _;
use http::{Response, StatusCode, header};
use mappable_rc::Marc;
use mlib::item::link::HasId as _;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io, process::Stdio, time::Duration};
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
    categories: Vec<(&'static str, Vec<Category>)>,
    // artists: Vec<Category>,
    // genres: Vec<Category>,
    // liked_by: Vec<Category>,
    // languages: Vec<Category>,
    // free_categories: Vec<Category>,
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
    songs: usize,
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
struct UserAction {
    #[serde(default, rename = "s")]
    shuffle: bool,
    #[serde(default, rename = "b")]
    category_blob: Option<String>,
    #[serde(default, rename = "t")]
    toggle: Option<String>,
    #[serde(default, rename = "f")]
    field: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CategoryFilter {
    #[serde(default, rename = "d")]
    disabled: CategoryFilterFields,
    #[serde(default, rename = "m")]
    must_have: CategoryFilterFields,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CategoryFilterFields {
    #[serde(default, rename = "f")]
    free: HashSet<String>,
    #[serde(default, rename = "a")]
    artists: HashSet<String>,
    #[serde(default, rename = "g")]
    genres: HashSet<String>,
    #[serde(default, rename = "s")]
    language: HashSet<String>,
    #[serde(default, rename = "l")]
    liked_by: HashSet<String>,
}

mod fields {
    pub const OTHER: &str = "other";
    pub const GENRES: &str = "genres";
    pub const LANGUAGES: &str = "languages";
    pub const LIKED_BY: &str = "liked_by";
    pub const ARTISTS: &str = "artists";
}

async fn playlist(
    backend: State<Backend>,
    Query(mut query): Query<UserAction>,
) -> Result<impl IntoResponse, Error> {
    let mut filter = query
        .category_blob
        .and_then(|s| {
            serde_json::from_slice::<CategoryFilter>(
                &base64::engine::general_purpose::URL_SAFE.decode(s).ok()?,
            )
            .ok()
        })
        .unwrap_or_default();
    if let Some((toggle, field)) = query
        .toggle
        .take()
        .and_then(|t| query.field.take().map(|f| (t, f)))
    {
        let (disabled, must_have) = match field.as_str() {
            fields::OTHER => (&mut filter.disabled.free, &mut filter.must_have.free),
            fields::GENRES => (&mut filter.disabled.genres, &mut filter.must_have.genres),
            fields::LANGUAGES => (
                &mut filter.disabled.language,
                &mut filter.must_have.language,
            ),
            fields::LIKED_BY => (
                &mut filter.disabled.liked_by,
                &mut filter.must_have.liked_by,
            ),
            fields::ARTISTS => (&mut filter.disabled.artists, &mut filter.must_have.artists),
            f => return Err(Error::BadRequest(format!("invalid field: {f}"))),
        };
        if disabled.contains(&toggle) {
            disabled.remove(&toggle);
        } else if must_have.contains(&toggle) {
            must_have.remove(&toggle);
            disabled.insert(toggle);
        } else {
            must_have.insert(toggle);
        }
    }
    let playlist = load_playlist(&backend).await?;
    let free_categories = calculate_categories(
        &playlist,
        |s| s.categories.iter().map(|s| s.as_str()),
        &filter.disabled.free,
        &filter.must_have.free,
    );
    let artists = calculate_categories(
        &playlist,
        |s| s.artist.as_deref().into_iter(),
        &filter.disabled.artists,
        &filter.must_have.artists,
    );
    let genres = calculate_categories(
        &playlist,
        |s| s.genres.iter().map(|s| s.as_str()),
        &filter.disabled.genres,
        &filter.must_have.genres,
    );
    let liked_by = calculate_categories(
        &playlist,
        |s| {
            s.liked_by
                .iter()
                .chain(&s.recommended_by)
                .filter(|s| *s != "balao")
                .map(|s| s.as_str())
        },
        &filter.disabled.liked_by,
        &filter.must_have.liked_by,
    );
    let languages = calculate_categories(
        &playlist,
        |s| s.language.as_deref().into_iter(),
        &filter.disabled.language,
        &filter.must_have.language,
    );
    let mut songs = playlist
        .songs
        .iter()
        .filter(|s| include_song(s, &filter))
        .map(song_map)
        .rev()
        .collect::<Vec<_>>();
    if query.shuffle {
        songs.shuffle(&mut rand::rng());
    }
    return Ok(Html(
        Playlist {
            shuffled: query.shuffle,
            categories: Vec::from_iter([
                (fields::ARTISTS, artists),
                (fields::GENRES, genres),
                (fields::LANGUAGES, languages),
                (fields::OTHER, free_categories),
                (fields::LIKED_BY, liked_by),
            ]),
            songs,
            qstring: format!(
                "b={}",
                base64::engine::general_purpose::URL_SAFE
                    .encode(serde_json::to_vec(&filter).unwrap())
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

    fn include_song(s: &mlib::playlist::Song, filter: &CategoryFilter) -> bool {
        let any = |f: &CategoryFilterFields| {
            let CategoryFilterFields {
                free,
                artists,
                genres,
                language,
                liked_by,
            } = f;
            s.categories.iter().any(|c| free.contains(c))
                || s.artist.iter().any(|c| artists.contains(c))
                || s.genres.iter().any(|c| genres.contains(c))
                || s.language.iter().any(|c| language.contains(c))
                || s.liked_by.iter().any(|c| liked_by.contains(c))
                || s.recommended_by.iter().any(|c| liked_by.contains(c))
        };
        if any(&filter.disabled) {
            return false;
        }
        let CategoryFilterFields {
            free,
            artists,
            genres,
            language,
            liked_by,
        } = &filter.must_have;

        if [free, artists, genres, language, liked_by]
            .into_iter()
            .all(|s| s.is_empty())
        {
            return true;
        }
        any(&filter.must_have)
    }

    fn calculate_categories<'p, F, I>(
        playlist: &'p mlib::playlist::Playlist,
        proj: F,
        disabled: &HashSet<String>,
        must_have: &HashSet<String>,
    ) -> Vec<Category>
    where
        F: Fn(&'p mlib::playlist::Song) -> I,
        I: Iterator<Item = &'p str>,
    {
        let categories = playlist.categories_of_kind(proj);
        let mut categories = categories
            .into_iter()
            .filter(|(_, freq)| *freq > 2)
            .map(|(c, freq)| Category {
                mode: if disabled.contains(c) {
                    CategoryFilterMode::CantHave
                } else if must_have.contains(c) {
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
    }
}

#[derive(Deserialize)]
struct AudioQuery {
    id: String,
}

async fn audio(
    client: State<Backend>,
    query: Path<AudioQuery>,
) -> Result<impl IntoResponse, Error> {
    // Fetch the original .mka file into memory
    let mut response = client
        .get(&format!("/playlist/song/audio/{}", query.0.id))
        .expect("url should always parse")
        .send()
        .await?
        .error_for_status()?;

    if response
        .headers()
        .get("x-audio-source")
        .and_then(|h| h.to_str().ok())
        == Some("navidrome")
    {
        metrics::playlist_audio_streams("navidrome").inc();
        let mut response_builder = Response::builder().status(response.status());
        *response_builder.headers_mut().unwrap() = std::mem::take(response.headers_mut());
        Ok(response_builder
            .body(Body::from_stream(response.bytes_stream()))
            .map_err(|e| Error::Io(io::Error::other(e)))?)
    } else {
        metrics::playlist_audio_streams("ffmpeg").inc();
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
}

pub async fn load_playlist(client: &Backend) -> Result<Marc<mlib::playlist::Playlist>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

    async fn init(client: &Backend) -> Result<mlib::playlist::Playlist, Error> {
        let playlist_request = client.get("/playlist").unwrap().send().await?;

        let text = playlist_request.text().await.map_err(io::Error::other)?;

        Ok(mlib::playlist::Playlist::load_from_str(&text)?)
    }

    cache::get_or_init(Default::default(), || init(client), ONE_HOUR).await
}

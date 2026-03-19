use crate::{domain::playlist::NavidromeId, net::auth_client::Client};
use http::HeaderMap;
use md5::Digest as _;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::str;

const SUBSONIC_USER: &str = "blind-eternities-api";
const SUBSONIC_PASS: &str = "?8@Rh~]6pe?AkS3";
const SUBSONIC_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT: &str = "blind-eternities";
const SUBSONIC_FORMAT: &str = "json";

#[derive(Serialize)]
struct BaseNavidromeQuery {
    #[serde(rename = "u")]
    username: &'static str,
    #[serde(rename = "t")]
    salted_token: String,
    #[serde(rename = "s")]
    salt: String,
    #[serde(rename = "v")]
    version: &'static str,
    #[serde(rename = "c")]
    client: &'static str,
    #[serde(rename = "f")]
    format: &'static str,
}

impl BaseNavidromeQuery {
    fn base() -> Self {
        let hex =
            |n: &mut dyn Iterator<Item = u8>| n.map(|n| format!("{n:02x}")).collect::<String>();
        let salt = hex(&mut rand::random_iter::<u8>().take(6));
        let salted_token = {
            let mut md5 = md5::Md5::new();
            md5.update(format!("{SUBSONIC_PASS}{salt}"));
            hex(&mut md5.finalize().iter().copied())
        };
        Self {
            username: SUBSONIC_USER,
            salted_token,
            salt,
            version: SUBSONIC_VERSION,
            client: SUBSONIC_CLIENT,
            format: SUBSONIC_FORMAT,
        }
    }
}

async fn api<T: Serialize, R: DeserializeOwned>(
    client: &Client,
    path: &str,
    query: T,
) -> reqwest::Result<R> {
    #[derive(Serialize)]
    struct Body<T> {
        #[serde(flatten)]
        base: BaseNavidromeQuery,
        #[serde(flatten)]
        query: T,
    }
    Ok(client
        .get(path)
        .unwrap()
        .query(&Body {
            base: BaseNavidromeQuery::base(),
            query,
        })
        .send()
        .await?
        .error_for_status()?
        .json::<SubsonicResponse<R>>()
        .await?
        .subsonic_response)
}

pub fn client() -> Client {
    Client::new("http://navidrome.pendrellvale.home".parse().unwrap()).unwrap()
}

#[derive(Debug, Deserialize)]
struct SubsonicResponse<T> {
    #[serde(rename = "subsonic-response")]
    subsonic_response: T,
}

#[tracing::instrument(skip(client))]
pub async fn stream(
    client: &Client,
    id: &NavidromeId,
    headers: HeaderMap,
    query: &str,
) -> reqwest::Result<reqwest::Response> {
    #[derive(Serialize)]
    struct NavidromeQuery<'s> {
        #[serde(flatten)]
        base: BaseNavidromeQuery,
        id: &'s str,
    }
    tracing::info!("requesting from navidrome");
    let response = client
        .get(&format!("/rest/stream?{query}"))
        .unwrap()
        .query(&NavidromeQuery {
            base: BaseNavidromeQuery::base(),
            id: id.as_str(),
        })
        .headers(headers)
        .send()
        .await?;

    if let Err(error) = response.error_for_status_ref() {
        let body = response.text().await?;
        tracing::error!(?error, %body, "failed to stream audio");
        return Err(error);
    }
    Ok(response)
}

#[derive(Debug, Deserialize)]
pub struct SongResult {
    pub id: NavidromeId,
    pub title: String,
    pub album: String,
    pub artist: String,
}

pub async fn search(client: &Client, query: &str) -> reqwest::Result<Vec<SongResult>> {
    #[derive(Deserialize)]
    struct SearchResult2 {
        #[serde(rename = "searchResult2")]
        search_result_2: SearchResult,
    }

    #[derive(Deserialize)]
    struct SearchResult {
        song: Option<Vec<SongResult>>,
    }

    let res = api::<_, SearchResult2>(client, "/rest/search2", json!({ "query": query })).await?;

    Ok(res.search_result_2.song.unwrap_or_default())
}

pub async fn get_playlist_tracks(
    client: &Client,
    playlist_id: &NavidromeId,
) -> reqwest::Result<Vec<SongResult>> {
    #[derive(Debug, Deserialize)]
    struct PlaylistResponse {
        playlist: NavidromePlaylist,
    }

    #[derive(Debug, Deserialize)]
    struct NavidromePlaylist {
        #[serde(default)]
        entry: Vec<SongResult>,
    }

    let res = api::<_, PlaylistResponse>(
        client,
        "/rest/getPlaylist",
        json!({ "id": playlist_id.as_str() }),
    )
    .await?;

    Ok(res.playlist.entry)
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("navidrome api error: {0}")]
    Navidrome(String),
}

pub async fn add_to_playlist(
    client: &Client,
    playlist_id: &NavidromeId,
    song_to_add: &NavidromeId,
) -> Result<(), Error> {
    #[derive(Serialize)]
    struct Body<'s> {
        #[serde(rename = "playlistId")]
        playlist_id: &'s str,
        #[serde(rename = "songIdToAdd")]
        song_to_add: &'s str,
    }

    {
        let song = api::<_, serde_json::Value>(
            client,
            "/rest/getSong",
            json!({ "id": song_to_add.as_str() }),
        )
        .await?;
        if song.get("status").is_some_and(|s| s.as_str() != Some("ok")) {
            return Err(Error::Navidrome(song.to_string()));
        }
    }

    let res = api::<_, serde_json::Value>(
        client,
        "/rest/updatePlaylist",
        Body {
            playlist_id: playlist_id.as_str(),
            song_to_add: song_to_add.as_str(),
        },
    )
    .await?;

    if res.get("status").is_some_and(|s| s.as_str() == Some("ok")) {
        Ok(())
    } else {
        Err(Error::Navidrome(res.to_string()))
    }
}

pub async fn remove_from_playlist(
    client: &Client,
    playlist_id: &NavidromeId,
    song_to_remove: &NavidromeId,
) -> Result<bool, Error> {
    #[derive(Serialize)]
    struct Body<'s> {
        #[serde(rename = "playlistId")]
        playlist_id: &'s str,
        #[serde(rename = "songIndexToRemove")]
        song_index_to_remove: usize,
    }

    let playlist = get_playlist_tracks(client, playlist_id).await?;

    let Some(position) = playlist.iter().position(|s| &s.id == song_to_remove) else {
        return Ok(false);
    };

    let res = api::<_, serde_json::Value>(
        client,
        "/rest/updatePlaylist",
        Body {
            playlist_id: playlist_id.as_str(),
            song_index_to_remove: position,
        },
    )
    .await?;

    if res.get("status").is_some_and(|s| s.as_str() == Some("ok")) {
        Ok(true)
    } else {
        Err(Error::Navidrome(res.to_string()))
    }
}

#[derive(Debug, Deserialize)]
pub struct Playlist {
    pub owner: String,
    pub name: String,
    pub id: NavidromeId,
}

pub async fn managed_playlists(client: &Client) -> reqwest::Result<Vec<Playlist>> {
    #[derive(Deserialize)]
    struct Playlists {
        playlists: Playlists2,
    }

    #[derive(Deserialize)]
    struct Playlists2 {
        playlist: Vec<Playlist>,
    }

    let mut result = api::<_, Playlists>(client, "/rest/getPlaylists", json!({})).await?;
    result
        .playlists
        .playlist
        .retain(|p| p.owner == SUBSONIC_USER);
    Ok(result.playlists.playlist)
}

pub struct CreatePlaylist<'s> {
    pub name: &'s str,
    pub comment: &'s str,
    pub public: bool,
}

pub async fn create_playlist(
    client: &Client,
    create_params: CreatePlaylist<'_>,
) -> reqwest::Result<NavidromeId> {
    #[derive(Deserialize)]
    struct CreatePlaylistResponse {
        playlist: Playlist,
    }

    #[derive(Deserialize)]
    struct Playlist {
        id: NavidromeId,
    }

    let playlist = api::<_, CreatePlaylistResponse>(
        client,
        "/rest/createPlaylist",
        json!({ "name": create_params.name }),
    )
    .await?;
    let id = playlist.playlist.id;
    api::<_, serde_json::Value>(
        client,
        "/rest/updatePlaylist",
        json!({
            "playlistId": id.as_str(),
            "comment": create_params.comment,
            "public": create_params.public,
        }),
    )
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::playlist::NavidromeId, net::auth_client::Client};
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;

    fn test_song() -> NavidromeId {
        NavidromeId::try_from("OCKMW6PjDV2TVKyisDYzKL".to_owned()).unwrap()
    }

    fn test_bad_song() -> NavidromeId {
        NavidromeId::try_from("OCxxxxxxxxxxVKyisDYzKL".to_owned()).unwrap()
    }

    #[derive(Debug)]
    struct TestPlaylist {
        id: NavidromeId,
    }

    async fn api<T: Serialize, R: DeserializeOwned>(client: &Client, path: &str, query: T) -> R {
        #[derive(Serialize)]
        struct Body<T> {
            #[serde(flatten)]
            base: BaseNavidromeQuery,
            #[serde(flatten)]
            query: T,
        }
        client
            .get(path)
            .unwrap()
            .query(&Body {
                base: BaseNavidromeQuery::base(),
                query,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<SubsonicResponse<R>>()
            .await
            .unwrap()
            .subsonic_response
    }

    impl Drop for TestPlaylist {
        fn drop(&mut self) {
            let id = self.id.clone();
            std::thread::spawn(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        let client = client();

                        let res = api::<_, serde_json::Value>(
                            &client,
                            "/rest/deletePlaylist",
                            json!({ "id": id.as_str() }),
                        )
                        .await;
                        println!(
                            "delete playlist result: {}",
                            serde_json::to_string_pretty(&res).unwrap()
                        );
                    });
            })
            .join()
            .unwrap()
        }
    }

    async fn create_test_playlist(client: &Client) -> TestPlaylist {
        let id = create_playlist(
            client,
            CreatePlaylist {
                name: "test-playlist",
                comment: "test playlist comment",
                public: true,
            },
        )
        .await
        .unwrap();

        TestPlaylist { id }
    }

    #[tokio::test]
    async fn add_and_remove() {
        let client = client();
        let playlist = create_test_playlist(&client).await;

        let test_song = test_song();

        super::add_to_playlist(&client, &playlist.id, &test_song)
            .await
            .unwrap();

        let tracks = get_playlist_tracks(&client, &playlist.id).await.unwrap();
        assert!(tracks.iter().any(|s| s.id == test_song));
        assert_eq!(tracks.len(), 1);

        let removed = super::remove_from_playlist(&client, &playlist.id, &test_song)
            .await
            .unwrap();
        assert!(removed);

        let tracks = get_playlist_tracks(&client, &playlist.id).await.unwrap();
        assert_eq!(tracks.len(), 0);
    }

    #[tokio::test]
    async fn adding_invalid_id_fails() {
        let client = client();
        let playlist = create_test_playlist(&client).await;

        let test_song = test_bad_song();

        let result = super::add_to_playlist(&client, &playlist.id, &test_song).await;

        assert!(result.is_err());
    }
}

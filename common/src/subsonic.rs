use crate::{domain::playlist::NavidromeId, net::auth_client::Client};
use http::HeaderMap;
use md5::Digest as _;
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize)]
pub struct SongResult {
    pub id: NavidromeId,
    pub title: String,
    pub album: String,
    pub artist: String,
}

pub async fn search(client: &Client, query: &str) -> reqwest::Result<Vec<SongResult>> {
    #[derive(Serialize)]
    struct Body<'s> {
        #[serde(flatten)]
        base: BaseNavidromeQuery,
        query: &'s str,
    }

    #[derive(Deserialize)]
    struct Response {
        #[serde(rename = "subsonic-response")]
        subsonic_response: SubsonicResponse,
    }

    #[derive(Deserialize)]
    struct SubsonicResponse {
        #[serde(rename = "searchResult2")]
        search_result_2: SearchResult,
    }

    #[derive(Deserialize)]
    struct SearchResult {
        song: Vec<SongResult>,
    }

    let res = client
        .get("/rest/search2")
        .unwrap()
        .query(&Body {
            base: BaseNavidromeQuery::base(),
            query,
        })
        .send()
        .await?
        .error_for_status()?
        .json::<Response>()
        .await?;

    Ok(res.subsonic_response.search_result_2.song)
}

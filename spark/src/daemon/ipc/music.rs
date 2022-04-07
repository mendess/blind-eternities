use common::{domain::music::PlayerIdx, net::AuthenticatedClient};
use reqwest::RequestBuilder;
use spark_protocol::{
    music::{MpvMeta, MusicCmd, MusicCmdKind, PlayerRef},
    ErrorResponse, Response,
};
use std::future::{self, Future};

pub async fn local(MusicCmd { index, command }: MusicCmd<'_>) -> Result<Response, ErrorResponse> {
    let map_err = |e: mlib::Error| match e {
        mlib::Error::Io(e) => ErrorResponse::IoError(e.to_string()),
        mlib::Error::NoMpvInstance => {
            ErrorResponse::ForwardedError(format!("no mpv instance at {index}"))
        }
        _ => ErrorResponse::ForwardedError(e.to_string()),
    };
    let mut socket = mlib::socket::local::LocalMpvSocket::by_index(index)
        .await
        .map_err(map_err)?;
    match command {
        MusicCmdKind::Fire(msg) => socket
            .fire(msg)
            .await
            .map(|_| Response::Unit)
            .map_err(|e| ErrorResponse::IoError(e.to_string())),
        MusicCmdKind::Compute(cmd) => socket
            .compute_raw::<serde_json::Value, _>(cmd)
            .await
            .map_err(map_err)
            .and_then(|c| {
                serde_json::to_value(&c)
                    .map_err(|e| ErrorResponse::DeserializingCommand(e.to_string()))
            })
            .map(Response::ForwardValue),
        MusicCmdKind::Execute(msg) => socket
            .fire(msg)
            .await
            .map(|_| Response::Unit)
            .map_err(|e| ErrorResponse::IoError(e.to_string())),
        MusicCmdKind::Observe(_) => todo!("observer not implemented yet"),
    }
}

pub async fn backend(
    mpv_meta: MpvMeta<'_>,
    client: &AuthenticatedClient,
) -> Result<Response, ErrorResponse> {
    use common::net::auth_client;

    return match mpv_meta {
        MpvMeta::LastFetch(player) => get(client, &(url_from_ref(&player) + "/last")).await,
        MpvMeta::LastReset(player) => {
            set(|| client.delete(&(url_from_ref(&player) + "/last"))).await
        }
        MpvMeta::LastSet(n, player) => {
            set(|| {
                client
                    .post(&(url_from_ref(&player) + "/last"))
                    .map(|r| r.json(&n))
            })
            .await
        }
        MpvMeta::CreatePlayer(index) => {
            set(|| client.post(&url_from_ref(&ref_for_localhost(index)))).await
        }
        MpvMeta::DeletePlayer(index) => {
            set(|| client.delete(&url_from_ref(&ref_for_localhost(index)))).await
        }
        MpvMeta::SetCurrentPlayer(index) => {
            set(|| client.patch(&url_from_ref(&ref_for_localhost(index)))).await
        }
        MpvMeta::ListPlayers => get(client, "music/player").await,
        MpvMeta::GetCurrentPlayer => get(client, "music/player/current").await,
    };

    fn url_from_ref(PlayerRef { machine, index }: &PlayerRef<'_>) -> String {
        format!("music/player/{machine}/{index}")
    }

    fn ref_for_localhost(index: PlayerIdx) -> PlayerRef<'static> {
        PlayerRef {
            machine: whoami::hostname().into(),
            index,
        }
    }

    // String::from("music/player/current")
    async fn handle_response<F, Fut>(
        response: reqwest::Response,
        f: F,
    ) -> Result<Response, ErrorResponse>
    where
        F: FnOnce(reqwest::Response) -> Fut,
        Fut: Future<Output = Result<Response, ErrorResponse>>,
    {
        if response.status().is_success() {
            f(response).await
        } else {
            Err(ErrorResponse::HttpError {
                status: response.status().as_u16(),
                message: response
                    .bytes()
                    .await
                    .map_err(|e| ErrorResponse::DeserializingResponse(e.to_string()))?
                    .to_vec(),
            })
        }
    }

    fn map_err(e: reqwest::Error) -> ErrorResponse {
        ErrorResponse::NetworkError(e.to_string())
    }

    async fn get(client: &AuthenticatedClient, url: &str) -> Result<Response, ErrorResponse> {
        handle_response(
            client
                .get(url)
                .expect("correct url")
                .send()
                .await
                .map_err(map_err)?,
            |response| async move {
                response
                    .json()
                    .await
                    .map_err(|e| ErrorResponse::DeserializingResponse(e.to_string()))
                    .map(Response::ForwardValue)
            },
        )
        .await
    }

    async fn set<M>(method: M) -> Result<Response, ErrorResponse>
    where
        M: FnOnce() -> auth_client::Result<RequestBuilder>,
    {
        handle_response(
            method()
                .expect("correct url")
                .send()
                .await
                .map_err(map_err)?,
            |_| future::ready(Ok(Response::Unit)),
        )
        .await
    }
}

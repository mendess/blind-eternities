use common::{domain::music::PlayerIdx, net::AuthenticatedClient};
use mlib::socket::local::LocalMpvSocket;
use reqwest::RequestBuilder;
use serde::Serialize;
use spark_protocol::{
    music::{LocalMetadata, MpvMeta, MusicCmd, MusicCmdKind},
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

    let unit = |r: Result<(), mlib::Error>| r.map(|_| Response::Unit).map_err(map_err);

    fn value<T: Serialize>(r: Result<T, ErrorResponse>) -> Result<Response, ErrorResponse> {
        r.and_then(|c| {
            serde_json::to_value(&c)
                .map_err(|e| ErrorResponse::DeserializingResponse(e.to_string()))
        })
        .map(Response::ForwardValue)
    }

    let mut socket = LocalMpvSocket::by_index(index).await.map_err(map_err)?;

    match command {
        MusicCmdKind::Fire(msg) => unit(socket.fire(msg).await.map_err(Into::into)),
        MusicCmdKind::Compute(cmd) => value(
            socket
                .compute_raw::<serde_json::Value, _>(cmd)
                .await
                .map_err(map_err),
        ),
        MusicCmdKind::Execute(msg) => unit(socket.fire(msg).await.map_err(Into::into)),
        MusicCmdKind::Observe(_) => todo!("observer not implemented yet"),
        MusicCmdKind::Meta(command) => {
            let last = socket.last();
            match command {
                LocalMetadata::LastFetch => value(last.fetch().await.map_err(map_err)),
                LocalMetadata::LastReset => unit(last.reset().await),
                LocalMetadata::LastSet(n) => unit(last.set(n).await),
            }
        }
    }
}

pub async fn backend(
    mpv_meta: MpvMeta<'_>,
    client: &AuthenticatedClient,
) -> Result<Response, ErrorResponse> {
    use common::net::auth_client;

    return match mpv_meta {
        MpvMeta::CreatePlayer(index) => set(|| client.post(&url_from_ref(index))).await,
        MpvMeta::DeletePlayer(index) => set(|| client.delete(&url_from_ref(index))).await,
        MpvMeta::SetCurrentPlayer(index) => set(|| client.patch(&url_from_ref(index))).await,
        MpvMeta::ListPlayers => get(client, "music/player").await,
        MpvMeta::GetCurrentPlayer => get(client, "music/player/current").await,
        MpvMeta::_Unused(_) => unreachable!("unused"),
    };

    fn url_from_ref(index: PlayerIdx) -> String {
        format!("music/player/{}/{index}", whoami::hostname())
    }

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

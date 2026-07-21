use axum::{
    body::Body,
    response::{AppendHeaders, IntoResponse, Response},
};
use http::{HeaderValue, header};
use std::{io, time::SystemTime};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

pub fn reqwest_to_axum(mut response: reqwest::Response) -> io::Result<Response> {
    let mut response_builder = Response::builder().status(response.status());
    *response_builder.headers_mut().unwrap() = std::mem::take(response.headers_mut());
    response_builder
        .body(Body::from_stream(response.bytes_stream()))
        .map_err(io::Error::other)
}

pub async fn named_file<P>(path: P) -> io::Result<impl IntoResponse + use<P>>
where
    P: AsRef<std::path::Path>,
{
    let path = path.as_ref();
    let file = File::open(path).await?;
    let meta = file.metadata().await?;
    let len = meta.len();
    let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let filename = path.file_name().unwrap().to_string_lossy();

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let headers = AppendHeaders([
        (
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref()).unwrap(),
        ),
        (
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&len.to_string()).unwrap(),
        ),
        // (
        //     header::ACCEPT_RANGES,
        //     const { HeaderValue::from_static("bytes") },
        // ),
        (
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("inline; filename=\"{}\"", filename)).unwrap(),
        ),
        (
            header::LAST_MODIFIED,
            HeaderValue::from_str(&httpdate::fmt_http_date(modified)).unwrap(),
        ),
        (
            header::DATE,
            HeaderValue::from_str(&httpdate::fmt_http_date(SystemTime::now())).unwrap(),
        ),
        (header::ETAG, {
            let dur = modified.duration_since(SystemTime::UNIX_EPOCH).unwrap();
            // Simple ETag: "<len>:<modified>"
            let etag = format!("\"{:x}:{:x}\"", len, dur.as_secs());
            HeaderValue::from_str(&etag).unwrap()
        }),
    ]);

    Ok((headers, body))
}

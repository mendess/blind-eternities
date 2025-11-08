use axum::{
    body::Body,
    response::{AppendHeaders, IntoResponse},
};
use futures::{Stream, StreamExt as _};
use http::{HeaderValue, header};
use httpdate::fmt_http_date;
use rand::Rng as _;
use std::{
    future, io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::fs::{self, File};
use tokio_stream::wrappers::ReadDirStream;
use tokio_util::io::ReaderStream;

pub async fn random_file(dir: impl AsRef<Path>) -> io::Result<fs::DirEntry> {
    let mut walls = ReadDirStream::new(fs::read_dir(dir.as_ref()).await?)
        .filter_map(|x| future::ready(x.ok()))
        .collect::<Vec<_>>()
        .await;
    if walls.is_empty() {
        Err(io::ErrorKind::NotFound.into())
    } else {
        let index = rand::rng().random_range(0..walls.len());
        Ok(walls.swap_remove(index))
    }
}

pub async fn list_files_at(dir: impl AsRef<Path>) -> io::Result<impl Stream<Item = PathBuf>> {
    Ok(ReadDirStream::new(fs::read_dir(dir.as_ref()).await?)
        .filter_map(|x| future::ready(x.ok()))
        .map(|p| PathBuf::from(p.file_name()))
        .filter(|p| future::ready(p != Path::new(".keep"))))
}

pub async fn named_file(path: &std::path::Path) -> io::Result<impl IntoResponse + use<>> {
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
            HeaderValue::from_str(&fmt_http_date(modified)).unwrap(),
        ),
        (
            header::DATE,
            HeaderValue::from_str(&fmt_http_date(SystemTime::now())).unwrap(),
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

use futures::{Stream, StreamExt as _};
use rand::Rng as _;
use std::{
    future, io,
    path::{Path, PathBuf},
};
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

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

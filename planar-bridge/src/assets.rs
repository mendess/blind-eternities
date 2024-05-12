use actix_files::NamedFile;
use actix_web::web::{self, get};
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn routes() -> actix_web::Scope {
    web::scope("/assets").route("/{path}", get().to(file))
}

async fn file(path: web::Path<PathBuf>) -> io::Result<NamedFile> {
    println!("file: {path:?}");
    NamedFile::open(dbg!(Path::new("planar-bridge/assets").join(&*path)))
}

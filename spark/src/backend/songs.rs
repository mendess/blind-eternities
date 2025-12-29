use anyhow::{Context, bail};
use common::{
    domain::playlist::{NavidromeId, SONG_META_HEADER, SongId, SongMetadata},
    net::{AuthenticatedClient, auth_client::Client},
    subsonic::{self, SongResult},
};
use lofty::{file::TaggedFileExt as _, picture::Picture, probe::Probe};
use mlib::{item::link::HasId as _, playlist::PartialSearchResult};
use reqwest::header::{self, HeaderValue};
use serde::Deserialize;
use std::{
    io::Write as _,
    ops::Not,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs::File, process::Command};

pub fn pick<T: std::fmt::Display>(items: &[T]) -> Option<&T> {
    println!("Multiple matches found:");
    for (i, item) in items.iter().enumerate() {
        println!(" - [{i}] {item}");
    }

    let index: usize = loop {
        eprint!("Pick: ");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf).ok()?;
        let buf = buf.trim();
        if buf.is_empty() {
            return None;
        }
        match buf.parse() {
            Ok(i) => break i,
            Err(_) => {
                eprintln!("Invalid index")
            }
        }
    };

    Some(&items[index])
}

#[tracing::instrument(skip(client))]
pub async fn upgrade_song(
    client: AuthenticatedClient,
    title: String,
    strict: bool,
) -> anyhow::Result<()> {
    let playlist = mlib::playlist::Playlist::load().await?;
    let id = {
        tracing::info!("searching for song in playlist");
        match playlist.partial_name_search(title.split_whitespace()) {
            PartialSearchResult::None => {
                bail!("no song matches {title:?}")
            }
            PartialSearchResult::One(s) => s,
            PartialSearchResult::Many(items) => {
                let Some(pick) = pick(&items) else {
                    return Ok(());
                };
                let PartialSearchResult::One(song) =
                    playlist.partial_name_search([pick.as_str()].into_iter())
                else {
                    panic!("this should always result in one result")
                };

                song
            }
        }
    };

    if strict {
        #[derive(Deserialize)]
        struct NavId {
            navidrome_id: Option<NavidromeId>,
        }
        let NavId { navidrome_id } = client
            .get(&format!(
                "/playlist/song/metadata/{}",
                id.link.id().as_str()
            ))?
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if navidrome_id.is_some() {
            bail!("already upgraded, skipping");
        }
    }

    let nav_id = {
        tracing::info!("searching for song in navidrome");
        let client = Client::new("http://192.168.42.2:4533".parse().unwrap())?;

        let mut songs = subsonic::search(&client, &title).await?;
        match songs.len() {
            0 => bail!("song '{title}' not in navidrome"),
            1 => songs.remove(0).id,
            _ => {
                struct DisplaySong(SongResult);
                impl std::fmt::Display for DisplaySong {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "{} - {} - {}", self.0.title, self.0.album, self.0.artist)
                    }
                }
                let songs = songs.into_iter().map(DisplaySong).collect::<Vec<_>>();
                let Some(song) = pick(&songs) else {
                    return Ok(());
                };
                song.0.id.clone()
            }
        }
    };

    tracing::info!(?nav_id, id = ?id.link.id(), "upgrading song");

    client
        .patch(&format!(
            "/playlist/song/navidrome/{}/{}",
            id.link.id().as_str(),
            nav_id.as_str(),
        ))?
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

#[tracing::instrument(skip(client))]
pub async fn add_song(
    client: AuthenticatedClient,
    title: String,
    artist: Option<String>,
    uri: String,
    thumb: Option<PathBuf>,
) -> anyhow::Result<()> {
    tracing::info!("adding a new song");
    if uri.contains("http") {
        let path = dl_song(uri).await?;
        match add_song_file(client, title, artist, &path, thumb).await {
            Ok(()) => tokio::fs::remove_file(path)
                .await
                .context("removing downloaded song file"),
            Err(e) => return Err(e),
        }
    } else {
        add_song_file(client, title, artist, Path::new(&uri), thumb).await
    }
}

#[tracing::instrument]
pub async fn dl_song(uri: String) -> anyhow::Result<PathBuf> {
    tracing::info!("downloading");
    let proxy = async {
        let response = reqwest::get("http://10.0.0.1:25500")
            .await
            .context("failed to connect to yt-dlp proxy")?
            .error_for_status()
            .context("request error")?;

        let region = response
            .headers()
            .get("x-region")
            .and_then(|r| r.to_str().ok())
            .map(str::to_owned);

        let port = response
            .text()
            .await
            .context("failed to parse body of request")?
            .parse::<u16>()
            .context("failed to parse port")?;

        anyhow::Ok((region, format!("http://10.0.0.1:{port}")))
    }
    .await;

    let (region, proxy) = match proxy {
        Err(e) => {
            tracing::warn!(error = ?e, "failed to init proxy");
            (None, None)
        }
        Ok((region, proxy)) => (region, Some(proxy)),
    };

    tracing::info!(?region, ?proxy, ?uri, "launching yt-dlp");
    let mut yt_dlp = Command::new("yt-dlp");
    yt_dlp
        .arg(uri)
        .args(["--print", "after_move:filename"])
        .arg("--embed-thumbnail")
        .arg("--embed-metadata");
    if let Some(proxy) = proxy {
        yt_dlp.args(["--proxy", &proxy]);
    }
    let output = yt_dlp.output().await.context("waiting for yt-dlp failed")?;

    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    anyhow::ensure!(output.status.success(), "yt-dlp returned a non 0 exit code");

    let mut file_name = String::from_utf8(output.stdout).context("file name was not utf8")?;
    while file_name.ends_with('\n') {
        file_name.pop();
    }

    let mut file_name = PathBuf::from(file_name);

    // yt-dlp sometimes lies about the file extension.
    if !tokio::fs::try_exists(&file_name).await? {
        let mut dir = tokio::fs::read_dir(".").await?;
        let file_stem = file_name.file_stem().unwrap();
        while let Some(f) = dir.next_entry().await? {
            let path = f.path();
            if path
                .file_name()
                .unwrap()
                .as_bytes()
                .starts_with(file_stem.as_bytes())
            {
                file_name = path;
                break;
            }
        }
    }

    Ok(file_name)
}

#[tracing::instrument(skip(client))]
pub async fn add_song_file(
    client: AuthenticatedClient,
    title: String,
    artist: Option<String>,
    path: &Path,
    thumb: Option<PathBuf>,
) -> anyhow::Result<()> {
    let duration = audio_duration(path).await?;
    let _tmp_thumb;
    let thumb = match &thumb {
        Some(t) => t,
        None => {
            _tmp_thumb = tempfile::Builder::new().suffix(".jpg").tempfile()?;
            extract_thumbnail(path, _tmp_thumb.path()).await?;
            _tmp_thumb.path()
        }
    };
    let audio = tempfile::Builder::new().tempfile()?;
    extract_audio_and_embed_thumb_cli(path, audio.path(), title.clone(), artist.as_deref(), thumb)
        .await?;
    let audio_path = audio.into_temp_path();
    tracing::info!("uploading song");
    let response = client
        .post("/playlist/song/audio")?
        .header(
            SONG_META_HEADER,
            HeaderValue::from_bytes(
                &serde_json::to_vec(&SongMetadata { title, duration }).unwrap(),
            )?,
        )
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("audio/x-matroska"),
        )
        .body(File::open(&audio_path).await?)
        .send()
        .await?;

    if let Err(error) = response.error_for_status_ref() {
        for (k, v) in response.headers() {
            eprintln!("{k}: {}", v.to_str().unwrap());
        }
        return Err(error).context(response.text().await?);
    };

    let id: SongId = response.json().await?;

    let response = client
        .post(&format!("/playlist/song/thumb/{}", id.as_str()))?
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(mime_guess::mime::IMAGE_JPEG.as_ref()),
        )
        .body(File::open(thumb).await?)
        .send()
        .await?;
    if let Err(error) = response.error_for_status_ref() {
        for (k, v) in response.headers() {
            eprintln!("{k}: {}", v.to_str().unwrap());
        }
        return Err(error).context(response.text().await?);
    };

    let song_uri = client.hostname().join("/playlist/song/audio/")?.join(&id)?;

    println!("{song_uri}");

    let mut m = Command::new("m");
    m.arg("new").arg(song_uri.as_str());
    if let Some(artist) = artist {
        m.args(["--artist", &artist]);
    }
    let mut proc = m.spawn()?;
    let status = proc.wait().await?;
    if !status.success() {
        tracing::error!("failed to spawn `m`");
    }

    Ok(())
}

#[tracing::instrument]
async fn audio_duration(path: &Path) -> anyhow::Result<Duration> {
    tracing::info!("getting audio duration");
    let output = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap(),
        ])
        .output()
        .await?;

    anyhow::ensure!(
        output.status.success(),
        "ffprobe failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout)?;
    let secs: f64 = stdout.trim().parse()?;
    Ok(Duration::from_secs_f64(secs))
}

#[tracing::instrument]
pub async fn extract_thumbnail(input: &Path, output: &Path) -> anyhow::Result<()> {
    tracing::info!("extracting thumbnail");
    // --- 1. Try with `lofty` first for audio files ---
    // This is generally faster and more reliable for audio cover art.
    if let Ok(tagged_file) = Probe::open(input)?.read()
        && let Some(picture) = tagged_file.primary_tag().and_then(|t| t.pictures().first())
    {
        return save_lofty_picture(picture, output);
    }

    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .args(["-ss", "7"]) // fast seek to 7 seconds
        .arg("-i")
        .arg(input)
        .args(["-frames:v", "1"]) // grab a single frame
        .args(["-f", "image2"]) // force image muxer
        .args(["-pix_fmt", "rgb24"]) // RGB24 pixel format
        .arg("-y")
        .arg(output)
        .status()
        .await?; // run command and wait for exit

    if !status.success() {
        anyhow::bail!("ffmpeg failed with status: {:?}", status.code());
    }

    Ok(())
}

/// Helper to save a `lofty::Picture` to a file.
fn save_lofty_picture(picture: &Picture, output: &Path) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create output file at {}", output.display()))?;
    file.write_all(picture.data())
        .with_context(|| format!("Failed to write image data to {}", output.display()))?;
    Ok(())
}

#[tracing::instrument]
pub async fn extract_audio_and_embed_thumb_cli(
    input: &Path,
    output: &Path,
    title: String,
    artist: Option<&str>,
    thumb: &Path,
) -> anyhow::Result<()> {
    tracing::info!("extracting audio");
    fn o(s: &str) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(s)
    }
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.arg("-hide_banner");
    ffmpeg.args([o("-i"), input.as_os_str()]);
    ffmpeg.args([o("-i"), thumb.as_os_str()]);
    ffmpeg.args(["-map", "0:a:0", "-map", "1:v:0"]);
    ffmpeg.args(["-c:a", "copy"]);
    ffmpeg.args(["-c:v", "mjpeg"]);
    ffmpeg.args(["-disposition:v:0", "attached_pic"]);
    ffmpeg.args(["-metadata", &format!("title={title}")]);
    if let Some(artist) = artist {
        ffmpeg.args(["-metadata", &format!("artist={artist}")]);
    }
    let output_file = output.with_extension("mka");
    ffmpeg.args([&output_file]);
    let ffmpeg_output = ffmpeg.output().await?;
    if ffmpeg_output.status.success().not() {
        print!("failed to run ffmpeg");
        for a in ffmpeg.as_std().get_args() {
            print!("'{}' ", a.to_str().unwrap());
        }
        println!();
        println!(
            "stdout:\n{}",
            String::from_utf8_lossy(&ffmpeg_output.stdout)
        );
        println!(
            "stderr:\n{}",
            String::from_utf8_lossy(&ffmpeg_output.stderr)
        );
        anyhow::bail!("failed to encode {ffmpeg_output:?}");
    }
    tokio::fs::rename(output_file, output).await?;
    Ok(())
}

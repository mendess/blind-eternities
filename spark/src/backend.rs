use crate::config::Config;
use anyhow::Context;
use chrono::Utc;
use common::{
    domain::{Hostname, music_session::ExpiresAt},
    net::AuthenticatedClient,
    playlist::{SONG_META_HEADER, SongId, SongMetadata},
};
use ffmpeg_next::{self as ffmpeg, Dictionary};
use lofty::{file::TaggedFileExt as _, picture::Picture, probe::Probe};
use reqwest::header::{self, HeaderValue};
use std::{
    io::Write as _,
    ops::Not,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs::File, process::Command};

pub(super) async fn handle(cmd: super::Backend, config: Config) -> anyhow::Result<()> {
    let client = AuthenticatedClient::try_from(&config)?;
    match cmd {
        crate::Backend::Persistents => display_persistent_connections(client).await?,
        crate::Backend::CreateMusicSession {
            hostname,
            expire_in,
            show_link,
        } => create_music_session(client, hostname, expire_in, show_link).await?,
        crate::Backend::DeleteMusicSession { session } => {
            delete_music_session(client, session).await?
        }
        crate::Backend::AddSong {
            title,
            artist,
            path,
            thumb,
        } => {
            add_song_file(client, title, artist, path, thumb).await?;
        }
    }
    Ok(())
}

async fn display_persistent_connections(client: AuthenticatedClient) -> anyhow::Result<()> {
    let conns: Vec<Hostname> = client
        .get("/persistent-connections/ws")?
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    println!("connected hosts are:");
    for c in conns {
        println!("- {c}");
    }

    Ok(())
}

async fn create_music_session(
    client: AuthenticatedClient,
    hostname: Hostname,
    expire_in: Option<Duration>,
    show_link: bool,
) -> anyhow::Result<()> {
    let token = client
        .get(&format!("/admin/music-session/{hostname}"))?
        .query(&ExpiresAt {
            expires_at: expire_in.map(|d| Utc::now() + d),
        })
        .send()
        .await?
        .error_for_status()?
        .json::<String>()
        .await?;

    if show_link {
        println!("url: https://planar-bridge.mendess.xyz/music?session={token}");
    } else {
        println!("session id is: {token}");
    }
    Ok(())
}

async fn delete_music_session(client: AuthenticatedClient, session: String) -> anyhow::Result<()> {
    client
        .delete(&format!("/admin/music-session/{session}"))?
        .send()
        .await?
        .error_for_status()?;

    println!("session deleted");
    Ok(())
}

async fn add_song_file(
    client: AuthenticatedClient,
    title: String,
    artist: Option<String>,
    path: PathBuf,
    thumb: Option<PathBuf>,
) -> anyhow::Result<()> {
    ffmpeg_next::init()?; // initialize FFmpeg

    let duration = audio_duration(&path).await?;
    let _tmp_thumb;
    let thumb = match &thumb {
        Some(t) => t,
        None => {
            _tmp_thumb = tempfile::Builder::new().suffix(".jpg").tempfile()?;
            extract_thumbnail(&path, _tmp_thumb.path())?;
            _tmp_thumb.path()
        }
    };
    let audio = tempfile::Builder::new().tempfile()?;
    extract_audio_and_embed_thumb_cli(&path, audio.path(), title.clone(), artist, thumb).await?;
    let audio_path = audio.into_temp_path();
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

    Ok(())
}
async fn audio_duration(path: &Path) -> anyhow::Result<Duration> {
    let output = Command::new("ffprobe")
        .args([
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

    anyhow::ensure!(output.status.success(), "ffprobe failed");

    let stdout = String::from_utf8(output.stdout)?;
    let secs: f64 = stdout.trim().parse()?;
    Ok(Duration::from_secs_f64(secs))
}

pub fn extract_thumbnail(input: &Path, output: &Path) -> anyhow::Result<()> {
    // --- 1. Try with `lofty` first for audio files ---
    // This is generally faster and more reliable for audio cover art.
    if let Ok(tagged_file) = Probe::open(input)?.read()
        && let Some(picture) = tagged_file.primary_tag().and_then(|t| t.pictures().first())
    {
        return save_lofty_picture(picture, output);
    }

    let mut ictx = ffmpeg_next::format::input(input)
        .context("the provided file does not appear to be a supported media file")?;
    let input = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .context("No video stream could be found in the file.")?;
    let input_parameters = input.parameters();

    let video_stream_index = input.index();
    let time_base = input.time_base();

    // --- SEEK TO 3 SECONDS ---
    // Calculate the timestamp for 3 seconds based on the video's time base.
    const TARGET_SECS: i64 = 7;
    let target_ts = TARGET_SECS * time_base.denominator() as i64 / time_base.numerator() as i64;

    // Seek to the nearest keyframe before our target timestamp.
    ictx.seek(target_ts, ..)
        .context("Failed to seek to the 3-second mark. The video might be too short.")?;
    // --- END SEEK ---

    let context_decoder = ffmpeg_next::codec::context::Context::from_parameters(input_parameters)?;
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = ffmpeg_next::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg_next::format::Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
    )?;

    let mut rgb_frame = ffmpeg_next::frame::Video::empty();
    let mut decoded_frame = ffmpeg_next::frame::Video::empty();

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                // We need to check if the frame's timestamp is at or after our target.
                if let Some(pts) = decoded_frame.timestamp()
                    && pts >= target_ts
                {
                    // This is the frame we want. Scale it, save it, and we are done.
                    scaler.run(&decoded_frame, &mut rgb_frame)?;
                    save_ffmpeg_frame(&rgb_frame, output)?;
                    return Ok(());
                }
            }
        }
    }
    anyhow::bail!("Failed to receive a decoded frame from the video.");
}

/// Helper to save a `lofty::Picture` to a file.
fn save_lofty_picture(picture: &Picture, output: &Path) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create output file at {}", output.display()))?;
    file.write_all(picture.data())
        .with_context(|| format!("Failed to write image data to {}", output.display()))?;
    Ok(())
}

fn save_ffmpeg_frame(frame: &ffmpeg_next::frame::Video, path: &Path) -> anyhow::Result<()> {
    let width = frame.width();
    let height = frame.height();
    let data = frame.data(0);
    let stride = frame.stride(0);

    let bytes_per_row = width as usize * 3; // For Rgb8 (3 bytes per pixel)

    // If the stride (bytes per line) is the same as the width * bytes_per_pixel,
    // the data is tightly packed and can be saved directly.
    let data = if stride == bytes_per_row {
        std::borrow::Cow::Borrowed(data)
    } else {
        // If the stride is larger, the frame has padding. We must copy the data
        // row by row into a new, tightly-packed buffer to remove the padding.
        let mut tightly_packed_data = Vec::with_capacity((height as usize) * bytes_per_row);
        for y in 0..height as usize {
            let start = y * stride;
            let end = start + bytes_per_row;
            tightly_packed_data.extend_from_slice(&data[start..end]);
        }

        std::borrow::Cow::Owned(tightly_packed_data)
    };
    image::save_buffer_with_format(
        path,
        &data,
        width,
        height,
        image::ColorType::Rgb8,
        image::ImageFormat::Jpeg,
    )
    .with_context(|| format!("Failed to save frame to {}", path.display()))
}

pub async fn extract_audio_and_embed_thumb_cli(
    input: &Path,
    output: &Path,
    title: String,
    artist: Option<String>,
    thumb: &Path,
) -> anyhow::Result<()> {
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

pub fn extract_audio_and_embed_thumb(
    input: &Path,
    output: &Path,
    title: String,
    artist: Option<String>,
    thumb: &Path,
) -> anyhow::Result<()> {
    eprintln!(
        "extracting audio and embedding thumbnail. input={input:?}, output={output:?}, title={title}, artist={artist:?}, thumb={thumb:?}"
    );
    // --- Part 1: Setup Inputs and Output ---

    // Open the primary media input file.
    let mut ictx_media = ffmpeg::format::input(&input)
        .with_context(|| format!("Failed to open input file: {:?}", input))?;

    // Open the thumbnail image file as a second input.
    let mut ictx_thumb = ffmpeg::format::input(&thumb)
        .with_context(|| format!("Failed to open thumbnail file: {:?}", thumb))?;

    // Find the best audio stream in the primary input.
    let in_audio_stream = ictx_media
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .context("No audio stream found in input file")?;
    let in_audio_stream_index = in_audio_stream.index();

    // Find the video stream in the thumbnail input (it will be the only one).
    let in_thumb_stream = ictx_thumb
        .streams()
        .best(ffmpeg::media::Type::Video)
        .context("No video stream found in thumbnail file")?;

    // Always use the .mka extension for Matroska audio.
    let output_path = output.with_extension("mka");

    // Let FFmpeg infer the Matroska muxer from the file extension.
    let mut octx = ffmpeg::format::output(&output_path).context("opening output path")?;

    // --- Part 2: Configure Output Streams and Metadata ---

    // Add and configure the audio output stream (index 0).
    let mut out_audio_stream = octx
        .add_stream(ffmpeg::codec::Id::None)
        .context("adding audio stream")?;

    let in_audio_params = in_audio_stream.parameters();
    if in_audio_params.id() == ffmpeg::codec::Id::AAC {
        let mut out_params = in_audio_params;
        unsafe {
            let ptr = out_params.as_mut_ptr();
            // codecpar is a pointer to the AVCodecParameters struct for this stream
            (*ptr).codec_tag = 0;
        }
        out_audio_stream.set_parameters(out_params);
    } else {
        out_audio_stream.set_parameters(in_audio_params);
    }
    out_audio_stream.set_time_base(in_audio_stream.time_base());

    // Add and configure the thumbnail video output stream (index 1).
    let mut out_thumb_stream = octx
        .add_stream(ffmpeg::codec::Id::None)
        .context("adding thumb stream")?;
    out_thumb_stream.set_parameters(in_thumb_stream.parameters());

    // Set the disposition flag directly on the underlying C struct. This tells
    // players that this video stream is an attached picture and not meant for playback.
    unsafe {
        let ptr = out_thumb_stream.as_mut_ptr();
        (*ptr).disposition = ffmpeg::ffi::AV_DISPOSITION_ATTACHED_PIC;
    }

    let thumb_extension = thumb.extension().unwrap().to_str().unwrap();
    let mimetype = match thumb_extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => "application/octet-stream", // Fallback
    };
    let mut thumb_metadata = out_thumb_stream.metadata().to_owned();
    thumb_metadata.set("mimetype", mimetype);
    thumb_metadata.set("filename", "cover.jpg");
    out_thumb_stream.set_metadata(thumb_metadata);

    // Add global metadata to the output file.
    let mut metadata = octx.metadata().to_owned();
    metadata.set("title", &title);
    if let Some(artist) = artist {
        metadata.set("artist", &artist);
    }
    octx.set_metadata(metadata);

    // --- Part 3: Write File ---

    octx.write_header_with(Dictionary::from_iter([
        ("reserve_index_space", "65536"),
        ("write_index", "1"),
    ]))
    .context("writing header")?;

    // Get the final time bases after the header is written.
    let out_audio_time_base = octx.stream(0).unwrap().time_base();
    let out_thumb_time_base = octx.stream(1).unwrap().time_base();

    // First, write the single packet for the thumbnail image.
    if let Some((s, mut packet)) = ictx_thumb.packets().next() {
        packet.set_stream(1); // Mapped to output stream 1
        packet.rescale_ts(s.time_base(), out_thumb_time_base);
        packet
            .write_interleaved(&mut octx)
            .context("writing thumbnail packet")?;
    }

    // Then, loop through the primary input and copy all audio packets.
    for (s, mut packet) in ictx_media.packets() {
        if s.index() == in_audio_stream_index {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
            packet.set_stream(0); // Mapped to output stream 0
            packet.rescale_ts(s.time_base(), out_audio_time_base);
            packet
                .write_interleaved(&mut octx)
                .context("writing audio packet")?;
        }
    }

    octx.write_trailer().context("writing trailer")?;

    std::fs::rename(output_path, output)?;

    Ok(())
}

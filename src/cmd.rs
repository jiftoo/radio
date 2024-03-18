use std::{
	path::{Path, PathBuf},
	process::Stdio,
};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

pub fn check_executables() -> (bool, Vec<(String, bool)>) {
	let info = ["ffmpeg", "ffprobe"]
		.into_iter()
		.map(|x| {
			(x.to_string(), { std::process::Command::new(x).spawn().map(|mut x| x.kill()).is_ok() })
		})
		.collect::<Vec<_>>();
	(info.iter().all(|x| x.1), info)
}

#[allow(clippy::option_if_let_else)]
pub fn spawn_ffmpeg(
	input: &Path,
	sweeper: Option<&Path>,
	bitrate_bps: u32,
	copy_codec: bool,
) -> tokio::process::Child {
	if let Some(sweeper) = sweeper {
		build_with_sweeper(input, sweeper, bitrate_bps)
	} else {
		build_without_sweeper(input, bitrate_bps, copy_codec)
	}
	.spawn()
	.unwrap()
}

fn build_without_sweeper(input: &Path, bitrate_bps: u32, copy_codec: bool) -> Command {
	build_opus(input, bitrate_bps, copy_codec)
}

fn build_mp3(input: &Path, bitrate_bps: u32, copy_codec: bool) -> Command {
	let mut cmd = Command::new("ffmpeg");
	cmd.args(["-hide_banner", "-loglevel", "fatal"])
		.args(["-re", "-threads", "1", "-i"])
		.arg(input);
	if copy_codec {
		cmd.args(["-c:a", "copy"]);
	} else {
		cmd.args(["-c:a", "mp3", "-b:a", &bitrate_bps.to_string()]);
	}
	cmd.args([
		"-write_xing",
		"0",
		"-id3v2_version",
		"0",
		"-map_metadata",
		"-1",
		"-vn",
		// this speeds up encoding a little for some reason
		"-map",
		"0:a",
		"-f",
		"mp3",
		"-",
	])
	.stdout(Stdio::piped())
	.stderr(Stdio::piped())
	.stdin(Stdio::null());
	cmd
}

// https://wiki.xiph.org/OggOpus
// To stream ogg I'll need to send the metadata to the client as he connects.
// The metadata is the OggS packet which, the followign OpusHead and OpusTags packets.
// Hopefully I'll be able to send this only once whenever someone connects,
// and then just stream normally.

fn build_opus(input: &Path, bitrate_bps: u32, copy_codec: bool) -> Command {
	let mut cmd = Command::new("ffmpeg");
	cmd.args(["-hide_banner", "-loglevel", "fatal"])
		.args(["-re", "-threads", "1", "-i"])
		.arg(input);
	if copy_codec {
		cmd.args(["-c:a", "copy"]);
	} else {
		cmd.args(["-c:a", "libopus", "-b:a", &bitrate_bps.to_string()]);
	}
	cmd.args([
		// "-map_metadata",
		// "-1",
		"-vn",
		"-map",
		"0:a",
		"-f",
		"ogg",
		"-",
	])
	.stdout(Stdio::piped())
	.stderr(Stdio::piped())
	.stdin(Stdio::null());
	cmd
}

// temporarily permanent i think
pub const SWEEPER_DIR: &str = "./sweepers";

pub fn build_with_sweeper(
	input: impl AsRef<Path>,
	sweeper: impl AsRef<Path>,
	bitrate_bps: u32,
) -> Command {
	let mut cmd = Command::new("ffmpeg");
	cmd.args(["-hide_banner", "-loglevel", "fatal"])
		.args(["-re", "-threads", "1"])
		.arg("-i")
		.arg(input.as_ref())
		.arg("-i")
		.arg(sweeper.as_ref())
		.args(["-c:a", "mp3", "-b:a", &bitrate_bps.to_string()]);
	cmd.args([
		"-write_xing",
		"0",
		"-id3v2_version",
		"0",
		"-map_metadata",
		"-1",
		"-vn",
		"-filter_complex",
		"[0]atrim=0:1[in];[1]adelay=1s:all=1[voice];[in][voice][0]amix=inputs=3:weights='1, 1, 0.1':dropout_transition=0.5[out]",
		"-map",
		"[out]",
		"-f",
		"mp3",
		"-",
	])
	.stdout(Stdio::piped())
	.stderr(Stdio::piped())
	.stdin(Stdio::null());
	cmd
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Mediainfo {
	pub filename: PathBuf,
	pub title: Option<String>,
	pub album: Option<String>,
	pub artist: Option<String>,
	pub album_artist: Option<String>,
	pub publisher: Option<String>,
	pub disc: Option<String>,
	pub track: Option<String>,
	pub genre: Option<String>,
	pub bitrate: Option<u32>,
	pub codec: String,
}

pub async fn mediainfo(input: &Path) -> Result<Mediainfo, String> {
	let child = Command::new("ffprobe")
		.args([
			"-loglevel",
			"fatal",
			"-select_streams",
			"a:0",
			"-show_entries",
			"format_tags:stream=codec_name,bit_rate:format=filename,bit_rate",
			"-of",
			"json=c=1",
		])
		.arg(input)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.stdin(Stdio::null())
		.spawn()
		.unwrap();
	let output = child.wait_with_output().await.unwrap();

	if !output.status.success() {
		return Err(format!("ffprobe failed: {}", String::from_utf8_lossy(&output.stderr)));
	}

	#[derive(Deserialize)]
	struct P {
		streams: [PStreams; 1],
		format: PFormat,
	}

	#[derive(Deserialize)]
	struct PStreams {
		codec_name: String,
		bit_rate: Option<String>,
	}

	#[derive(Deserialize)]
	struct PFormat {
		filename: PathBuf,
		bit_rate: Option<String>,
		tags: Option<PMediainfo>,
	}

	#[derive(Deserialize)]
	pub struct PMediainfo {
		#[serde(alias = "TITLE")]
		pub title: Option<String>,
		#[serde(alias = "ALBUM")]
		pub album: Option<String>,
		#[serde(alias = "ARTIST")]
		pub artist: Option<String>,
		#[serde(alias = "ALBUM_ARTIST")]
		pub album_artist: Option<String>,
		#[serde(alias = "PUBLISHER")]
		pub publisher: Option<String>,
		#[serde(alias = "DISC")]
		pub disc: Option<String>,
		#[serde(alias = "TRACK")]
		pub track: Option<String>,
		#[serde(alias = "GENRE")]
		pub genre: Option<String>,
	}

	let output: P = match serde_json::from_str(&String::from_utf8_lossy(&output.stdout)) {
		Ok(x) => x,
		Err(e) => {
			return Err(format!("ffprobe failed for {}: {}", input.display(), e));
		}
	};

	let [stream] = output.streams;
	let Some(tags) = output.format.tags else {
		return Ok(Mediainfo {
			filename: output.format.filename.file_name().unwrap_or_default().into(),
			title: None,
			album: None,
			artist: None,
			album_artist: None,
			publisher: None,
			disc: None,
			track: None,
			genre: None,
			bitrate: stream.bit_rate.or(output.format.bit_rate).and_then(|x| x.parse().ok()),
			codec: stream.codec_name,
		});
	};
	Ok(Mediainfo {
		filename: output.format.filename.file_name().unwrap_or_default().into(),
		title: tags.title,
		album: tags.album,
		artist: tags.artist,
		album_artist: tags.album_artist,
		publisher: tags.publisher,
		disc: tags.disc,
		track: tags.track,
		genre: tags.genre,
		bitrate: stream.bit_rate.or(output.format.bit_rate).and_then(|x| x.parse().ok()),
		codec: stream.codec_name,
	})
}

/// returns true if album art was found
/// sets length to 0 if no art is found
pub async fn album_art_png(input: &Path) -> Result<Option<Vec<u8>>, String> {
	let child = Command::new("ffmpeg")
		.args(["-loglevel", "fatal", "-i"])
		.arg(input)
		.args(["-an", "-c:v", "png", "-f", "image2pipe", "-"])
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.stdin(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.unwrap();
	let output = child.wait_with_output().await.unwrap();

	if !output.status.success() {
		let msg = String::from_utf8(output.stderr).unwrap();
		if msg.contains("Output file does not contain any stream") {
			return Ok(None);
		}
		return Err(format!("ffmpeg failed: {}", msg));
	}

	if output.stdout.is_empty() {
		return Ok(None);
	}

	Ok(Some(output.stdout))
}

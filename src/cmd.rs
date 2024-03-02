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

pub fn spawn_ffmpeg(input: &Path, bitrate_bps: u32, copy_codec: bool) -> tokio::process::Child {
	let mut cmd = Command::new("ffmpeg");
	cmd.args(["-hide_banner", "-loglevel", "error"])
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
		"-f",
		"mp3",
		"-",
	])
	.stdout(Stdio::piped())
	.stderr(Stdio::piped())
	.stdin(Stdio::null())
	.spawn()
	.unwrap()
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
	pub codec: String,
}

pub async fn mediainfo(input: &Path) -> Result<Mediainfo, String> {
	let child = Command::new("ffprobe")
		.args([
			"-loglevel",
			"error",
			"-select_streams",
			"a:0",
			"-show_entries",
			"format_tags:stream=codec_name:format=filename",
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
	}

	#[derive(Deserialize)]
	struct PFormat {
		filename: PathBuf,
		tags: PMediainfo,
	}

	#[derive(Deserialize)]
	pub struct PMediainfo {
		pub title: Option<String>,
		pub album: Option<String>,
		pub artist: Option<String>,
		pub album_artist: Option<String>,
		pub publisher: Option<String>,
		pub disc: Option<String>,
		pub track: Option<String>,
		pub genre: Option<String>,
	}

	let output: P = match serde_json::from_str(&String::from_utf8_lossy(&output.stdout)) {
		Ok(x) => x,
		Err(e) => {
			return Err(format!("ffprobe failed: {}\n", e));
		}
	};
	let [stream] = output.streams;
	Ok(Mediainfo {
		filename: output.format.filename.file_name().unwrap_or_default().into(),
		title: output.format.tags.title,
		album: output.format.tags.album,
		artist: output.format.tags.artist,
		album_artist: output.format.tags.album_artist,
		publisher: output.format.tags.publisher,
		disc: output.format.tags.disc,
		track: output.format.tags.track,
		genre: output.format.tags.genre,
		codec: stream.codec_name,
	})
}

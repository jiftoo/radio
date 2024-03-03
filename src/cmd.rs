use std::{
	path::{Path, PathBuf},
	process::Stdio,
};

use rand::seq::IteratorRandom;
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

pub fn spawn_ffmpeg(
	input: &Path,
	bitrate_bps: u32,
	copy_codec: bool,
	insert_sweeper: bool,
) -> tokio::process::Child {
	if insert_sweeper {
		build_with_sweeper(input, bitrate_bps)
	} else {
		build_without_sweeper(input, bitrate_bps, copy_codec)
	}
	.spawn()
	.unwrap()
}

fn build_without_sweeper(input: &Path, bitrate_bps: u32, copy_codec: bool) -> Command {
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

pub fn build_with_sweeper(input: impl AsRef<Path>, bitrate_bps: u32) -> Command {
	fn pick_sweeper() -> PathBuf {
		std::fs::read_dir("./sweepers")
			.unwrap()
			.flatten()
			.choose(&mut rand::thread_rng())
			.unwrap()
			.path()
	}

	let mut cmd = Command::new("ffmpeg");
	let sweeper = pick_sweeper();
	println!("sweeper: {:?}", sweeper);
	cmd.args(["-hide_banner", "-loglevel", "error"])
		.args(["-re", "-threads", "1"])
		.arg("-i")
		.arg(input.as_ref())
		.arg("-i")
		.arg(sweeper)
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
			"error",
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

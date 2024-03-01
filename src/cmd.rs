use std::{path::Path, process::Stdio, sync::atomic::Ordering};

use tokio::process::Command;

use crate::player;

pub fn check_executables() -> (bool, Vec<(String, bool)>) {
	let info = ["ffmpeg", "mediainfo"]
		.into_iter()
		.map(|x| {
			(x.to_string(), { std::process::Command::new(x).spawn().map(|mut x| x.kill()).is_ok() })
		})
		.collect::<Vec<_>>();
	(info.iter().all(|x| x.1), info)
}

#[cfg(feature = "ffmpeg")]
pub fn spawn_ffmpeg(input: &Path) -> tokio::process::Child {
	use crate::DerefOnceCell;

	Command::new("ffmpeg")
		.args(["-hide_banner", "-loglevel", "error"])
		.args(["-re", "-threads", "1", "-i"])
		.arg(input)
		.args([
			"-c:a",
			"mp3",
			"-b:a",
			&format!("{}k", crate::CONFIG.transcode_non_mp3.bitrate_k),
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

pub struct Mediainfo {
	pub title: Option<String>,
	pub album: Option<String>,
	pub album_performer: Option<String>,
	pub track: Option<String>,
	pub performer: Option<String>,
	pub genre: Option<String>,
}

pub async fn mediainfo(input: &Path) -> Result<Mediainfo, String> {
	let child = Command::new("mediainfo")
		.arg("--Output=General;%Title%,%Album%,%Album/Performer%,%Track%,%Performer%,%Genre%")
		.arg(input)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.stdin(Stdio::null())
		.spawn()
		.unwrap();
	let output = child.wait_with_output().await.unwrap();

	if !output.status.success() {
		return Err(format!("mediainfo failed: {}", String::from_utf8_lossy(&output.stderr)));
	}

	let str = String::from_utf8_lossy(&output.stdout);
	let mut info = str
		.split(',')
		.map(|x| x.trim().is_empty().then_some(None).unwrap_or_else(|| Some(x.to_string())));

	Ok(Mediainfo {
		title: info.next().unwrap(),
		album: info.next().unwrap(),
		album_performer: info.next().unwrap(),
		track: info.next().unwrap(),
		performer: info.next().unwrap(),
		genre: info.next().unwrap(),
	})
}

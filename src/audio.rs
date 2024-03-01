use std::{
	fs::File,
	io::Read,
	path::{Path, PathBuf},
};

use tokio::io::AsyncReadExt;

use crate::cmd;

#[derive(Debug)]
pub enum Data {
	Audio(usize),
	Error(String),
}

pub trait AudioReader {
	fn open(input: &Path) -> Self;
	async fn read_data(&mut self, buf: &mut [u8]) -> Result<Data, std::io::Error>;
	#[cfg(feature = "mediainfo")]
	async fn read_metadata(&mut self) -> Result<crate::cmd::Mediainfo, String>;
}

pub struct FFMpegAudioReader {
	file: PathBuf,
	error_buf: String,
	handle: tokio::process::Child,
	stdout: tokio::process::ChildStdout,
	stderr: tokio::process::ChildStderr,
}

pub struct CopyAudioReader {
	file: File,
}

impl AudioReader for FFMpegAudioReader {
	fn open(input: &Path) -> Self {
		let mut handle = cmd::spawn_ffmpeg(input);
		let stdout = handle.stdout.take().unwrap();
		let stderr = handle.stderr.take().unwrap();
		Self { file: input.to_path_buf(), error_buf: String::new(), handle, stdout, stderr }
	}

	async fn read_data(&mut self, buf: &mut [u8]) -> Result<Data, std::io::Error> {
		tokio::select! {
			biased;
			read_result = self.stdout.read(buf) => {
				match read_result {
					Ok(read) => Ok(Data::Audio(read)),
					Err(e) => Err(e),
				}
			},
			_ = self.stderr.read_to_string(&mut self.error_buf) => {
				Ok(Data::Error(self.error_buf.clone()))
			}
		}
	}

	#[cfg(feature = "mediainfo")]
	async fn read_metadata(&mut self) -> Result<cmd::Mediainfo, String> {
		cmd::mediainfo(&self.file).await
	}
}

impl Drop for FFMpegAudioReader {
	fn drop(&mut self) {
		self.handle.start_kill().unwrap();
	}
}

impl AudioReader for CopyAudioReader {
	fn open(input: &Path) -> Self {
		Self { file: File::open(input).unwrap() }
	}

	async fn read_data(&mut self, buf: &mut [u8]) -> Result<Data, std::io::Error> {
		match self.file.read(buf) {
			Ok(read) => Ok(Data::Audio(read)),
			Err(e) => Err(e),
		}
	}

	#[cfg(feature = "mediainfo")]
	async fn read_metadata(&mut self) -> Result<cmd::Mediainfo, String> {
		Ok(cmd::Mediainfo {
			title: None,
			album: None,
			album_performer: None,
			track: None,
			performer: None,
			genre: None,
		})
	}
}

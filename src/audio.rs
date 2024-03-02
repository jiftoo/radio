use std::path::{Path, PathBuf};

use tokio::io::AsyncReadExt;

use crate::cmd::{self};

#[derive(Debug)]
pub enum Data {
	Audio(usize),
	Error(String),
}

#[async_trait::async_trait]
pub trait AudioReader: Send {
	async fn read_data(&mut self, buf: &mut [u8]) -> Result<Data, std::io::Error>;
	async fn read_metadata(&mut self) -> Result<cmd::Mediainfo, String>;
}

pub struct FFMpegAudioReader {
	file: PathBuf,
	metadata: Option<cmd::Mediainfo>,
	error_buf: String,
	handle: tokio::process::Child,
	stdout: tokio::process::ChildStdout,
	stderr: tokio::process::ChildStderr,
}

impl FFMpegAudioReader {
	pub fn new(input: &Path, bitrate: u32, copy_codec: bool) -> Self {
		let mut handle = cmd::spawn_ffmpeg(input, bitrate, copy_codec);
		let stdout = handle.stdout.take().unwrap();
		let stderr = handle.stderr.take().unwrap();
		Self {
			file: input.to_path_buf(),
			error_buf: String::new(),
			handle,
			stdout,
			stderr,
			metadata: None,
		}
	}
}

#[async_trait::async_trait]
impl AudioReader for FFMpegAudioReader {
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

	async fn read_metadata(&mut self) -> Result<cmd::Mediainfo, String> {
		match self.metadata {
			Some(ref x) => Ok(x.clone()),
			None => {
				let metadata = cmd::mediainfo(&self.file).await?;
				self.metadata = Some(metadata.clone());
				Ok(metadata)
			}
		}
	}
}

impl Drop for FFMpegAudioReader {
	fn drop(&mut self) {
		self.handle.start_kill().unwrap();
	}
}

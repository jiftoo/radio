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
	error_buf: Vec<u8>,
	handle: tokio::process::Child,
	stdout: tokio::process::ChildStdout,
	stderr: tokio::process::ChildStderr,
}

impl FFMpegAudioReader {
	pub fn start(
		input: impl AsRef<Path>,
		bitrate: u32,
		copy_codec: bool,
		insert_sweeper: bool,
	) -> Self {
		let mut handle = cmd::spawn_ffmpeg(input.as_ref(), bitrate, copy_codec, insert_sweeper);
		let stdout = handle.stdout.take().unwrap();
		let stderr = handle.stderr.take().unwrap();
		Self {
			file: input.as_ref().to_path_buf(),
			error_buf: Default::default(),
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
			Ok(x) = self.stderr.read_buf(&mut self.error_buf) => {
				if x > 0 {
					let mut message = String::from_utf8(self.error_buf.clone()).unwrap();
					self.stderr.read_to_string(&mut message).await.unwrap();
					Ok(Data::Error(message))
				} else {
					Ok(Data::Audio(0))
				}
			},
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

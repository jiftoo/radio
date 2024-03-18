use std::{
	path::{Path, PathBuf},
	time::Duration,
};

use tokio::{io::AsyncReadExt, sync::Notify};

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
		sweeper: Option<impl AsRef<Path>>,
		bitrate: u32,
		copy_codec: bool,
	) -> Self {
		let mut handle = cmd::spawn_ffmpeg(
			input.as_ref(),
			sweeper.as_ref().map(|x| x.as_ref()),
			bitrate,
			copy_codec,
		);
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
					// i'm scared of locking up the whole thing on read_buf
					// the following is tested and doesn't work very well.
					// tokio::time::sleep(Duration::from_millis(200)).await;
					// tokio::select! {
					//   biased;
					//   _ = self.stderr.read_buf(&mut self.error_buf) => {},
					//   _ = std::future::ready(()) => {},
					// }

					let (tx, mut rx) = tokio::sync::oneshot::channel();
					tokio::spawn(async move {
						tokio::time::sleep(Duration::from_millis(200)).await;
						tx.send(()).unwrap();
					});
					loop {
						tokio::select! {
							_ = self.stderr.read_buf(&mut self.error_buf) => {},
							_ = &mut rx => break,
						}
					}
					Ok(Data::Error(String::from_utf8_lossy(&self.error_buf).to_string()))
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

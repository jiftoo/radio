use std::{
	path::{Path, PathBuf},
	process::Stdio,
	sync::{
		atomic::{AtomicBool, AtomicUsize, Ordering},
		Arc,
	},
	time::Duration,
};

use axum::body::Bytes;
use rand::Rng;

use tokio::io::AsyncReadExt;

use crate::{
	audio::{self, AudioReader},
	cmd, CONFIG,
};

#[derive(Clone)]
pub struct Player {
	inner: Arc<Inner>,
}

pub struct Inner {
	playlist: Box<[PathBuf]>,
	index: AtomicUsize,
	tx: tokio::sync::broadcast::Sender<Bytes>,
	task_control_tx: tokio::sync::watch::Sender<TaskControlMessage>,
}

#[derive(Clone, Copy)]
enum TaskControlMessage {
	Play,
	Pause,
}

pub type PlayerRx = tokio::sync::broadcast::Receiver<Bytes>;

#[derive(Debug)]
pub enum Error {
	EmptyPlayilist,
}

impl Player {
	pub fn new(playlist: Vec<PathBuf>) -> Result<Self, Error> {
		if playlist.is_empty() {
			return Err(Error::EmptyPlayilist);
		}

		let index = 0;
		let tx = tokio::sync::broadcast::channel(4).0;

		let player = Self {
			inner: Arc::new(Inner {
				playlist: playlist.into_boxed_slice(),
				index: index.into(),
				tx,
				task_control_tx: tokio::sync::watch::channel(TaskControlMessage::Play).0,
			}),
		};

		player.clone().spawn_task();

		Ok(player)
	}

	fn spawn_task(self) {
		tokio::spawn({
			async move {
				let mut rx = self.inner.task_control_tx.subscribe();
				loop {
					let msg = *rx.borrow();
					match msg {
						TaskControlMessage::Play => loop {
							tokio::select! {
								_ = self.play_next() => (),
								_ = rx.changed() => break,
							}
						},
						TaskControlMessage::Pause => {
							tokio::select! {
								_ = tokio::time::sleep(Duration::from_secs(999)) => (),
								_ = rx.changed() => break,
							}
						}
					}
				}
			}
		});
	}

	async fn play_next(&self) {
		let Inner { playlist, index, tx, .. } = &*self.inner;
		let index = index.load(Ordering::Relaxed);
		println!("playing: {:?}", playlist[index].file_name().unwrap());

		let mut reader = audio::FFMpegAudioReader::open(&playlist[index]);

		let buf = &mut [0u8; 4096];
		loop {
			let data = reader.read_data(buf).await.unwrap();
			match data {
				audio::Data::Audio(0) => break,
				audio::Data::Audio(read) => {
					let _ = tx.send(Bytes::copy_from_slice(&buf[..read]));
				}
				audio::Data::Error(err) => {
					println!("ffmpeg error: {}", err);
					break;
				}
			}
		}
		self.next();
	}

	pub fn set_index(&mut self, index: usize) {
		self.inner.index.store(index, Ordering::Relaxed);
	}

	pub fn index(&self) -> usize {
		self.inner.index.load(Ordering::Relaxed)
	}

	pub fn current(&self) -> &Path {
		&self.inner.playlist[self.inner.index.load(Ordering::Relaxed)]
	}

	pub fn files(&self) -> &[PathBuf] {
		&self.inner.playlist
	}

	pub fn subscribe(&self) -> PlayerRx {
		self.inner.tx.subscribe()
	}

	fn next(&self) {
		let Inner { index, playlist, .. } = &*self.inner;
		let shuffle = CONFIG.shuffle;

		let mut loaded_index = index.load(Ordering::Relaxed);
		if shuffle {
			let mut rng = rand::thread_rng();
			loaded_index = loop {
				let new_index = rng.gen_range(0..playlist.len());
				if new_index != loaded_index {
					break new_index;
				}
			}
		} else {
			loaded_index = (loaded_index + 1) % playlist.len();
		}

		index.store(loaded_index, Ordering::Relaxed);
	}
}

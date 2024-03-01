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

#[derive(Clone)]
pub struct Player {
	inner: Arc<Inner>,
}

pub struct Inner {
	playlist: Box<[PathBuf]>,
	index: AtomicUsize,
	tx: tokio::sync::broadcast::Sender<Bytes>,
	task_control_tx: tokio::sync::watch::Sender<TaskControlMessage>,
	config: Config,
}

pub struct Config {
	shuffle: AtomicBool,
	bitrate_k: AtomicUsize,
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
				config: Config { shuffle: false.into(), bitrate_k: 128.into() },
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
		use tokio::process::Command;

		let Inner { playlist, index, tx, config, .. } = &*self.inner;
		let index = index.load(Ordering::Relaxed);
		println!("playing: {:?}", playlist[index].file_name().unwrap());

		let mut handle = Command::new("ffmpeg")
			.args(["-hide_banner", "-loglevel", "error"])
			.args(["-re", "-threads", "1", "-i"])
			.arg(&playlist[index])
			.args([
				"-c:a",
				"mp3",
				"-b:a",
				&format!("{}k", config.bitrate_k.load(Ordering::Relaxed)),
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
			.unwrap();
		let mut stdout = handle.stdout.take().unwrap();
		let mut stderr = handle.stderr.take().unwrap();

		let buf = &mut [0u8; 4096];
		let err_buf = &mut String::new();
		while let Ok(read) = stdout.read(buf).await {
			if read == 0 {
				stderr.read_to_string(err_buf).await.unwrap();
				if !err_buf.is_empty() {
					println!("ffmpeg error: {err_buf}");
					std::process::exit(1);
				}
				break;
			}

			let _ = tx.send(Bytes::copy_from_slice(&buf[..read]));
		}
		handle.wait().await.unwrap();
		self.next();
	}

	pub fn set_shuffle(&self, shuffle: bool) {
		self.inner.config.shuffle.store(shuffle, Ordering::Relaxed);
	}

	pub fn set_bitrate(&self, bitrate: usize) {
		self.inner.config.bitrate_k.store(bitrate, Ordering::Relaxed);
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
		let Inner { index, playlist, config, .. } = &*self.inner;

		let mut loaded_index = index.load(Ordering::Relaxed);
		if config.shuffle.load(Ordering::Relaxed) {
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

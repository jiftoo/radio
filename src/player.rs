use std::{
	collections::VecDeque,
	path::{Path, PathBuf},
	pin::Pin,
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	},
	time::Duration,
};

use axum::body::Bytes;
use futures_core::Stream;
use rand::Rng;
use tokio::sync::{oneshot, RwLock};

use crate::{
	audio::{self, AudioReader},
	cmd, config,
};

#[derive(Clone)]
pub struct Player {
	inner: Arc<Inner>,
}

pub struct FixedDeque<T>(VecDeque<T>, usize);

impl<T> FixedDeque<T> {
	pub fn new(size: usize) -> Self {
		Self(VecDeque::with_capacity(size), size)
	}

	pub fn push(&mut self, value: T) {
		let x = &mut self.0;
		if x.len() == self.1 {
			x.pop_back();
		}
		x.push_front(value);
		x.make_contiguous();
	}

	pub fn as_slice(&self) -> &[T] {
		let (a, b) = self.0.as_slices();
		assert!(b.is_empty());
		a
	}
}

pub struct TrackDropStream<T: futures_core::Stream>(T, Option<oneshot::Sender<()>>);

impl<T: Stream + Unpin> TrackDropStream<T> {
	fn create(stream: T) -> (Self, oneshot::Receiver<()>) {
		let (tx, drop_rx) = oneshot::channel();
		let this = Self(stream, Some(tx));
		(this, drop_rx)
	}
}

impl<T: Stream> Drop for TrackDropStream<T> {
	fn drop(&mut self) {
		let _ = self.1.take().unwrap().send(());
	}
}

impl<T: Stream + Unpin> futures_core::Stream for TrackDropStream<T> {
	type Item = T::Item;

	fn poll_next(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Option<Self::Item>> {
		Pin::new(&mut self.0).poll_next(cx)
	}
}

#[derive(Debug, Default)]
pub struct Statistics {
	pub time_played: Duration,
	pub listeners: usize,
	pub max_listeners: usize,
	pub bytes_transcoded: usize,
	pub bytes_copied: usize,
	pub bytes_sent: usize,
	pub target_badwidth: usize,
}

pub struct Inner {
	playlist: Box<[PathBuf]>,
	index: AtomicUsize,
	mediainfo: RwLock<FixedDeque<cmd::Mediainfo>>,
	tx: tokio::sync::broadcast::Sender<Bytes>,
	task_control_tx: tokio::sync::watch::Sender<TaskControlMessage>,
	config: Arc<config::Config>,
	statistics: Arc<RwLock<Statistics>>,
}

#[derive(Clone, Copy)]
enum TaskControlMessage {
	Play,
	Pause,
}

pub type PlayerRx = TrackDropStream<tokio_stream::wrappers::BroadcastStream<Bytes>>;

#[derive(Debug)]
pub enum Error {
	EmptyPlayilist,
}

impl Player {
	pub fn new(playlist: Vec<PathBuf>, config: Arc<config::Config>) -> Result<Self, Error> {
		if playlist.is_empty() {
			return Err(Error::EmptyPlayilist);
		}

		let index =
			if config.shuffle { rand::thread_rng().gen_range(0..playlist.len()) } else { 0 };
		let tx = tokio::sync::broadcast::channel(4).0;

		let player = Self {
			inner: Arc::new(Inner {
				playlist: playlist.into_boxed_slice(),
				index: index.into(),
				mediainfo: FixedDeque::new(config.mediainfo_history.get()).into(),
				tx,
				task_control_tx: tokio::sync::watch::channel(TaskControlMessage::Play).0,
				config,
				statistics: Default::default(),
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
								_ = rx.changed() => break,
								_ = self.play_next() => (),
							}
						},
						TaskControlMessage::Pause => {
							tokio::select! {
								_ = rx.changed() => break,
								_ = tokio::time::sleep(Duration::from_secs(999)) => (),
							}
						}
					}
				}
			}
		});
	}

	#[allow(clippy::significant_drop_tightening)]
	async fn play_next(&self) {
		let mut start_instant = tokio::time::Instant::now();

		let Inner { playlist, index, tx, config, .. } = &*self.inner;
		let index = index.load(Ordering::Relaxed);

		let input = &playlist[index];

		let Ok(mediainfo) = cmd::mediainfo(input).await else {
			println!("{:?}\tbroken file - skipping.", playlist[index].file_name().unwrap());
			self.next();
			return;
		};

		let copy_codec = !config.transcode_all && mediainfo.codec == "mp3";

		let mut reader: Box<dyn AudioReader> =
			Box::new(audio::FFMpegAudioReader::new(input, config.bitrate, copy_codec));

		println!(
			"{:?}\t(codec: {}, copy: {})",
			playlist[index].file_name().unwrap(),
			mediainfo.codec,
			if copy_codec { "yes" } else { "no" }
		);

		self.inner.mediainfo.write().await.push(mediainfo);

		let buf = &mut [0u8; 4096];
		let mut bandwidth_instant = tokio::time::Instant::now();
		let mut bandwidth_acc = 0;
		loop {
			let data = reader.read_data(buf).await.unwrap();
			match data {
				audio::Data::Audio(0) => break,
				audio::Data::Audio(read) => {
					let _ = tx.send(Bytes::copy_from_slice(&buf[..read]));
					bandwidth_acc += read;

					let mut stats = self.inner.statistics.write().await;
					if copy_codec {
						stats.bytes_copied += read;
					} else {
						stats.bytes_transcoded += read;
					}
					stats.bytes_sent += read * tx.receiver_count();

					if bandwidth_instant.elapsed() >= Duration::from_secs(1) {
						stats.target_badwidth = bandwidth_acc * tx.receiver_count();
						bandwidth_acc = 0;
						bandwidth_instant = tokio::time::Instant::now();
					}
				}
				audio::Data::Error(err) => {
					println!("ffmpeg error: {}", err);
					break;
				}
			}
			self.inner.statistics.write().await.time_played = start_instant.elapsed();
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
		let stream = tokio_stream::wrappers::BroadcastStream::new(self.inner.tx.subscribe());
		let (stream, drop_rx) = TrackDropStream::create(stream);

		tokio::spawn({
			let statistics = self.inner.statistics.clone();
			async move {
				{
					let mut statistics = statistics.write().await;
					statistics.listeners += 1;
					statistics.max_listeners = statistics.max_listeners.max(statistics.listeners);
				}
				drop_rx.await.unwrap();
				{
					let mut statistics = statistics.write().await;
					statistics.listeners -= 1;
				}
			}
		});

		stream
	}

	pub fn config(&self) -> &config::Config {
		&self.inner.config
	}

	pub fn statistics(&self) -> &RwLock<Statistics> {
		&self.inner.statistics
	}

	pub async fn read_mediainfo<R, F: Send + FnOnce(&[cmd::Mediainfo]) -> R>(&self, f: F) -> R {
		f(self.inner.mediainfo.read().await.as_slice())
	}

	fn next(&self) {
		let Inner { index, playlist, config, .. } = &*self.inner;
		let shuffle = config.shuffle;

		let mut loaded_index = index.load(Ordering::Relaxed);
		if shuffle {
			let mut rng = rand::thread_rng();
			loaded_index = loop {
				let new_index = rng.gen_range(0..playlist.len());
				if new_index != loaded_index || playlist.len() == 1 {
					break new_index;
				}
			}
		} else {
			loaded_index = (loaded_index + 1) % playlist.len();
		}

		index.store(loaded_index, Ordering::Relaxed);
	}
}

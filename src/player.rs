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
use rand::{seq::IteratorRandom, Rng};
use tokio::{
	sync::{oneshot, RwLock},
	task::JoinSet,
};

use crate::{
	audio::{self, AudioReader, FFMpegAudioReader},
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
	sweeper_list: Box<[PathBuf]>,
	album_art: RwLock<AlbumImage>,
	index: AtomicUsize,
	mediainfo: RwLock<FixedDeque<cmd::Mediainfo>>,
	tx: tokio::sync::broadcast::Sender<Bytes>,
	next_song_tx: tokio::sync::watch::Sender<()>,
	task_control_tx: tokio::sync::watch::Sender<TaskControlMessage>,
	config: Arc<config::Config>,
	statistics: RwLock<Statistics>,
}

#[derive(Debug, Default)]
pub struct AlbumImage(Vec<u8>);

impl AlbumImage {
	// public method for getting the album art if it exists
	pub fn get(&self) -> Option<&[u8]> {
		(!self.0.is_empty()).then_some(&self.0)
	}

	pub fn checksum(&self) -> Option<i64> {
		self.get().map(|x| x.iter().map(|x| (*x).into()).fold(0i64, |a, b| a.wrapping_add(b)))
	}

	// get the inner buffer for modification by player
	fn set(&mut self, new: Vec<u8>) {
		self.0 = new;
	}

	fn clear(&mut self) {
		self.0.clear();
	}
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
	pub fn new(
		playlist: Vec<PathBuf>,
		sweeper_list: Vec<PathBuf>,
		config: Arc<config::Config>,
	) -> Result<Self, Error> {
		if playlist.is_empty() {
			return Err(Error::EmptyPlayilist);
		}

		let index =
			if config.shuffle { rand::thread_rng().gen_range(0..playlist.len()) } else { 0 };
		let tx = tokio::sync::broadcast::channel(4).0;
		let next_song_tx = tokio::sync::watch::channel(()).0;

		let player = Self {
			inner: Arc::new(Inner {
				playlist: playlist.into_boxed_slice(),
				sweeper_list: sweeper_list.into_boxed_slice(),
				album_art: Default::default(),
				index: index.into(),
				mediainfo: FixedDeque::new(config.mediainfo_history.get()).into(),
				tx,
				next_song_tx,
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
				let player_init_instant = tokio::time::Instant::now();
				loop {
					let msg = *rx.borrow();
					match msg {
						TaskControlMessage::Play => loop {
							tokio::select! {
								_ = rx.changed() => break,
								_ = self.play_next(player_init_instant) => (),
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

	// #[allow(clippy::significant_drop_tightening, clippy::significant_drop_in_scrutinee)]
	async fn play_next(&self, player_init_instant: tokio::time::Instant) {
		let Inner { playlist, sweeper_list, album_art, index, tx, config, .. } = &*self.inner;
		let index = index.load(Ordering::Relaxed);

		let input = &playlist[index];

		let mediainfo = match cmd::mediainfo(input).await {
			Ok(x) => x,
			Err(x) => {
				println!("{:?}\tbroken file - skipping: {x}", playlist[index].file_name().unwrap());
				self.next();
				return;
			}
		};

		let album_image_path = match try_album_arts(input).await {
			Some(data) => {
				album_art.write().await.set(data.1);
				Some(data.0)
			}
			None => {
				album_art.write().await.clear();
				None
			}
		};

		let sweeper_path = {
			let mut rng = rand::thread_rng();
			(rng.gen::<f32>() <= config.sweeper_chance).then(|| {
				// checked non empty in main
				sweeper_list.iter().choose(&mut rng).unwrap()
			})
		};
		let copy_codec = !config.transcode_all && mediainfo.codec == "mp3";

		println!(
			"{:?}\t(codec: {}, copy: {}, sweeper: {}, album image: {})",
			playlist[index].file_name().unwrap(),
			mediainfo.codec,
			if copy_codec { "yes" } else { "no" },
			sweeper_path.as_ref().map(|x| x.file_name().unwrap().to_str().unwrap()).unwrap_or("no"),
			album_image_path
				.as_ref()
				.map(|x| x.file_name().unwrap().to_str().unwrap())
				.unwrap_or("none"),
		);

		self.inner.mediainfo.write().await.push(mediainfo);

		// notify about next song after everything is updated
		let _ = self.inner.next_song_tx.send(());

		#[allow(clippy::significant_drop_tightening)]
		let transmit_reader = |mut reader: FFMpegAudioReader| async move {
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

						// clippy::significant_drop_tightening
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
						println!("ffmpeg error: {:?}", err);
						break;
					}
				}
				self.inner.statistics.write().await.time_played = player_init_instant.elapsed();
			}
		};

		transmit_reader(audio::FFMpegAudioReader::start(
			input,
			sweeper_path,
			config.bitrate,
			copy_codec,
		))
		.await;
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
		let _tx = self.inner.tx.subscribe();
		let stream = tokio_stream::wrappers::BroadcastStream::new(self.inner.tx.subscribe());
		let (stream, drop_rx) = TrackDropStream::create(stream);

		tokio::spawn({
			let inner = self.inner.clone();
			async move {
				let statistics = &inner.statistics;
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

	pub fn subscribe_next_song(&self) -> tokio::sync::watch::Receiver<()> {
		self.inner.next_song_tx.subscribe()
	}

	pub fn config(&self) -> &config::Config {
		&self.inner.config
	}

	pub fn statistics(&self) -> &RwLock<Statistics> {
		&self.inner.statistics
	}

	pub fn album_art(&self) -> &RwLock<AlbumImage> {
		&self.inner.album_art
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

/// try to read embedded album art and if it fails, try to read some image from the same directory
async fn try_album_arts(input: impl AsRef<Path> + Send) -> Option<(PathBuf, Vec<u8>)> {
	async fn try_album_art(input: impl AsRef<Path> + Send) -> Option<(PathBuf, Vec<u8>)> {
		match cmd::album_art_png(input.as_ref()).await {
			Ok(None) => None,
			Ok(Some(buf)) => Some((input.as_ref().to_owned(), buf)),
			Err(_) => None,
		}
	}

	// try from the file itself
	if let Some(x) = try_album_art(input.as_ref()).await {
		return Some(x);
	}

	let mut futures_vec = vec![];
	// then try from the same directory
	if let Some(x) = input.as_ref().parent() {
		let images = std::fs::read_dir(x)
			.unwrap()
			.flatten()
			.filter(|x| {
				let Ok(file_type) = x.file_type() else {
					return false;
				};
				file_type.is_file()
					&& x.path()
						.extension()
						.map_or(false, |x| x == "png" || x == "jpg" || x == "jpeg")
			})
			.collect::<Vec<_>>();
		// try to read "cover.*" from the same directory
		if let Some(x) =
			images.iter().find(|x| x.path().file_stem().map_or(false, |x| x == "cover"))
		{
			futures_vec.push(try_album_art(x.path()));
		} else {
			for x in images {
				futures_vec.push(try_album_art(x.path()));
			}
		}
	}

	let mut joins = JoinSet::new();
	for x in futures_vec {
		joins.spawn(x);
	}

	let mut result = None;
	while let Some(fut) = joins.join_next().await {
		if let Ok(Some(x)) = fut {
			result = Some(x);
			joins.abort_all();
			break;
		}
	}

	result
}

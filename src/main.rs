mod files;
mod player;

use std::{
	collections::LinkedList,
	error::Error,
	fmt::{Debug, Display, Formatter},
	fs::OpenOptions,
	io::Write,
	path::{Path, PathBuf},
	sync::{atomic::AtomicUsize, Arc},
	thread,
	time::Duration,
};

use axum::{
	body::Body, debug_handler, extract::State, response::IntoResponse, routing::get, BoxError,
	Router,
};
use futures::{SinkExt, StreamExt, TryStream};
use player::Player;
use rayon::iter::{ParallelBridge, ParallelIterator};
use tokio::{fs, io::AsyncReadExt};

#[tokio::main]
async fn main() {
	// let path: &Path = "C:\\Users\\Jiftoo\\Downloads".as_ref();
	let path: &Path = "./".as_ref();

	let player = Player::new(files::collect_files(path));

	println!("Files: {:?}", player.files());

	let app = Router::new().route("/", get(stream)).with_state(player.clone());

	let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
	axum::serve(listener, app).await.unwrap();
}

#[derive(Debug)]
struct Eof;

impl Error for Eof {}
impl Display for Eof {
	fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
		Debug::fmt(self, f)
	}
}

#[debug_handler]
async fn stream(State(player): State<Player>) -> Result<impl IntoResponse, String> {
	// let mut file =
	// 	fs::File::options().read(true).open(player.current()).await.map_err(|x| x.to_string())?;
	let rx = player.subscribe();

	Ok(spawn_listener(rx))
}

fn spawn_listener(rx: player::PlayerRx) -> impl IntoResponse {
	let mut headers = axum::http::HeaderMap::new();
	headers.insert("Content-Type", "audio/mpeg".parse().unwrap());

	let stream = tokio_stream::wrappers::BroadcastStream::new(rx);

	println!("sending body");
	(headers, Body::from_stream(stream).into_response())
}

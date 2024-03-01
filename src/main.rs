#![warn(clippy::nursery)]
#![deny(clippy::semicolon_if_nothing_returned)]
#![allow(unused)]

mod files;
mod player;

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
	path::Path,
};

use axum::{
	body::Body, debug_handler, extract::State, response::IntoResponse, routing::get, Router,
};

use player::Player;

#[tokio::main]
async fn main() {
	let path: &Path = "C:\\Users\\Jiftoo\\Downloads".as_ref();
	// let path: &Path = "./".as_ref();

	let player = Player::new(files::collect(path));

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
	let rx = player.subscribe();
	println!("subscribed");

	Ok(spawn_listener(rx))
}

fn spawn_listener(rx: player::PlayerRx) -> impl IntoResponse {
	let mut headers = axum::http::HeaderMap::new();
	headers.insert("Content-Type", "audio/mpeg".parse().unwrap());

	let stream = tokio_stream::wrappers::BroadcastStream::new(rx);

	println!("sending body");
	(headers, Body::from_stream(stream).into_response())
}

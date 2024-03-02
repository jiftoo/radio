#![warn(clippy::nursery)]
#![deny(clippy::semicolon_if_nothing_returned)]
// #![allow(unused)]

mod audio;
mod cmd;
mod config;
mod files;
mod player;

use axum::{
	body::Body, debug_handler, extract::State, response::IntoResponse, routing::get, Router,
};
use clap::Parser;
use player::Player;
use std::ops::Deref;
use tokio::sync::OnceCell;

pub struct DerefOnceCell<T>(OnceCell<T>);

impl<T> Deref for DerefOnceCell<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		self.0.get().expect("DerefOnceCell uninitialized")
	}
}

static CONFIG: DerefOnceCell<config::Config> = DerefOnceCell(OnceCell::const_new());

#[tokio::main]
async fn main() {
	match cmd::check_executables() {
		(true, _) => {}
		(false, missing) => {
			println!(
				"Could not execute: {}",
				missing.into_iter().map(|x| format!("{:?}", x.0)).collect::<Vec<_>>().join(", ")
			);
			let x = std::env::current_dir()
				.map(|x| format!(" ({:?})", x.display()))
				.unwrap_or_default();
			println!("Make sure ffmpeg is installed and accessible. Or just put those two in the current directory{x}.");

			return;
		}
	}

	let cfg: config::Config =
		if std::env::args().nth(1).map(|x| x == "--use-config").unwrap_or(false) {
			config::create_and_load()
		} else {
			config::CliConfig::parse().into()
		};

	CONFIG.0.set(cfg).unwrap();

	let port = CONFIG.port;
	let path = CONFIG.dirs[0].root.clone();

	if !path.exists() {
		println!("{} does not exist", path.display());
		return;
	}

	if !path.is_dir() {
		println!("{} is not a directory", path.display());
		return;
	}

	let player = match Player::new(files::collect(&path)) {
		Ok(player) => player,
		Err(e) => {
			println!("Player error: {:?}", e);
			return;
		}
	};

	println!("Playlist:");
	let take = 10;
	for x in player.files().iter().take(take) {
		println!("  {}", x.display());
	}
	if player.files().len() > take {
		println!(" ... and {} more", player.files().len() - take);
	}

	let app = define_routes(Router::new()).with_state(player.clone());

	let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
	println!("Listening on port {}", port);

	axum::serve(listener, app).await.unwrap();
}

fn define_routes(r: Router<Player>) -> Router<Player> {
	r.route("/", get(stream))
}

#[debug_handler]
async fn stream(State(player): State<Player>) -> Result<impl IntoResponse, String> {
	fn spawn_listener(rx: player::PlayerRx) -> impl IntoResponse {
		let mut headers = axum::http::HeaderMap::new();
		headers.insert("Content-Type", "audio/mpeg".parse().unwrap());

		let stream = tokio_stream::wrappers::BroadcastStream::new(rx);

		(headers, Body::from_stream(stream).into_response())
	}

	let rx = player.subscribe();

	Ok(spawn_listener(rx))
}

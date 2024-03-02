#![warn(clippy::nursery)]
#![deny(clippy::semicolon_if_nothing_returned)]
#![allow(unused)]

mod audio;
mod cmd;
mod config;
mod files;
mod player;

use axum::{
	body::Body, debug_handler, extract::State, http::header, response::IntoResponse, routing::get,
	Router,
};
use clap::Parser;
use player::Player;
use std::sync::Arc;

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

	if std::env::args().any(|x| x == "--generate-config") {
		if let Err(error) = config::generate_config_file() {
			match error {
				config::Error::Io(e) => println!("Could not generate config file: {}", e),
				config::Error::Parse(_) => unreachable!(),
			}
		}
	};
	let config: Arc<config::Config> = if std::env::args().any(|x| x == "--use-config") {
		println!("Loading config from {}", config::config_path().display());
		match config::generate_or_load() {
			Ok(x) => Arc::new(x),
			Err(error) => {
				match error {
					config::Error::Io(e) => println!("Could not generate or load config: {}", e),
					config::Error::Parse(e) => println!("Could not parse config:\n{}", e),
				}
				return;
			}
		}
	} else {
		Arc::new(config::CliConfig::parse().into())
	};

	let port = config.port;
	let path = config.dirs[0].root.clone();

	if !path.exists() {
		println!("{} does not exist", path.display());
		return;
	}

	if !path.is_dir() {
		println!("{} is not a directory", path.display());
		return;
	}

	let player = match Player::new(files::collect(&path), config.clone()) {
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

	let app = define_routes(Router::new(), &config).with_state(player.clone());

	let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
	println!("Listening on port {}", port);

	axum::serve(listener, app).await.unwrap();
}

fn define_routes(r: Router<Player>, config: &Arc<config::Config>) -> Router<Player> {
	let r = r.route("/", get(stream));
	if config.enable_mediainfo {
		r.route("/mediainfo", get(mediainfo))
	} else {
		r
	}
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

async fn mediainfo(State(player): State<Player>) -> impl IntoResponse {
	let mediainfo_json = player.read_mediainfo(|x| serde_json::to_string(x).unwrap()).await;
	([(header::CONTENT_TYPE, "application/json")], mediainfo_json)
}

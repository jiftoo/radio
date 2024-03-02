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
use clap::{CommandFactory, Parser};
use player::Player;
use std::{ffi::OsString, sync::Arc};

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

	// i dont want to see this code
	// into a function it goes
	let Some(config) = config_shit() else {
		return;
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

fn config_shit() -> Option<Arc<config::Config>> {
	let pre_config = config::PreCliConfig::try_parse();
	let mut use_config = None;
	if let Ok(config::PreCliConfig { generate_config, use_config: use_config_arg, .. }) = pre_config
	{
		if let Some(x) = generate_config {
			let path = match x {
				config::UseConfigArg::Custom(path) => path,
				config::UseConfigArg::Default => config::config_path(),
			};
			if let Err(error) = config::generate_config_file(&path) {
				match error {
					config::Error::Io(e) => println!("Could not generate config file: {}", e),
					config::Error::Parse(_) => unreachable!(),
				}
			} else {
				println!("Config file generated in {}", path.display());
			}
		}
		use_config = use_config_arg;
	}

	let config: Arc<config::Config> = if let Some(use_config) = use_config {
		let path = match use_config {
			config::UseConfigArg::Custom(path) => path,
			config::UseConfigArg::Default => config::config_path(),
		};
		println!("Loading config from {}", path.display());
		match config::generate_or_load(&path) {
			Ok(x) => Arc::new(x),
			Err(error) => {
				match error {
					config::Error::Io(e) => println!("Could not generate or load config: {}", e),
					config::Error::Parse(e) => println!("Could not parse config:\n{}", e),
				}
				return None;
			}
		}
	} else {
		Arc::new(config::CliConfig::parse().into())
	};

	Some(config)
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

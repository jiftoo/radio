#![warn(clippy::nursery)]
#![allow(clippy::redundant_pub_crate)]
#![deny(clippy::semicolon_if_nothing_returned)]
#![allow(unused)]

mod audio;
mod cmd;
mod config;
mod files;
mod player;

use axum::{
	body::Body,
	debug_handler,
	extract::{
		ws::{self, rejection::WebSocketUpgradeRejection},
		Path, State, WebSocketUpgrade,
	},
	http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
	response::{Html, IntoResponse},
	routing::get,
	Router,
};
use clap::Parser;

use player::Player;
use std::{fmt::Write, sync::Arc};

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

	let sweeper_list = files::collect(cmd::SWEEPER_DIR);
	if config.sweeper_chance > 0.0 && sweeper_list.is_empty() {
		println!(
			"Sweeper chance is set to {}, but no sweepers found in {}",
			config.sweeper_chance,
			cmd::SWEEPER_DIR
		);
		return;
	}

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

	let player = match Player::new(files::collect(&path), sweeper_list, config.clone()) {
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

	let app = define_routes(Router::new(), &config)
		.layer(tower_http::cors::CorsLayer::permissive())
		.with_state(player.clone());

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
	let mut r = r
		.route("/stream", get(stream))
		.route("/", get(webpage))
		.route("/*file", get(webpage_assets))
		.route("/album_art", get(album_art));
	if config.enable_mediainfo {
		r = r.route("/mediainfo", get(mediainfo));
		r = r.route("/mediainfo/ws", get(mediainfo_ws));
	}
	if config.enable_webui {
		r = r.route("/webui", get(webui));
	}
	r
}

#[derive(rust_embed::RustEmbed)]
#[folder = "radio-webapp/dist/"]
struct WebappAssets;

async fn webpage() -> impl IntoResponse {
	Html(WebappAssets::get("index.html").unwrap().data)
}

async fn webpage_assets(Path(path): Path<String>) -> impl IntoResponse {
	if path.starts_with("assets/") {
		path.replace("assets/", "");
	} else {
		return StatusCode::NOT_FOUND.into_response();
	}

	match WebappAssets::get(&path) {
		Some(x) => {
			let mime = mime_guess::from_path(path).first_or_octet_stream();
			([(header::CONTENT_TYPE, mime.as_ref())], x.data).into_response()
		}
		None => StatusCode::NOT_FOUND.into_response(),
	}
}

#[debug_handler]
async fn stream(State(player): State<Player>) -> Result<impl IntoResponse, String> {
	let stream = player.subscribe();

	let mut headers = axum::http::HeaderMap::new();
	headers.insert("Content-Type", "audio/mpeg".parse().unwrap());
	headers.insert(
		"x-bitrate",
		if player.config().transcode_all {
			player.config().bitrate.to_string().parse().unwrap()
		} else {
			"vary".parse().unwrap()
		},
	);

	Ok((headers, Body::from_stream(stream).into_response()))
}

async fn mediainfo(State(player): State<Player>) -> impl IntoResponse {
	let mediainfo_json = player.read_mediainfo(|x| serde_json::to_string(x).unwrap()).await;
	([(header::CONTENT_TYPE, "application/json")], mediainfo_json)
}

async fn mediainfo_ws(
	State(player): State<Player>,
	ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
	headers: HeaderMap,
) -> impl IntoResponse {
	let (ws) = match ws {
		Ok(x) => x,
		Err(x) => {
			println!("No websocket upgrade: {x:?}");
			return Err(x);
		}
	};

	Ok(ws
		.on_failed_upgrade(|x| {
			println!("Failed to upgrade: {:?}", x);
		})
		.on_upgrade(move |mut socket| async move {
			let mut rx = player.subscribe_next_song();
			loop {
				tokio::select! {
					biased;
					None = socket.recv() => break,
					_ = rx.changed() => {
						println!("sending new song to socket");
						let _ = socket.send(ws::Message::Text("next".to_string())).await;
					},
				}
			}
		}))
}

async fn webui(State(player): State<Player>) -> impl IntoResponse {
	fn display_bytes(x: usize) -> String {
		match x {
			x if x < 1024 => format!("{x} B"),
			x if x < 1024 * 1024 => format!("{:.2} KiB", x as f64 / 1024.0),
			x if x < 1024 * 1024 * 1024 => format!("{:.2} MiB", x as f64 / 1024.0 / 1024.0),
			x => format!("{:.2} GiB", x as f64 / 1024.0 / 1024.0 / 1024.0),
		}
	}

	fn display_time(x: std::time::Duration) -> String {
		let mut x = x.as_secs();
		let seconds = x % 60;
		x /= 60;
		let minutes = x % 60;
		x /= 60;
		let hours = x;
		format!("{hours:02}:{minutes:02}:{seconds:02}")
	}

	let body = {
		let mut body = String::new();
		let stats = player.statistics().read().await;
		writeln!(&mut body, "Time played: {}", display_time(stats.time_played)).unwrap();
		writeln!(&mut body, "Listeners: {}", stats.listeners).unwrap();
		writeln!(&mut body, "Max listeners: {}", stats.max_listeners).unwrap();
		writeln!(&mut body, "Sent: {}", display_bytes(stats.bytes_sent)).unwrap();
		writeln!(&mut body, "Transcoded: {}", display_bytes(stats.bytes_transcoded)).unwrap();
		writeln!(&mut body, "Copied: {}", display_bytes(stats.bytes_copied)).unwrap();
		writeln!(&mut body, "Target bandwidth: {}/s", display_bytes(stats.target_badwidth))
			.unwrap();
		body
	};
	([(header::CONTENT_TYPE, "text/plain")], body)
}
#[allow(clippy::significant_drop_tightening)]
async fn album_art(State(player): State<Player>, headers: HeaderMap) -> impl IntoResponse {
	let album_art = &player.album_art().read().await;

	let checksum_header_value =
		album_art.checksum().map(HeaderValue::from).unwrap_or(HeaderValue::from_static("no-image"));

	if let Some(x) = headers.get(header::IF_NONE_MATCH) {
		if x == checksum_header_value {
			return StatusCode::NOT_MODIFIED.into_response();
		}
	}

	let album_art = album_art.get();

	let body = album_art.map_or_else(|| Err(StatusCode::NO_CONTENT), |x| Ok(x.to_owned()));

	let mut headers = HeaderMap::new();
	headers.insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
	headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
	headers.insert("ETag", checksum_header_value);

	(headers, body).into_response()
}

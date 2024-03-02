use serde::{Deserialize, Serialize};
use std::{
	fmt::{Display, Formatter},
	num::{NonZeroU32, NonZeroUsize},
	path::{Path, PathBuf},
	str::FromStr,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
	pub host: String,
	pub port: u16,
	pub dirs: Box<[DirectoryConfig]>,
	pub enable_webui: bool,
	pub shuffle: bool,
	pub bitrate: u32,
	pub transcode: bool,
	pub enable_mediainfo: bool,
	pub mediainfo_history: NonZeroUsize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UseConfigArg {
	Default,
	Custom(PathBuf),
}

impl FromStr for UseConfigArg {
	type Err = String;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.is_empty() {
			Ok(Self::Default)
		} else {
			Ok(Self::Custom(PathBuf::from(s)))
		}
	}
}

impl Display for UseConfigArg {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Default => write!(f, "Default"),
			Self::Custom(x) => write!(f, "Custom({})", x.display()),
		}
	}
}

#[derive(clap::Parser, Debug)]
pub struct PreCliConfig {
	#[clap(
		long = "generate-config",
		default_missing_value = "",
		num_args(0..=1),
	)]
	pub generate_config: Option<UseConfigArg>,
	#[clap(
		long = "use-config",
		default_missing_value = "",
		num_args(0..=1),
	)]
	pub use_config: Option<UseConfigArg>,
}

#[derive(Debug, Serialize, Deserialize, clap::Parser)]
pub struct CliConfig {
	// these are here just to appear in --help
	#[clap(
		long = "generate-config",
		value_name = "FILE",
		help = "Overwrite existing or create a new config file. Optionally pass a path to the config file to be created (not directory).",
		default_missing_value = "",
		num_args(0..=1),
		group = "config",
	)]
	_generate: Option<UseConfigArg>,
	#[clap(
		long = "use-config",
		value_name = "FILE",
		long_help = "Use the config file instead of the command line. Generates a new config if none exists.
All arguments except '--generate-config' are ignored if this is present.
Optionally pass a path to the config file to be created/read (not directory).",
		default_missing_value = "",
		num_args(0..=1),
		group = "config",
	)]
	_use: Option<UseConfigArg>,

	#[clap(long, help = "The host to bind to.", default_value = "127.0.0.1")]
	pub host: String,
	#[clap(long, default_value_t = 9005)]
	pub port: u16,
	#[clap(
		long,
		action,
		// help = "Enable /dashboard endpoint. Lets you view some statistics.",
		help = "not implemented.",
		default_value_t = false
	)]
	pub enable_webui: bool,
	#[clap(long, action, help = "Choose next song randomly.", default_value_t = true)]
	pub shuffle: bool,
	#[clap(
		long = "bitrate",
		help = "The bitrate to use for transcoding. Plain value for bps and suffixed with 'k' for kbps.",
		default_value = "128k"
	)]
	pub transcode_bitrate: Bitrate,
	#[clap(
		long,
		action,
		help = "Enable /mediainfo endpoint. It serves metadata for the current song in JSON format.",
		default_value_t = true,
		group = "mediainfo"
	)]
	pub enable_mediainfo: bool,
	#[clap(
		long,
		action,
		value_name = "SIZE",
		help = "The size of song history to keep track of. Must be greater than 0.",
		default_value = "16",
		group = "mediainfo",
		requires = "mediainfo"
	)]
	pub mediainfo_history: NonZeroUsize,
	#[clap(
		long,
		action,
		help = "Transcode files that can be sent without transcoding. Set to true if you want to reduce bandwidth a little.",
		default_value_t = false
	)]
	pub transcode: bool,
	#[clap(long, help = "The root directory to recursively search for music.")]
	pub root: PathBuf,
	#[command(flatten, help = "Optionally include or exclude directories or files.")]
	pub mode: Option<DirectoryConfigModeCli>,
}

#[derive(Debug, Serialize, Deserialize, clap::Parser, Clone)]
pub struct Bitrate {
	pub bits_per_second: NonZeroU32,
}

impl FromStr for Bitrate {
	type Err = String;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let bits_per_second = s.parse::<NonZeroU32>().map_err(|x| x.to_string()).or_else(|x| {
			let last_char =
				s.chars().last().ok_or_else(|| "Empty string".to_string())?.to_ascii_lowercase();

			if last_char == 'k' {
				s[..s.len() - 1]
					.parse::<NonZeroU32>()
					.map(|x| (x.get() * 1000).try_into().unwrap())
					.map_err(|x| x.to_string())
			} else {
				Err(x)
			}
		})?;
		Ok(Self { bits_per_second })
	}
}

#[derive(Debug, Serialize, Deserialize, clap::Args)]
#[group(required = false, multiple = false)]
pub struct DirectoryConfigModeCli {
	#[clap(long)]
	include: Vec<PathBuf>,
	#[clap(long)]
	exclude: Vec<PathBuf>,
}

impl From<CliConfig> for Config {
	fn from(cli: CliConfig) -> Self {
		let mut dir = Vec::new();
		let mode = match cli.mode {
			Some(DirectoryConfigModeCli { include, exclude }) => {
				if !include.is_empty() {
					dir.push(DirectoryConfig {
						root: cli.root.clone(),
						mode: DirectoryConfigMode::Include(include.into_boxed_slice()),
					});
				}
				if !exclude.is_empty() {
					dir.push(DirectoryConfig {
						root: cli.root.clone(),
						mode: DirectoryConfigMode::Exclude(exclude.into_boxed_slice()),
					});
				}
				unreachable!()
			}
			None => Some(DirectoryConfig {
				root: cli.root,
				mode: DirectoryConfigMode::Exclude([].into()),
			}),
		};
		if let Some(mode) = mode {
			dir.push(mode);
		}

		Self {
			host: cli.host,
			port: cli.port,
			dirs: dir.into_boxed_slice(),
			enable_webui: cli.enable_webui,
			shuffle: cli.shuffle,
			bitrate: cli.transcode_bitrate.bits_per_second.get(),
			transcode: cli.transcode,
			enable_mediainfo: cli.enable_mediainfo,
			mediainfo_history: cli.mediainfo_history,
		}
	}
}

impl Default for Config {
	fn default() -> Self {
		Self {
			host: "0.0.0.0".to_string(),
			port: 9005,
			dirs: Box::new([DirectoryConfig {
				root: PathBuf::from("./"),
				mode: DirectoryConfigMode::Exclude([].into()),
			}]),
			shuffle: true,
			enable_webui: true,
			bitrate: 128_000,
			transcode: false,
			enable_mediainfo: true,
			mediainfo_history: NonZeroUsize::new(16).unwrap(),
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "mode", content = "paths")]
pub enum DirectoryConfigMode {
	Include(Box<[PathBuf]>),
	Exclude(Box<[PathBuf]>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryConfig {
	pub root: PathBuf,
	pub mode: DirectoryConfigMode,
}

pub enum Error {
	Parse(String),
	Io(std::io::Error),
}

impl From<std::io::Error> for Error {
	fn from(value: std::io::Error) -> Self {
		Self::Io(value)
	}
}

impl From<toml::de::Error> for Error {
	fn from(value: toml::de::Error) -> Self {
		Self::Parse(value.to_string())
	}
}

pub fn generate_or_load(path: &Path) -> Result<Config, Error> {
	if !path.exists() {
		generate_config_file(path)
	} else {
		let x = std::fs::read_to_string(path).unwrap();
		Ok(toml::from_str(&x)?)
	}
}

pub fn generate_config_file(path: &Path) -> Result<Config, Error> {
	if !path.exists() {
		path.parent()
			.ok_or_else(|| {
				std::io::Error::new(std::io::ErrorKind::InvalidData, "No parent directory")
			})
			.and_then(std::fs::create_dir_all)?;
	}
	let config = Config::default();
	std::fs::write(path, toml::to_string(&config).unwrap())?;

	Ok(config)
}

pub fn config_path() -> PathBuf {
	#[cfg(target_os = "linux")]
	if is_root::is_root() {
		PathBuf::from("/etc/radio/config.toml")
	} else {
		PathBuf::from("~/.config/radio/config.toml")
	}
	#[cfg(target_os = "windows")]
	if is_root::is_root() {
		windirs::known_folder_path(windirs::FolderId::RoamingAppData)
	} else {
		windirs::known_folder_path(windirs::FolderId::LocalAppData)
	}
	.unwrap()
	.join("radio/config.toml")
}

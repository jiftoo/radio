use std::{io::ErrorKind, process::Command};

fn main() {
	let generated_warnings = check_exec("ffmpeg") || check_exec("mediainfo");

	if generated_warnings {
		println!(
			"cargo:warning=You can ignore the above warnings if you're not planning to run {}/{}",
			env!("CARGO_PKG_NAME"),
			env!("CARGO_PKG_VERSION"),
		);
	}
}

fn check_exec(name: &str) -> bool {
	match Command::new(name).spawn().or(Command::new(format!("./{name}")).spawn()) {
		Ok(mut x) => {
			let _ = x.kill();
			false
		}
		Err(err) => match err.kind() {
			ErrorKind::NotFound => {
				println!("cargo:warning='{name}' not found on your system. Make sure it's in cwd of the program or in PATH.");
				true
			}
			x => {
				println!("cargo:warning={x:?} encountered while calling '{name}'");
				true
			}
		},
	}
}

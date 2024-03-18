use std::{io::ErrorKind, process::Command};

fn main() {
	println!("cargo:rerun-if-env-changed=BASE_URL");
	println!("cargo:rerun-if-changed=./radio-webapp/src");

	let generated_warnings = (cannot_run("ffmpeg")) || (cannot_run("ffprobe"));

	if generated_warnings {
		println!(
			"cargo:warning=You can ignore the above warnings if you're not planning to run {}/{}",
			env!("CARGO_PKG_NAME"),
			env!("CARGO_PKG_VERSION"),
		);
	}

	#[cfg(feature = "webapp")]
	build_web();
}

fn build_web() {
	if cannot_run("pnpm") {
		println!("cargo:warning=You need to install pnpm to build the crate.");
		std::process::exit(1);
	}
	fn run(cmd: &mut Command) {
		let output = cmd.output().unwrap();
		let status = output.status;
		let output = String::from_utf8(output.stdout).unwrap();
		let output = output.split('\n').collect::<Vec<_>>();
		for line in output {
			println!("cargo:warning={}", line);
		}
		if !status.success() {
			panic!("Command failed: {:?}", cmd)
		};
	}

	let base_url = env!("BASE_URL");
	run(Command::new("pnpm").arg("i").current_dir("./radio-webapp"));
	run(Command::new("pnpm").arg("build").env("BASE_URL", base_url).current_dir("./radio-webapp"));
	println!("cargo:warning=Webapp built successfully. Base url: {:?}", base_url);
}

fn cannot_run(name: &str) -> bool {
	let in_path = Command::new(name).spawn();
	let in_cd = Command::new(format!("./{name}")).spawn();
	let result = match in_path.as_ref().or(in_cd.as_ref()) {
		Ok(_) => false,
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
	};
	let _ = in_path.map(|mut x| x.kill());
	let _ = in_cd.map(|mut x| x.kill());
	result
}

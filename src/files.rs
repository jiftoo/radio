use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

use rayon::prelude::*;

pub fn collect(path: impl AsRef<Path> + Send) -> Vec<PathBuf> {
	const SUPPORTED_FORMATS: [&str; 4] = ["mp3", "flac", "opus", "wav"];

	let pool = Arc::new(rayon::ThreadPoolBuilder::new().build().unwrap());

	pool.install(|| {
		jwalk::WalkDir::new(path)
			.parallelism(jwalk::Parallelism::RayonExistingPool {
				pool: pool.clone(),
				busy_timeout: None,
			})
			.into_iter()
			.par_bridge()
			.flatten()
			.flat_map(|x| {
				let cond = x.file_type.is_file()
					&& Path::new(&x.file_name)
						.extension()
						.and_then(|x| x.to_str())
						.is_some_and(|x| SUPPORTED_FORMATS.contains(&x));
				cond.then(|| x.path())
			})
			.collect::<Vec<_>>()
	})
}

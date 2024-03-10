use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

use rayon::{prelude::*, ThreadPool};

use crate::config::{self, DirectoryConfig};

const SUPPORTED_FORMATS: [&str; 4] = ["mp3", "flac", "opus", "wav"];

pub fn collect(path: &[config::DirectoryConfig]) -> Vec<PathBuf> {
	let pool = Arc::new(rayon::ThreadPoolBuilder::new().build().unwrap());

	pool.install(|| path.iter().flat_map(|x| walk(x.clone(), pool.clone())).collect())
}

fn walk(path: DirectoryConfig, pool: Arc<ThreadPool>) -> Vec<PathBuf> {
	jwalk::WalkDir::new(path.root)
		.parallelism(jwalk::Parallelism::RayonExistingPool { pool, busy_timeout: None })
		.into_iter()
		.par_bridge()
		.flatten()
		.filter(|x| match &path.mode {
			config::DirectoryConfigMode::Exclude(dirs) => {
				dirs.iter().any(|y| !x.path().starts_with(y))
			}
			config::DirectoryConfigMode::Include(dirs) => {
				dirs.iter().any(|y| x.path().starts_with(y))
			}
		})
		.flat_map(|x| {
			let cond = x.file_type.is_file()
				&& Path::new(&x.file_name)
					.extension()
					.and_then(|x| x.to_str())
					.is_some_and(|x| SUPPORTED_FORMATS.contains(&x));
			cond.then(|| x.path())
		})
		.collect::<Vec<_>>()
}

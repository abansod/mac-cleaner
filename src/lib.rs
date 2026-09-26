mod cli;
mod engine;
mod login_items;
mod macos_space;
pub mod models;
pub mod safety;
pub mod scanners;
mod ui;

use std::sync::OnceLock;

use rayon::{ThreadPool, ThreadPoolBuilder};

/// Metadata syscalls contend in the kernel past a few threads, so filesystem walks and
/// existence checks share this small pool instead of the global one.
const FS_THREADS: usize = 4;

pub(crate) fn fs_pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(FS_THREADS);
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("fs-{i}"))
            .build()
            .expect("build filesystem thread pool")
    })
}

pub fn run() -> anyhow::Result<()> {
    cli::run()
}

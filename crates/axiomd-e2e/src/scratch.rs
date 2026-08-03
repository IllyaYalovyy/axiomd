//! A directory that exists for the duration of one test and is removed with it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Everything one test's application owns on disk: its display's runtime directory,
/// its configuration, its logs, its captures.
///
/// The name says which suite and which process made it, so anything left behind by a
/// killed run says where it came from.
pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub(crate) fn new(label: &str) -> Scratch {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "axiomd-e2e-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {path:?}: {error}"));
        Scratch { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

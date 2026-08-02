//! What the tests need from outside the process: real files, and a rule for
//! touching WebKit.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Runs `body` with exclusive use of WebKit.
///
/// WebKitGTK builds its shared preference store the first time any `WebKitSettings`
/// is constructed, lazily and without locking. Two threads constructing one at the
/// same time race inside `WebPreferencesStore::defaults()` and take the process down
/// — reproduced here on WebKitGTK 2.52.5 and confirmed from the core dump, where two
/// threads sit in `webkit_settings_init` and one is rehashing the store the other is
/// reading.
///
/// The application never has this problem: it touches WebKit only from the GTK main
/// thread. The test binary runs tests on several threads at once, so it has to be
/// told.
pub(crate) fn with_webkit<T>(body: impl FnOnce() -> T) -> T {
    static WEBKIT: Mutex<()> = Mutex::new(());

    let _guard = WEBKIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    body()
}

/// A directory that exists for the duration of one test and is removed with it.
///
/// The scheme handler's whole job is deciding which files a document may read, so
/// its tests cannot work on an in-memory stand-in: a symlink that leaves the
/// document's directory only exists on a filesystem.
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Creates an empty directory whose name mentions `label`, so a leftover after a
    /// crashed test says which test left it.
    pub(crate) fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axiomd-{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `relative` inside the directory, creating parents, and
    /// returns the full path.
    pub(crate) fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create scratch subdirectory");
        }
        std::fs::write(&path, contents).expect("write scratch file");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

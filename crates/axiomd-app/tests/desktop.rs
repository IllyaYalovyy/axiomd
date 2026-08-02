//! The desktop's half of UT-001: double-click a Markdown file, get axiomd.
//!
//! The final dispatch is the platform's contract, not ours (`docs/TESTING.md`,
//! accepted category 2). Everything up to that boundary is asserted here: the entry
//! is a valid desktop file, the type it registers is the type Markdown files
//! actually have, and installing it makes axiomd the handler the desktop picks.
//!
//! Each check runs against a throwaway `XDG_DATA_HOME`, so the suite never changes
//! which application the machine running it opens Markdown with.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The desktop file's base name is the application id; the session, the desktop file
/// and the D-Bus name all have to agree on it.
const DESKTOP_ID: &str = "io.github.etf.axiomd.desktop";

fn desktop_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/{DESKTOP_ID}"))
}

/// Runs a desktop-integration tool, insisting it is present rather than quietly
/// skipping: a check that silently does not run is a check that is not there.
fn tool(name: &str) -> Command {
    if which(name).is_none() {
        panic!(
            "{name} is required to verify desktop integration \
             (Fedora: desktop-file-utils / xdg-utils, Debian: desktop-file-utils / xdg-utils)",
        );
    }
    Command::new(name)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| candidate.is_file())
}

/// A throwaway XDG data home with the desktop file installed in it.
struct InstalledEntry {
    root: PathBuf,
}

impl InstalledEntry {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "axiomd-desktop-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        let applications = root.join("data/applications");
        std::fs::create_dir_all(root.join("config")).expect("create scratch config home");
        std::fs::create_dir_all(&applications).expect("create scratch applications directory");
        std::fs::copy(desktop_file(), applications.join(DESKTOP_ID)).expect("install desktop file");

        let updated = tool("update-desktop-database")
            .arg(&applications)
            .output()
            .expect("run update-desktop-database");
        assert!(
            updated.status.success(),
            "update-desktop-database failed: {}",
            String::from_utf8_lossy(&updated.stderr),
        );

        Self { root }
    }

    /// `xdg-mime`, looking only at this installation and never at the real session.
    fn xdg_mime(&self) -> Command {
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
        let path = std::env::var("PATH").expect("a PATH to find xdg-mime on");

        let mut command = tool("xdg-mime");
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.root)
            .env("XDG_CURRENT_DESKTOP", "GNOME")
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_DIRS", data_dirs);
        command
    }

    fn query(&self, arguments: &[&str]) -> String {
        let output = self
            .xdg_mime()
            .arg("query")
            .args(arguments)
            .output()
            .expect("run xdg-mime query");
        assert!(
            output.status.success(),
            "xdg-mime query {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8(output.stdout)
            .expect("xdg-mime prints utf-8")
            .trim()
            .to_owned()
    }
}

impl Drop for InstalledEntry {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_desktop_entry_is_valid() {
    let output = tool("desktop-file-validate")
        .arg(desktop_file())
        .output()
        .expect("run desktop-file-validate");

    let complaints = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    assert!(
        output.status.success() && complaints.is_empty(),
        "desktop-file-validate is not happy with {DESKTOP_ID}:\n{complaints}",
    );
}

/// The entry registers `text/markdown` and nothing else; this is the check that the
/// extensions users actually have really carry that type.
#[test]
fn the_type_it_registers_is_the_type_markdown_files_have() {
    let installed = InstalledEntry::new();
    let documents = installed.root.join("data");

    for name in ["README.md", "NOTES.markdown"] {
        let file = documents.join(name);
        std::fs::write(&file, "# Heading\n\nText.\n").expect("write a markdown file");

        assert_eq!(
            installed.query(&["filetype", &file.to_string_lossy()]),
            "text/markdown",
            "{name} is not recognised as Markdown",
        );
    }
}

/// UT-001 up to the boundary: with the entry installed, the desktop hands Markdown
/// documents to axiomd.
#[test]
fn installing_the_entry_makes_axiomd_the_handler_for_markdown() {
    let installed = InstalledEntry::new();

    assert_eq!(installed.query(&["default", "text/markdown"]), DESKTOP_ID);
}
